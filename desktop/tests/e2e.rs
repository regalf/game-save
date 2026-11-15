//! End to end test against a running Game Save server.
//!
//! The test is skipped unless the environment describes a server:
//!
//! ```text
//! GAMESAVE_E2E_URL         https://127.0.0.1:18443
//! GAMESAVE_E2E_KEY         gsk_...
//! GAMESAVE_E2E_FINGERPRINT AA:BB:...
//! ```

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gamesave::api::ApiClient;
use gamesave::config::{Config, GameEntry, GameState, Paths};
use gamesave::sync;

/// Read an environment variable, skipping the test when it is missing.
fn setting(name: &str) -> Option<String> {
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => Some(value),
        _ => {
            eprintln!("skipping: {name} is not set");
            None
        }
    }
}

/// Build a configuration from the environment.
fn configuration() -> Option<Config> {
    let server_url = setting("GAMESAVE_E2E_URL")?;
    let api_key = setting("GAMESAVE_E2E_KEY")?;
    let fingerprint = setting("GAMESAVE_E2E_FINGERPRINT")?;
    Some(Config {
        server_url,
        api_key,
        fingerprint,
        device_name: "e2e".to_string(),
        ..Config::default()
    })
}

/// A temporary save folder with one file in it.
fn save_folder(root: &Path) -> PathBuf {
    let dir = root.join("save");
    fs::create_dir_all(dir.join("saves")).expect("creating the save folder");
    fs::write(dir.join("saves/slot1.dat"), b"first").expect("writing the save");
    dir
}

/// A library entry pointing at the temporary save folder.
fn entry(save_dir: &Path, server_game_id: String) -> GameEntry {
    GameEntry {
        id: "e2e".to_string(),
        name: "e2e".to_string(),
        exe: String::new(),
        save_dir: save_dir.to_string_lossy().to_string(),
        sync_paths: Vec::new(),
        excludes: Vec::new(),
        server_game_id: Some(server_game_id),
        added_at: String::new(),
    }
}

/// A unique name for the namespace, so the test can run again after a
/// failure without cleaning up first.
fn unique_name(prefix: &str) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{prefix}-{now}")
}

#[tokio::test]
async fn files_are_uploaded_and_updated() {
    let Some(cfg) = configuration() else {
        return;
    };
    let api = ApiClient::connect(&cfg)
        .await
        .expect("connecting to the server");

    let tmp = tempfile::tempdir().expect("temporary directory");
    let save_dir = save_folder(tmp.path());
    let paths = Paths {
        config_dir: tmp.path().join("config"),
        data_dir: tmp.path().join("data"),
    };
    paths.ensure().expect("creating the directories");

    let game = api
        .create_game(&unique_name("e2e-upload"))
        .await
        .expect("creating the namespace");
    let entry = entry(&save_dir, game.id.clone());
    let mut state = GameState::default();

    // The first synchronisation uploads the file.
    let outcome = sync::sync_game(&api, &entry, &mut state, &paths, &cfg)
        .await
        .expect("the first synchronisation");
    assert_eq!(outcome.uploaded, vec!["saves/slot1.dat".to_string()]);
    assert!(outcome.conflicts.is_empty());
    assert_eq!(outcome.downloaded.len(), 0);

    // The server knows about it.
    let manifest = api.manifest(&game.id).await.expect("the manifest");
    assert_eq!(manifest.files.len(), 1);
    assert_eq!(manifest.files[0].path, "saves/slot1.dat");
    assert_eq!(manifest.files[0].size, 5);

    // The content can be read back.
    let content = api
        .download(&game.id, "saves/slot1.dat")
        .await
        .expect("downloading");
    assert_eq!(content, b"first".to_vec());

    // A second synchronisation with nothing changed does nothing.
    let outcome = sync::sync_game(&api, &entry, &mut state, &paths, &cfg)
        .await
        .expect("the second synchronisation");
    assert!(outcome.uploaded.is_empty());
    assert!(outcome.is_empty());

    // Change the file: a different length makes sure it is noticed.
    fs::write(save_dir.join("saves/slot1.dat"), b"a longer save").expect("writing the save");
    tokio::time::sleep(Duration::from_millis(20)).await;

    let outcome = sync::sync_game(&api, &entry, &mut state, &paths, &cfg)
        .await
        .expect("the third synchronisation");
    assert_eq!(outcome.uploaded, vec!["saves/slot1.dat".to_string()]);
    assert!(outcome.conflicts.is_empty());

    let content = api
        .download(&game.id, "saves/slot1.dat")
        .await
        .expect("downloading again");
    assert_eq!(content, b"a longer save".to_vec());

    api.delete_game(&game.id).await.expect("cleaning up");
}

#[tokio::test]
async fn a_change_on_both_sides_is_a_conflict() {
    let Some(cfg) = configuration() else {
        return;
    };
    let api = ApiClient::connect(&cfg)
        .await
        .expect("connecting to the server");

    let tmp = tempfile::tempdir().expect("temporary directory");
    let save_dir = save_folder(tmp.path());
    let paths = Paths {
        config_dir: tmp.path().join("config"),
        data_dir: tmp.path().join("data"),
    };
    paths.ensure().expect("creating the directories");

    let game = api
        .create_game(&unique_name("e2e-conflict"))
        .await
        .expect("creating the namespace");
    let entry = entry(&save_dir, game.id.clone());
    let mut state = GameState::default();

    sync::sync_game(&api, &entry, &mut state, &paths, &cfg)
        .await
        .expect("the first synchronisation");

    // The local file changes.
    fs::write(save_dir.join("saves/slot1.dat"), b"local change").expect("writing the save");
    tokio::time::sleep(Duration::from_millis(20)).await;

    // And the same file changes on the server, from another device.
    api.upload(
        &game.id,
        "saves/slot1.dat",
        b"remote change",
        0,
        None,
        "other-device",
    )
    .await
    .expect("uploading from another device");

    // Now both sides changed: the engine must ask, not decide.
    let outcome = sync::sync_game(&api, &entry, &mut state, &paths, &cfg)
        .await
        .expect("the second synchronisation");
    assert_eq!(outcome.conflicts.len(), 1);
    let conflict = &outcome.conflicts[0];
    assert_eq!(conflict.path, "saves/slot1.dat");
    assert_eq!(conflict.local_size, 12);
    assert_eq!(conflict.cloud_size, 13);
    assert!(outcome.uploaded.is_empty());

    api.delete_game(&game.id).await.expect("cleaning up");
}
