//! The graphical interface: the library table, the settings and the conflict
//! dialog.

use std::collections::{BTreeMap, VecDeque};
use std::time::{Duration, Instant};

use egui::{Align, Layout, RichText};

use crate::autostart;
use crate::config::{now_string, Config, GameEntry, Library, Paths};
use crate::session;
use crate::sync::{Conflict, Resolution};
use crate::tray::{Tray, TrayEvent};
use crate::worker::{Cmd, Ev, Worker};

/// One line of the log.
struct LogLine {
    time: String,
    text: String,
    error: bool,
}

/// The state of one game, as shown in the table.
#[derive(Default)]
struct RowState {
    status: String,
    last_sync: String,
}

/// A conflict waiting for a decision.
#[derive(Clone)]
struct PendingConflict {
    game_id: String,
    game_name: String,
    conflict: Conflict,
}

/// The fields of the "add a game" dialog.
#[derive(Default)]
struct Draft {
    name: String,
    exe: String,
    save_dir: String,
    sync_paths: String,
    excludes: String,
}

/// The application.
pub struct GameSaveApp {
    paths: Paths,
    cfg: Config,
    library: Library,
    worker: Worker,
    log: VecDeque<LogLine>,
    rows: BTreeMap<String, RowState>,
    running: BTreeMap<String, bool>,
    pending: Vec<PendingConflict>,
    show_settings: bool,
    show_add: bool,
    draft: Draft,
    status: String,
    status_ok: bool,
    last_poll: Instant,
    last_attempt: Instant,
    next_id: u64,
    tray: Option<Tray>,
}

impl GameSaveApp {
    /// Build the application from what is on disk.
    pub fn new(paths: Paths, cfg: Config, library: Library) -> Self {
        let worker = Worker::start();
        let mut app = Self {
            paths,
            cfg,
            library,
            worker,
            log: VecDeque::new(),
            rows: BTreeMap::new(),
            running: BTreeMap::new(),
            pending: Vec::new(),
            show_settings: false,
            show_add: false,
            draft: Draft::default(),
            status: "not connected".to_string(),
            status_ok: false,
            last_poll: Instant::now(),
            last_attempt: Instant::now(),
            next_id: 1,
            tray: Tray::new(),
        };
        app.push_log("Game Save started".into(), false);
        app.connect();
        app
    }

    /// Push a line into the log.
    fn push_log(&mut self, text: String, error: bool) {
        self.log.push_back(LogLine {
            time: now_string(),
            text,
            error,
        });
        while self.log.len() > 500 {
            self.log.pop_front();
        }
    }

    /// Try to connect to the server.
    fn connect(&mut self) {
        self.status = "connecting...".into();
        self.status_ok = false;
        self.last_attempt = Instant::now();
        let cfg = self.cfg.clone();
        self.worker.send(Cmd::Connect { cfg });
    }

    /// Take every event the worker produced.
    fn drain_events(&mut self) {
        for event in self.worker.drain() {
            match event {
                Ev::Log(text) => self.push_log(text, false),
                Ev::Connected(text) => {
                    self.status = text.clone();
                    self.status_ok = true;
                    self.push_log(text, false);
                }
                Ev::Failed(text) => {
                    self.status = text.clone();
                    self.status_ok = false;
                    self.push_log(text, true);
                }
                Ev::Synced { id, summary } => {
                    let row = self.rows.entry(id.clone()).or_default();
                    row.status = "synced".into();
                    row.last_sync = now_string();
                    self.push_log(format!("{id}: {summary}"), false);
                }
                Ev::Conflicts { id, conflicts } => {
                    let name = self
                        .library
                        .find(&id)
                        .map(|g| g.name.clone())
                        .unwrap_or_else(|| id.clone());
                    for conflict in conflicts {
                        self.pending.push(PendingConflict {
                            game_id: id.clone(),
                            game_name: name.clone(),
                            conflict,
                        });
                    }
                    let row = self.rows.entry(id).or_default();
                    row.status = "conflict".into();
                }
            }
        }
    }

