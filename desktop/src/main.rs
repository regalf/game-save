//! The Game Save companion application.
//!
//! It watches the games registered in the library and keeps their saves in
//! sync with a self hosted Game Save server. The save folder of a game is
//! only ever read, never written.

use eframe::egui::ViewportBuilder;
use gamesave::{app::GameSaveApp, config};

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let paths = match config::Paths::discover() {
        Ok(paths) => paths,
        Err(err) => {
            eprintln!("Game Save cannot find its directories: {err}");
            std::process::exit(1);
        }
    };
    if let Err(err) = paths.ensure() {
        eprintln!("Game Save cannot create its directories: {err}");
        std::process::exit(1);
    }

    let cfg = config::Config::load(&paths.config_file()).unwrap_or_default();
    let library = config::Library::load(&paths.library_file()).unwrap_or_default();

    let options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_inner_size([1100.0, 680.0])
            .with_min_inner_size([820.0, 520.0]),
        ..Default::default()
    };

    let result = eframe::run_native(
        "Game Save",
        options,
        Box::new(move |_cc| Ok(Box::new(GameSaveApp::new(paths, cfg, library)))),
    );
    if let Err(err) = result {
        eprintln!("Game Save stopped: {err}");
        std::process::exit(1);
    }
}
