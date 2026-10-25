//! Against the real session bus; run with `cargo test -- --ignored` while a
//! player is up.
use std::time::{Duration, Instant};

#[test]
#[ignore = "needs a session bus with a playing MPRIS player"]
fn reads_a_player_and_stops_cleanly() {
    let watcher = nowplaying::Watcher::start().expect("session bus");
    // `start` waits for the first scan, so the player is there already.
    let status = watcher.status().expect("a player on the bus");
    assert!(!status.bus_name.is_empty() && !status.identity.is_empty());
    let generation = watcher.generation();

    // The position moves on its own, without the generation moving.
    if status.playing() {
        let p0 = status.position();
        std::thread::sleep(Duration::from_millis(300));
        assert!(watcher.status().unwrap().position() > p0);
        assert_eq!(watcher.generation(), generation);
    }

    let t = Instant::now();
    drop(watcher);
    assert!(
        t.elapsed() < Duration::from_secs(1),
        "drop took {:?}",
        t.elapsed()
    );
}
