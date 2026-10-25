//! The D-Bus side: a worker thread that subscribes to the bus and keeps
//! [`State`] current, and the control calls.
//!
//! The worker runs its own single-threaded executor. One task watches the
//! bus for MPRIS names coming and going and keeps one task per player; each
//! player task reads the player once, then applies the `PropertiesChanged`
//! and `Seeked` signals as they arrive. zbus does its socket I/O on its own
//! internal thread, so these tasks only wait on events.
//!
//! MPRIS, for reference: a player owns `org.mpris.MediaPlayer2.<name>` and
//! serves `/org/mpris/MediaPlayer2` with two interfaces. The root one
//! (`org.mpris.MediaPlayer2`) has `Identity`; the player one
//! (`org.mpris.MediaPlayer2.Player`) has `PlaybackStatus`, `Rate`,
//! `Position`, the `Can*` flags, `Metadata` (a dictionary of `xesam:` and
//! `mpris:` keys), the `Seeked` signal, and the transport methods.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::task::{Poll, Waker};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use async_executor::{LocalExecutor, Task};
use futures_lite::future;
use futures_lite::stream::StreamExt;
use zbus::fdo::{DBusProxy, PropertiesProxy};
use zbus::names::BusName;
use zbus::proxy::CacheProperties;
use zbus::zvariant::{Dict, OwnedValue, Value};
use zbus::{Connection, Proxy};

use crate::status::{Playback, Status, Track};

const NAME_PREFIX: &str = "org.mpris.MediaPlayer2.";
const PATH: &str = "/org/mpris/MediaPlayer2";
const ROOT_IFACE: &str = "org.mpris.MediaPlayer2";
const PLAYER_IFACE: &str = "org.mpris.MediaPlayer2.Player";

/// The player properties read on a first look and after an invalidation.
const PLAYER_PROPERTIES: [&str; 7] = [
    "PlaybackStatus",
    "Rate",
    "Metadata",
    "CanGoNext",
    "CanGoPrevious",
    "CanPlay",
    "CanPause",
];

// ---------------------------------------------------------------- shared state

/// What the worker keeps and the [`Watcher`](crate::Watcher) reads.
#[derive(Default)]
pub(crate) struct State {
    shared: Mutex<Shared>,
    /// Signalled when the first scan of the bus has been read in full.
    settled: Condvar,
}

impl State {
    /// The players, under the lock. A poisoned lock is taken anyway: the
    /// state is plain data that is always left consistent.
    pub(crate) fn lock(&self) -> MutexGuard<'_, Shared> {
        self.shared
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The first scan found `count` players; each reports in once.
    fn expect(&self, count: usize) {
        let mut shared = self.lock();
        shared.unsettled = count;
        if count == 0 {
            self.settled.notify_all();
        }
    }

    /// One player from the first scan has been read (or failed to be).
    fn settle_one(&self) {
        let mut shared = self.lock();
        shared.unsettled = shared.unsettled.saturating_sub(1);
        if shared.unsettled == 0 {
            self.settled.notify_all();
        }
    }

    /// Waits until every player from the first scan has been read, or
    /// `timeout` passes.
    pub(crate) fn wait_settled(&self, timeout: Duration) {
        let shared = self.lock();
        let _ = self
            .settled
            .wait_timeout_while(shared, timeout, |s| s.unsettled > 0);
    }
}

/// The players on the bus.
pub(crate) struct Shared {
    players: HashMap<String, Entry>,
    /// Bumped on every change, so readers can tell if anything moved.
    pub(crate) generation: u64,
    /// Hands out the order in which players started playing.
    play_serial: u64,
    /// Players from the first scan not yet read; starts above zero so a
    /// reader cannot see "settled" before the scan has even been counted.
    unsettled: usize,
}

struct Entry {
    status: Status,
    /// When the player started playing, as a serial; cleared when it stops.
    /// The bar shows the player that has been playing the longest.
    playing_since: Option<u64>,
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            players: HashMap::new(),
            generation: 0,
            play_serial: 0,
            unsettled: usize::MAX,
        }
    }
}

