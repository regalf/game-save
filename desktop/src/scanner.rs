//! Read only scanning of a game's save folder.
//!
//! This module is the only one that touches the save folder, and it only ever
//! opens files for reading. There is no write, rename or delete in it, on
//! purpose: the companion must never be able to damage a save.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::UNIX_EPOCH;

use anyhow::{Context, Result};
use globset::{Glob, GlobSetBuilder};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use crate::config::{GameEntry, GameState};

/// One file found in the save folder.
#[derive(Debug, Clone)]
pub struct ScannedFile {
    /// Path relative to the save folder, with forward slashes.
    pub path: String,
    /// Size in bytes.
    pub size: u64,
    /// Modification time in nanoseconds since the epoch.
    pub mtime_ns: i64,
    /// SHA-256 of the content.
    pub sha256: String,
}

/// The outcome of a scan.
#[derive(Debug, Default)]
pub struct ScanResult {
    /// The files that will be considered, keyed by relative path.
    pub files: BTreeMap<String, ScannedFile>,
    /// Files that were skipped, and why.
    pub warnings: Vec<String>,
}

/// Scan the save folder of a game, read only.
///
/// Files whose size and modification time did not change since the last scan
/// are not read at all: their digest is taken from the state file. That keeps
/// the scan cheap even for a save folder with thousands of files.
pub fn scan(game: &GameEntry, state: &GameState, max_file_size: u64) -> Result<ScanResult> {
    let root = Path::new(&game.save_dir);
    if !root.is_dir() {
        return Err(anyhow::anyhow!(
            "the save folder {} does not exist",
            root.display()
        ));
    }

    let excludes = build_excludes(&game.excludes);
    let mut result = ScanResult::default();

    let walker = WalkDir::new(root)
        .follow_links(false)
        .max_depth(16)
        .into_iter()
        .filter_entry(|entry| {
            // Never descend into a symlinked directory: it could point outside
            // the save folder.
            !(entry.file_type().is_symlink() && entry.depth() > 0)
        });

    for entry in walker {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                result.warnings.push(format!("cannot read an entry: {err}"));
                continue;
            }
        };

        if !entry.file_type().is_file() {
            continue;
        }

        let relative = match relative_path(root, entry.path()) {
            Some(rel) => rel,
            None => continue,
        };

        if !in_sync_paths(game, &relative) {
            continue;
        }
        if is_excluded(&excludes, &relative) {
            continue;
        }

        let metadata = match entry.metadata() {
            Ok(meta) => meta,
            Err(err) => {
                result
                    .warnings
                    .push(format!("{relative}: cannot read the metadata: {err}"));
                continue;
            }
        };
        let size = metadata.len();
        let mtime_ns = modification_time(&metadata);

        if size > max_file_size {
            result.warnings.push(format!(
                "{relative}: skipped, {size} bytes is over the {max_file_size} bytes limit"
            ));
            continue;
        }

        // The fast path: nothing changed since the last scan, so the known
        // digest is still valid and the file does not have to be read.
        if let Some(known) = state.files.get(&relative) {
            if let Some(local) = &known.local {
                if local.size == size && local.mtime_ns == mtime_ns && !local.sha256.is_empty() {
                    result.files.insert(
                        relative.clone(),
                        ScannedFile {
                            path: relative,
                            size,
                            mtime_ns,
                            sha256: local.sha256.clone(),
                        },
                    );
                    continue;
                }
            }
        }

        // The file may be written by the game right now. Read it, then check
        // that it did not change while it was being read.
        let before = (size, mtime_ns);
        let digest = match hash_file(entry.path()) {
            Ok(digest) => digest,
            Err(err) => {
                result
                    .warnings
                    .push(format!("{relative}: cannot be read: {err}"));
                continue;
            }
        };
        let after = match fs::metadata(entry.path()) {
            Ok(meta) => (meta.len(), modification_time(&meta)),
            Err(err) => {
                result
                    .warnings
                    .push(format!("{relative}: vanished while reading: {err}"));
                continue;
            }
        };
        if before != after {
            result.warnings.push(format!(
                "{relative}: changed while being read, skipped for now"
            ));
            continue;
        }

        result.files.insert(
            relative.clone(),
            ScannedFile {
                path: relative,
                size,
                mtime_ns,
                sha256: digest,
            },
        );
    }

    Ok(result)
}

