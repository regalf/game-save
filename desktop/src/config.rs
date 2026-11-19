//! Configuration, paths and on disk state of the companion app.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// The user visible configuration of the companion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Base URL of the Game Save server.
    pub server_url: String,
    /// SHA-256 fingerprint of the server certificate, colon separated.
    pub fingerprint: String,
    /// API key used to authenticate.
    pub api_key: String,
    /// Name of this machine, shown in the server history.
    pub device_name: String,
    /// How often the session watcher looks for the game, in seconds.
    pub poll_interval_secs: u64,
    /// Sync automatically when a game session ends.
    pub auto_sync: bool,
    /// Largest file that is uploaded, in bytes.
    pub max_file_size: u64,
    /// Remove from the server the files the game deletes from its save folder.
    pub delete_missing: bool,
    /// Start the application together with the session.
    pub autostart: bool,
    /// The theme of the interface: "dark" or "light".
    pub theme: String,
    /// Write the log to a file, next to the state.
    pub enable_logs: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server_url: "https://127.0.0.1:8443".to_string(),
            fingerprint: String::new(),
            api_key: String::new(),
            device_name: default_device_name(),
            poll_interval_secs: 2,
            auto_sync: true,
            max_file_size: 1 << 30,
            delete_missing: false,
            autostart: false,
            theme: "dark".to_string(),
            enable_logs: true,
        }
    }
}

impl Config {
    /// Load the configuration, falling back to the defaults when the file does
    /// not exist yet.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = read_text(path)?;
        let cfg: Config =
            toml::from_str(&raw).with_context(|| format!("parsing {}", path.display()))?;
        Ok(cfg)
    }

    /// Save the configuration atomically.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let body = toml::to_string_pretty(self)?;
        write_atomic(path, body.as_bytes())
    }
}

/// Default device name: the machine hostname, or "desktop" when unknown.
fn default_device_name() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "desktop".to_string())
}

/// The directories the companion owns.
#[derive(Debug, Clone)]
pub struct Paths {
    /// Where the configuration file lives.
    pub config_dir: PathBuf,
    /// Where the library, the state and the mirror live.
    pub data_dir: PathBuf,
}

impl Paths {
    /// Discover the standard locations for the current platform.
    ///
    /// The `GAMESAVE_HOME` environment variable overrides them, which is
    /// handy for a portable installation or for a throwaway test.
    pub fn discover() -> Result<Self> {
        if let Some(home) = std::env::var_os("GAMESAVE_HOME") {
            let home = PathBuf::from(home);
            return Ok(Self {
                config_dir: home.join("config"),
                data_dir: home.join("data"),
            });
        }
        let config_dir = dirs::config_dir()
            .context("no configuration directory available")?
            .join("gamesave");
        let data_dir = dirs::data_dir()
            .context("no data directory available")?
            .join("gamesave");
        Ok(Self {
            config_dir,
            data_dir,
        })
    }

    /// Path of the configuration file.
    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    /// Path of the library, the list of registered games.
    pub fn library_file(&self) -> PathBuf {
        self.data_dir.join("library.json")
    }

    /// Path of the state file of one game.
    pub fn state_file(&self, slug: &str) -> PathBuf {
        self.data_dir.join("state").join(format!("{slug}.json"))
    }

    /// Directory holding the mirror of the cloud content of one game.
    pub fn mirror_dir(&self, slug: &str) -> PathBuf {
        self.data_dir.join("mirror").join(slug)
    }

    /// Create every directory the app needs.
    pub fn ensure(&self) -> Result<()> {
        fs::create_dir_all(&self.config_dir)?;
        fs::create_dir_all(self.data_dir.join("state"))?;
        fs::create_dir_all(self.data_dir.join("mirror"))?;
        Ok(())
    }
}

/// Read a file as text, dropping a leading UTF-8 byte order mark.
///
/// Editors on Windows like to add one, and a parser that does not expect it
/// refuses the whole file.
fn read_text(path: &Path) -> Result<String> {
    let raw = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let raw = raw.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&raw);
    Ok(String::from_utf8_lossy(raw).to_string())
}

/// Write a file atomically: write to a temporary file, then rename it.
///
/// A crash in the middle leaves the previous version intact.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes).with_context(|| format!("writing {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("renaming into {}", path.display()))?;
    Ok(())
}

/// Normalise a fingerprint for comparison: upper case, colon separated.
pub fn normalize_fingerprint(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    cleaned
        .as_bytes()
        .chunks(2)
        .map(|c| String::from_utf8_lossy(c).to_string())
        .collect::<Vec<_>>()
        .join(":")
}

/// One entry of the library: a registered game.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameEntry {
    /// Stable identifier of the entry, local to this machine.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Path of the executable, used to detect the game session.
    pub exe: String,
    /// Folder the game stores its saves in. Read only for the companion.
    pub save_dir: String,
    /// Sub folders to synchronise. Empty means the whole folder.
    pub sync_paths: Vec<String>,
    /// Glob patterns that are never synchronised.
    pub excludes: Vec<String>,
    /// An icon file to use instead of the one of the executable.
    pub icon: String,
    /// Identifier of the matching namespace on the server.
    pub server_game_id: Option<String>,
    /// When the entry was added.
    pub added_at: String,
}

