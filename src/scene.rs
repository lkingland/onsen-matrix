//! The scene: columns, droplets, glyphs, and the camera, as entities with
//! transforms and velocities. Every tunable is a named constant here.
//!
//! Space is in units; the camera's field of view is set so that one unit
//! one unit ahead of the camera spans the screen's height. Bevy's camera
//! looks along -z, so "ahead" is -z. y is up.

use bevy::input::keyboard::KeyboardInput;
use bevy::input::mouse::{MouseButtonInput, MouseMotion, MouseWheel};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};

use crate::look::{GlyphMaterial, GlyphParams, Look};
use crate::Args;

// ------------------------------------------------------------------ camera
const WALK: f32 = 0.2;           // units per second forward
const RISE: f32 = 0.0;           // units per second upward

// ------------------------------------------------------------------- glyph
const GLYPH_SIZE: f32 = 0.03125; // a glyph's cell height, in units
const LIFE: f32 = 10.0;          // seconds from stamped to gone
const TRAIL_MIN: f32 = 0.1;      // a glyph's brightness at birth is its own, between these
const TRAIL_MAX: f32 = 0.7;
const DECAY: f32 = 6.0;          // e-folds of fading across a life
const FLOOR: f32 = 0.08;         // the fraction of birth brightness the tail holds at
const TICK: f32 = 0.75;          // seconds between characters as it fades

// ----------------------------------------------------------------- droplet
const FALL: f32 = 0.28125;       // units per second downward
const SPAWN: f32 = 0.125;        // seconds between stamps
const HEAD: f32 = 1.3;           // the head's brightness
const HEAD_SIZE: f32 = 1.25;     // the head's size, relative to a stamped glyph
const HEAD_WHITE: f32 = 0.2;     // how far the head's colour is pulled toward white

// ----------------------------------------------------------------- columns
/// Where droplets fall: (x, z, where in the text the first one starts).
const COLUMNS: &[(f32, f32, usize)] = &[(0.5, -3.0, 0)];

pub struct ScenePlugin;

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup)
            .add_systems(Update, (drift, columns, droplets, glyphs, end_on_input, snapshot).chain());
    }
}

/// Anything that moves: its transform advances by this each second.
#[derive(Component, Clone, Copy)]
pub struct Velocity(pub Vec3);

/// A character at a point in space, fading from its own birth brightness
/// to nothing at LIFE and then gone; reads on through the text as it fades.
#[derive(Component)]
pub struct Glyph {
    born: f32,
    start: usize,
    bright: f32,
}

/// A falling point reading the text, with a head glyph as a child.
#[derive(Component)]
pub struct Droplet {
    born: f32,
    start: usize,
    stamped: u32,
    bottom: f32,
    head: Entity,
}

/// A place where droplets fall one after another.
#[derive(Component)]
pub struct Column {
    x: f32,
    z: f32,
    next_start: usize,
    live: Option<Entity>,
}

fn setup(mut commands: Commands) {
    // The camera: at the origin, looking ahead, with a velocity. The field
    // of view makes one unit, one unit ahead, span the screen's height.
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection { fov: 2.0 * (0.5f32).atan(), near: 0.01, ..default() }),
        Transform::from_xyz(0.0, 0.0, 0.0),
        Velocity(Vec3::new(0.0, RISE, -WALK)),
        bevy::camera::Hdr,
        bevy::core_pipeline::tonemapping::Tonemapping::TonyMcMapface,
        Bloom::NATURAL,
    ));
    for &(x, z, start) in COLUMNS {
        commands.spawn(Column { x, z, next_start: start, live: None });
    }
}

fn drift(time: Res<Time>, mut movers: Query<(&mut Transform, &Velocity)>) {
    let dt = time.delta_secs();
    for (mut t, v) in &mut movers {
        t.translation += v.0 * dt;
    }
}

/// A random number in 0..1 for an integer, stable from frame to frame.
fn hash(n: u32) -> f32 {
    let mut x = n.wrapping_mul(747796405).wrapping_add(2891336453);
    x = ((x >> ((x >> 28) + 4)) ^ x).wrapping_mul(277803737);
    x = (x >> 22) ^ x;
    x as f32 / u32::MAX as f32
}

/// How tall the window is at a depth z ahead of the camera, in units.
fn window_height(z_ahead: f32) -> f32 {
    z_ahead.max(0.0)
}