/// Build the glob set of the excluded patterns.
fn build_excludes(patterns: &[String]) -> globset::GlobSet {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        let pattern = pattern.trim();
        if pattern.is_empty() {
            continue;
        }
        // A pattern without a slash matches the file name anywhere.
        let candidate = if pattern.contains('/') {
            pattern.to_string()
        } else {
            format!("**/{pattern}")
        };
        if let Ok(glob) = Glob::new(&candidate) {
            builder.add(glob);
        }
        if let Ok(glob) = Glob::new(pattern) {
            builder.add(glob);
        }
    }
    builder
        .build()
        .unwrap_or_else(|_| GlobSetBuilder::new().build().expect("empty glob set"))
}

/// Whether a relative path matches one of the exclusions.
fn is_excluded(set: &globset::GlobSet, relative: &str) -> bool {
    set.is_match(relative)
}

/// Whether a relative path belongs to the configured sub folders.
fn in_sync_paths(game: &GameEntry, relative: &str) -> bool {
    if game.sync_paths.is_empty() {
        return true;
    }
    game.sync_paths
        .iter()
        .map(|p| p.trim().trim_matches('/'))
        .filter(|p| !p.is_empty())
        .any(|prefix| relative == prefix || relative.starts_with(&format!("{prefix}/")))
}

/// The path of a file relative to the save folder, with forward slashes.
fn relative_path(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let mut parts = Vec::new();
    for component in rel.components() {
        let text = component.as_os_str().to_string_lossy().to_string();
        if text.is_empty() || text == "." {
            continue;
        }
        parts.push(text);
    }
    if parts.is_empty() {
        return None;
    }
    Some(parts.join("/"))
}

/// Modification time in nanoseconds since the epoch, zero when unavailable.
fn modification_time(metadata: &fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0)
}

/// SHA-256 of the content of a file.
pub fn hash_file(path: &Path) -> Result<String> {
    use std::io::Read;

    let mut file = fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;

    fn entry(name: &str, save_dir: &Path) -> GameEntry {
        GameEntry {
            id: "1".into(),
            name: name.into(),
            exe: "/bin/true".into(),
            save_dir: save_dir.to_string_lossy().to_string(),
            sync_paths: vec![],
            excludes: vec![],
            icon: String::new(),
            server_game_id: None,
            added_at: String::new(),
        }
    }

    #[test]
    fn scan_finds_files_and_hashes_them() {
        let dir = tempfile::tempdir().unwrap();
        let saves = dir.path().join("saves");
        fs::create_dir_all(&saves).unwrap();
        let mut f = File::create(saves.join("slot1.dat")).unwrap();
        f.write_all(b"hello").unwrap();
        drop(f);

        let game = entry("Game", dir.path());
        let state = GameState::default();
        let result = scan(&game, &state, 1 << 20).unwrap();

        assert_eq!(result.files.len(), 1);
        let file = &result.files["saves/slot1.dat"];
        assert_eq!(file.size, 5);
        assert_eq!(
            file.sha256,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn exclusions_are_honoured() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("keep.dat"), b"a").unwrap();
        fs::write(dir.path().join("skip.tmp"), b"a").unwrap();

        let mut game = entry("Game", dir.path());
        game.excludes = vec!["*.tmp".into()];
        let result = scan(&game, &GameState::default(), 1 << 20).unwrap();

        assert_eq!(result.files.len(), 1);
        assert!(result.files.contains_key("keep.dat"));
    }

    #[test]
    fn sync_paths_restrict_the_scan() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("saves")).unwrap();
        fs::create_dir_all(dir.path().join("config")).unwrap();
        fs::write(dir.path().join("saves/a.dat"), b"a").unwrap();
        fs::write(dir.path().join("config/b.cfg"), b"b").unwrap();

        let mut game = entry("Game", dir.path());
        game.sync_paths = vec!["saves".into()];
        let result = scan(&game, &GameState::default(), 1 << 20).unwrap();

        assert_eq!(result.files.len(), 1);
        assert!(result.files.contains_key("saves/a.dat"));
    }

    #[test]
    fn the_fast_path_reuses_the_known_digest() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.dat"), b"content").unwrap();

        let game = entry("Game", dir.path());
        let first = scan(&game, &GameState::default(), 1 << 20).unwrap();

        let mut state = GameState::default();
        let file_state = crate::config::FileState {
            local: Some(crate::config::LocalInfo {
                sha256: "stale-but-trusted".into(),
                size: first.files["a.dat"].size,
                mtime_ns: first.files["a.dat"].mtime_ns,
                observed_at: String::new(),
            }),
            ..Default::default()
        };
        state.files.insert("a.dat".into(), file_state);

        let second = scan(&game, &state, 1 << 20).unwrap();
        assert_eq!(second.files["a.dat"].sha256, "stale-but-trusted");
    }
}
