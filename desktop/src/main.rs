//! The Game Save companion application.
//!
//! It watches the games registered in the library and keeps their saves in
//! sync with a self hosted Game Save server. The save folder of a game is
//! only ever read, never written.

// On Windows a program built for the console subsystem opens a black window
// next to the interface. The release build is a graphical program, the debug
// build keeps the console because the logs are useful there.
//
// The attribute is only applied on Windows, so the other platforms do not
// complain about an attribute they cannot use.
#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

use eframe::egui::ViewportBuilder;
use gamesave::{app::GameSaveApp, config};

/// The log file is truncated when it grows past this size.
const MAX_LOG_BYTES: u64 = 1024 * 1024;

fn main() {
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

    init_logging(&paths);

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
        Box::new(move |cc| {
            Ok(Box::new(GameSaveApp::new(
                &cc.egui_ctx,
                paths,
                cfg,
                library,
            )))
        }),
    );
    if let Err(err) = result {
        eprintln!("Game Save stopped: {err}");
        std::process::exit(1);
    }
}

/// Set the logging up.
///
/// The interface has a log panel of its own for the everyday use. This is for
/// the rest: a release build on Windows has no console, so the log goes to a
/// file and a panic before the window opens still leaves a trace.
fn init_logging(paths: &config::Paths) {
    let mut builder =
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"));

    if cfg!(all(target_os = "windows", not(debug_assertions))) {
        let path = paths.data_dir.join("gamesave.log");
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        // Start from scratch when the file became too big.
        if let Ok(meta) = std::fs::metadata(&path) {
            if meta.len() > MAX_LOG_BYTES {
                let _ = std::fs::remove_file(&path);
            }
        }
        if let Ok(file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            builder.target(env_logger::Target::Pipe(Box::new(file)));
        }
    }

    let _ = builder.try_init();
}
