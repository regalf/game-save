//! The graphical interface: the library of games, the settings and the
//! conflict dialog.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::time::{Duration, Instant};

use egui::{Align, Layout, RichText, Stroke, TextureHandle};

use crate::autostart;
use crate::config::{now_string, Config, GameEntry, Library, Paths};
use crate::overlay::{self, Overlay};
use crate::session;
use crate::sync::{Conflict, Resolution};
use crate::theme::{palette, size};
use crate::tray::{Tray, TrayEvent};
use crate::widgets;
use crate::worker::{Cmd, Ev, Worker};

/// One line of the log.
struct LogLine {
    #[allow(dead_code)]
    time: String,
    text: String,
    error: bool,
}

/// The icon of a game, with the size it has in the executable.
struct GameIcon {
    texture: TextureHandle,
    width: u32,
    height: u32,
}

/// The side of the square the icon sits in.
const ICON_BOX: f32 = 140.0;

/// The margin inside that square.
const ICON_PADDING: f32 = 6.0;

/// The height of every card of the grid.
const CARD_HEIGHT: f32 = 280.0;

/// The state of one game, as shown in the card.
#[derive(Default, Clone)]
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
    icon: String,
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
    settings_overlay: Overlay,
    add_overlay: Overlay,
    conflict_overlay: Overlay,
    draft: Draft,
    /// The game being edited, if any.
    editing: Option<String>,
    /// What is wrong with the draft, shown in red.
    draft_error: String,
    /// The icons of the games, once they have been read.
    icons: HashMap<String, GameIcon>,
    status: String,
    status_ok: bool,
    last_poll: Instant,
    last_attempt: Instant,
    next_id: u64,
    /// Set when the user asked to quit from the tray: the window is then
    /// really allowed to close.
    quitting: bool,
    tray: Option<Tray>,
}

