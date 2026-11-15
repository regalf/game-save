//! Detection of the game session.
//!
//! A session runs from the moment the game process appears until it goes
//! away. A crash, a normal exit and a kill are the same event, because the
//! signal is the process disappearing, not its exit status.

use std::path::Path;

/// Whether the session watcher can work on this platform.
pub const SUPPORTED: bool = cfg!(target_os = "linux");

/// Whether the game is running right now.
pub fn is_running(exe: &str) -> bool {
    if exe.trim().is_empty() {
        return false;
    }
    #[cfg(target_os = "linux")]
    {
        linux_is_running(Path::new(exe))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = Path::new(exe);
        false
    }
}

/// Human readable name of the platform capability.
pub fn support_label() -> &'static str {
    if SUPPORTED {
        "session detection available"
    } else {
        "session detection not available on this platform yet"
    }
}

/// Linux implementation: compare the device and inode of the running process
/// with the registered executable.
///
/// Comparing the inode is robust against a different path spelling, against
/// spaces in the path and against a launcher script.
#[cfg(target_os = "linux")]
fn linux_is_running(exe: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;

    let target = match std::fs::metadata(exe) {
        Ok(meta) => (meta.dev(), meta.ino()),
        Err(_) => return false,
    };

    let Ok(entries) = std::fs::read_dir("/proc") else {
        return false;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let exe_link = path.join("exe");
        let Ok(meta) = std::fs::metadata(&exe_link) else {
            continue;
        };
        if meta.dev() == target.0 && meta.ino() == target.1 {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_executable_is_not_running() {
        assert!(!is_running("/definitely/not/here/at/all"));
    }

    #[test]
    fn an_empty_executable_is_not_running() {
        assert!(!is_running(""));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_current_process_is_detected() {
        // The test binary is a running process on this machine.
        let exe = std::env::current_exe().unwrap();
        assert!(is_running(&exe.to_string_lossy()));
    }
}
