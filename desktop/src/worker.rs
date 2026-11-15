//! The background worker: network and disk work happens here, never in the
//! interface thread.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;

use crate::api::ApiClient;
use crate::config::{Config, GameEntry, GameState, Paths};
use crate::sync::{self, Conflict, Resolution};

/// A command sent to the worker.
pub enum Cmd {
    /// Connect to the server and read the account information.
    Connect { cfg: Config },
    /// Synchronise every game of the library.
    SyncAll {
        cfg: Config,
        games: Vec<GameEntry>,
        paths: Paths,
    },
    /// Synchronise one game.
    SyncGame {
        cfg: Config,
        entry: GameEntry,
        paths: Paths,
    },
    /// Apply the decision taken about a conflicting file.
    Resolve {
        cfg: Config,
        entry: GameEntry,
        conflict: Conflict,
        resolution: Resolution,
        paths: Paths,
    },
    /// Delete the namespace of a game on the server.
    DeleteGame { cfg: Config, game_id: String },
    /// Read the icon of the games.
    Icons { games: Vec<GameEntry>, paths: Paths },
}

/// An event produced by the worker.
#[derive(Debug)]
pub enum Ev {
    /// A line for the log.
    Log(String),
    /// The connection is established.
    Connected(String),
    /// Something failed.
    Failed(String),
    /// A game finished synchronising.
    Synced { id: String, summary: String },
    /// A game has files that need a decision.
    Conflicts {
        id: String,
        conflicts: Vec<Conflict>,
    },
    /// The icon of a game was read.
    Icon {
        id: String,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    },
}

/// The worker handle used by the interface.
pub struct Worker {
    tx: Sender<Cmd>,
    rx: Receiver<Ev>,
}

impl Worker {
    /// Start the worker thread.
    pub fn start() -> Self {
        let (cmd_tx, cmd_rx) = channel::<Cmd>();
        let (ev_tx, ev_rx) = channel::<Ev>();

        thread::Builder::new()
            .name("gamesave-worker".into())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(_) => return,
                };

                while let Ok(cmd) = cmd_rx.recv() {
                    runtime.block_on(handle(cmd, &ev_tx));
                }
            })
            .expect("starting the worker thread");

        Self {
            tx: cmd_tx,
            rx: ev_rx,
        }
    }

    /// Queue a command.
    pub fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
    }

    /// Take every event produced so far.
    pub fn drain(&self) -> Vec<Ev> {
        let mut events = Vec::new();
        while let Ok(event) = self.rx.try_recv() {
            events.push(event);
        }
        events
    }
}