impl Shared {
    /// Records a player's state, keeping its playing order.
    fn update(&mut self, status: Status) {
        let playing = status.playing();
        let entry = match self.players.get_mut(&status.bus_name) {
            Some(entry) => {
                entry.status = status;
                entry
            }
            None => self
                .players
                .entry(status.bus_name.clone())
                .or_insert(Entry {
                    status,
                    playing_since: None,
                }),
        };
        match (playing, entry.playing_since) {
            (true, None) => {
                self.play_serial += 1;
                entry.playing_since = Some(self.play_serial);
            }
            (false, Some(_)) => entry.playing_since = None,
            _ => {}
        }
        self.generation += 1;
    }

    fn remove(&mut self, bus_name: &str) {
        if self.players.remove(bus_name).is_some() {
            self.generation += 1;
        }
    }

    pub(crate) fn best(&self) -> Option<&Status> {
        self.ranked().into_iter().next()
    }

    /// Every player, best first: playing before not, earlier playing before
    /// later, with a track before without, controllable before inert, then by
    /// name so the order is stable.
    pub(crate) fn ranked(&self) -> Vec<&Status> {
        let rank = |e: &Entry| {
            let s = &e.status;
            (
                !s.playing(),
                e.playing_since.unwrap_or(u64::MAX),
                s.track.is_none(),
                !s.controllable(),
            )
        };
        let mut entries: Vec<&Entry> = self.players.values().collect();
        entries.sort_by(|a, b| {
            rank(a)
                .cmp(&rank(b))
                .then_with(|| a.status.bus_name.cmp(&b.status.bus_name))
        });
        entries.into_iter().map(|e| &e.status).collect()
    }
}

// ----------------------------------------------------------------- stop flag

/// A flag the owner raises to end the worker; the worker awaits it.
#[derive(Clone, Default)]
pub(crate) struct Stop(Arc<Mutex<StopInner>>);

#[derive(Default)]
struct StopInner {
    raised: bool,
    waker: Option<Waker>,
}

impl Stop {
    pub(crate) fn raise(&self) {
        let mut inner = self.0.lock().unwrap_or_else(|p| p.into_inner());
        inner.raised = true;
        if let Some(waker) = inner.waker.take() {
            waker.wake();
        }
    }

