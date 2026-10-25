# nowplaying

What is playing right now, for a Rust program that wants to show it: the
track, the artist, the thumbnail, the position, and the controls to drive
the player. The screensaver's `src/media.rs` is its consumer; it has no
Bevy in it and could serve any program.

## How the Omarchy bar does it

Every media player that speaks MPRIS registers on the D-Bus session bus as
`org.mpris.MediaPlayer2.<name>` and publishes its state at
`/org/mpris/MediaPlayer2`: the `Metadata` dictionary (`xesam:title`,
`xesam:artist`, `xesam:album`, `mpris:artUrl`, `mpris:length`),
`PlaybackStatus`, `Position`, `Rate`, the `Can*` capability flags, and the
`PlayPause`, `Next` and `Previous` methods. Browsers do this for the page
that is playing (Brave for SoundCloud, say), Spotify and mpv do it natively.

The Omarchy bar's media service (`shell/plugins/services/media/Service.qml`
plus `MediaModel.js`) reads that interface through Quickshell's Mpris
service and adds one rule for choosing among several players: the one that
is playing, and if more than one is, the one that started playing first;
failing that, one with a track loaded; failing that, one that can be
controlled. This crate is the same thing in Rust, with the same rule.

Brave's thumbnails arrive as a `file://` URL naming a 150x150 PNG it writes
under `/tmp`, a new file per track. Spotify's are `https://` URLs.

## Design

Nothing is polled. `Watcher::start` connects and starts a worker thread
with a single-threaded executor. One task subscribes to the bus's
`NameOwnerChanged` signal and keeps one task per MPRIS player. A player
task reads the player once, then applies `PropertiesChanged` and `Seeked`
signals as they come. The position is carried forward from the last value
the player reported, at the reported rate, so `Status::position()` is
current without a round trip. On a track or state change the position is
read again, because players do not announce it.

The watcher is `Send + Sync`. Reads copy the latest state under a mutex and
never touch the bus; `generation()` moves on every change so a per-frame
reader can skip work. Controls send without waiting for a reply, so a
sluggish player cannot stall the caller. Dropping the watcher cancels the
tasks and joins the thread.

Dependencies: `zbus` (async-io flavour), `async-executor`, `futures-lite`,
`log`. No Bevy.

## Use

```rust
let watcher = nowplaying::Watcher::start()?;
if let Some(status) = watcher.status() {
    if let Some(track) = &status.track {
        println!("{} - {}", track.artist, track.title);
        if let Some(path) = track.art_path() { /* decode the thumbnail */ }
    }
    println!("{:?} at {:?}", status.playback, status.position());
}
watcher.play_pause()?;
watcher.next()?;
if status.can_seek { watcher.restart()?; } // back to the start of the track
```

## Try it

From the repository's root:

```
cargo run -p nowplaying --bin nowplaying-dump   # prints changes, and the position once a second
cargo test -p nowplaying                        # unit tests
cargo test -p nowplaying -- --ignored           # against the live bus, with a player up
```
