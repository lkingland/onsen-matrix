//! The host side: the canvas and the displays; ending the run on input, on
//! losing focus (the session locking), or at the time limit; snapshots for
//! the loop; and frame-time logging.
//!
//! The canvas is one image as wide as every display side by side and as
//! tall as the tallest. The scene is rendered into it once per frame by
//! one camera, bloom and all, so the displays are one picture with no
//! seams. Each display then gets a fullscreen window showing its own
//! slice of the canvas, scaled to fill it.

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{RenderTarget, ScalingMode};
use bevy::diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin};
use bevy::input::keyboard::KeyboardInput;
use bevy::input::mouse::{MouseButtonInput, MouseMotion, MouseWheel};
use bevy::prelude::*;
use bevy::render::render_resource::{TextureFormat, TextureUsages};
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use bevy::window::{Monitor, MonitorSelection, WindowFocused, WindowMode, WindowRef};

use crate::scene::{spawn_camera, View};
use crate::Args;

/// How far the pointer may drift before it counts as movement, in pixels.
const POINTER_SLACK: f32 = 12.0;

pub struct HostPlugin;

impl Plugin for HostPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            FrameTimeDiagnosticsPlugin::default(),
            // The frame rate and frame time, every few seconds; nothing else.
            LogDiagnosticsPlugin {
                wait_duration: std::time::Duration::from_secs(5),
                filter: Some([FrameTimeDiagnosticsPlugin::FPS, FrameTimeDiagnosticsPlugin::FRAME_TIME].into_iter().collect()),
                ..default()
            },
        ))
            .add_systems(Update, (open_displays, take_focus, end_on_input, end_on_lock_or_limit, snapshot))
            .add_systems(Last, pace);
    }
}

/// The canvas: the whole picture, every display wide.
#[derive(Resource)]
pub struct Canvas(pub Handle<Image>);

/// Makes the canvas and opens a fullscreen window on each wanted monitor,
/// once the monitors are known.
fn open_displays(mut commands: Commands, args: Res<Args>, canvas: Option<Res<Canvas>>, mut images: ResMut<Assets<Image>>, monitors: Query<(Entity, &Monitor)>) {
    if canvas.is_some() || monitors.is_empty() {
        return;
    }
    // The monitors to cover, left to right.
    let descriptions = descriptions();
    let matches = |m: &Monitor, selectors: &[String]| {
        let name = m.name.clone().unwrap_or_default();
        let desc = descriptions.get(&name).cloned().unwrap_or_default().to_lowercase();
        selectors.iter().any(|s| match s.strip_prefix("desc:") {
            Some(part) => desc.contains(&part.to_lowercase()),
            None => *s == name,
        })
    };
    let mut chosen: Vec<(Entity, &Monitor)> = monitors
        .iter()
        .filter(|(_, m)| args.outputs.is_empty() || matches(m, &args.outputs))
        .filter(|(_, m)| !matches(m, &args.exclude))
        .collect();
    chosen.sort_by_key(|(_, m)| m.physical_position.x);
    if chosen.is_empty() {
        error!("no monitor matched {:?} less {:?}; monitors: {:?}", args.outputs, args.exclude, descriptions);
        commands.write_message(AppExit::error());
        return;
    }
    // The canvas: the monitors side by side in physical pixels, left to
    // right, with no gaps. (Their reported positions are not usable: winit
    // rounds a fractional scale, so positions and sizes disagree.)
    let full = UVec2::new(chosen.iter().map(|(_, m)| m.physical_width).sum(), chosen.iter().map(|(_, m)| m.physical_height).max().unwrap_or(1));
    let mut image = Image::new_target_texture(full.x, full.y, TextureFormat::Rgba8UnormSrgb, None);
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC; // for snapshots
    let canvas = images.add(image);
    commands.insert_resource(Canvas(canvas.clone()));
    commands.insert_resource(View { aspect: full.x as f32 / full.y as f32 });
    // The scene, rendered into the canvas first.
    spawn_camera(&mut commands, RenderTarget::Image(canvas.clone().into()));
    // Each display: a window, and a 2D camera on its own render layer
    // showing that display's slice of the canvas, exactly filling the window
    // whatever the window's scale.
    let mut x = 0u32;
    for (i, (entity, m)) in chosen.into_iter().enumerate() {
        let name = m.name.clone().unwrap_or_default();
        let size = UVec2::new(m.physical_width, m.physical_height);
        info!("display {name} ({}): {}x{} at {} of {}x{}", descriptions.get(&name).map(String::as_str).unwrap_or("?"), size.x, size.y, x, full.x, full.y);
        let window = commands
            .spawn(Window {
                title: format!("onsen-matrix: {name}"),
                name: Some(args.app_id.clone()),
                mode: if args.windowed { WindowMode::Windowed } else { WindowMode::BorderlessFullscreen(MonitorSelection::Entity(entity)) },
                present_mode: args.present_mode,
                desired_maximum_frame_latency: args.latency,
                decorations: false,
                ..default()
            })
            .id();
        let layer = RenderLayers::layer(i + 1);
        commands.spawn((
            Camera2d,
            Camera { order: i as isize + 1, ..default() },
            RenderTarget::Window(WindowRef::Entity(window)),
            Projection::Orthographic(OrthographicProjection { scaling_mode: ScalingMode::Fixed { width: size.x as f32, height: size.y as f32 }, ..OrthographicProjection::default_2d() }),
            layer.clone(),
        ));
        commands.spawn((
            Sprite {
                image: canvas.clone(),
                rect: Some(Rect::new(x as f32, 0.0, (x + size.x) as f32, size.y as f32)),
                custom_size: Some(size.as_vec2()),
                ..default()
            },
            Transform::default(),
            layer,
        ));
        x += size.x;
    }
}

