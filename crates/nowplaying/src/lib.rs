//! What is playing right now: the track, the artist, the artwork, the
//! position, and the controls to drive the player, from whichever media
//! player is playing.
//!
//! Every player that speaks MPRIS publishes itself on the D-Bus session bus
//! as `org.mpris.MediaPlayer2.<name>`: browsers (Brave, Chromium, Firefox)
//! for the page that is playing, Spotify, mpv, VLC and the rest. The Omarchy
//! bar reads that interface, and so does this crate, with the bar's rule for
//! which player to show when there are several.
//!
//! Nothing is polled. A [`Watcher`] subscribes to the bus: players appearing
//! and leaving, each player's property changes (track, state, what it can
//! do) and seeks. The position is carried forward from the last value the
//! player reported, so [`Status::position`] is current without a round trip.
//!
//! ```no_run
//! let watcher = nowplaying::Watcher::start()?;
//! // Once a frame:
//! if let Some(status) = watcher.status() {
//!     if let Some(track) = &status.track {
//!         println!("{} - {} ({:?})", track.artist, track.title, status.position());
//!     }
//! }
//! watcher.play_pause()?;
//! # Ok::<(), nowplaying::Error>(())
//! ```
//!
//! The crate knows nothing of what shows the result: no graphics, no async
//! runtime imposed on the caller. Its dependencies are `zbus` and the small
//! executor the worker thread runs on.

#![warn(missing_docs)]

mod art;
mod bus;
mod status;

pub use status::{Playback, Status, Track};

use std::fmt;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use bus::{State, Stop};

/// How long [`Watcher::start`] waits for the players already on the bus to
/// answer before returning anyway.
const FIRST_SCAN: Duration = Duration::from_secs(1);

/// What can go wrong.
#[derive(Debug)]
pub enum Error {
    /// The session bus could not be reached, or refused a call.
    Bus(zbus::Error),
    /// The worker thread could not be started.
    Thread(std::io::Error),
    /// A control was asked for with no player to send it to.
    NoPlayer,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Bus(e) => write!(f, "session bus: {e}"),
            Error::Thread(e) => write!(f, "worker thread: {e}"),
            Error::NoPlayer => write!(f, "no media player on the bus"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Bus(e) => Some(e),
            Error::Thread(e) => Some(e),
            Error::NoPlayer => None,
        }
    }
}

impl From<zbus::Error> for Error {
    fn from(e: zbus::Error) -> Self {
        Error::Bus(e)
    }
}

/// Watches the bus for media players and keeps their latest state.
///
/// It is `Send + Sync`, so it can live in a resource and be read from any
/// thread. Reads never touch the bus: they copy the latest state from under
/// a mutex. The controls send without waiting for a reply, so a sluggish
/// player cannot stall the caller. Dropping it unsubscribes and stops the
/// worker thread.
pub struct Watcher {
    state: Arc<State>,
    conn: zbus::blocking::Connection,
    stop: Stop,
    worker: Option<JoinHandle<()>>,
}

impl Watcher {
    /// Connects to the session bus, starts watching, and returns once the
    /// players already on the bus have been read (or after a second, if one
    /// is slow to answer), so the first [`Watcher::status`] is meaningful.
    ///
    /// Fails when there is no session bus to connect to, or no thread to be
    /// had.
    pub fn start() -> Result<Self, Error> {
        let conn = zbus::blocking::Connection::session()?;
        let state = Arc::new(State::default());
        let stop = Stop::default();
        let worker = bus::spawn_worker(conn.inner().clone(), Arc::clone(&state), stop.clone())
            .map_err(Error::Thread)?;
        state.wait_settled(FIRST_SCAN);
        Ok(Self {
            state,
            conn,
            stop,
            worker: Some(worker),
        })
    }

    /// The player to show: playing beats paused, the one that started playing
    /// first beats the others, a track loaded beats none, controllable beats
    /// inert. `None` when no player is on the bus.
    pub fn status(&self) -> Option<Status> {
        self.state.lock().best().cloned()
    }

    /// Every player on the bus, best first, by the same rule as [`Watcher::status`].
    pub fn players(&self) -> Vec<Status> {
        self.state.lock().ranked().into_iter().cloned().collect()
    }

    /// A counter that moves whenever anything changes: a player comes or
    /// goes, a track or state changes, a seek. A consumer that reads once a
    /// frame can compare it with the last value it saw and skip the rest.
    /// The position moving on its own does not count.
    pub fn generation(&self) -> u64 {
        self.state.lock().generation
    }

    /// Toggles the shown player between playing and paused.
    pub fn play_pause(&self) -> Result<(), Error> {
        self.control("PlayPause")
    }

    /// Skips the shown player to the next track.
    pub fn next(&self) -> Result<(), Error> {
        self.control("Next")
    }

    /// Skips the shown player to the previous track, or restarts this one.
    pub fn previous(&self) -> Result<(), Error> {
        self.control("Previous")
    }

    fn control(&self, method: &str) -> Result<(), Error> {
        let status = self.status().ok_or(Error::NoPlayer)?;
        bus::control(&self.conn, &status.bus_name, method)?;
        Ok(())
    }
}

impl fmt::Debug for Watcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.state.lock();
        f.debug_struct("Watcher")
            .field("players", &state.ranked().len())
            .field("generation", &state.generation)
            .finish_non_exhaustive()
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.stop.raise();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
