//! The synchronisation engine.
//!
//! It compares the save folder, the local state and the server manifest, and
//! decides what to upload, what to download and what needs the user.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::api::{ApiClient, ServerFile};
use crate::config::{now_string, CloudInfo, Config, GameEntry, GameState, LocalInfo, Paths};
use crate::scanner::{self, ScannedFile};

/// What the user decided about a conflicting file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    /// The cloud version wins: it is restored into the save folder.
    KeepCloud,
    /// The local version wins: it is uploaded, the cloud one is archived.
    KeepLocal,
    /// Both are kept: the local one is uploaded, the cloud one is archived.
    KeepBoth,
}

/// A file that changed on both sides.
#[derive(Debug, Clone)]
pub struct Conflict {
    /// Path relative to the save folder.
    pub path: String,
    /// Digest of the local file.
    pub local_sha256: String,
    /// Digest of the cloud file.
    pub cloud_sha256: String,
    /// Size of the local file.
    pub local_size: u64,
    /// Size of the cloud file.
    pub cloud_size: i64,
    /// Modification time of the local file, in nanoseconds.
    pub local_mtime_ns: i64,
    /// When the cloud file was last changed.
    pub cloud_updated_at: String,
    /// Version of the cloud file.
    pub cloud_version: i64,
}

/// What a synchronisation did.
#[derive(Debug, Default)]
pub struct Outcome {
    /// Files uploaded to the server.
    pub uploaded: Vec<String>,
    /// Files downloaded into the mirror.
    pub downloaded: Vec<String>,
    /// Files removed on the server.
    pub deleted: Vec<String>,
    /// Files that need the user to decide.
    pub conflicts: Vec<Conflict>,
    /// Files that disappeared locally and were kept in the mirror.
    pub missing_locally: Vec<String>,
    /// Anything worth telling the user.
    pub warnings: Vec<String>,
}

impl Outcome {
    /// Whether anything happened at all.
    pub fn is_empty(&self) -> bool {
        self.uploaded.is_empty()
            && self.downloaded.is_empty()
            && self.deleted.is_empty()
            && self.conflicts.is_empty()
            && self.missing_locally.is_empty()
    }

    /// A one line summary for the log.
    pub fn summary(&self) -> String {
        format!(
            "{} uploaded, {} downloaded, {} deleted, {} conflicts",
            self.uploaded.len(),
            self.downloaded.len(),
            self.deleted.len(),
            self.conflicts.len()
        )
    }
}

