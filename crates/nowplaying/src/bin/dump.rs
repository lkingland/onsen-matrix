//! Prints every player as things change, best first, and the shown
//! player's position once a second: the library against a real bus.
use std::time::Duration;

fn clock(d: Duration) -> String {
    let s = d.as_secs();
    format!("{}:{:02}", s / 60, s % 60)
}

fn main() {
    let watcher = match nowplaying::Watcher::start() {
        Ok(w) => w,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let mut seen = None;
    loop {
        let generation = watcher.generation();
        if seen != Some(generation) {
            seen = Some(generation);
            let players = watcher.players();
            if players.is_empty() {
                println!("no player");
            }
            for (i, s) in players.iter().enumerate() {
                let track = s.track.clone().unwrap_or_default();
                let art = track
                    .art_path()
                    .map(|p| p.display().to_string())
                    .unwrap_or(track.art_url);
                println!(
                    "{} [{}] {} | {} - {} | art={art} | next={} prev={} play={} pause={}",
                    if i == 0 { "*" } else { " " },
                    s.identity,
                    s.playback,
                    track.artist,
                    track.title,
                    s.can_go_next,
                    s.can_go_previous,
                    s.can_play,
                    s.can_pause,
                );
            }
        }
        if let Some(s) = watcher.status().filter(|s| s.playing()) {
            let length = s
                .track
                .as_ref()
                .and_then(|t| t.length)
                .map(clock)
                .unwrap_or_else(|| "?".into());
            println!("  {} / {length}", clock(s.position()));
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}
