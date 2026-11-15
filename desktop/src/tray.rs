//! The system tray icon.
//!
//! It offers the actions that make sense without opening the window:
//! synchronise now, show the window, quit.

use std::sync::mpsc::{channel, Receiver};

use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

/// An action coming from the tray.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayEvent {
    /// Synchronise every game now.
    Sync,
    /// Bring the window to the front.
    Show,
    /// Close the application.
    Quit,
}

/// The tray icon.
pub struct Tray {
    // The icon must be kept alive, dropping it removes it from the tray.
    _icon: TrayIcon,
    rx: Receiver<TrayEvent>,
}

impl Tray {
    /// Create the tray icon. Returns `None` when the platform refuses it,
    /// which is not fatal: the window alone is enough to use the app.
    pub fn new() -> Option<Self> {
        let menu = Menu::new();
        let _ = menu.append(&PredefinedMenuItem::separator());
        let _ = menu.append(&MenuItem::with_id(
            MenuId::new("sync"),
            "Sync now",
            true,
            None,
        ));
        let _ = menu.append(&MenuItem::with_id(
            MenuId::new("show"),
            "Show Game Save",
            true,
            None,
        ));
        let _ = menu.append(&MenuItem::with_id(MenuId::new("quit"), "Quit", true, None));

        let icon = Icon::from_rgba(icon_rgba(), ICON_SIZE, ICON_SIZE).ok()?;
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_icon(icon)
            .with_tooltip("Game Save")
            .build()
            .ok()?;

        let (tx, rx) = channel::<TrayEvent>();
        std::thread::Builder::new()
            .name("gamesave-tray".into())
            .spawn(move || {
                while let Ok(event) = MenuEvent::receiver().recv() {
                    let message = match event.id.0.as_str() {
                        "sync" => TrayEvent::Sync,
                        "show" => TrayEvent::Show,
                        "quit" => TrayEvent::Quit,
                        _ => continue,
                    };
                    if tx.send(message).is_err() {
                        break;
                    }
                }
            })
            .ok()?;

        Some(Self { _icon: tray, rx })
    }

    /// Take every action produced since the last call.
    pub fn drain(&self) -> Vec<TrayEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.rx.try_recv() {
            events.push(event);
        }
        events
    }
}

/// The size of the icon, in pixels.
const ICON_SIZE: u32 = 22;

/// Build the icon: a soft green disc with a lighter core.
fn icon_rgba() -> Vec<u8> {
    let size = ICON_SIZE as i32;
    let centre = (size - 1) as f32 / 2.0;
    let radius = centre;
    let mut pixels = Vec::with_capacity((size * size * 4) as usize);

    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - centre;
            let dy = y as f32 - centre;
            let distance = (dx * dx + dy * dy).sqrt();

            let (r, g, b, a) = if distance <= radius * 0.72 {
                (235u8, 245u8, 235u8, 255u8)
            } else if distance <= radius {
                (70u8, 170u8, 110u8, 255u8)
            } else {
                (0u8, 0u8, 0u8, 0u8)
            };
            pixels.extend_from_slice(&[r, g, b, a]);
        }
    }
    pixels
}
