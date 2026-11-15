//! Game Save, the companion app.
//!
//! The library exposes the pieces the application is made of, so they can be
//! tested on their own:
//!
//! - [`config`]: the configuration, the paths and the on disk state;
//! - [`api`]: the HTTP client, with the server certificate pinned;
//! - [`scanner`]: the read only walk of a game's save folder;
//! - [`session`]: the detection of the game session;
//! - [`sync`]: the synchronisation engine;
//! - [`worker`]: the background worker;
//! - [`tray`]: the system tray icon;
//! - [`app`]: the graphical interface.

pub mod api;
pub mod app;
pub mod autostart;
pub mod config;
pub mod icons;
pub mod scanner;
pub mod session;
pub mod sync;
pub mod theme;
pub mod tray;
pub mod worker;
