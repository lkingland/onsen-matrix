//! onsen-matrix: a screensaver, as a Bevy scene.
//!
//! The scene is small and high level (`scene.rs`). A Droplet is a point
//! with a velocity that carries a bright head glyph and stamps glyphs
//! behind it as it reads the text; the rain births droplets at a steady
//! rate into the camera's view, and the camera has a velocity too. Glyphs
//! are records in one buffer, drawn all at once by one material whose
//! shader works out each glyph's fade and character from the clock
//! (`look.rs`, `assets/shaders/glyph.wesl`, hot-reloaded on save). The
//! characters come from a signed-distance atlas (`atlas.rs`). The host
//! (`host.rs`) renders the scene once into a canvas as wide as every
//! display and shows each display its slice. The settings come from
//! `config.toml` (`config.rs`), overridden by the command line. What is
//! playing, read from the media players on the session bus, is shown at
//! the bottom of the picture (`media.rs`, on the `nowplaying` crate).

mod atlas;
mod config;
mod host;
mod inhibit;
mod look;
mod media;
mod scene;

use bevy::prelude::*;
use bevy::window::ExitCondition;

/// The command line and environment: the switches that are not settings.
/// `--outputs`, `--exclude`, `--fps` and `--limit` override the config.
#[derive(Resource, Clone)]
pub struct Args {
    /// `--app-id ID`: the windows' app id (class), `onsen-matrix` unless told
    /// otherwise; Omarchy's idle service and lock know `org.omarchy.screensaver`.
    pub app_id: String,
    /// `--hold`: never end on input, for iterating.
    pub hold: bool,
    /// `ONSEN_MATRIX_SNAP=PATH`: while that file exists, the next frame is
    /// written to the file it names (PNG) and the request removed.
    pub snap: Option<std::path::PathBuf>,
}

/// Where the assets are: `ONSEN_MATRIX_ASSETS` if set, else the installed
/// location if it exists, else the source tree this binary was built from.
fn assets_dir() -> String {
    if let Ok(dir) = std::env::var("ONSEN_MATRIX_ASSETS") {
        return dir;
    }
    let installed = "/usr/share/onsen-matrix/assets";
    if std::path::Path::new(installed).is_dir() {
        return installed.into();
    }
    concat!(env!("CARGO_MANIFEST_DIR"), "/assets").into()
}

/// When stderr is not a terminal (a launch from a menu or on idle), the log
/// goes to a file under the state directory instead, so a crash leaves a
/// trace: ~/.local/state/onsen-matrix/log, kept to the last run.
/// ONSEN_MATRIX_LOG_STDERR keeps it on stderr, for a loop that redirects it.
fn log_to_file_when_detached() {
    use std::os::fd::AsRawFd;
    if unsafe { libc::isatty(2) } == 1 || std::env::var_os("ONSEN_MATRIX_LOG_STDERR").is_some() {
        return;
    }
    let dir = std::env::var("XDG_STATE_HOME").map(std::path::PathBuf::from).unwrap_or_else(|_| std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/state")).join("onsen-matrix");
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(file) = std::fs::File::create(dir.join("log")) {
        unsafe { libc::dup2(file.as_raw_fd(), 2) };
        std::mem::forget(file);
    }
}

fn main() {
    log_to_file_when_detached();
    let value = |name: &str| std::env::args().skip_while(|a| a != name).nth(1);
    let mut config = config::Config::load();
    let list = |v: String| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect::<Vec<_>>();
    if let Some(v) = value("--outputs") {
        config.run.outputs = list(v);
    }
    if let Some(v) = value("--exclude") {
        config.run.exclude = list(v);
    }
    if let Some(v) = value("--fps").and_then(|v| v.parse().ok()) {
        config.run.fps = v;
    }
    if let Some(v) = value("--limit").and_then(|v| v.parse().ok()) {
        config.run.limit = v;
    }
    let args = Args {
        app_id: value("--app-id").unwrap_or_else(|| "onsen-matrix".into()),
        hold: std::env::args().any(|a| a == "--hold"),
        snap: std::env::var("ONSEN_MATRIX_SNAP").ok().map(Into::into),
    };
    App::new()
        .insert_resource(args)
        .insert_resource(config)
        .insert_resource(ClearColor(Color::BLACK))
        .add_plugins(
            DefaultPlugins
                .set(AssetPlugin { file_path: assets_dir(), ..default() })
                // No window up front: host.rs opens one per monitor, each
                // fullscreen on its own, once the monitors are known.
                .set(WindowPlugin { primary_window: None, exit_condition: ExitCondition::OnAllClosed, ..default() }),
        )
        .add_plugins((look::LookPlugin, scene::ScenePlugin, host::HostPlugin, inhibit::InhibitPlugin, media::MediaPlugin))
        .run();
}
