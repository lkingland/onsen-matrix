// How a glyph looks. Edit and save: Bevy reloads this file.
//
// A glyph is a quad, 1 + 2 * AURA_REACH cells wide, with the character's
// cell in the middle. Every glyph shares one material, so they are all
// drawn as instances; which character, how bright and how white comes
// from the instance's tag, packed as src/look.rs packs it.
// Three layers: the aura, a wide tinted repeat of the character's shape
// beyond its cell; the tube's glow, the character's light scattered and
// smeared downward; and the tube's core, the character sharp. The glow and
// core sit under an aperture grille of red, green and blue stripes and
// slowly rolling scanlines, the light breathing a little.

import bevy_pbr::render::{
    mesh_view_bindings::globals,
    mesh_functions,
    view_transformations::position_world_to_clip,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var atlas: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var atlas_sampler: sampler;

const BRIGHT_MAX: f32 = 2.0;     // brightness is 8 bits over 0..BRIGHT_MAX; matches src/look.rs

struct Vertex {
  @builtin(instance_index) instance_index: u32,
  @location(0) position: vec3<f32>,
  @location(1) normal: vec3<f32>,
  @location(2) uv: vec2<f32>,
};

struct VertexOutput {
  @builtin(position) position: vec4<f32>,
  @location(0) uv: vec2<f32>,
  @location(1) @interpolate(flat) code: u32,
  @location(2) @interpolate(flat) bright: f32,
  @location(3) @interpolate(flat) white: f32,
};

@vertex
fn vertex(v: Vertex) -> VertexOutput {
  let tag = mesh_functions::get_tag(v.instance_index);
  let world_from_local = mesh_functions::get_world_from_local(v.instance_index);
  let world = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(v.position, 1.0));
  var out: VertexOutput;
  out.position = position_world_to_clip(world.xyz);
  out.uv = v.uv;
  out.code = tag & 0x3ffu;
  out.bright = f32((tag >> 10u) & 0xffu) / 255.0 * BRIGHT_MAX;
  out.white = f32((tag >> 18u) & 0x3fu) / 63.0;
  return out;
}

// ------------------------------------------------------------------- atlas
//
// One signed distance field per character in a grid of cells: 0.5 at the
// glyph's edge, above inside, below outside, the full range spanning
// ATLAS_SPREAD pixels of the cell either side of the edge. Matches src/atlas.rs.

const ATLAS_COLS: u32 = 64u;
const ATLAS_ROWS: u32 = 12u;
const ATLAS_CELL: f32 = 128.0;
const ATLAS_SPREAD: f32 = 20.0;
const AURA_REACH: f32 = 0.35;    // how far the quad extends beyond the cell, in cell units; matches src/look.rs

fn glyph_sdf(code: u32, uv: vec2<f32>) -> f32 {
  let cell = vec2<f32>(f32(code % ATLAS_COLS), f32(code / ATLAS_COLS));
  let inset = 0.5 / ATLAS_CELL;
  let cuv = clamp(uv, vec2<f32>(inset), vec2<f32>(1.0 - inset));
  return textureSample(atlas, atlas_sampler, (cell + cuv) / vec2<f32>(f32(ATLAS_COLS), f32(ATLAS_ROWS))).r;
}

// Crisp coverage, antialiased over one screen pixel.
fn glyph_cover(code: u32, uv: vec2<f32>) -> f32 {
  let d = glyph_sdf(code, uv);
  let aa = 0.7 * fwidth(d);
  return smoothstep(0.5 - aa, 0.5 + aa, d);
}

fn cell_fade(uv: vec2<f32>) -> f32 {
  let e = 0.18;
  return smoothstep(0.0, e, uv.x) * smoothstep(0.0, e, 1.0 - uv.x) * smoothstep(0.0, e, uv.y) * smoothstep(0.0, e, 1.0 - uv.y);
}

fn in_cell(uv: vec2<f32>) -> f32 {
  return step(0.0, uv.x) * step(uv.x, 1.0) * step(0.0, uv.y) * step(uv.y, 1.0);
}

// --------------------------------------------------------------- tube look

