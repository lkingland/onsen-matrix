//! The scene: the rain of droplets, the glyphs they leave, and the camera.
//! Droplets are entities with transforms and velocities; glyphs are
//! records in the rain's buffer (see look.rs), drawn by the GPU from the
//! clock. Bevy does the 3D.
//!
//! # The coordinate system
//!
//! Space is in units, y up. Bevy's camera looks along -z, so ahead is -z.
//! The camera's field of view is set so that one unit, one unit ahead,
//! spans the canvas's height; a glyph of GLYPH_SIZE units at that distance
//! is GLYPH_SIZE of the canvas tall. The camera starts at the origin and
//! has a velocity like anything else.
//!
//! The rain falls in the camera's view: from RAIN_NEAR to RAIN_FAR ahead,
//! and at each depth as wide as the view is there (RAIN_SPREAD of its
//! width), born just above the top of the view at that depth and gone
//! below its bottom, or once behind the camera. There are always DROPLETS
//! of them; when one goes, another is born somewhere in the view.
//!
//! Every tunable is a named constant below.

use bevy::camera::RenderTarget;
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::storage::ShaderBuffer;

use crate::look::{self, GlyphMaterial, GlyphRecord, Params, Rain};

// ------------------------------------------------------------------ camera
const WALK: f32 = 0.2;           // units per second forward
const RISE: f32 = 0.0;           // units per second upward

// -------------------------------------------------------------------- rain
const DROPLETS: usize = 1200;    // how many droplets are falling at once
const RAIN_NEAR: f32 = 0.3;      // units ahead of the camera
const RAIN_FAR: f32 = 14.0;      // the furthest, and smallest
const RAIN_SPREAD: f32 = 1.0;    // how much of the view's width at a depth the rain covers, 1 = all of it
const RAIN_MARGIN: f32 = 0.3;    // units above the view a droplet is born, and below it that it dies

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

/// Records per droplet: its head, and a slot for each glyph that can be
/// alive at once. A droplet's slots are its own until its trail has faded.
const SLOTS: usize = 1 + (LIFE / SPAWN) as usize + 1;
/// Blocks of slots in the pool: enough for every droplet, and for the trails
/// of as many dead ones still fading.
const BLOCKS: usize = 2 * DROPLETS;

pub struct ScenePlugin;

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Rng(0x2545F491))
            .insert_resource(Pool { free: (0..BLOCKS).rev().collect(), retiring: Vec::new() })
            .add_systems(PreStartup, setup)
            .add_systems(Update, (drift, rain, droplets, upload).chain());
    }
}

/// The shape of the whole view the camera sees, across every display: its
/// width over its height. The host inserts it when it opens the displays;
/// until then there is no rain.
#[derive(Resource)]
pub struct View {
    pub aspect: f32,
}

/// Anything that moves: its transform advances by this each second.
#[derive(Component, Clone, Copy)]
pub struct Velocity(pub Vec3);

/// The camera into the scene, which the rain follows. There is one; it
/// renders the whole view into the host's canvas.
#[derive(Component)]
pub struct Walker;

/// A falling point reading the text, with a block of glyph records: its
/// head first, then its trail. Every SPAWN seconds it reads the next
/// character and stamps the one it is leaving as a glyph where it is.
/// Below `bottom`, or once behind the camera, it is despawned; its trail
/// fades on.
#[derive(Component)]
pub struct Droplet {
    born: f32,
    /// Where in the text it began reading; its k-th stamp starts at start + k.
    start: usize,
    /// How many glyphs it has stamped so far.
    stamped: u32,
    bottom: f32,
    /// Its first record: the head; the trail follows.
    block: usize,
}

/// The blocks of records not in use, and those whose trails are still fading.
#[derive(Resource)]
struct Pool {
    free: Vec<usize>,
    retiring: Vec<(f32, usize)>, // (when it is free again, block)
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

fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>, mut meshes: ResMut<Assets<Mesh>>, mut buffers: ResMut<Assets<ShaderBuffer>>, mut materials: ResMut<Assets<GlyphMaterial>>) {
    let params = Params { now: 0.0, life: LIFE, tick: TICK, glyph_size: GLYPH_SIZE, head_size: HEAD_SIZE, decay: DECAY, floor: FLOOR, _pad: 0.0 };
    look::spawn_rain(&mut commands, &mut images, &mut meshes, &mut buffers, &mut materials, BLOCKS * SLOTS, params);
}