    /// Take every action the tray produced.
    fn drain_tray(&mut self, ctx: &egui::Context) {
        let Some(tray) = &self.tray else {
            return;
        };
        for event in tray.drain() {
            match event {
                TrayEvent::Sync => self.sync_all(),
                TrayEvent::Show => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                TrayEvent::Quit => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }

    /// Look for the games and trigger a synchronisation when a session ends.
    fn poll_sessions(&mut self, ctx: &egui::Context) {
        if self.last_poll.elapsed() < Duration::from_secs(self.cfg.poll_interval_secs.max(1)) {
            return;
        }
        self.last_poll = Instant::now();

        // When the server could not be reached, try again every half minute.
        if !self.status_ok && self.last_attempt.elapsed() >= Duration::from_secs(30) {
            self.connect();
        }

        let mut to_sync: Vec<GameEntry> = Vec::new();
        for entry in &self.library.games {
            let running = session::is_running(&entry.exe);
            let was_running = self.running.get(&entry.id).copied().unwrap_or(false);
            self.running.insert(entry.id.clone(), running);
            if was_running && !running && self.cfg.auto_sync {
                to_sync.push(entry.clone());
            }
        }

        for entry in to_sync {
            self.push_log(
                format!("{}: the game stopped, synchronising", entry.name),
                false,
            );
            let row = self.rows.entry(entry.id.clone()).or_default();
            row.status = "syncing".into();
            let cfg = self.cfg.clone();
            let paths = self.paths.clone();
            self.worker.send(Cmd::SyncGame { cfg, entry, paths });
        }

        ctx.request_repaint_after(Duration::from_millis(500));
    }

    /// The top bar.
    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Game Save");
            ui.separator();

            let colour = if self.status_ok {
                egui::Color32::from_rgb(130, 200, 130)
            } else {
                egui::Color32::from_rgb(220, 140, 120)
            };
            let status = self.status.clone();
            if ui.colored_label(colour, status.clone()).clicked() {
                self.connect();
            }
            if !self.status_ok && ui.small_button("Reconnect").clicked() {
                self.connect();
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Settings").clicked() {
                    self.show_settings = true;
                }
                if ui.button("Add a game").clicked() {
                    self.draft = Draft::default();
                    self.show_add = true;
                }
                if ui.button("Sync all").clicked() {
                    self.sync_all();
                }
            });
        });
    }

    /// Queue a synchronisation of every game.
    fn sync_all(&mut self) {
        if self.library.games.is_empty() {
            self.push_log("the library is empty".into(), false);
            return;
        }
        for entry in &self.library.games {
            let row = self.rows.entry(entry.id.clone()).or_default();
            row.status = "syncing".into();
        }
        let cfg = self.cfg.clone();
        let games = self.library.games.clone();
        let paths = self.paths.clone();
        self.worker.send(Cmd::SyncAll { cfg, games, paths });
    }

    /// The library table.
    fn library_table(&mut self, ui: &mut egui::Ui) {
        if self.library.games.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new("No games yet.").weak());
                ui.label(RichText::new("Use \"Add a game\" to register one.").weak());
            });
            return;
        }

        ui.horizontal(|ui| {
            ui.strong("Game");
            ui.strong("Save folder");
            ui.strong("State");
            ui.strong("Last sync");
        });
        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            let games: Vec<GameEntry> = self.library.games.clone();
            for entry in games {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&entry.name).strong());
                    ui.add(
                        egui::Label::new(RichText::new(shorten(&entry.save_dir, 42)).small())
                            .truncate(),
                    );
                    let row = self.rows.entry(entry.id.clone()).or_default();
                    let status = if row.status.is_empty() {
                        "idle".to_string()
                    } else {
                        row.status.clone()
                    };
                    let colour = match status.as_str() {
                        "synced" => egui::Color32::from_rgb(130, 200, 130),
                        "syncing" => egui::Color32::from_rgb(120, 170, 230),
                        "conflict" => egui::Color32::from_rgb(230, 190, 110),
                        _ => egui::Color32::from_rgb(160, 160, 160),
                    };
                    ui.colored_label(colour, status);
                    ui.label(RichText::new(row.last_sync.clone()).small());

                    if ui.button("Sync").clicked() {
                        let row = self.rows.entry(entry.id.clone()).or_default();
                        row.status = "syncing".into();
                        let cfg = self.cfg.clone();
                        let paths = self.paths.clone();
                        let entry = entry.clone();
                        self.worker.send(Cmd::SyncGame { cfg, entry, paths });
                    }
                    if ui.button("Remove").clicked() {
                        self.remove_game(&entry.id);
                    }
                });
                ui.separator();
            }
        });
    }

    /// Remove a game from the library, and its namespace on the server.
    fn remove_game(&mut self, id: &str) {
        let Some(entry) = self.library.find(id).cloned() else {
            return;
        };
        self.library.remove(id);
        self.rows.remove(id);
        self.running.remove(id);
        if let Err(err) = self.library.save(&self.paths.library_file()) {
            self.push_log(format!("cannot save the library: {err}"), true);
        }
        self.push_log(format!("{}: removed from the library", entry.name), false);

        // The namespace on the server goes away with it.
        if let Some(game_id) = entry.server_game_id {
            let cfg = self.cfg.clone();
            self.worker.send(Cmd::DeleteGame { cfg, game_id });
        }
    }

    /// The log panel.
    fn log_panel(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .max_height(140.0)
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for line in &self.log {
                    let colour = if line.error {
                        egui::Color32::from_rgb(230, 140, 120)
                    } else {
                        egui::Color32::from_rgb(170, 170, 170)
                    };
                    ui.colored_label(
                        colour,
                        RichText::new(format!("{}  {}", line.time, line.text)).small(),
                    );
                }
            });
    }

    /// The settings window.
    fn settings_window(&mut self, ui: &mut egui::Ui) {
        ui.label("Server");
        ui.add(egui::TextEdit::singleline(&mut self.cfg.server_url).hint_text("https://host:8443"));
        ui.label("Certificate fingerprint");
        ui.add(egui::TextEdit::singleline(&mut self.cfg.fingerprint).hint_text("AA:BB:CC:..."));
        ui.label("API key");
        ui.add(egui::TextEdit::singleline(&mut self.cfg.api_key).password(true));

        ui.add_space(8.0);
        ui.label("Device");
        ui.add(egui::TextEdit::singleline(&mut self.cfg.device_name));
        ui.label("Poll interval (seconds)");
        let mut interval = self.cfg.poll_interval_secs as f64;
        if ui
            .add(egui::Slider::new(&mut interval, 1.0..=60.0).text("seconds"))
            .changed()
        {
            self.cfg.poll_interval_secs = interval as u64;
        }
        ui.checkbox(
            &mut self.cfg.auto_sync,
            "Synchronise when a game session ends",
        );
        ui.checkbox(
            &mut self.cfg.delete_missing,
            "Remove from the server the files the game deletes",
        );
        ui.checkbox(&mut self.cfg.autostart, "Start with the session");
        ui.label(session::support_label());

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Save").clicked() {
                self.save_settings();
                self.show_settings = false;
            }
            if ui.button("Close").clicked() {
                self.show_settings = false;
            }
        });
    }

    /// Save the settings and reconnect.
    fn save_settings(&mut self) {
        if let Err(err) = self.cfg.save(&self.paths.config_file()) {
            self.push_log(format!("cannot save the settings: {err}"), true);
            return;
        }
        self.push_log("settings saved".into(), false);

        // The autostart entry follows the setting.
        match autostart::set_enabled(self.cfg.autostart) {
            Ok(()) => {
                let state = if self.cfg.autostart {
                    "enabled"
                } else {
                    "disabled"
                };
                self.push_log(format!("starting with the session: {state}"), false);
            }
            Err(err) => {
                self.push_log(format!("cannot change the autostart entry: {err}"), true);
            }
        }

        self.connect();
    }

    /// The "add a game" window.
    fn add_window(&mut self, ui: &mut egui::Ui) {
        ui.label("Name");
        ui.add(egui::TextEdit::singleline(&mut self.draft.name).hint_text("Hollow Knight"));

        ui.label("Executable");
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.draft.exe).hint_text("/games/hk/hk"));
            if ui.button("Browse").clicked() {
                if let Some(path) = rfd::FileDialog::new().pick_file() {
                    self.draft.exe = path.to_string_lossy().to_string();
                    if self.draft.name.is_empty() {
                        if let Some(name) = path.file_stem() {
                            self.draft.name = name.to_string_lossy().to_string();
                        }
                    }
                }
            }
        });

        ui.label("Save folder");
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.draft.save_dir)
                    .hint_text("~/.local/share/Hollow Knight"),
            );
            if ui.button("Browse").clicked() {
                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                    self.draft.save_dir = path.to_string_lossy().to_string();
                    if self.draft.name.is_empty() {
                        if let Some(name) = path.file_name() {
                            self.draft.name = name.to_string_lossy().to_string();
                        }
                    }
                }
            }
        });

        ui.label("Sub folders to synchronise (comma separated, empty means all)");
        ui.add(egui::TextEdit::singleline(&mut self.draft.sync_paths).hint_text("saves"));
        ui.label("Exclusions (comma separated glob patterns)");
        ui.add(egui::TextEdit::singleline(&mut self.draft.excludes).hint_text("*.tmp,*.lock"));

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Add").clicked() {
                self.add_game();
                self.show_add = false;
            }
            if ui.button("Cancel").clicked() {
                self.show_add = false;
            }
        });
    }
    /// Register a new game in the library.
    fn add_game(&mut self) {
        let name = self.draft.name.trim().to_string();
        if name.is_empty() || self.draft.save_dir.trim().is_empty() {
            self.push_log("a game needs a name and a save folder".into(), true);
            return;
        }
        let id = format!("g{}", self.next_id);
        self.next_id += 1;

        let entry = GameEntry {
            id: id.clone(),
            name,
            exe: self.draft.exe.trim().to_string(),
            save_dir: self.draft.save_dir.trim().to_string(),
            sync_paths: split_list(&self.draft.sync_paths),
            excludes: split_list(&self.draft.excludes),
            server_game_id: None,
            added_at: now_string(),
        };
        let name = entry.name.clone();
        self.library.games.push(entry);
        if let Err(err) = self.library.save(&self.paths.library_file()) {
            self.push_log(format!("cannot save the library: {err}"), true);
            return;
        }
        self.push_log(format!("{name}: added to the library"), false);
    }

    /// The conflict dialog.
    fn conflict_window(&mut self, ctx: &egui::Context) {
        let mut open = true;
        egui::Window::new("Conflicts")
            .open(&mut open)
            .resizable(false)
            .show(&ctx.clone(), |ui| {
                if self.pending.is_empty() {
                    ui.label("No conflicts.");
                    return;
                }
                let pending = self.pending.clone();
                for item in pending {
                    ui.label(
                        RichText::new(format!("{} — {}", item.game_name, item.conflict.path))
                            .strong(),
                    );
                    ui.label(format!(
                        "local: {} bytes, sha {}",
                        item.conflict.local_size,
                        &item.conflict.local_sha256[..12.min(item.conflict.local_sha256.len())]
                    ));
                    ui.label(format!(
                        "cloud: {} bytes, sha {}, version {}, updated {}",
                        item.conflict.cloud_size,
                        &item.conflict.cloud_sha256[..12.min(item.conflict.cloud_sha256.len())],
                        item.conflict.cloud_version,
                        item.conflict.cloud_updated_at
                    ));
                    ui.horizontal(|ui| {
                        if ui.button("Keep the cloud version").clicked() {
                            self.resolve(&item, Resolution::KeepCloud);
                        }
                        if ui.button("Keep the local version").clicked() {
                            self.resolve(&item, Resolution::KeepLocal);
                        }
                        if ui.button("Keep both").clicked() {
                            self.resolve(&item, Resolution::KeepBoth);
                        }
                    });
                    ui.separator();
                }
            });
        if !open {
            self.pending.clear();
        }
    }

    /// Send the decision about a conflict to the worker.
    fn resolve(&mut self, item: &PendingConflict, resolution: Resolution) {
        let Some(entry) = self.library.find(&item.game_id).cloned() else {
            return;
        };
        self.pending
            .retain(|p| !(p.game_id == item.game_id && p.conflict.path == item.conflict.path));
        let cfg = self.cfg.clone();
        let paths = self.paths.clone();
        let conflict = item.conflict.clone();
        self.worker.send(Cmd::Resolve {
            cfg,
            entry,
            conflict,
            resolution,
            paths,
        });
    }
}

