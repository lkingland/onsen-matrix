//! onsen-matrix: a screensaver, as a Bevy scene.
//!
//! The scene is small and high level (`scene.rs`). A Glyph is a quad with
//! the tube look on it, at a point in space, fading. A Droplet is a point
//! with a velocity that carries a bright head glyph and stamps glyphs
//! behind it as it reads the text. The rain keeps a number of droplets
//! falling in a box that follows the camera, which has a velocity too.
//! Bevy does the rest: transforms, projection, instancing, bloom.
//!
//! The look (`look.rs`) is one material on one quad, shared by every glyph,
//! with the per-glyph character and brightness packed into the instance's
//! tag; its shader is `assets/shaders/glyph.wesl`, hot-reloaded on save.
//! The characters come from a signed-distance atlas (`atlas.rs`).

mod atlas;
mod host;
mod look;
mod scene;

use bevy::prelude::*;
use bevy::window::{ExitCondition, PresentMode};

/// The command line and environment.
#[derive(Resource, Clone)]
pub struct Args {
    /// `--outputs A,B`: the monitors to cover, by name; all of them otherwise.
    pub outputs: Vec<String>,
    /// `--app-id ID`: the windows' app id (class), `onsen-matrix` unless told
    /// otherwise; Omarchy's idle service and lock know `org.omarchy.screensaver`.
    pub app_id: String,
    /// `--hold`: never end on input, for iterating.
    pub hold: bool,
    /// How each window is presented: vsync unless a pacing knob says otherwise.
    pub present_mode: PresentMode,
    pub latency: Option<std::num::NonZero<u32>>,
    pub windowed: bool,
    /// `--fps N`: hold the frame rate at N, trading frames for density; 0 means the display's rate.
    pub fps: u32,
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

/// When nothing is reading stderr (a launch from a menu or on idle), the log
/// goes to a file under the state directory instead, so a crash leaves a
/// trace: ~/.local/state/onsen-matrix/log, kept to the last run.
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
    // Diagnostics knobs, for chasing frame pacing without a rebuild.
    let env = |k: &str| std::env::var(k).ok();
    let value = |name: &str| std::env::args().skip_while(|a| a != name).nth(1);
    let latency = env("ONSEN_MATRIX_LATENCY").and_then(|v| v.parse().ok()).and_then(std::num::NonZero::new);
    let windowed = env("ONSEN_MATRIX_WINDOWED").is_some();
    let present_mode = match env("ONSEN_MATRIX_PRESENT").as_deref() {
        Some("novsync") => PresentMode::AutoNoVsync,
        Some("immediate") => PresentMode::Immediate,
        Some("mailbox") => PresentMode::Mailbox,
        _ => PresentMode::AutoVsync,
    };
    let args = Args {
        outputs: value("--outputs").map(|v| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()).unwrap_or_default(),
        app_id: value("--app-id").unwrap_or_else(|| "onsen-matrix".into()),
        hold: std::env::args().any(|a| a == "--hold"),
        present_mode,
        latency,
        windowed,
        fps: value("--fps").and_then(|v| v.parse().ok()).unwrap_or(0),
        snap: env("ONSEN_MATRIX_SNAP").map(Into::into),
    };
    App::new()
        .insert_resource(args)
        .insert_resource(ClearColor(Color::BLACK))
        .add_plugins(
            DefaultPlugins
                .set(AssetPlugin { file_path: assets_dir(), ..default() })
                // No window up front: host.rs opens one per monitor, each
                // fullscreen on its own, once the monitors are known.
                .set(WindowPlugin { primary_window: None, exit_condition: ExitCondition::OnAllClosed, ..default() }),
        )
        .add_plugins((look::LookPlugin, scene::ScenePlugin, host::HostPlugin))
        .run();
}