    async fn raised(&self) {
        future::poll_fn(|cx| {
            let mut inner = self.0.lock().unwrap_or_else(|p| p.into_inner());
            if inner.raised {
                Poll::Ready(())
            } else {
                inner.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        })
        .await
    }
}

// ------------------------------------------------------------------- worker

/// Starts the worker thread. It runs until `stop` is raised.
pub(crate) fn spawn_worker(
    conn: Connection,
    state: Arc<State>,
    stop: Stop,
) -> std::io::Result<JoinHandle<()>> {
    thread::Builder::new()
        .name("nowplaying".into())
        .spawn(move || {
            let ex = LocalExecutor::new();
            future::block_on(ex.run(async {
                // `or` polls the watch first; when the flag is raised the watch
                // and every player task it holds are dropped.
                future::or(watch_bus(&ex, &conn, &state), stop.raised()).await;
            }));
        })
}

/// Keeps a task per MPRIS player on the bus, for as long as it is there.
async fn watch_bus(ex: &LocalExecutor<'_>, conn: &Connection, state: &Arc<State>) {
    let dbus = match DBusProxy::new(conn).await {
        Ok(d) => d,
        Err(e) => return fail(state, format_args!("bus proxy: {e}")),
    };
    // Subscribe before listing, so a player appearing in between is not missed.
    let mut owners = match dbus.receive_name_owner_changed().await {
        Ok(s) => s,
        Err(e) => return fail(state, format_args!("NameOwnerChanged: {e}")),
    };
    let mut tasks: HashMap<String, Task<()>> = HashMap::new();
    let spawn = |name: &str, first_scan: bool| {
        ex.spawn(watch_player(
            conn.clone(),
            name.to_owned(),
            Arc::clone(state),
            first_scan,
        ))
    };

    match dbus.list_names().await {
        Ok(names) => {
            let players: Vec<&str> = names
                .iter()
                .map(|n| n.as_str())
                .filter(|n| is_player(n))
                .collect();
            state.expect(players.len());
            for name in players {
                tasks.insert(name.to_owned(), spawn(name, true));
            }
        }
        Err(e) => fail(state, format_args!("ListNames: {e}")),
    }

    while let Some(signal) = owners.next().await {
        let Ok(args) = signal.args() else { continue };
        let name = args.name().as_str();
        if !is_player(name) {
            continue;
        }
        // A player that restarts under the same name yields one signal with
        // both owners set; the old task is replaced and its state re-read.
        if args.new_owner().is_some() {
            tasks.insert(name.to_owned(), spawn(name, false));
        } else {
            tasks.remove(name);
            state.lock().remove(name);
        }
    }
    log::error!("nowplaying: the bus connection ended; no more changes will be seen");
}

/// The watch cannot run: say so, and let `start()` return rather than wait.
fn fail(state: &State, why: fmt::Arguments<'_>) {
    log::error!("nowplaying: {why}");
    state.expect(0);
}

/// Every MPRIS name, except playerctld's proxy of the others.
fn is_player(name: &str) -> bool {
    name.starts_with(NAME_PREFIX) && !name.contains("playerctld")
}

/// Reads one player and follows its changes until the task is dropped.
/// `first_scan` players report in once read, so `start()` can wait for them.
async fn watch_player(conn: Connection, name: String, state: Arc<State>, first_scan: bool) {
    let settle = || {
        if first_scan {
            state.settle_one();
        }
    };
    if let Err(e) = follow_player(&conn, &name, &state, &settle).await {
        // Usual cause: the player left while being read. The owner watch
        // removes it from the shared state.
        log::debug!("nowplaying: {name}: {e}");
        settle();
    }
}

async fn follow_player(
    conn: &Connection,
    name: &str,
    state: &State,
    settle: &dyn Fn(),
) -> zbus::Result<()> {
    let player = proxy(conn, name, PLAYER_IFACE).await?;
    let root = proxy(conn, name, ROOT_IFACE).await?;
    let props = PropertiesProxy::builder(conn)
        .destination(name.to_owned())?
        .path(PATH)?
        .build()
        .await?;
    // Subscribe before the first read, so nothing changes unseen.
    let mut changes = props.receive_properties_changed().await?;
    let mut seeks = player.receive_signal("Seeked").await?;

    let mut status = Status::new(name.to_owned());
    read_all(&player, &root, &mut status).await;
    state.lock().update(status.clone());
    settle();

    loop {
        let change = async { Event::Properties(changes.next().await) };
        let seek = async { Event::Seeked(seeks.next().await) };
        match future::or(change, seek).await {
            Event::Properties(None) | Event::Seeked(None) => return Ok(()),
            Event::Properties(Some(signal)) => {
                let Ok(args) = signal.args() else { continue };
                let changed = args.changed_properties();
                for (key, value) in changed {
                    apply(&mut status, key, value);
                }
                if !args.invalidated_properties().is_empty() {
                    read_all(&player, &root, &mut status).await;
                }
                // A new track or a state change restarts the clock, and
                // players do not announce Position; ask.
                if changed.contains_key("Metadata") || changed.contains_key("PlaybackStatus") {
                    read_position(&player, &mut status).await;
                }
            }
            Event::Seeked(Some(signal)) => {
                if let Ok(micros) = signal.body().deserialize::<i64>() {
                    status.set_position(micros_to_duration(micros), Instant::now());
                }
            }
        }
        state.lock().update(status.clone());
    }
}

enum Event {
    Properties(Option<zbus::fdo::PropertiesChanged>),
    Seeked(Option<zbus::Message>),
}

async fn proxy<'a>(
    conn: &Connection,
    name: &str,
    interface: &'static str,
) -> zbus::Result<Proxy<'a>> {
    // No property cache: the state is kept here, from the same signals.
    zbus::proxy::Builder::<Proxy<'a>>::new(conn)
        .destination(name.to_owned())?
        .path(PATH)?
        .interface(interface)?
        .cache_properties(CacheProperties::No)
        .build()
        .await
}

/// Reads every property into `status`; one missing is left as it was.
async fn read_all(player: &Proxy<'_>, root: &Proxy<'_>, status: &mut Status) {
    if let Ok(identity) = root.get_property::<String>("Identity").await {
        status.identity = identity;
    }
    for key in PLAYER_PROPERTIES {
        if let Ok(value) = player.get_property::<OwnedValue>(key).await {
            apply(status, key, &value);
        }
    }
    read_position(player, status).await;
}

async fn read_position(player: &Proxy<'_>, status: &mut Status) {
    if let Ok(micros) = player.get_property::<i64>("Position").await {
        status.set_position(micros_to_duration(micros), Instant::now());
    }
}