/// Synchronise one game.
pub async fn sync_game(
    api: &ApiClient,
    game: &GameEntry,
    state: &mut GameState,
    paths: &Paths,
    cfg: &Config,
) -> Result<Outcome> {
    let mut outcome = Outcome::default();

    // The namespace on the server is created the first time the game syncs.
    let server_game_id = match state
        .server_game_id
        .clone()
        .or_else(|| game.server_game_id.clone())
    {
        Some(id) => id,
        None => {
            let created = api.create_game(&game.name).await?;
            state.server_game_id = Some(created.id.clone());
            created.id
        }
    };

    let manifest = api.manifest(&server_game_id).await?;
    if manifest.game_id != server_game_id {
        bail!(
            "the server answered with the namespace {} instead of {}",
            manifest.game_id,
            server_game_id
        );
    }
    state.server_version = manifest.version;

    let mut cloud: BTreeMap<String, ServerFile> = BTreeMap::new();
    for file in manifest.files {
        cloud.insert(file.path.clone(), file);
    }

    let scanned = scanner::scan(game, state, cfg.max_file_size)?;
    outcome.warnings.extend(scanned.warnings.iter().cloned());

    let local_paths: Vec<String> = scanned.files.keys().cloned().collect();
    let cloud_paths: Vec<String> = cloud.keys().cloned().collect();

    let mut all_paths: Vec<String> = local_paths;
    for path in cloud_paths {
        if !all_paths.contains(&path) {
            all_paths.push(path);
        }
    }
    all_paths.sort();

    for path in all_paths {
        let local = scanned.files.get(&path);
        let remote = cloud.get(&path);
        let known = state.files.get(&path);

        let base_local = known
            .and_then(|k| k.local.as_ref())
            .map(|l| l.sha256.clone());
        let base_cloud = known
            .and_then(|k| k.cloud.as_ref())
            .map(|c| c.sha256.clone());

        let local_changed = match (local, &base_local) {
            (Some(file), Some(base)) => file.sha256.as_str() != base.as_str(),
            (Some(_), None) => true,
            (None, _) => false,
        };
        let cloud_changed = match (remote, &base_cloud) {
            (Some(file), Some(base)) => file.sha256.as_str() != base.as_str(),
            (Some(_), None) => true,
            (None, _) => false,
        };

        match (local, remote) {
            // Changed locally, unchanged on the server: upload it.
            (Some(file), remote) if local_changed && !cloud_changed => {
                upload(api, &server_game_id, game, state, file, remote, cfg).await?;
                outcome.uploaded.push(path.clone());
            }

            // Unchanged locally, changed on the server: download it.
            (None, Some(file)) if cloud_changed => {
                download(api, &server_game_id, paths, game, &path, file, state).await?;
                outcome.downloaded.push(path.clone());
                outcome.missing_locally.push(path.clone());
                outcome.warnings.push(format!(
                    "{path}: the game removed this file, the cloud version was kept in the mirror"
                ));
            }

            // Changed on both sides: ask the user.
            (Some(file), Some(remote)) if local_changed && cloud_changed => {
                outcome.conflicts.push(Conflict {
                    path: path.clone(),
                    local_sha256: file.sha256.clone(),
                    cloud_sha256: remote.sha256.clone(),
                    local_size: file.size,
                    cloud_size: remote.size,
                    local_mtime_ns: file.mtime_ns,
                    cloud_updated_at: remote.updated_at.clone(),
                    cloud_version: remote.version,
                });
            }

            // The game removed the file and the server has no newer version.
            (None, Some(file)) => {
                if cfg.delete_missing {
                    // The user asked for it: the file leaves the server too.
                    let expect = base_cloud.clone();
                    match api
                        .delete_file(&server_game_id, &path, expect.as_deref(), &cfg.device_name)
                        .await
                    {
                        Ok(()) => {
                            outcome.deleted.push(path.clone());
                            outcome
                                .warnings
                                .push(format!("{path}: removed from the server"));
                        }
                        Err(err) => {
                            outcome
                                .warnings
                                .push(format!("{path}: cannot be removed: {err}"));
                        }
                    }
                } else {
                    outcome.deleted.push(path.clone());
                    outcome.warnings.push(format!(
                        "{path}: the game removed this file, it is still on the server"
                    ));
                }
                let _ = file;
            }

            // Nothing to do.
            _ => {}
        }
    }

    // Remember what the local side looks like now.
    for (path, file) in &scanned.files {
        let entry = state.files.entry(path.clone()).or_default();
        entry.local = Some(LocalInfo {
            sha256: file.sha256.clone(),
            size: file.size,
            mtime_ns: file.mtime_ns,
            observed_at: now_string(),
        });
    }
    for (path, file) in &cloud {
        let entry = state.files.entry(path.clone()).or_default();
        entry.cloud = Some(CloudInfo {
            sha256: file.sha256.clone(),
            size: file.size as u64,
            version: file.version,
            updated_at: file.updated_at.clone(),
            device: file.device.clone(),
        });
    }

    Ok(outcome)
}

/// Upload one local file.
async fn upload(
    api: &ApiClient,
    server_game_id: &str,
    game: &GameEntry,
    state: &mut GameState,
    file: &ScannedFile,
    remote: Option<&ServerFile>,
    cfg: &Config,
) -> Result<()> {
    let path = Path::new(&game.save_dir).join(&file.path);
    let content = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;

    // The digest the server is expected to hold. Sending it makes the server
    // refuse the upload when the file changed there in the meantime.
    let expect = remote.map(|r| r.sha256.as_str());

    let uploaded = api
        .upload(
            server_game_id,
            &file.path,
            &content,
            file.mtime_ns,
            expect,
            &cfg.device_name,
        )
        .await?;

    let entry = state.files.entry(file.path.clone()).or_default();
    entry.local = Some(LocalInfo {
        sha256: uploaded.sha256.clone(),
        size: uploaded.size as u64,
        mtime_ns: file.mtime_ns,
        observed_at: now_string(),
    });
    entry.cloud = Some(CloudInfo {
        sha256: uploaded.sha256.clone(),
        size: uploaded.size as u64,
        version: uploaded.version,
        updated_at: uploaded.updated_at.clone(),
        device: uploaded.device.clone(),
    });
    Ok(())
}

