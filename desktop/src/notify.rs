//! The notification that says the app is still running.
//!
//! When the window is closed the app does not quit: it hides into the tray and
//! keeps synchronising. On Windows this module shows a toast, so the user is
//! told that the program is still alive. On the other platforms there is
//! nothing to do: the tray icon is already there.

/// The app user model id, the identity a toast is attributed to.
#[cfg(target_os = "windows")]
const AUMID: &str = "GameSave";

/// Tell the user that the window went to the tray and the app keeps running.
pub fn hidden_to_tray() {
    #[cfg(target_os = "windows")]
    {
        register_aumid();
        windows_toast();
    }
    #[cfg(not(target_os = "windows"))]
    {}
}

/// Register the app user model id once, so Windows accepts the toast.
#[cfg(target_os = "windows")]
static REGISTER_AUMID: std::sync::Once = std::sync::Once::new();

#[cfg(target_os = "windows")]
fn register_aumid() {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_OPTION_NON_VOLATILE,
    };
    use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;

    REGISTER_AUMID.call_once(|| {
        // The registry entry is what makes the notification center accept a
        // toast from a desktop program that has no Start menu shortcut. It is
        // per user, so no administrator rights are needed, and it is written
        // again on every launch, so a shortcut that lost the id self-heals.
        let subkey = wide(&format!("Software\\Classes\\AppUserModelId\\{AUMID}"));
        let mut key = HKEY(std::ptr::null_mut());
        let created = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                None,
                &mut key,
                None,
            )
        };
        if created == ERROR_SUCCESS {
            set_value(key, "DisplayName", "Game Save");
            if let Ok(exe) = std::env::current_exe() {
                set_value(key, "IconUri", &exe.to_string_lossy());
            }
            unsafe {
                let _ = RegCloseKey(key);
            }
        } else {
            log::warn!("the notification identity cannot be registered: {created:?}");
        }

        // Pin the process to the same identity, so the toast is attributed to
        // this program and not to the system heuristic.
        let id = wide(AUMID);
        if let Err(err) = unsafe { SetCurrentProcessExplicitAppUserModelID(PCWSTR(id.as_ptr())) } {
            log::warn!("the notification identity cannot be set: {err}");
        }
    });
}

/// Write one string value under an open key.
#[cfg(target_os = "windows")]
fn set_value(key: windows::Win32::System::Registry::HKEY, name: &str, value: &str) {
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{RegSetValueExW, REG_SZ};

    let name_wide = wide(name);
    let data = wide(value);
    let bytes = unsafe { std::slice::from_raw_parts(data.as_ptr() as *const u8, data.len() * 2) };
    let written = unsafe {
        RegSetValueExW(
            key,
            windows::core::PCWSTR(name_wide.as_ptr()),
            None,
            REG_SZ,
            Some(bytes),
        )
    };
    if written != ERROR_SUCCESS {
        log::warn!("the notification identity value {name:?} cannot be written: {written:?}");
    }
}

/// UTF-16, NUL-terminated, the form the Windows registry and the shell want.
#[cfg(target_os = "windows")]
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Show a Windows toast.
#[cfg(target_os = "windows")]
fn windows_toast() {
    use windows::core::HSTRING;
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};

    let app_id = HSTRING::from(AUMID);
    let xml = HSTRING::from(
        "<toast>\
            <visual><binding template=\"ToastGeneric\">\
                <text>Game Save</text>\
                <text>Game Save keeps running in the background.</text>\
            </binding></visual>\
        </toast>",
    );

    let Ok(document) = XmlDocument::new() else {
        log::warn!("the notification document cannot be created");
        return;
    };
    if let Err(err) = document.LoadXml(&xml) {
        log::warn!("the notification content is not valid: {err}");
        return;
    }
    let Ok(toast) = ToastNotification::CreateToastNotification(&document) else {
        log::warn!("the notification cannot be created");
        return;
    };
    match ToastNotificationManager::CreateToastNotifierWithId(&app_id) {
        Ok(notifier) => {
            if let Err(err) = notifier.Show(&toast) {
                log::warn!("the notification cannot be shown: {err}");
            }
        }
        Err(err) => log::warn!("the notification notifier cannot be created: {err}"),
    }
}