impl GameEntry {
    /// A filesystem safe name derived from the display name.
    pub fn slug(&self) -> String {
        let mut slug = String::new();
        let mut dash = false;
        for c in self.name.chars() {
            if c.is_ascii_alphanumeric() {
                slug.push(c.to_ascii_lowercase());
                dash = false;
            } else if !dash && !slug.is_empty() {
                slug.push('-');
                dash = true;
            }
        }
        let slug = slug.trim_matches('-').to_string();
        if slug.is_empty() {
            "game".to_string()
        } else {
            slug
        }
    }
}

/// The library: every game registered on this machine.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Library {
    /// Format version, for future migrations.
    pub schema: u32,
    /// The games.
    pub games: Vec<GameEntry>,
}

impl Library {
    /// Load the library, returning an empty one when the file is missing.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = read_text(path)?;
        let lib: Library =
            serde_json::from_str(&raw).with_context(|| format!("parsing {}", path.display()))?;
        Ok(lib)
    }

    /// Save the library atomically.
    pub fn save(&self, path: &Path) -> Result<()> {
        let body = serde_json::to_vec_pretty(self)?;
        write_atomic(path, &body)
    }

    /// Find an entry by its local identifier.
    pub fn find(&self, id: &str) -> Option<&GameEntry> {
        self.games.iter().find(|g| g.id == id)
    }

    /// Remove an entry by its local identifier.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.games.len();
        self.games.retain(|g| g.id != id);
        self.games.len() != before
    }
}

/// What the companion knows about one file of one game.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileState {
    /// The last state observed in the game's save folder.
    pub local: Option<LocalInfo>,
    /// The last state known to be on the server.
    pub cloud: Option<CloudInfo>,
}

/// The observed state of a local file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LocalInfo {
    /// SHA-256 of the content.
    pub sha256: String,
    /// Size in bytes.
    pub size: u64,
    /// Modification time, in nanoseconds since the epoch.
    pub mtime_ns: i64,
    /// When it was observed.
    pub observed_at: String,
}

/// The known state of a file on the server.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CloudInfo {
    /// SHA-256 of the content.
    pub sha256: String,
    /// Size in bytes.
    pub size: u64,
    /// Server side version number.
    pub version: i64,
    /// When it was last changed on the server.
    pub updated_at: String,
    /// Which device uploaded it.
    pub device: String,
}

/// The per game sync state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GameState {
    /// Format version.
    pub schema: u32,
    /// Identifier of the namespace on the server.
    pub server_game_id: Option<String>,
    /// Last server version seen.
    pub server_version: i64,
    /// Per file state.
    pub files: BTreeMap<String, FileState>,
}

impl GameState {
    /// Load the state of a game, returning an empty one when missing.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = read_text(path)?;
        let state: GameState =
            serde_json::from_str(&raw).with_context(|| format!("parsing {}", path.display()))?;
        Ok(state)
    }

    /// Save the state of a game atomically.
    pub fn save(&self, path: &Path) -> Result<()> {
        let body = serde_json::to_vec_pretty(self)?;
        write_atomic(path, &body)
    }
}

/// Format a unix timestamp as `YYYY-MM-DD HH:MM:SS` in UTC.
pub fn format_time(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (h, mi, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);

    // days since 1970-01-01 to civil date, from Howard Hinnant's algorithm.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!("{y:04}-{m:02}-{d:02} {h:02}:{mi:02}:{s:02}")
}

/// Format a byte count with a binary unit: `254 MB`, `5.0 GB`.
pub fn format_bytes(bytes: i64) -> String {
    if bytes <= 0 {
        return "0 B".to_string();
    }
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", bytes, UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Current unix time in seconds.
pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Current time formatted for display.
pub fn now_string() -> String {
    format_time(now_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_normalised() {
        assert_eq!(normalize_fingerprint("8b:92:0e"), "8B:92:0E");
        assert_eq!(normalize_fingerprint("8b920e"), "8B:92:0E");
        assert_eq!(normalize_fingerprint("  8b 92 0e "), "8B:92:0E");
    }

    #[test]
    fn slug_is_filesystem_safe() {
        let entry = GameEntry {
            id: "1".into(),
            name: "Hollow Knight".into(),
            exe: "/x".into(),
            save_dir: "/y".into(),
            sync_paths: vec![],
            excludes: vec![],
            icon: String::new(),
            server_game_id: None,
            added_at: String::new(),
        };
        assert_eq!(entry.slug(), "hollow-knight");

        let entry = GameEntry {
            name: "  Celeste (Steam) ".into(),
            ..entry
        };
        assert_eq!(entry.slug(), "celeste-steam");
    }

    #[test]
    fn time_is_formatted() {
        assert_eq!(format_time(0), "1970-01-01 00:00:00");
        assert_eq!(format_time(1_700_000_000), "2023-11-14 22:13:20");
    }

    #[test]
    fn bytes_are_formatted() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(254 * 1024 * 1024), "254.0 MB");
        assert_eq!(format_bytes(5 * 1024 * 1024 * 1024), "5.0 GB");
    }
}
