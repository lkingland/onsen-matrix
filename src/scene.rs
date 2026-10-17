//! The scene: the rain of droplets, the glyphs they leave, and the camera,
//! as entities with transforms and velocities. Bevy does the 3D.
//!
//! # The coordinate system
//!
//! Space is in units, y up. Bevy's camera looks along -z, so ahead is -z.
//! The camera's field of view is set so that one unit, one unit ahead,
//! spans the screen's height; a glyph of GLYPH_SIZE units at that distance
//! is GLYPH_SIZE of the screen tall. The camera starts at the origin and
//! has a velocity like anything else.
//!
//! The rain falls in the camera's view: from RAIN_NEAR to RAIN_FAR ahead,
//! and at each depth as wide as the view is there (RAIN_SPREAD of its
//! width), born just above the top of the view at that depth and gone
//! below its bottom, or once behind the camera. There are always DROPLETS
//! of them; when one goes, another is born somewhere in the view.
//!
//! Every tunable is a named constant below.

use bevy::mesh::MeshTag;
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;

use crate::look::{tag, Look};

// ------------------------------------------------------------------ camera
const WALK: f32 = 0.2;           // units per second forward
const RISE: f32 = 0.0;           // units per second upward

// -------------------------------------------------------------------- rain
const DROPLETS: usize = 195;     // how many droplets are falling at once
const RAIN_NEAR: f32 = 0.3;      // units ahead of the camera
const RAIN_FAR: f32 = 14.0;      // the furthest, and smallest
const RAIN_SPREAD: f32 = 1.0;    // how much of the view's width at a depth the rain covers, 1 = all of it
const RAIN_MARGIN: f32 = 0.3;    // units above the view a droplet is born, and below it that it dies
const ASPECT: f32 = 16.0 / 9.0;  // the view's width over its height

// ----------------------------------------------------------------- droplet
const FALL: f32 = 0.28125;       // units per second downward
const SPAWN: f32 = 0.125;        // seconds between stamps
const HEAD: f32 = 1.3;           // the head's brightness
const HEAD_SIZE: f32 = 1.25;     // the head's size, relative to a stamped glyph
const HEAD_WHITE: f32 = 0.2;     // how far the head's colour is pulled toward white

// ------------------------------------------------------------------- glyph
const GLYPH_SIZE: f32 = 0.03125; // a glyph's cell height, in units
const LIFE: f32 = 10.0;          // seconds from stamped to gone
const TRAIL_MIN: f32 = 0.1;      // a glyph's brightness at birth is its own, between these
const TRAIL_MAX: f32 = 0.7;
const DECAY: f32 = 6.0;          // e-folds of fading across a life
const FLOOR: f32 = 0.08;         // the fraction of birth brightness the tail holds at
const TICK: f32 = 0.75;          // seconds between characters as it fades

pub struct ScenePlugin;

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Rng(0x2545F491))
            .add_systems(Startup, setup)
            .add_systems(Update, (drift, rain, droplets, glyphs).chain());
    }
}

/// Anything that moves: its transform advances by this each second.
#[derive(Component, Clone, Copy)]
pub struct Velocity(pub Vec3);

/// The camera, which the rain follows.
#[derive(Component)]
pub struct Walker;

/// A falling point reading the text, carrying its head glyph as a child.
/// Every SPAWN seconds it reads the next character and stamps the one it
/// is leaving as a Glyph where it is. Below `bottom`, or once behind the
/// camera, it is despawned.
#[derive(Component)]
pub struct Droplet {
    born: f32,
    /// Where in the text it began reading; its k-th stamp starts at start + k.
    start: usize,
    /// How many glyphs it has stamped so far.
    stamped: u32,
    bottom: f32,
    head: Entity,
}

/// A character at a point in space. Born at `bright`, it fades to nothing
/// at LIFE and is despawned; as it fades it reads on through the text from
/// `start`, a character every TICK.
#[derive(Component)]
pub struct Glyph {
    born: f32,
    start: usize,
    bright: f32,
}

/// A small random number generator, so the rain needs no crate.
#[derive(Resource)]
pub struct Rng(u32);

impl Rng {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        hash(self.0)
    }
    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next()
    }
}

/// A random number in 0..1 for an integer, stable from frame to frame.
fn hash(n: u32) -> f32 {
    let mut x = n.wrapping_mul(747796405).wrapping_add(2891336453);
    x = ((x >> ((x >> 28) + 4)) ^ x).wrapping_mul(277803737);
    x = (x >> 22) ^ x;
    x as f32 / u32::MAX as f32
}

/// The brightness a glyph fades to by `age`: exponential decay down to a
/// floor, and a linear term that takes it exactly to nothing at LIFE.
fn fade(bright: f32, age: f32) -> f32 {
    let a = (age / LIFE).clamp(0.0, 1.0);
    bright * (-DECAY * a).exp().max(FLOOR) * (1.0 - a)
}

