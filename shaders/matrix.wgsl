// The picture. Edit and save: the running host reloads this file on the next
// frame. Errors are printed by the host and the previous shader stays up.

// The Droplet: the system's one element. A glyph that, every TICK seconds,
// becomes a different random character.
const CYCLE: bool = false;       // false holds STILL_CHAR; true cycles every TICK
const STILL_CHAR: u32 = 66u;     // B
const TICK: f32 = 0.75;          // seconds between character changes
const FIRST_CHAR: u32 = 33u;     // the pool: printable ASCII, '!' through '~'
const CHAR_COUNT: u32 = 94u;

// A random number in 0..1 for an integer, stable from frame to frame.
fn hash(n: u32) -> f32 {
  var x = n * 747796405u + 2891336453u;
  x = ((x >> ((x >> 28u) + 4u)) ^ x) * 277803737u;
  x = (x >> 22u) ^ x;
  return f32(x) / 4294967295.0;
}

// The character a droplet with the given seed shows at time t.
fn droplet_char(seed: u32, t: f32) -> u32 {
  if (!CYCLE) { return STILL_CHAR; }
  let tick = u32(floor(t / TICK));
  return FIRST_CHAR + u32(hash(seed * 7919u + tick) * f32(CHAR_COUNT));
}

struct Uniforms {
  resolution: vec2<f32>,
  time: f32,
  _pad: f32,
};
@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;

const ATLAS_COLS: f32 = 16.0;
const ATLAS_ROWS: f32 = 8.0;

// The signed distance field of character `code` at `uv` (0..1 across its
// cell, y down): 0.5 at the edge, above inside, below outside.
fn glyph_sdf(code: u32, uv: vec2<f32>) -> f32 {
  let cell = vec2<f32>(f32(code % 16u), f32(code / 16u));
  let cuv = clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0));
  return textureSample(atlas, atlas_sampler, (cell + cuv) / vec2<f32>(ATLAS_COLS, ATLAS_ROWS)).r;
}

// Crisp coverage of character `code` at `uv`, antialiased for the screen.
fn glyph(code: u32, uv: vec2<f32>) -> f32 {
  let d = glyph_sdf(code, uv);
  let aa = 0.7 * fwidth(d);
  let inside = step(0.0, uv.x) * step(uv.x, 1.0) * step(0.0, uv.y) * step(uv.y, 1.0);
  return smoothstep(0.5 - aa, 0.5 + aa, d) * inside;
}

struct VertexOut {
  @builtin(position) pos: vec4<f32>,
  @location(0) uv: vec2<f32>,
};

// One triangle covering the screen; uv runs 0..1 with y down.
@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VertexOut {
  var out: VertexOut;
  // Two triangles: 0,1,2 and 3,4,5 as a quad of the whole screen.
  var corners = array<vec2<f32>, 6>(vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(-1.0, 1.0), vec2<f32>(-1.0, 1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0));
  let x = corners[i].x;
  let y = corners[i].y;
  out.pos = vec4<f32>(x, y, 0.0, 1.0);
  out.uv = vec2<f32>(x, -y) * 0.5 + 0.5;
  return out;
}

// The tube look.
const STRIPE_PERIOD: f32 = 6.0;  // scanline spacing, in drawn pixels
const STRIPE_DEPTH: f32 = 0.55;  // how dark the gap between lines gets, 0..1
const BLUR: f32 = 0.06;          // edge softness, in SDF units (0.5 spans SPREAD px of the cell)
const GLOW: f32 = 0.35;          // halo strength beyond the edge
const GLOW_REACH: f32 = 0.25;    // halo reach, in SDF units
const INK: vec3<f32> = vec3<f32>(0.6, 1.0, 0.65);

// Light fades to nothing before the cell's edge, so no cell shows as a tile.
fn fade(uv: vec2<f32>) -> f32 {
  let e = 0.18;
  return smoothstep(0.0, e, uv.x) * smoothstep(0.0, e, 1.0 - uv.x) * smoothstep(0.0, e, uv.y) * smoothstep(0.0, e, 1.0 - uv.y);
}

