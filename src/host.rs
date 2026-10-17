//! The host side: a fullscreen window on each display, each a camera into
//! the one scene; ending the run on input; snapshots for the loop; and
//! frame-time logging.

use bevy::camera::{RenderTarget, SubCameraView};
use bevy::diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin};
use bevy::input::keyboard::KeyboardInput;
use bevy::input::mouse::{MouseButtonInput, MouseMotion, MouseWheel};
use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use bevy::window::{Monitor, MonitorSelection, WindowMode, WindowRef};

use crate::scene::spawn_camera;
use crate::Args;

/// How far the pointer may drift before it counts as movement, in pixels.
const POINTER_SLACK: f32 = 12.0;

pub struct HostPlugin;

impl Plugin for HostPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((FrameTimeDiagnosticsPlugin::default(), LogDiagnosticsPlugin::default()))
            .add_systems(Update, (open_displays, end_on_input, snapshot));
    }
}

/// Set once the displays are open.
#[derive(Resource)]
struct Opened;

/// Opens a fullscreen window on each wanted monitor, once the monitors are
/// known, with a camera that sees that monitor's slice of the whole
/// picture: the monitors side by side, in their logical layout, are one
/// wide view onto the scene.
fn open_displays(mut commands: Commands, args: Res<Args>, opened: Option<Res<Opened>>, monitors: Query<(Entity, &Monitor)>) {
    if opened.is_some() || monitors.is_empty() {
        return;
    }
    commands.insert_resource(Opened);
    // The monitors to cover, left to right.
    let mut chosen: Vec<(Entity, &Monitor)> = monitors
        .iter()
        .filter(|(_, m)| args.outputs.is_empty() || m.name.as_ref().is_some_and(|n| args.outputs.contains(n)))
        .collect();
    chosen.sort_by_key(|(_, m)| m.physical_position.x);
    if chosen.is_empty() {
        error!("no monitor matched {:?}; monitors: {:?}", args.outputs, monitors.iter().filter_map(|(_, m)| m.name.clone()).collect::<Vec<_>>());
        commands.write_message(AppExit::error());
        return;
    }
    // The layout: the monitors side by side in physical pixels, left to
    // right, with no gaps. (Their reported positions are not usable: winit
    // rounds a fractional scale, so positions and sizes disagree.)
    let full = UVec2::new(chosen.iter().map(|(_, m)| m.physical_width).sum(), chosen.iter().map(|(_, m)| m.physical_height).max().unwrap_or(1));
    let mut x = 0u32;
    for (entity, m) in chosen {
        let name = m.name.clone().unwrap_or_default();
        let size = UVec2::new(m.physical_width, m.physical_height);
        let offset = Vec2::new(x as f32, 0.0);
        x += size.x;
        info!("display {name}: {}x{} at {} of {}x{}", size.x, size.y, offset.x, full.x, full.y);
        let window = commands
            .spawn(Window {
                title: format!("onsen-matrix: {name}"),
                name: Some("onsen-matrix".into()),
                mode: if args.windowed { WindowMode::Windowed } else { WindowMode::BorderlessFullscreen(MonitorSelection::Entity(entity)) },
                present_mode: args.present_mode,
                desired_maximum_frame_latency: args.latency,
                decorations: false,
                ..default()
            })
            .id();
        let view = if full == size { None } else { Some(SubCameraView { full_size: full, offset, size }) };
        spawn_camera(&mut commands, RenderTarget::Window(WindowRef::Entity(window)), view);
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
fn snapshot(args: Res<Args>, mut commands: Commands, mut last: Local<f32>, time: Res<Time>, windows: Query<Entity, With<Window>>) {
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
        // The first window, which is the leftmost display.
        commands.spawn(Screenshot::window(windows.iter().next().unwrap_or(Entity::PLACEHOLDER))).observe(save_to_disk(target));
    }
}
