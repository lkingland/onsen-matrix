//! The host side: the canvas and the displays; ending the run when the
//! input asks (`input.rs`), on losing focus (the session locking), or at
//! the time limit; snapshots for the loop; and frame-time logging.
//!
//! The canvas is one image as wide as every display side by side and as
//! tall as the tallest. The scene is rendered into it once per frame by
//! one camera, bloom and all, so the displays are one picture with no
//! seams. Each display then gets a fullscreen window showing its own
//! slice of the canvas, scaled to fill it.

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{RenderTarget, ScalingMode};
use bevy::diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::{TextureFormat, TextureUsages};
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use bevy::window::{Monitor, MonitorSelection, RawHandleWrapper, WindowFocused, WindowMode, WindowRef};

use crate::config::Config;
use crate::input::Action;
use crate::scene::{spawn_camera, View};
use crate::Args;

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
fn open_displays(mut commands: Commands, args: Res<Args>, cfg: Res<Config>, canvas: Option<Res<Canvas>>, mut images: ResMut<Assets<Image>>, monitors: Query<(Entity, &Monitor)>) {
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
        .filter(|(_, m)| cfg.run.outputs.is_empty() || matches(m, &cfg.run.outputs))
        .filter(|(_, m)| !matches(m, &cfg.run.exclude))
        .collect();
    chosen.sort_by_key(|(_, m)| m.physical_position.x);
    if chosen.is_empty() {
        error!("no monitor matched {:?} less {:?}; monitors: {:?}", cfg.run.outputs, cfg.run.exclude, descriptions);
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
    spawn_camera(&mut commands, &cfg, RenderTarget::Image(canvas.clone().into()));
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
                mode: WindowMode::BorderlessFullscreen(MonitorSelection::Entity(entity)),
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
fn take_focus(args: Res<Args>, mut done: Local<bool>, windows: Query<(), (With<Window>, With<RawHandleWrapper>)>) {
    // Once the windows exist on the compositor, which their handles mark.
    if args.hold || *done || windows.is_empty() {
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
fn pace(cfg: Res<Config>, mut last: Local<Option<std::time::Instant>>) {
    if cfg.run.fps == 0 {
        return;
    }
    let period = std::time::Duration::from_secs_f64(1.0 / cfg.run.fps as f64);
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
fn end_on_lock_or_limit(args: Res<Args>, cfg: Res<Config>, time: Res<Time>, mut focus: MessageReader<WindowFocused>, mut focused: Local<std::collections::HashMap<Entity, bool>>, mut unfocused_since: Local<Option<f32>>, mut exit: MessageWriter<AppExit>) {
    for f in focus.read() {
        focused.insert(f.window, f.focused);
    }
    if args.hold {
        return;
    }
    let now = time.elapsed_secs();
    let lost = !focused.is_empty() && !focused.values().any(|&f| f);
    if !lost {
        *unfocused_since = None;
    } else if now - *unfocused_since.get_or_insert(now) > 0.5 {
        info!("ending: focus lost (the session locked, or something else took the screen)");
        exit.write(AppExit::Success);
    }
    if cfg.run.limit > 0.0 && now >= cfg.run.limit * 60.0 {
        info!("ending: time limit of {} minutes", cfg.run.limit);
        exit.write(AppExit::Success);
    }
}

/// The input asking for the end (any key or pointer movement that is not
/// the panel's, see `input.rs`) ends the run, unless --hold.
fn end_on_input(args: Res<Args>, mut actions: MessageReader<Action>, mut exit: MessageWriter<AppExit>) {
    for action in actions.read() {
        if let Action::End(why) = action {
            if args.hold {
                continue;
            }
            info!("ending: {why}");
            exit.write(AppExit::Success);
        }
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