fn setup(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection { fov: 2.0 * (0.5f32).atan(), near: 0.01, ..default() }),
        Transform::from_xyz(0.0, 0.0, 0.0),
        Velocity(Vec3::new(0.0, RISE, -WALK)),
        Walker,
        bevy::camera::Hdr,
        bevy::core_pipeline::tonemapping::Tonemapping::TonyMcMapface,
        Bloom::NATURAL,
    ));
}

/// Everything with a velocity moves.
fn drift(time: Res<Time>, mut movers: Query<(&mut Transform, &Velocity)>) {
    let dt = time.delta_secs();
    for (mut t, v) in &mut movers {
        t.translation += v.0 * dt;
    }
}

/// Keeps DROPLETS falling: each one that has gone is replaced by one born
/// at the top of the box, somewhere in it, reading from a random place.
fn rain(mut commands: Commands, time: Res<Time>, look: Res<Look>, mut rng: ResMut<Rng>, camera: Single<&Transform, With<Walker>>, live: Query<(), With<Droplet>>) {
    let cam = camera.translation;
    for _ in live.iter().count()..DROPLETS {
        let ahead = rng.range(RAIN_NEAR, RAIN_FAR);
        // The view at that depth: `ahead` units tall, ASPECT times as wide.
        let half_w = 0.5 * ahead * ASPECT * RAIN_SPREAD;
        let half_h = 0.5 * ahead;
        let x = cam.x + rng.range(-half_w, half_w);
        let z = cam.z - ahead;
        let (top, bottom) = (cam.y + half_h + RAIN_MARGIN, cam.y - half_h - RAIN_MARGIN);
        let start = (rng.next() * look.text.len() as f32) as usize;
        let head = commands
            .spawn((
                Mesh3d(look.quad.clone()),
                MeshMaterial3d(look.material.clone()),
                tag(look.char_at(start), HEAD, HEAD_WHITE),
                Transform::from_scale(Vec3::splat(GLYPH_SIZE * HEAD_SIZE)),
            ))
            .id();
        commands
            .spawn((
                Droplet { born: time.elapsed_secs(), start, stamped: 0, bottom, head },
                Transform::from_xyz(x, top, z),
                Visibility::default(),
                Velocity(Vec3::new(0.0, -FALL, 0.0)),
            ))
            .add_child(head);
    }
}

/// Droplets stamp a glyph every SPAWN seconds, the head reading on; at the
/// bottom, or once behind the camera, they are gone.
fn droplets(mut commands: Commands, time: Res<Time>, look: Res<Look>, camera: Single<&Transform, With<Walker>>, mut drops: Query<(Entity, &mut Droplet, &Transform)>, mut heads: Query<&mut MeshTag>) {
    let now = time.elapsed_secs();
    let cam_z = camera.translation.z;
    for (entity, mut d, t) in &mut drops {
        if t.translation.y < d.bottom || t.translation.z > cam_z {
            commands.entity(entity).despawn();
            continue;
        }
        let due = ((now - d.born) / SPAWN) as u32;
        while d.stamped < due {
            let k = d.stamped as usize;
            let when = d.born + (k as f32) * SPAWN;
            // Where the droplet was at the moment of the stamp: higher, by what it has fallen since.
            let pos = t.translation + Vec3::Y * FALL * (now - when);
            let bright = TRAIL_MIN + (TRAIL_MAX - TRAIL_MIN) * hash((d.start + k) as u32 * 7919 + (d.born * 997.0) as u32);
            commands.spawn((
                Glyph { born: when, start: d.start + k, bright },
                Mesh3d(look.quad.clone()),
                MeshMaterial3d(look.material.clone()),
                tag(look.char_at(d.start + k), bright, 0.0),
                Transform::from_translation(pos).with_scale(Vec3::splat(GLYPH_SIZE)),
            ));
            d.stamped += 1;
        }
        if let Ok(mut head) = heads.get_mut(d.head) {
            *head = tag(look.char_at(d.start + d.stamped as usize), HEAD, HEAD_WHITE);
        }
    }
}

/// Glyphs fade and read on; at LIFE, or once behind the camera, they are
/// despawned, and nothing refers to them.
fn glyphs(mut commands: Commands, time: Res<Time>, look: Res<Look>, camera: Single<&Transform, With<Walker>>, mut glyphs: Query<(Entity, &Glyph, &Transform, &mut MeshTag)>) {
    let now = time.elapsed_secs();
    let cam_z = camera.translation.z;
    for (entity, g, t, mut mesh_tag) in &mut glyphs {
        let age = now - g.born;
        if age >= LIFE || t.translation.z > cam_z {
            commands.entity(entity).despawn();
            continue;
        }
        let step = (age.max(0.0) / TICK) as usize;
        *mesh_tag = tag(look.char_at(g.start + step), fade(g.bright, age), 0.0);
    }
}