/// Download one file into the mirror.
async fn download(
    api: &ApiClient,
    server_game_id: &str,
    paths: &Paths,
    game: &GameEntry,
    path: &str,
    file: &ServerFile,
    state: &mut GameState,
) -> Result<()> {
    let content = api.download(server_game_id, path).await?;
    let destination = mirror_path(paths, game, path);
    write_file_atomic(&destination, &content)?;

    let entry = state.files.entry(path.to_string()).or_default();
    entry.cloud = Some(CloudInfo {
        sha256: file.sha256.clone(),
        size: file.size as u64,
        version: file.version,
        updated_at: file.updated_at.clone(),
        device: file.device.clone(),
    });
    Ok(())
}

/// Apply the decision the user took about a conflicting file.
///
/// This is the only place where the companion writes into the save folder,
/// and it happens only when the user explicitly asks for it.
pub async fn resolve_conflict(
    api: &ApiClient,
    game: &GameEntry,
    state: &mut GameState,
    paths: &Paths,
    cfg: &Config,
    conflict: &Conflict,
    resolution: Resolution,
) -> Result<Outcome> {
    let mut outcome = Outcome::default();
    let server_game_id = state
        .server_game_id
        .clone()
        .context("the game has no namespace on the server yet")?;

    let local_path = Path::new(&game.save_dir).join(&conflict.path);

    match resolution {
        // The cloud version wins: restore it into the save folder.
        Resolution::KeepCloud => {
            let content = api.download(&server_game_id, &conflict.path).await?;
            write_file_atomic(&local_path, &content)?;
            let entry = state.files.entry(conflict.path.clone()).or_default();
            entry.local = Some(LocalInfo {
                sha256: conflict.cloud_sha256.clone(),
                size: conflict.cloud_size as u64,
                mtime_ns: conflict.local_mtime_ns,
                observed_at: now_string(),
            });
            outcome.uploaded.clear();
            outcome.warnings.push(format!(
                "{}: the cloud version was restored into the save folder",
                conflict.path
            ));
        }

        // The local version wins: archive the cloud one, then upload.
        Resolution::KeepLocal | Resolution::KeepBoth => {
            let content = api.download(&server_game_id, &conflict.path).await?;
            let archive = conflict_archive_path(paths, game, &conflict.path);
            write_file_atomic(&archive, &content)?;

            let local_content = fs::read(&local_path)
                .with_context(|| format!("reading {}", local_path.display()))?;
            let uploaded = api
                .upload(
                    &server_game_id,
                    &conflict.path,
                    &local_content,
                    conflict.local_mtime_ns,
                    None,
                    &cfg.device_name,
                )
                .await?;

            let entry = state.files.entry(conflict.path.clone()).or_default();
            entry.local = Some(LocalInfo {
                sha256: uploaded.sha256.clone(),
                size: uploaded.size as u64,
                mtime_ns: conflict.local_mtime_ns,
                observed_at: now_string(),
            });
            entry.cloud = Some(CloudInfo {
                sha256: uploaded.sha256.clone(),
                size: uploaded.size as u64,
                version: uploaded.version,
                updated_at: uploaded.updated_at.clone(),
                device: uploaded.device.clone(),
            });

            outcome.uploaded.push(conflict.path.clone());
            outcome.warnings.push(format!(
                "{}: the cloud version was archived in {}",
                conflict.path,
                archive.display()
            ));
        }
    }

    Ok(outcome)
}

/// The path of a file inside the mirror of a game.
pub fn mirror_path(paths: &Paths, game: &GameEntry, relative: &str) -> PathBuf {
    paths.mirror_dir(&game.slug()).join(relative)
}

/// The path where a losing version is archived.
pub fn conflict_archive_path(paths: &Paths, game: &GameEntry, relative: &str) -> PathBuf {
    let name = relative.replace('/', "_");
    paths
        .mirror_dir(&game.slug())
        .join(".conflicts")
        .join(format!("{name}.{}", crate::config::now_secs()))
}

/// Write a file atomically, creating the parent directories.
pub fn write_file_atomic(path: &Path, content: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let tmp = path.with_extension("part");
    fs::write(&tmp, content).with_context(|| format!("writing {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("renaming into {}", path.display()))?;
    Ok(())
}
