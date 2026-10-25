//! What the watcher reports: a player and the track it is on.

use std::fmt;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// A player's playback state, as MPRIS names it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Playback {
    /// The track is playing.
    Playing,
    /// The track is paused where it is.
    Paused,
    /// Nothing is playing; the position is at the start.
    #[default]
    Stopped,
}

impl Playback {
    pub(crate) fn parse(s: &str) -> Self {
        match s {
            "Playing" => Self::Playing,
            "Paused" => Self::Paused,
            _ => Self::Stopped,
        }
    }

    /// The state as a word: `playing`, `paused` or `stopped`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Playing => "playing",
            Self::Paused => "paused",
            Self::Stopped => "stopped",
        }
    }
}

impl fmt::Display for Playback {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The track a player is on.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Track {
    /// The title, or empty.
    pub title: String,
    /// The artists, joined with ", " when there are several; or empty.
    pub artist: String,
    /// The album, or empty.
    pub album: String,
    /// The artwork as the player published it: a `file://` URL (browsers
    /// write the thumbnail to a temp file), an `https://` URL (Spotify), a
    /// `data:` URI, or empty.
    pub art_url: String,
    /// The track's length, when the player knows it.
    pub length: Option<Duration>,
}

impl Track {
    /// The artwork as a local file, when [`Track::art_url`] names one.
    pub fn art_path(&self) -> Option<PathBuf> {
        crate::art::file_path(&self.art_url)
    }

    /// Whether there is nothing to show: no title and no artist.
    pub fn is_empty(&self) -> bool {
        self.title.is_empty() && self.artist.is_empty()
    }
}

/// A player and where it is.
#[derive(Clone, Debug, PartialEq)]
pub struct Status {
    /// The player's bus name, e.g. `org.mpris.MediaPlayer2.brave.instance1122929`.
    pub bus_name: String,
    /// The player's own name, e.g. `Brave`; empty if it gave none.
    pub identity: String,
    /// Playing, paused or stopped.
    pub playback: Playback,
    /// The track loaded, when the player has one with a title or artist.
    pub track: Option<Track>,
    /// Whether [`Watcher::next`](crate::Watcher::next) would do anything.
    pub can_go_next: bool,
    /// Whether [`Watcher::previous`](crate::Watcher::previous) would do anything.
    pub can_go_previous: bool,
    /// Whether the player can be started.
    pub can_play: bool,
    /// Whether the player can be paused.
    pub can_pause: bool,
    /// Where the player said it was, and when; the position is carried
    /// forward from here while playing.
    position: Duration,
    position_at: Instant,
    /// Playback rate, 1.0 for normal speed.
    rate: f64,
}

impl Status {
    pub(crate) fn new(bus_name: String) -> Self {
        Self {
            bus_name,
            identity: String::new(),
            playback: Playback::Stopped,
            track: None,
            can_go_next: false,
            can_go_previous: false,
            can_play: false,
            can_pause: false,
            position: Duration::ZERO,
            position_at: Instant::now(),
            rate: 1.0,
        }
    }

    /// Whether the player is playing.
    pub fn playing(&self) -> bool {
        self.playback == Playback::Playing
    }

    /// The position in the track now: the last reported value, advanced by
    /// the time since at the playback rate while playing, and never past the
    /// track's length.
    pub fn position(&self) -> Duration {
        self.position_as_of(Instant::now())
    }

    /// Whether any transport control would do something.
    pub fn controllable(&self) -> bool {
        self.can_go_next || self.can_go_previous || self.can_play || self.can_pause
    }

    pub(crate) fn position_as_of(&self, now: Instant) -> Duration {
        let mut position = self.position;
        if self.playing() {
            let elapsed = now.saturating_duration_since(self.position_at);
            position += elapsed.mul_f64(self.rate.max(0.0));
        }
        match self.track.as_ref().and_then(|t| t.length) {
            Some(length) => position.min(length),
            None => position,
        }
    }

    pub(crate) fn set_position(&mut self, position: Duration, at: Instant) {
        self.position = position;
        self.position_at = at;
    }

    pub(crate) fn set_rate(&mut self, rate: f64) {
        self.rate = rate;
    }

    #[cfg(test)]
    pub(crate) fn rate(&self) -> f64 {
        self.rate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(playback: Playback, length: Option<Duration>) -> Status {
        let mut s = Status::new("org.mpris.MediaPlayer2.test".into());
        s.playback = playback;
        s.track = Some(Track {
            title: "t".into(),
            length,
            ..Default::default()
        });
        s
    }

    #[test]
    fn position_advances_only_while_playing() {
        let t0 = Instant::now();
        let mut s = status(Playback::Playing, None);
        s.set_position(Duration::from_secs(10), t0);
        assert_eq!(
            s.position_as_of(t0 + Duration::from_secs(5)),
            Duration::from_secs(15)
        );

        s.playback = Playback::Paused;
        assert_eq!(
            s.position_as_of(t0 + Duration::from_secs(5)),
            Duration::from_secs(10)
        );
    }

    #[test]
    fn position_follows_rate_and_stops_at_length() {
        let t0 = Instant::now();
        let mut s = status(Playback::Playing, Some(Duration::from_secs(30)));
        s.set_rate(2.0);
        s.set_position(Duration::from_secs(10), t0);
        assert_eq!(
            s.position_as_of(t0 + Duration::from_secs(5)),
            Duration::from_secs(20)
        );
        assert_eq!(
            s.position_as_of(t0 + Duration::from_secs(60)),
            Duration::from_secs(30)
        );
    }

    #[test]
    fn position_never_runs_backwards_on_clock_skew() {
        let t0 = Instant::now();
        let mut s = status(Playback::Playing, None);
        s.set_position(Duration::from_secs(10), t0 + Duration::from_secs(5));
        assert_eq!(s.position_as_of(t0), Duration::from_secs(10));
    }

    #[test]
    fn playback_reads_as_a_word() {
        assert_eq!(Playback::parse("Playing").to_string(), "playing");
        assert_eq!(Playback::parse("Paused").to_string(), "paused");
        assert_eq!(Playback::parse("anything else"), Playback::Stopped);
    }
}