impl eframe::App for GameSaveApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // egui only draws when something asks it to. The interface has to
        // keep waking up, or the events coming from the worker would sit
        // unread until the user happens to click something.
        ui.ctx().request_repaint_after(Duration::from_millis(250));

        self.drain_events();
        self.drain_tray(ui.ctx());
        self.poll_sessions(ui.ctx());

        let ctx = ui.ctx().clone();

        egui::Panel::top("top")
            .frame(egui::Frame::default().inner_margin(12.0))
            .show(ui, |ui| self.top_bar(ui));

        egui::Panel::bottom("log")
            .frame(egui::Frame::default().inner_margin(8.0))
            .show(ui, |ui| self.log_panel(ui));

        egui::CentralPanel::default().show(ui, |ui| self.library_table(ui));

        if self.show_settings {
            egui::Window::new("Settings").show(&ctx, |ui| self.settings_window(ui));
        }
        if self.show_add {
            egui::Window::new("Add a game").show(&ctx, |ui| self.add_window(ui));
        }
        if !self.pending.is_empty() {
            self.conflict_window(&ctx);
        }
    }
}

/// Split a comma separated list into trimmed, non empty items.
fn split_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Shorten a path for the table.
fn shorten(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    let chars: Vec<char> = value.chars().collect();
    let tail: String = chars[chars.len() - (max - 3)..].iter().collect();
    format!("...{tail}")
}