/// Each monitor's make and model, by connector name, as Hyprland reports
/// them; empty when not on Hyprland.
fn descriptions() -> std::collections::HashMap<String, String> {
    let Ok(out) = std::process::Command::new("hyprctl").args(["monitors", "-j"]).output() else { return Default::default() };
    let Ok(list) = serde_json::from_slice::<Vec<serde_json::Value>>(&out.stdout) else { return Default::default() };
    list.iter()
        .filter_map(|m| Some((m.get("name")?.as_str()?.to_string(), m.get("description")?.as_str()?.to_string())))
        .collect()
}

/// Keys reach only the focused window, and a window opened from a script
/// does not get focus on Hyprland, so ask for it once the windows are up,
/// the way Omarchy's own screensaver launcher does. Not when holding, so
/// the loop does not steal the keyboard.
fn take_focus(args: Res<Args>, canvas: Option<Res<Canvas>>, mut done: Local<bool>, mut frames: Local<u32>) {
    if args.hold || *done || canvas.is_none() {
        return;
    }
    // A few frames in, so the windows have mapped.
    *frames += 1;
    if *frames < 30 {
        return;
    }
    *done = true;
    let class = regex_escape(&args.app_id);
    let dispatch = |cmd: &str| std::process::Command::new("hyprctl").args(["dispatch", cmd]).output();
    if dispatch(&format!("hl.dsp.focus({{ window = \"class:^({class})$\" }})")).map(|o| !o.status.success() || String::from_utf8_lossy(&o.stdout).contains("Invalid")).unwrap_or(true) {
        let _ = dispatch(&format!("focuswindow class:^({class})$"));
    }
}

/// Dots and the like in an app id, escaped for Hyprland's regex.
fn regex_escape(s: &str) -> String {
    s.chars().flat_map(|c| if ".+*?()[]{}|^$\\".contains(c) { vec!['\\', c] } else { vec![c] }).collect()
}

/// Holds the frame rate at --fps by sleeping out the rest of each frame,
/// when a rate is asked for. Vsync still applies on top.
fn pace(args: Res<Args>, mut last: Local<Option<std::time::Instant>>) {
    if args.fps == 0 {
        return;
    }
    let period = std::time::Duration::from_secs_f64(1.0 / args.fps as f64);
    let now = std::time::Instant::now();
    if let Some(prev) = *last {
        let next = prev + period;
        if next > now {
            std::thread::sleep(next - now);
        }
        *last = Some(next.max(now - period));
    } else {
        *last = Some(now);
    }
}

/// Losing focus ends the run: when the session locks, the compositor
/// takes keyboard focus from every window, and the same happens when
/// anything else takes the screen. The windows pass focus between
/// themselves as they open, so the run only ends once none of them has
/// had it for a moment. And the time limit ends it too: a screensaver
/// nobody is watching should not go on drawing.
fn end_on_lock_or_limit(args: Res<Args>, time: Res<Time>, mut focus: MessageReader<WindowFocused>, mut focused: Local<std::collections::HashMap<Entity, bool>>, mut unfocused_since: Local<Option<f32>>, mut exit: MessageWriter<AppExit>) {
    for f in focus.read() {
        focused.insert(f.window, f.focused);
    }
    if args.hold {
        return;
    }
    let now = time.elapsed_secs();
    let any = focused.values().any(|&f| f);
    let had_any = !focused.is_empty();
    match (had_any && !any, *unfocused_since) {
        (true, None) => *unfocused_since = Some(now),
        (true, Some(since)) if now - since > 0.5 && now > 3.0 => {
            info!("ending: focus lost (the session locked, or something else took the screen)");
            exit.write(AppExit::Success);
        }
        (false, _) => *unfocused_since = None,
        _ => {}
    }
    if args.limit > 0.0 && now >= args.limit * 60.0 {
        info!("ending: time limit of {} minutes", args.limit);
        exit.write(AppExit::Success);
    }
}

/// Any key, click, wheel or pointer movement ends the run, unless --hold.
#[allow(clippy::too_many_arguments)]
fn end_on_input(args: Res<Args>, mut keys: MessageReader<KeyboardInput>, mut buttons: MessageReader<MouseButtonInput>, mut wheel: MessageReader<MouseWheel>, mut motion: MessageReader<MouseMotion>, mut drifted: Local<Vec2>, mut exit: MessageWriter<AppExit>) {
    *drifted += motion.read().map(|m| m.delta).sum::<Vec2>();
    let key = keys.read().next().map(|k| format!("key {:?}", k.key_code));
    let why = key
        .or_else(|| buttons.read().next().map(|b| format!("button {:?}", b.button)))
        .or_else(|| wheel.read().next().map(|_| "wheel".to_string()))
        .or_else(|| (drifted.length() > POINTER_SLACK).then(|| format!("pointer moved {:.0} px", drifted.length())));
    if let Some(why) = why {
        if args.hold {
            return;
        }
        info!("ending: {why}");
        exit.write(AppExit::Success);
    }
}

/// While the request file exists, the next frame is written to the file it
/// names, and the request removed. Checked ten times a second.
fn snapshot(args: Res<Args>, mut commands: Commands, mut last: Local<f32>, time: Res<Time>, canvas: Option<Res<Canvas>>) {
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
    if let (false, Some(canvas)) = (target.is_empty(), canvas) {
        // The whole canvas: every display in one picture.
        commands.spawn(Screenshot::image(canvas.0.clone())).observe(save_to_disk(target));
    }
}
