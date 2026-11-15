//! Starting the application together with the session.
//!
//! On Linux an autostart entry is written into the XDG autostart directory,
//! on Windows the run key of the current user is used.

use anyhow::{Context, Result};
use auto_launch::AutoLaunchBuilder;

/// The name the autostart entry is registered under.
const APP_NAME: &str = "Game Save";

/// Build the autostart entry for the running executable.
fn launcher() -> Result<auto_launch::AutoLaunch> {
    let path = std::env::current_exe().context("the path of the running program")?;
    AutoLaunchBuilder::new()
        .set_app_name(APP_NAME)
        .set_app_path(&path.to_string_lossy())
        .build()
        .context("the autostart entry could not be built")
}

/// Whether the application starts with the session.
pub fn is_enabled() -> bool {
    match launcher() {
        Ok(entry) => entry.is_enabled().unwrap_or(false),
        Err(_) => false,
    }
}

/// Turn the autostart entry on or off.
pub fn set_enabled(enabled: bool) -> Result<()> {
    let entry = launcher()?;
    if enabled {
        entry
            .enable()
            .context("the autostart entry could not be written")
    } else {
        entry
            .disable()
            .context("the autostart entry could not be removed")
    }
}
