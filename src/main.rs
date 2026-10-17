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
use bevy::window::{MonitorSelection, PresentMode, WindowMode};

/// The command line and environment.
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
        .add_plugins(
            DefaultPlugins
                // Assets live in the source tree, wherever the binary is.
                .set(AssetPlugin { file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/assets").into(), ..default() })
                // One borderless fullscreen window. On Hyprland it maps on
                // the focused monitor; the launcher chooses which by
                // focusing it first, as Omarchy's own screensaver does.
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "onsen-matrix".into(),
                        name: Some("onsen-matrix".into()),
                        mode: WindowMode::BorderlessFullscreen(MonitorSelection::Current),
                        present_mode: PresentMode::AutoVsync,
                        decorations: false,
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_plugins((look::LookPlugin, scene::ScenePlugin, host::HostPlugin))
        .run();
}