// Soft coverage: the edge spread over BLUR either side, plus a halo that
// decays with distance outside the edge.
fn glyph_soft(code: u32, uv: vec2<f32>) -> f32 {
  let d = glyph_sdf(code, uv);
  let inside = fade(uv);
  let body = smoothstep(0.5 - BLUR, 0.5 + BLUR, d);
  let halo = GLOW * exp(-max(0.5 - d, 0.0) / GLOW_REACH * 3.0) * (1.0 - body);
  return (body + halo) * inside;
}

// Scanlines: a raised cosine across the rows, dark between lines.
fn stripes(y_px: f32) -> f32 {
  let phase = fract(y_px / STRIPE_PERIOD);
  let line = 0.5 + 0.5 * cos(phase * 6.2831853);
  return 1.0 - STRIPE_DEPTH * (1.0 - line);
}

// The tube: an aperture grille of red, green and blue phosphor stripes under
// rolling scanlines; the light breathes, and the glow smears downward.
const GRILLE_PITCH: f32 = 3.0;   // width of one phosphor stripe, in drawn pixels
const GRILLE_FLOOR: f32 = 0.35;  // how much a stripe lets the other two colours through
const GRILLE_GAIN: f32 = 1.6;    // brightness back after the mask darkens things
const ROLL: f32 = 18.0;          // scanline roll, in pixels per second
const BREATHE: f32 = 0.1;        // brightness wobble, as a fraction
const SMEAR: f32 = 0.045;        // downward glow smear, in cell units
const SMEAR_GAIN: f32 = 0.9;     // the smear's brightness relative to the body
const EDGE_BLUR: f32 = 0.13;     // edge softness, in SDF units
const SCATTER: f32 = 0.8;        // halo strength
const SCATTER_REACH: f32 = 0.3;  // halo reach, in SDF units

fn tube(code: u32, uv: vec2<f32>, px: vec2<f32>, t: f32) -> vec3<f32> {
  let inside = fade(uv);
  let d = glyph_sdf(code, uv);
  // The smear: the glyph's light also falls a little below itself.
  let smear = max(d, SMEAR_GAIN * glyph_sdf(code, uv - vec2<f32>(0.0, SMEAR)));
  let body = smoothstep(0.5 - EDGE_BLUR, 0.5 + EDGE_BLUR, smear);
  // The halo reaches zero where the distance field bottoms out, so nothing
  // spills past the glow into the cell.
  let halo = SCATTER * exp(-max(0.5 - smear, 0.0) / SCATTER_REACH * 3.0) * (1.0 - body) * smoothstep(0.0, 0.2, smear);
  // The grille.
  let sub = u32(floor(px.x / GRILLE_PITCH)) % 3u;
  var mask = vec3<f32>(GRILLE_FLOOR);
  if (sub == 0u) { mask.r = 1.0; } else if (sub == 1u) { mask.g = 1.0; } else { mask.b = 1.0; }
  // Rolling scanlines and the breath.
  let phase = fract((px.y + t * ROLL) / STRIPE_PERIOD);
  let line = 0.5 + 0.5 * cos(phase * 6.2831853);
  let breathe = 1.0 - BREATHE + BREATHE * sin(t * 9.0) * sin(t * 2.3);
  let light = INK * (body + halo);
  return light * mask * GRILLE_GAIN * breathe * inside * (1.0 - STRIPE_DEPTH * (1.0 - line));
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
  // Screen space with the origin in the middle, one unit = screen height.
  let p = (in.uv * u.resolution - 0.5 * u.resolution) / u.resolution.y;
  let size = 0.25;                       // the character's cell, in screen heights
  let uv = p / size + vec2<f32>(0.5);
  let col = tube(droplet_char(1u, u.time), uv, in.uv * u.resolution, u.time);
  return vec4<f32>(col, 1.0);
}