impl GameSaveApp {
    /// Build the application from what is on disk.
    pub fn new(ctx: &egui::Context, paths: Paths, cfg: Config, library: Library) -> Self {
        // The fonts and the colours go in before anything is drawn.
        crate::theme::install_fonts(ctx);
        let dark = cfg.theme.trim().eq_ignore_ascii_case("dark");
        crate::theme::apply(ctx, dark);

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
            settings_overlay: Overlay::new(),
            add_overlay: Overlay::new(),
            conflict_overlay: Overlay::new(),
            draft: Draft::default(),
            editing: None,
            draft_error: String::new(),
            icons: HashMap::new(),
            status: "not connected".to_string(),
            status_ok: false,
            last_poll: Instant::now(),
            last_attempt: Instant::now(),
            next_id: 1,
            quitting: false,
            tray: Tray::new(),
        };
        app.push_log("Game Save started".into(), false);
        app.connect();
        app.load_icons();
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
    fn drain_events(&mut self, ctx: &egui::Context) {
        for event in self.worker.drain() {
            match event {
                Ev::Log(text) => self.push_log(text, false),
                Ev::Icon {
                    id,
                    width,
                    height,
                    rgba,
                } => {
                    let image = egui::ColorImage::from_rgba_unmultiplied(
                        [width as usize, height as usize],
                        &rgba,
                    );
                    let texture =
                        ctx.load_texture(format!("icon-{id}"), image, egui::TextureOptions::LINEAR);
                    self.icons.insert(
                        id,
                        GameIcon {
                            texture,
                            width,
                            height,
                        },
                    );
                }
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
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                TrayEvent::Quit => {
                    // The only close that really quits is the one asked here.
                    self.quitting = true;
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

    /// The bar on top: the name of the application and the actions.
    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("GameSave");
            ui.separator();

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if widgets::button(ui, "Settings") {
                    self.show_settings = true;
                }
                if widgets::button(ui, "Add game") {
                    self.open_add(None);
                }
                if widgets::button(ui, "Sync all") {
                    self.sync_all();
                }
            });
        });
    }

    /// The library: the games in a grid, and the card that adds one more.
    fn library_grid(&mut self, ui: &mut egui::Ui) {
        let games: Vec<GameEntry> = self.library.games.clone();
        let card = 250.0;
        let gap = 12.0;
        let available = ui.available_width();
        let per_row = ((available + gap) / (card + gap)).floor().max(1.0) as usize;

        // The card that adds a game is always the last one of the grid.
        let mut cells: Vec<Option<GameEntry>> = games.into_iter().map(Some).collect();
        cells.push(None);

        // The columns are stacked, so the cards fill the grid row by row.
        ui.columns(per_row, |columns| {
            for (index, cell) in cells.iter().enumerate() {
                let column = &mut columns[index % per_row];
                match cell {
                    Some(entry) => self.game_card(column, entry),
                    None => self.add_card(column),
                }
            }
        });
    }

    /// The card of one game.
    fn game_card(&mut self, ui: &mut egui::Ui, entry: &GameEntry) {
        let row = self.rows.get(&entry.id).cloned().unwrap_or_default();
        let border = palette::dark::BORDER;

        egui::Frame::new()
            .fill(palette::dark::SURFACE)
            .stroke(Stroke::new(1.0, border))
            .inner_margin(10.0)
            .show(ui, |ui| {
                // The width of the card, without the margins.
                let inner = ui.available_width();
                ui.set_width(inner);

                // The name and the last synchronisation.
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(&entry.name)
                            .color(palette::dark::TEXT_MUTED)
                            .size(size::BODY),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let last = if row.last_sync.is_empty() {
                            "never".to_string()
                        } else {
                            row.last_sync.clone()
                        };
                        ui.label(
                            RichText::new(format!("Last sync: {last}"))
                                .color(palette::dark::TEXT_FAINT)
                                .size(size::TINY),
                        );
                    });
                });
                ui.separator();

                // The icon of the game, in a square box in the middle of the
                // card. The box has a size of its own, so it is a square
                // whatever the icon looks like.
                let icon = self
                    .icons
                    .get(&entry.id)
                    .map(|icon| (icon.texture.id(), icon.width, icon.height));
                let side = ICON_BOX;
                let slot = ui.allocate_exact_size(egui::vec2(inner, side), egui::Sense::hover());
                let box_rect =
                    egui::Rect::from_center_size(slot.0.center(), egui::vec2(side, side));
                ui.painter().rect(
                    box_rect,
                    0.0,
                    palette::dark::SURFACE,
                    Stroke::new(1.0, border),
                    egui::StrokeKind::Middle,
                );

                match icon {
                    Some((texture, width, height)) => {
                        // The icon keeps its proportions and fits in the box.
                        let room = side - 2.0 * ICON_PADDING;
                        let ratio = width as f32 / height as f32;
                        let (w, h) = if ratio >= 1.0 {
                            (room, room / ratio)
                        } else {
                            (room * ratio, room)
                        };
                        let image_rect =
                            egui::Rect::from_center_size(box_rect.center(), egui::vec2(w, h));
                        ui.painter().image(
                            texture,
                            image_rect,
                            egui::Rect::from_min_size(egui::epaint::WHITE_UV, egui::vec2(1.0, 1.0)),
                            egui::Color32::WHITE,
                        );
                    }
                    None => {
                        ui.painter().text(
                            box_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "(no icon)",
                            egui::FontId::proportional(size::SMALL),
                            palette::dark::TEXT_FAINT,
                        );
                    }
                }

                ui.add_space(4.0);

                if widgets::sized_button(ui, "Sync", inner) {
                    self.sync_game(entry);
                }

                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let half = (inner - 8.0) / 2.0;
                    if widgets::sized_danger_button(ui, "Delete", half) {
                        self.remove_game(&entry.id);
                    }
                    if widgets::sized_button(ui, "Edit", half) {
                        self.open_add(Some(entry));
                    }
                });
            });
    }

    /// The card that adds a game: the last one of the grid.
    fn add_card(&mut self, ui: &mut egui::Ui) {
        let width = ui.available_width();
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(width, CARD_HEIGHT), egui::Sense::click());

        if ui.is_rect_visible(rect) {
            ui.painter().rect(
                rect,
                0.0,
                palette::dark::SURFACE,
                Stroke::new(1.0, palette::dark::BORDER),
                egui::StrokeKind::Middle,
            );

            let centre = rect.center();
            ui.painter().text(
                centre - egui::vec2(0.0, 16.0),
                egui::Align2::CENTER_CENTER,
                "+",
                egui::FontId::proportional(40.0),
                palette::dark::TEXT_FAINT,
            );
            ui.painter().text(
                centre + egui::vec2(0.0, 30.0),
                egui::Align2::CENTER_CENTER,
                "Add game",
                egui::FontId::proportional(size::BODY),
                palette::dark::TEXT_FAINT,
            );
        }

        // The whole card is a button.
        if response.clicked() {
            self.open_add(None);
        }
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

    /// Ask the worker to read the icons of the games.
    fn load_icons(&mut self) {
        let games = self.library.games.clone();
        let paths = self.paths.clone();
        self.worker.send(Cmd::Icons { games, paths });
    }

    /// Queue a synchronisation of one game.
    fn sync_game(&mut self, entry: &GameEntry) {
        let row = self.rows.entry(entry.id.clone()).or_default();
        row.status = "syncing".into();
        let cfg = self.cfg.clone();
        let paths = self.paths.clone();
        let entry = entry.clone();
        self.worker.send(Cmd::SyncGame { cfg, entry, paths });
    }

    /// Open the panel that adds a game, or that edits one.
    ///
    /// With a game, the panel is filled with what is already known.
    fn open_add(&mut self, entry: Option<&GameEntry>) {
        match entry {
            Some(entry) => {
                self.draft = Draft {
                    name: entry.name.clone(),
                    exe: entry.exe.clone(),
                    save_dir: entry.save_dir.clone(),
                    sync_paths: entry.sync_paths.join(", "),
                    excludes: entry.excludes.join(", "),
                    icon: String::new(),
                };
                self.editing = Some(entry.id.clone());
            }
            None => {
                self.draft = Draft::default();
                self.editing = None;
            }
        }
        self.draft_error.clear();
        self.show_add = true;
    }

    /// The bar at the bottom: the state of the connection.
    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let colour = if self.status_ok {
                palette::dark::TEXT_MUTED
            } else {
                palette::dark::DANGER
            };
            ui.label(RichText::new(&self.status).color(colour).size(size::SMALL));

            if let Some(line) = self.log.back() {
                let colour = if line.error {
                    palette::dark::DANGER
                } else {
                    palette::dark::TEXT_FAINT
                };
                ui.label(RichText::new(&line.text).color(colour).size(size::SMALL));
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if !self.status_ok && widgets::button(ui, "Reconnect") {
                    self.connect();
                }
            });
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

    /// The settings window.
    fn settings_window(&mut self, ui: &mut egui::Ui) {
        if overlay::header(ui, "Settings") {
            self.show_settings = false;
            return;
        }

        ui.label("Server IP");
        ui.add(
            egui::TextEdit::singleline(&mut self.cfg.server_url)
                .hint_text("https://127.0.0.1:8443"),
        );
        ui.label("Certificate fingerprint");
        ui.add(egui::TextEdit::singleline(&mut self.cfg.fingerprint).hint_text("AA:BB:CC:..."));
        ui.label("API key");
        ui.add(egui::TextEdit::singleline(&mut self.cfg.api_key).hint_text("Type here ..."));

        ui.add_space(8.0);
        ui.label("Device");
        ui.add(egui::TextEdit::singleline(&mut self.cfg.device_name).hint_text("Type here ..."));
        ui.horizontal(|ui| {
            ui.label("Poll interval");
            let mut interval = self.cfg.poll_interval_secs as f64;
            ui.add(
                egui::DragValue::new(&mut interval)
                    .range(0.5..=60.0)
                    .speed(0.2)
                    .prefix(""),
            );
            ui.label("seconds");
            if interval >= 0.5 {
                self.cfg.poll_interval_secs = interval as u64;
            }
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            widgets::check_box(ui, &mut self.cfg.auto_sync);
            ui.label("Synchronise when game close");
        });
        ui.horizontal(|ui| {
            widgets::check_box(ui, &mut self.cfg.delete_missing);
            ui.label("Remove from server the deleted game files");
        });
        ui.horizontal(|ui| {
            widgets::check_box(ui, &mut self.cfg.autostart);
            ui.label("Start with session");
        });
        ui.horizontal(|ui| {
            widgets::check_box(ui, &mut self.cfg.enable_logs);
            ui.label("Enable logs");
        });
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if widgets::sized_danger_button(ui, "Default", 110.0) {
                let theme = self.cfg.theme.clone();
                self.cfg = Config::default();
                self.cfg.theme = theme;
                crate::theme::apply(ui.ctx(), self.cfg.theme.trim().eq_ignore_ascii_case("dark"));
                self.push_log("the settings are back to the default".into(), false);
            }
            if widgets::button(ui, "Save") {
                self.save_settings();
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
        let title = if self.editing.is_some() {
            "Edit a game"
        } else {
            "Add a game"
        };
        if overlay::header(ui, title) {
            self.show_add = false;
            return;
        }

        ui.label("Name");
        ui.add(egui::TextEdit::singleline(&mut self.draft.name).hint_text("Super Cool Game"));

        ui.label("Executable");
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.draft.exe).hint_text("/SCG/bin/SCG.exe"));
            if widgets::button(ui, "Browse") {
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

        ui.label("Save Folder");
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.draft.save_dir).hint_text("/SCG/saves/"));
            if widgets::button(ui, "Browse") {
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

        ui.label("SubFolder to sync (Comma separated empty = all)");
        ui.add(egui::TextEdit::singleline(&mut self.draft.sync_paths).hint_text("saveslot0"));

        ui.label("Exclusion (Comma separated glob patterns)");
        ui.add(egui::TextEdit::singleline(&mut self.draft.excludes).hint_text("saveslot0"));

        ui.label("Game icon (Optional)");
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.draft.icon).hint_text("/SCG/icon.ico/"));
            if widgets::button(ui, "Browse") {
                if let Some(path) = rfd::FileDialog::new().pick_file() {
                    self.draft.icon = path.to_string_lossy().to_string();
                }
            }
        });

        // What is wrong, if anything.
        if !self.draft_error.is_empty() {
            ui.add_space(8.0);
            ui.label(
                RichText::new(&self.draft_error)
                    .color(palette::dark::DANGER)
                    .size(size::BODY),
            );
        }

        ui.add_space(12.0);
        let width = ui.available_width();
        if widgets::sized_button(ui, "Save", width) && self.add_game() {
            self.show_add = false;
        }
    }
    /// Register a new game in the library, or change one that is already there.
    ///
    /// It answers whether it worked: when it did not, the reason is in the
    /// error of the panel.
    fn add_game(&mut self) -> bool {
        let name = self.draft.name.trim().to_string();
        if name.is_empty() {
            self.draft_error = "a game needs a name".to_string();
            return false;
        }
        if self.draft.save_dir.trim().is_empty() {
            self.draft_error = "a game needs a save folder".to_string();
            return false;
        }
        self.draft_error.clear();

        let fields = |draft: &Draft| GameEntry {
            id: String::new(),
            name: name.clone(),
            exe: draft.exe.trim().to_string(),
            save_dir: draft.save_dir.trim().to_string(),
            sync_paths: split_list(&draft.sync_paths),
            excludes: split_list(&draft.excludes),
            icon: draft.icon.trim().to_string(),
            server_game_id: None,
            added_at: now_string(),
        };

        match self.editing.clone() {
            // Change a game that is already in the library.
            Some(id) => {
                let Some(entry) = self.library.games.iter_mut().find(|g| g.id == id) else {
                    return false;
                };
                let mut updated = fields(&self.draft);
                updated.id = entry.id.clone();
                updated.server_game_id = entry.server_game_id.clone();
                updated.added_at = entry.added_at.clone();
                *entry = updated;
                self.icons.remove(&id);
            }
            // Add a new one.
            None => {
                let id = format!("g{}", self.next_id);
                self.next_id += 1;
                let mut entry = fields(&self.draft);
                entry.id = id;
                self.library.games.push(entry);
            }
        }

        let name = self.draft.name.trim().to_string();
        if let Err(err) = self.library.save(&self.paths.library_file()) {
            self.draft_error = format!("cannot save the library: {err}");
            return false;
        }
        self.push_log(format!("{name}: saved in the library"), false);
        self.load_icons();
        true
    }

    /// The conflict dialog.
    fn conflict_window(&mut self, ctx: &egui::Context) {
        let mut visible = !self.pending.is_empty();
        // The overlay is taken out of self for the call: the content needs
        // self, and the overlay needs to be mutable at the same time.
        let mut overlay = std::mem::take(&mut self.conflict_overlay);
        overlay.show(ctx, "conflicts", &mut visible, |ui| {
            let pending = self.pending.clone();
            for item in pending {
                ui.label(
                    RichText::new(format!("{} — {}", item.game_name, item.conflict.path))
                        .color(palette::dark::TEXT),
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
        self.conflict_overlay = overlay;
        if !visible {
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
    /// The background tick.
    ///
    /// eframe calls this both when the window is shown and when it is hidden,
    /// so the session watcher and the worker keep running when the app sits in
    /// the tray.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_events(ctx);
        self.drain_tray(ctx);
        self.poll_sessions(ctx);

        // The X button asks to close the window. Instead of quitting, the app
        // hides into the tray and keeps synchronising. The only close that
        // really quits is the one asked from the tray menu.
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            crate::notify::hidden_to_tray();
            self.push_log("hidden in the tray, Game Save keeps running".into(), false);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // egui only draws when something asks it to. The interface has to
        // keep waking up, or the events coming from the worker would sit
        // unread until the user happens to click something.
        ui.ctx().request_repaint_after(Duration::from_millis(250));

        let ctx = ui.ctx().clone();

        egui::Panel::top("top")
            .frame(
                egui::Frame::new()
                    .fill(palette::dark::BACKGROUND)
                    .inner_margin(10.0),
            )
            .show(ui, |ui| self.top_bar(ui));

        egui::Panel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(palette::dark::BACKGROUND)
                    .inner_margin(10.0),
            )
            .show(ui, |ui| self.status_bar(ui));

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(palette::dark::BACKGROUND)
                    .inner_margin(10.0),
            )
            .show(ui, |ui| self.library_grid(ui));

        // The panels that come in the middle of the screen.
        {
            let mut visible = self.show_settings;
            let mut overlay = std::mem::take(&mut self.settings_overlay);
            overlay.show(&ctx, "settings", &mut visible, |ui| {
                self.settings_window(ui)
            });
            self.settings_overlay = overlay;
            if !visible {
                self.show_settings = false;
            }
        }
        {
            let mut visible = self.show_add;
            let mut overlay = std::mem::take(&mut self.add_overlay);
            overlay.show(&ctx, "add-game", &mut visible, |ui| self.add_window(ui));
            self.add_overlay = overlay;
            if !visible {
                self.show_add = false;
            }
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