/// One property, by its MPRIS name, into `status`.
fn apply(status: &mut Status, key: &str, value: &Value<'_>) {
    match key {
        "PlaybackStatus" => {
            if let Some(s) = as_str(value) {
                status.playback = Playback::parse(s);
            }
        }
        "Rate" => {
            if let Value::F64(rate) = value {
                status.set_rate(*rate);
            }
        }
        "Metadata" => {
            if let Value::Dict(dict) = value {
                let track = track_from(dict);
                status.track = (!track.is_empty()).then_some(track);
            }
        }
        "Identity" => {
            if let Some(s) = as_str(value) {
                status.identity = s.to_owned();
            }
        }
        "CanGoNext" => status.can_go_next = as_bool(value),
        "CanGoPrevious" => status.can_go_previous = as_bool(value),
        "CanPlay" => status.can_play = as_bool(value),
        "CanPause" => status.can_pause = as_bool(value),
        _ => {}
    }
}

/// The track in an MPRIS `Metadata` dictionary (`xesam:` and `mpris:` keys).
fn track_from(dict: &Dict<'_, '_>) -> Track {
    let mut track = Track::default();
    for (key, value) in dict.iter() {
        match as_str(key) {
            Some("xesam:title") => track.title = as_str(value).unwrap_or_default().to_owned(),
            Some("xesam:artist") => track.artist = strings(value).join(", "),
            Some("xesam:album") => track.album = as_str(value).unwrap_or_default().to_owned(),
            Some("mpris:artUrl") => track.art_url = as_str(value).unwrap_or_default().to_owned(),
            Some("mpris:length") => {
                track.length = as_i64(value).filter(|n| *n > 0).map(micros_to_duration)
            }
            _ => {}
        }
    }
    track
}

// Values arrive as variants, sometimes nested one deep; these look through.

fn as_str<'v>(value: &'v Value<'_>) -> Option<&'v str> {
    match value {
        Value::Str(s) => Some(s.as_str()),
        Value::Value(inner) => as_str(inner),
        _ => None,
    }
}

fn as_bool(value: &Value<'_>) -> bool {
    match value {
        Value::Bool(b) => *b,
        Value::Value(inner) => as_bool(inner),
        _ => false,
    }
}

fn as_i64(value: &Value<'_>) -> Option<i64> {
    match value {
        Value::I64(n) => Some(*n),
        Value::U64(n) => i64::try_from(*n).ok(),
        Value::I32(n) => Some(i64::from(*n)),
        Value::U32(n) => Some(i64::from(*n)),
        Value::F64(n) => Some(*n as i64),
        Value::Value(inner) => as_i64(inner),
        _ => None,
    }
}

/// `xesam:artist` is a list of strings; some players send one string.
fn strings(value: &Value<'_>) -> Vec<String> {
    match value {
        Value::Array(items) => items.iter().filter_map(as_str).map(str::to_owned).collect(),
        Value::Str(s) => vec![s.to_string()],
        Value::Value(inner) => strings(inner),
        _ => vec![],
    }
}

/// MPRIS times are microseconds; a negative one is nonsense, taken as zero.
fn micros_to_duration(micros: i64) -> Duration {
    Duration::from_micros(micros.max(0) as u64)
}

// ------------------------------------------------------------------ controls