/// The camera, rendering to `target` before anything else. The field of
/// view makes one unit, one unit ahead, span the picture's height.
pub fn spawn_camera(commands: &mut Commands, target: RenderTarget) {
    commands.spawn((
        Camera3d::default(),
        Camera { order: -1, ..default() },
        target,
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
/// at the top of the view, somewhere in it, reading from a random place.
/// The very first fill is spread over the whole height, so the scene starts
/// full rather than as a line of heads along the top.
fn rain(mut commands: Commands, time: Res<Time>, view: Option<Res<View>>, mut rng: ResMut<Rng>, mut pool: ResMut<Pool>, mut rain: ResMut<Rain>, cameras: Query<&Transform, With<Walker>>, live: Query<(), With<Droplet>>) {
    let Some(view) = view else { return };
    let Some(cam) = cameras.iter().next().map(|t| t.translation) else { return };
    let now = time.elapsed_secs();
    // Blocks whose trails have faded come back.
    pool.retiring.retain(|&(at, block)| if now >= at { pool_free(&mut rain, block); false } else { true });
    let first_fill = live.is_empty();
    for _ in live.iter().count()..DROPLETS {
        let Some(block) = pool.free.pop() else { break };
        let ahead = rng.range(RAIN_NEAR, RAIN_FAR);
        // The view at that depth: `ahead` units tall, and as wide as the view's shape makes it.
        let half_w = 0.5 * ahead * view.aspect * RAIN_SPREAD;
        let half_h = 0.5 * ahead;
        let x = cam.x + rng.range(-half_w, half_w);
        let z = cam.z - ahead;
        let (top, bottom) = (cam.y + half_h + RAIN_MARGIN, cam.y - half_h - RAIN_MARGIN);
        let y = if first_fill { rng.range(bottom, top) } else { top };
        let start = (rng.next() * rain.text.len() as f32) as usize;
        rain.records[block * SLOTS] = GlyphRecord { pos: [x, y, z], born: now, start: start as u32, bright: HEAD, white: HEAD_WHITE, kind: look::HEAD };
        commands.spawn((Droplet { born: now, start, stamped: 0, bottom, block }, Transform::from_xyz(x, y, z), Velocity(Vec3::new(0.0, -FALL, 0.0))));
    }
}

fn pool_free(rain: &mut Rain, block: usize) {
    for r in &mut rain.records[block * SLOTS..(block + 1) * SLOTS] {
        *r = GlyphRecord::default();
    }
}

/// Droplets move their head, stamp a glyph every SPAWN seconds with the
/// head reading on, and at the bottom, or once behind the camera, are gone:
/// the head record is cleared and the block kept until the trail has faded.
fn droplets(mut commands: Commands, time: Res<Time>, mut rain: ResMut<Rain>, mut pool: ResMut<Pool>, cameras: Query<&Transform, With<Walker>>, mut drops: Query<(Entity, &mut Droplet, &Transform)>) {
    let now = time.elapsed_secs();
    let Some(cam_z) = cameras.iter().next().map(|t| t.translation.z) else { return };
    for (entity, mut d, t) in &mut drops {
        let head = d.block * SLOTS;
        if t.translation.y < d.bottom || t.translation.z > cam_z {
            rain.records[head].kind = look::EMPTY;
            pool.retiring.push((now + LIFE, d.block));
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
            rain.records[head + 1 + k % (SLOTS - 1)] = GlyphRecord { pos: pos.to_array(), born: when, start: (d.start + k) as u32, bright, white: 0.0, kind: look::TRAIL };
            d.stamped += 1;
        }
        let r = &mut rain.records[head];
        r.pos = t.translation.to_array();
        r.start = (d.start + d.stamped as usize) as u32;
    }
}

/// The records and the clock go to the GPU.
fn upload(time: Res<Time>, rain: Res<Rain>, mut buffers: ResMut<Assets<ShaderBuffer>>, mut materials: ResMut<Assets<GlyphMaterial>>) {
    look::upload(&rain, time.elapsed_secs(), &mut buffers, &mut materials);
}
