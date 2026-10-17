//! onsen-matrix: a screensaver, as a Bevy scene.
//!
//! The scene is small and high level. A Glyph is a quad with the tube look
//! on it, at a point in space. A Droplet is a point with a velocity that
//! carries a bright head glyph and stamps glyphs behind it as it reads the
//! text. A Column is a place in space where droplets fall one after
//! another. The camera has a velocity too. Bevy does the rest: transforms,
//! projection, batching, bloom.
//!
//! The look is `assets/shaders/glyph.wesl`, hot-reloaded on save.

mod atlas;
mod look;
mod scene;

use bevy::diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin};
use bevy::prelude::*;
use bevy::window::{MonitorSelection, PresentMode, WindowMode};

/// The command line.
#[derive(Resource, Clone)]
pub struct Args {
    /// `--hold`: never end on input, for iterating.
    pub hold: bool,
    /// `ONSEN_MATRIX_SNAP=PATH`: while that file exists, the next frame is
    /// written to the file it names (PNG) and the request removed.
    pub snap: Option<std::path::PathBuf>,
}

fn main() {
    let args = Args { hold: std::env::args().any(|a| a == "--hold"), snap: std::env::var("ONSEN_MATRIX_SNAP").ok().map(Into::into) };
    App::new()
        .insert_resource(args)
        .insert_resource(ClearColor(Color::BLACK))
        .add_plugins(DefaultPlugins.set(AssetPlugin { file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/assets").into(), ..default() }).set(WindowPlugin {
            primary_window: Some(Window {
                title: "onsen-matrix".into(),
                name: Some("onsen-matrix".into()),
                mode: WindowMode::BorderlessFullscreen(MonitorSelection::Current),
                present_mode: PresentMode::AutoNoVsync,
                decorations: false,
                ..default()
            }),
            ..default()
        }))
        .add_plugins((look::LookPlugin, scene::ScenePlugin))
        // Frame times, logged every few seconds, to see what the GPU can take.
        .add_plugins((FrameTimeDiagnosticsPlugin::default(), LogDiagnosticsPlugin::default()))
        .run();
}
