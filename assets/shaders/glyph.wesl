// How a glyph looks. Edit and save: Bevy reloads this file.
//
// Every glyph is a record in one buffer and a quad in one mesh, 1 + 2 *
// AURA_REACH cells wide with the character's cell in the middle. The
// vertex shader places each quad from its record and the clock; the
// fragment shader draws the look.
// Three layers: the aura, a wide tinted repeat of the character's shape
// beyond its cell; the tube's glow, the character's light scattered and
// smeared downward; and the tube's core, the character sharp. The glow and
// core sit under an aperture grille of red, green and blue stripes and
// slowly rolling scanlines, the light breathing a little.

import bevy_pbr::render::{
    mesh_view_bindings::globals,
    view_transformations::position_world_to_clip,
};

// One glyph; matches GlyphRecord in src/look.rs.
struct Glyph {
  pos: vec3<f32>,
  born: f32,
  start: u32,
  bright: f32,
  white: f32,
  kind: u32,     // 0 empty, 1 trail, 2 head
};

// What drawing needs; matches Params in src/look.rs.
struct Params {
  now: f32,
  life: f32,
  tick: f32,
  glyph_size: f32,
  head_size: f32,
  decay: f32,
  floor: f32,
  _pad: f32,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var atlas: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var atlas_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var<storage, read> glyphs: array<Glyph>;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var<storage, read> text: array<u32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var<uniform> params: Params;

struct Vertex {
  @location(0) position: vec3<f32>,   // (record index, corner, 0)
  @location(1) normal: vec3<f32>,
  @location(2) uv: vec2<f32>,         // the corner's, 0..1 with y down
};

struct VertexOutput {
  @builtin(position) position: vec4<f32>,
  @location(0) uv: vec2<f32>,
  @location(1) @interpolate(flat) code: u32,
  @location(2) @interpolate(flat) bright: f32,
  @location(3) @interpolate(flat) white: f32,
};

// A trail glyph's brightness at `age`: exponential decay down to a floor,
// and a linear term that takes it exactly to nothing at the end of its life.
fn fade(bright: f32, age: f32) -> f32 {
  let a = clamp(age / params.life, 0.0, 1.0);
  return bright * max(exp(-params.decay * a), params.floor) * (1.0 - a);
}

// Each quad finds its record and places itself in the world from it, or
// puts itself outside the view if there is nothing to draw.
@vertex
fn vertex(v: Vertex) -> VertexOutput {
  var out: VertexOutput;
  let g = glyphs[u32(v.position.x)];
  let age = params.now - g.born;
  let gone = g.kind == 0u || (g.kind == 1u && age >= params.life);
  if (gone) {
    out.position = vec4<f32>(2.0, 2.0, 2.0, 1.0);
    return out;
  }
  var size = params.glyph_size;
  if (g.kind == 2u) {
    size *= params.head_size;
    out.code = text[g.start % arrayLength(&text)];
    out.bright = g.bright;
  } else {
    let step = u32(max(age, 0.0) / params.tick);
    out.code = text[(g.start + step) % arrayLength(&text)];
    out.bright = fade(g.bright, age);
  }
  out.white = g.white;
  out.uv = v.uv;
  let quad = size * (1.0 + 2.0 * AURA_REACH);
  let world = g.pos + vec3<f32>((v.uv.x - 0.5) * quad, (0.5 - v.uv.y) * quad, 0.0);
  out.position = position_world_to_clip(world);
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