/// Execute one command.
async fn handle(cmd: Cmd, events: &Sender<Ev>) {
    let emit = |text: String| {
        let _ = events.send(Ev::Log(text));
    };

    match cmd {
        Cmd::Connect { cfg } => match ApiClient::connect(&cfg).await {
            Ok(api) => match api.me().await {
                Ok(me) => {
                    let _ = events.send(Ev::Connected(format!(
                        "connected to {} as {} (key {})",
                        api.base(),
                        me.user.username,
                        me.key.label
                    )));
                    match api.list_games().await {
                        Ok(games) => {
                            let _ = events
                                .send(Ev::Log(format!("{} namespaces on the server", games.len())));
                        }
                        Err(err) => {
                            let _ = events.send(Ev::Log(format!("cannot list the games: {err}")));
                        }
                    }
                }
                Err(err) => {
                    let _ = events.send(Ev::Failed(format!(
                        "the server rejected the API key: {err}"
                    )));
                }
            },
            Err(err) => {
                let _ = events.send(Ev::Failed(format!("{err}")));
            }
        },

        Cmd::SyncAll { cfg, games, paths } => {
            let api = match ApiClient::connect(&cfg).await {
                Ok(api) => api,
                Err(err) => {
                    let _ = events.send(Ev::Failed(format!("{err}")));
                    return;
                }
            };
            for entry in games {
                run_one(&api, &cfg, &entry, &paths, events).await;
            }
        }

        Cmd::SyncGame { cfg, entry, paths } => {
            let api = match ApiClient::connect(&cfg).await {
                Ok(api) => api,
                Err(err) => {
                    let _ = events.send(Ev::Failed(format!("{err}")));
                    return;
                }
            };
            run_one(&api, &cfg, &entry, &paths, events).await;
        }

        Cmd::Resolve {
            cfg,
            entry,
            conflict,
            resolution,
            paths,
        } => {
            let api = match ApiClient::connect(&cfg).await {
                Ok(api) => api,
                Err(err) => {
                    let _ = events.send(Ev::Failed(format!("{err}")));
                    return;
                }
            };
            let mut state = match GameState::load(&paths.state_file(&entry.slug())) {
                Ok(state) => state,
                Err(err) => {
                    let _ = events.send(Ev::Failed(format!("{err}")));
                    return;
                }
            };
            match sync::resolve_conflict(
                &api, &entry, &mut state, &paths, &cfg, &conflict, resolution,
            )
            .await
            {
                Ok(outcome) => {
                    emit(outcome.summary());
                    for warning in outcome.warnings {
                        emit(warning);
                    }
                    if let Err(err) = state.save(&paths.state_file(&entry.slug())) {
                        let _ = events.send(Ev::Failed(format!("{err}")));
                    }
                }
                Err(err) => {
                    let _ = events.send(Ev::Failed(format!("{err}")));
                }
            }
        }

        Cmd::Icons { games, paths } => {
            for entry in games {
                // An icon the user pointed at wins over the one of the
                // executable.
                let icon = if !entry.icon.trim().is_empty() {
                    crate::icons::from_file(std::path::Path::new(&entry.icon), 128)
                } else {
                    let cache = crate::icons::cache_path(&paths.data_dir, &entry.slug());
                    crate::icons::load(std::path::Path::new(&entry.exe), &cache, 128)
                };
                if let Some(icon) = icon {
                    let _ = events.send(Ev::Icon {
                        id: entry.id.clone(),
                        width: icon.width,
                        height: icon.height,
                        rgba: icon.rgba,
                    });
                }
            }
        }

        Cmd::DeleteGame { cfg, game_id } => {
            let api = match ApiClient::connect(&cfg).await {
                Ok(api) => api,
                Err(err) => {
                    let _ = events.send(Ev::Failed(format!("{err}")));
                    return;
                }
            };
            match api.delete_game(&game_id).await {
                Ok(()) => {
                    let _ = events.send(Ev::Log(format!("the namespace {game_id} was removed")));
                }
                Err(err) => {
                    let _ = events.send(Ev::Failed(format!("cannot remove {game_id}: {err}")));
                }
            }
        }
    }
}

/// Synchronise one game and report the result.
async fn run_one(
    api: &ApiClient,
    cfg: &Config,
    entry: &GameEntry,
    paths: &Paths,
    events: &Sender<Ev>,
) {
    let mut state = match GameState::load(&paths.state_file(&entry.slug())) {
        Ok(state) => state,
        Err(err) => {
            let _ = events.send(Ev::Failed(format!("{}: {err}", entry.name)));
            return;
        }
    };

    match sync::sync_game(api, entry, &mut state, paths, cfg).await {
        Ok(outcome) => {
            let _ = events.send(Ev::Synced {
                id: entry.id.clone(),
                summary: outcome.summary(),
            });
            if outcome.is_empty() {
                let _ = events.send(Ev::Log(format!("{}: nothing to do", entry.name)));
            }
            for warning in outcome.warnings {
                let _ = events.send(Ev::Log(warning));
            }
            if let Err(err) = state.save(&paths.state_file(&entry.slug())) {
                let _ = events.send(Ev::Failed(format!("{err}")));
            }
            if !outcome.conflicts.is_empty() {
                let _ = events.send(Ev::Conflicts {
                    id: entry.id.clone(),
                    conflicts: outcome.conflicts,
                });
            }
        }
        Err(err) => {
            let _ = events.send(Ev::Failed(format!("{}: {err}", entry.name)));
        }
    }
}