const INK: vec3<f32> = vec3<f32>(0.2, 1.0, 0.35);
const STRIPE_PERIOD: f32 = 6.0;  // scanline spacing, in screen pixels
const STRIPE_DEPTH: f32 = 0.55;
const ROLL: f32 = 18.0;          // scanline roll, in pixels per second
const GRILLE_PITCH: f32 = 3.0;   // width of one phosphor stripe, in screen pixels
const GRILLE_FLOOR: f32 = 0.35;
const GRILLE_GAIN: f32 = 1.6;
const BREATHE: f32 = 0.1;
const EDGE_BLUR: f32 = 0.13;
const SCATTER: f32 = 0.8;
const SCATTER_REACH: f32 = 0.3;
const SMEAR: f32 = 0.045;
const SMEAR_GAIN: f32 = 0.9;
const CORE: f32 = 0.85;
const CORE_GRILLE: f32 = 0.5;
const BACK: f32 = 0.7;

fn grille(px: vec2<f32>) -> vec3<f32> {
  let sub = u32(floor(px.x / GRILLE_PITCH)) % 3u;
  var mask = vec3<f32>(GRILLE_FLOOR);
  if (sub == 0u) { mask.r = 1.0; } else if (sub == 1u) { mask.g = 1.0; } else { mask.b = 1.0; }
  return mask * GRILLE_GAIN;
}

fn scanlines(px: vec2<f32>, t: f32) -> f32 {
  let phase = fract((px.y + t * ROLL) / STRIPE_PERIOD);
  let line = 0.5 + 0.5 * cos(phase * 6.2831853);
  return 1.0 - STRIPE_DEPTH * (1.0 - line);
}

fn glow(code: u32, uv: vec2<f32>) -> f32 {
  let d = glyph_sdf(code, uv);
  let smear = max(d, SMEAR_GAIN * glyph_sdf(code, uv - vec2<f32>(0.0, SMEAR)));
  let body = smoothstep(0.5 - EDGE_BLUR, 0.5 + EDGE_BLUR, smear);
  let halo = SCATTER * exp(-max(0.5 - smear, 0.0) / SCATTER_REACH * 3.0) * (1.0 - body) * smoothstep(0.0, 0.2, smear);
  return (body + halo) * cell_fade(uv);
}

fn tube(code: u32, uv: vec2<f32>, px: vec2<f32>, t: f32) -> vec3<f32> {
  let breathe = 1.0 - BREATHE + BREATHE * sin(t * 9.0) * sin(t * 2.3);
  let back = BACK * INK * glow(code, uv) * grille(px) * breathe;
  let front = CORE * INK * glyph_cover(code, uv) * mix(vec3<f32>(1.0), grille(px), CORE_GRILLE);
  return min((back + front) * scanlines(px, t), vec3<f32>(1.0)) * in_cell(uv);
}

// -------------------------------------------------------------------- aura

const AURA: f32 = 0.45;
const AURA_TINT: vec3<f32> = vec3<f32>(0.2, 0.9, 0.5);

fn aura(code: u32, uv: vec2<f32>) -> vec3<f32> {
  var acc = 0.0;
  for (var i = 0u; i < 8u; i++) {
    let a = f32(i) * 0.7853982;
    let dir = vec2<f32>(cos(a), sin(a));
    acc += smoothstep(0.35, 0.65, glyph_sdf(code, uv + dir * AURA_REACH));
    acc += smoothstep(0.35, 0.65, glyph_sdf(code, uv + dir * AURA_REACH * 0.5));
  }
  acc += smoothstep(0.35, 0.65, glyph_sdf(code, uv));
  let edge = smoothstep(-AURA_REACH, 0.0, uv.x) * smoothstep(-AURA_REACH, 0.0, 1.0 - uv.x)
           * smoothstep(-AURA_REACH, 0.0, uv.y) * smoothstep(-AURA_REACH, 0.0, 1.0 - uv.y);
  return AURA_TINT * AURA * (acc / 17.0) * edge;
}

// ------------------------------------------------------------------- frame

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
  // The quad's uv, 0..1 with y down, mapped to the cell: 0..1 across the
  // character, beyond it out to the aura's reach.
  let uv = (in.uv - 0.5) * (1.0 + 2.0 * AURA_REACH) + 0.5;
  let px = in.position.xy;
  let t = globals.time;
  var col = aura(in.code, uv) + tube(in.code, uv, px, t);
  col = mix(col, vec3<f32>(max(max(col.r, col.g), col.b)), in.white);
  // Bevy's Add mode blends premultiplied: out = src + dst * (1 - alpha).
  // Alpha 0 makes it pure addition, so light only ever adds to light and
  // nothing a glyph draws can hide what is behind it.
  return vec4<f32>(col * in.bright, 0.0);
}