/// Calls a transport method on a player without waiting for the reply.
pub(crate) fn control(
    conn: &zbus::blocking::Connection,
    bus_name: &str,
    method: &str,
) -> zbus::Result<()> {
    let dest = BusName::try_from(bus_name.to_owned())?;
    let proxy = zbus::blocking::proxy::Builder::<zbus::blocking::Proxy<'_>>::new(conn)
        .destination(dest)?
        .path(PATH)?
        .interface(PLAYER_IFACE)?
        .cache_properties(CacheProperties::No)
        .build()?;
    proxy.call_noreply(method, &())
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::{Array, Signature};

    fn metadata(pairs: &[(&str, Value<'static>)]) -> Value<'static> {
        let mut dict = Dict::new(&Signature::Str, &Signature::Variant);
        for (k, v) in pairs {
            dict.add(k.to_string(), Value::Value(Box::new(v.clone())))
                .unwrap();
        }
        Value::Dict(dict)
    }

    fn artists(names: &[&str]) -> Value<'static> {
        let mut array = Array::new(&Signature::Str);
        for n in names {
            array.append(Value::Str(n.to_string().into())).unwrap();
        }
        Value::Array(array)
    }

    #[test]
    fn metadata_as_brave_sends_it() {
        let mut status = Status::new("org.mpris.MediaPlayer2.brave.instance1".into());
        let value = metadata(&[
            (
                "mpris:artUrl",
                Value::from("file:///tmp/.org.chromium.Chromium.CfSZuP"),
            ),
            ("mpris:length", Value::I64(171_328_494)),
            ("xesam:album", Value::from("")),
            ("xesam:artist", artists(&["Seeker"])),
            ("xesam:title", Value::from("Losing My Ways")),
        ]);
        apply(&mut status, "Metadata", &value);
        let track = status.track.expect("a track");
        assert_eq!(track.title, "Losing My Ways");
        assert_eq!(track.artist, "Seeker");
        assert_eq!(track.album, "");
        assert_eq!(
            track.art_path().unwrap().to_str().unwrap(),
            "/tmp/.org.chromium.Chromium.CfSZuP"
        );
        assert_eq!(track.length, Some(Duration::from_micros(171_328_494)));
    }

    #[test]
    fn several_artists_are_joined_and_empty_metadata_is_no_track() {
        let mut status = Status::new("x".into());
        apply(
            &mut status,
            "Metadata",
            &metadata(&[
                ("xesam:artist", artists(&["A", "B"])),
                ("xesam:title", Value::from("T")),
            ]),
        );
        assert_eq!(status.track.as_ref().unwrap().artist, "A, B");
        apply(
            &mut status,
            "Metadata",
            &metadata(&[("mpris:trackid", Value::from("/x"))]),
        );
        assert!(status.track.is_none());
    }

    #[test]
    fn state_and_flags() {
        let mut status = Status::new("x".into());
        apply(&mut status, "PlaybackStatus", &Value::from("Playing"));
        apply(&mut status, "CanGoNext", &Value::Bool(true));
        apply(&mut status, "Rate", &Value::F64(1.5));
        assert!(status.playing());
        assert!(status.can_go_next && !status.can_go_previous);
        assert_eq!(status.rate(), 1.5);
        apply(&mut status, "PlaybackStatus", &Value::from("Paused"));
        assert_eq!(status.playback, Playback::Paused);
    }

    fn player(name: &str, playback: Playback, track: bool) -> Status {
        let mut s = Status::new(name.into());
        s.playback = playback;
        s.can_play = true;
        if track {
            s.track = Some(Track {
                title: "t".into(),
                ..Default::default()
            });
        }
        s
    }

    #[test]
    fn the_bar_rule_for_which_player_to_show() {
        let mut shared = Shared::default();
        shared.update(player("idle", Playback::Stopped, false));
        shared.update(player("loaded", Playback::Paused, true));
        assert_eq!(shared.best().unwrap().bus_name, "loaded");

        shared.update(player("second", Playback::Playing, true));
        shared.update(player("first", Playback::Playing, true));
        // "second" started playing before "first" did.
        assert_eq!(shared.best().unwrap().bus_name, "second");
        let names: Vec<_> = shared
            .ranked()
            .iter()
            .map(|s| s.bus_name.as_str())
            .collect();
        assert_eq!(names, ["second", "first", "loaded", "idle"]);

        shared.update(player("second", Playback::Paused, true));
        assert_eq!(shared.best().unwrap().bus_name, "first");
        // Playing again means a fresh start: "first" keeps the lead.
        shared.update(player("second", Playback::Playing, true));
        assert_eq!(shared.best().unwrap().bus_name, "first");

        shared.remove("first");
        assert_eq!(shared.best().unwrap().bus_name, "second");
    }

    #[test]
    fn generation_moves_only_on_change() {
        let mut shared = Shared::default();
        let g0 = shared.generation;
        shared.remove("nobody");
        assert_eq!(shared.generation, g0);
        shared.update(player("a", Playback::Playing, true));
        assert!(shared.generation > g0);
    }

    #[test]
    fn player_names() {
        assert!(is_player("org.mpris.MediaPlayer2.brave.instance1122929"));
        assert!(is_player("org.mpris.MediaPlayer2.spotify"));
        assert!(!is_player("org.mpris.MediaPlayer2.playerctld"));
        assert!(!is_player("org.freedesktop.DBus"));
    }
}
