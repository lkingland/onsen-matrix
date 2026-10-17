//! The host side: ending the run on input, snapshots for the loop, and
//! frame-time logging.

use bevy::diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin};
use bevy::input::keyboard::KeyboardInput;
use bevy::input::mouse::{MouseButtonInput, MouseMotion, MouseWheel};
use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};

use crate::Args;

/// How far the pointer may drift before it counts as movement, in pixels.
const POINTER_SLACK: f32 = 12.0;

pub struct HostPlugin;

impl Plugin for HostPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((FrameTimeDiagnosticsPlugin::default(), LogDiagnosticsPlugin::default()))
            .add_systems(Update, (end_on_input, snapshot));
    }
}

/// Any key, click, wheel or pointer movement ends the run, unless --hold.
fn end_on_input(args: Res<Args>, mut keys: MessageReader<KeyboardInput>, mut buttons: MessageReader<MouseButtonInput>, mut wheel: MessageReader<MouseWheel>, mut motion: MessageReader<MouseMotion>, mut drifted: Local<Vec2>, mut exit: MessageWriter<AppExit>) {
    *drifted += motion.read().map(|m| m.delta).sum::<Vec2>();
    let any = keys.read().count() > 0 || buttons.read().count() > 0 || wheel.read().count() > 0 || drifted.length() > POINTER_SLACK;
    if any && !args.hold {
        exit.write(AppExit::Success);
    }
}

/// While the request file exists, the next frame is written to the file it
/// names, and the request removed. Checked ten times a second.
fn snapshot(args: Res<Args>, mut commands: Commands, mut last: Local<f32>, time: Res<Time>) {
    let Some(request) = args.snap.as_ref() else { return };
    if time.elapsed_secs() - *last < 0.1 {
        return;
    }
    *last = time.elapsed_secs();
    if !request.exists() {
        return;
    }
    let target = std::fs::read_to_string(request).unwrap_or_default().trim().to_string();
    let _ = std::fs::remove_file(request);
    if !target.is_empty() {
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(target));
    }
}
