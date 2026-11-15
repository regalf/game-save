//! Tests of the background worker.

use std::time::{Duration, Instant};

use gamesave::config::Config;
use gamesave::worker::{Cmd, Ev, Worker};

/// The worker has to report something back, even when the server cannot be
/// reached.
#[test]
fn the_worker_reports_a_failure() {
    let worker = Worker::start();

    // An empty configuration cannot work: the fingerprint is missing.
    worker.send(Cmd::Connect {
        cfg: Config::default(),
    });

    let start = Instant::now();
    loop {
        let events = worker.drain();
        if let Some(event) = events.into_iter().next() {
            match event {
                Ev::Failed(message) => {
                    assert!(!message.is_empty());
                    return;
                }
                Ev::Connected(message) => panic!("the connection should have failed: {message}"),
                other => panic!("unexpected event: {other:?}"),
            }
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "the worker did not report anything"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}