/// A column with no live droplet starts one at its top.
fn columns(mut commands: Commands, time: Res<Time>, look: Res<Look>, mut materials: ResMut<Assets<GlyphMaterial>>, mut cols: Query<&mut Column>, live: Query<(), With<Droplet>>) {
    for mut c in &mut cols {
        if c.live.is_some_and(|e| live.get(e).is_ok()) {
            continue;
        }
        // The window the camera's starting point sees at this depth.
        let height = window_height(-c.z);
        let (top, bottom) = (0.5 * height, -0.5 * height);
        let start = c.next_start;
        let stamps = (height / FALL / SPAWN) as usize + 1;
        c.next_start += stamps;
        let head = commands
            .spawn((
                Mesh3d(look.quad.clone()),
                MeshMaterial3d(materials.add(GlyphMaterial { params: GlyphParams { code: look.text[start % look.text.len()], bright: HEAD, white: HEAD_WHITE, _pad: 0.0 }, atlas: look.atlas.clone() })),
                Transform::from_scale(Vec3::splat(GLYPH_SIZE * HEAD_SIZE)),
            ))
            .id();
        let droplet = commands
            .spawn((Droplet { born: time.elapsed_secs(), start, stamped: 0, bottom, head }, Transform::from_xyz(c.x, top, c.z), Visibility::default(), Velocity(Vec3::new(0.0, -FALL, 0.0))))
            .add_child(head)
            .id();
        c.live = Some(droplet);
    }
}

/// Droplets stamp a glyph every SPAWN seconds, the head reading on; at the
/// bottom they are gone.
fn droplets(mut commands: Commands, time: Res<Time>, look: Res<Look>, mut materials: ResMut<Assets<GlyphMaterial>>, mut drops: Query<(Entity, &mut Droplet, &Transform)>, heads: Query<&MeshMaterial3d<GlyphMaterial>>) {
    let now = time.elapsed_secs();
    for (entity, mut d, t) in &mut drops {
        if t.translation.y < d.bottom {
            commands.entity(entity).despawn();
            continue;
        }
        let due = ((now - d.born) / SPAWN) as u32;
        while d.stamped < due {
            let k = d.stamped as usize;
            let bright = TRAIL_MIN + (TRAIL_MAX - TRAIL_MIN) * hash((d.start + k) as u32 * 7919 + (d.born * 997.0) as u32);
            // Where the droplet was at the moment of the stamp.
            let when = d.born + (k as f32) * SPAWN;
            let pos = t.translation + Vec3::new(0.0, -FALL * (when - now), 0.0);
            commands.spawn((
                Glyph { born: when, start: d.start + k, bright },
                Mesh3d(look.quad.clone()),
                MeshMaterial3d(materials.add(GlyphMaterial { params: GlyphParams { code: look.text[(d.start + k) % look.text.len()], bright, white: 0.0, _pad: 0.0 }, atlas: look.atlas.clone() })),
                Transform::from_translation(pos).with_scale(Vec3::splat(GLYPH_SIZE)),
            ));
            d.stamped += 1;
        }
        if let Ok(head) = heads.get(d.head) {
            if let Some(mut m) = materials.get_mut(&head.0) {
                m.params.code = look.text[(d.start + d.stamped as usize) % look.text.len()];
            }
        }
    }
}

/// Glyphs fade, read on, and at LIFE are despawned: the entity and its
/// material go, and nothing refers to them.
fn glyphs(mut commands: Commands, time: Res<Time>, look: Res<Look>, mut materials: ResMut<Assets<GlyphMaterial>>, glyphs: Query<(Entity, &Glyph, &MeshMaterial3d<GlyphMaterial>)>) {
    let now = time.elapsed_secs();
    for (entity, g, material) in &glyphs {
        let age = now - g.born;
        if age >= LIFE {
            commands.entity(entity).despawn();
            continue;
        }
        let a = (age / LIFE).clamp(0.0, 1.0);
        let step = (age.max(0.0) / TICK) as usize;
        if let Some(mut m) = materials.get_mut(&material.0) {
            m.params.bright = g.bright * (-DECAY * a).exp().max(FLOOR) * (1.0 - a);
            m.params.code = look.text[(g.start + step) % look.text.len()];
        }
    }
}

/// Any key, click, wheel or pointer movement ends the run, unless --hold.
fn end_on_input(args: Res<Args>, mut keys: MessageReader<KeyboardInput>, mut buttons: MessageReader<MouseButtonInput>, mut wheel: MessageReader<MouseWheel>, mut motion: MessageReader<MouseMotion>, mut drifted: Local<Vec2>, mut exit: MessageWriter<AppExit>) {
    let moved: Vec2 = motion.read().map(|m| m.delta).sum();
    *drifted += moved;
    let any = keys.read().count() > 0 || buttons.read().count() > 0 || wheel.read().count() > 0 || drifted.length() > 12.0;
    if any && !args.hold {
        exit.write(AppExit::Success);
    }
}

/// A snapshot request names the file to write the frame to.
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
    if target.is_empty() {
        return;
    }
    commands.spawn(Screenshot::primary_window()).observe(save_to_disk(target));
}
