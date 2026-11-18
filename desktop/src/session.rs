//! Detection of the game session.
//!
//! A session runs from the moment the game process appears until it goes
//! away. A crash, a normal exit and a kill are the same event, because the
//! signal is the process disappearing, not its exit status.

use std::path::Path;

/// Whether the session watcher can work on this platform.
pub const SUPPORTED: bool = cfg!(any(target_os = "linux", target_os = "windows"));

/// Whether the game is running right now.
pub fn is_running(exe: &str) -> bool {
    if exe.trim().is_empty() {
        return false;
    }
    #[cfg(target_os = "linux")]
    {
        linux_is_running(Path::new(exe))
    }
    #[cfg(target_os = "windows")]
    {
        windows_is_running(Path::new(exe))
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
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

/// Windows implementation: enumerate the processes and compare the full path
/// of their image with the registered executable.
///
/// The comparison is made on the canonical path, lower cased, so it survives a
/// different spelling, the short name of a component and the case difference
/// that Windows does not care about.
#[cfg(target_os = "windows")]
fn windows_is_running(exe: &Path) -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    let target = normalize_path(exe);

    let snapshot = match unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) } {
        Ok(snapshot) => snapshot,
        Err(_) => return false,
    };

    let found = unsafe {
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };

        let mut current = Process32FirstW(snapshot, &mut entry);
        let mut found = false;
        while current.is_ok() {
            if let Some(path) = process_image_path(entry.th32ProcessID) {
                if normalize_path(Path::new(&path)) == target {
                    found = true;
                    break;
                }
            }
            current = Process32NextW(snapshot, &mut entry);
        }
        let _ = CloseHandle(snapshot);
        found
    };

    found
}

/// The full path of the image of a process, or `None` when it cannot be read.
#[cfg(target_os = "windows")]
unsafe fn process_image_path(pid: u32) -> Option<String> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };

    // LIMITED_INFORMATION is enough to read the path and works for the
    // processes the caller cannot fully open.
    let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
    let mut buffer = [0u16; 1024];
    let mut length = buffer.len() as u32;
    let result = QueryFullProcessImageNameW(
        handle,
        PROCESS_NAME_WIN32,
        PWSTR(buffer.as_mut_ptr()),
        &mut length,
    );
    let _ = CloseHandle(handle);
    if result.is_err() {
        return None;
    }
    Some(String::from_utf16_lossy(&buffer[..length as usize]))
}

/// The comparable form of a path: canonical, lower case, forward slashes and
/// without the extended length prefix.
#[cfg(target_os = "windows")]
fn normalize_path(path: &Path) -> String {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let text = canonical.to_string_lossy().to_string();
    let text = if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        text
    };
    text.to_ascii_lowercase().replace('\\', "/")
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

    #[cfg(target_os = "windows")]
    #[test]
    fn the_current_process_is_detected() {
        // The test binary is a running process on this machine.
        let exe = std::env::current_exe().unwrap();
        assert!(is_running(&exe.to_string_lossy()));
    }
}
