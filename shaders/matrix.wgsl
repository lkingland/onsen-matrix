// The picture. Edit and save: the running host reloads this file on the next
// frame. Errors go to the host's log and the previous picture stays up.
//
// Sections: the bindings; the camera and space; the Droplet; the glyph
// atlas; the tube look; the frame.

// ---------------------------------------------------------------- bindings

struct Uniforms {
  resolution: vec2<f32>,   // the drawn buffer, in pixels
  time: f32,               // seconds since the start
  loaded: f32,             // when this shader was loaded, on the same clock
};
@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;
@group(0) @binding(3) var<storage, read> text: array<u32>;   // the gongyō, as atlas indices in reading order

struct VertexOut {
  @builtin(position) pos: vec4<f32>,
  @location(0) uv: vec2<f32>,   // 0..1 across the screen, y down
};

// Two triangles covering the screen.
@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VertexOut {
  var corners = array<vec2<f32>, 6>(
    vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(-1.0, 1.0),
    vec2<f32>(-1.0, 1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0));
  var out: VertexOut;
  out.pos = vec4<f32>(corners[i], 0.0, 1.0);
  out.uv = vec2<f32>(corners[i].x, -corners[i].y) * 0.5 + 0.5;
  return out;
}

// ---------------------------------------------------------- camera and space
//
// Space is in screen heights at the near plane: a thing at (x, y, 0) of
// size s appears s screen heights tall, s heights right of centre at x = s,
// y up. The camera sits CAMERA_DEPTH behind the near plane and looks along
// +z, so a thing at depth z is scaled by CAMERA_DEPTH / (CAMERA_DEPTH + z):
// half size at z = CAMERA_DEPTH, a quarter at three times that. Nothing is
// nearer than z = 0.

const CAMERA_DEPTH: f32 = 1.0;

struct Projected {
  centre: vec2<f32>,   // on screen, in screen heights from the centre, y up
  scale: f32,          // size on screen per unit of size in space
};

fn project(pos: vec3<f32>) -> Projected {
  let scale = CAMERA_DEPTH / (CAMERA_DEPTH + max(pos.z, 0.0));
  return Projected(pos.xy * scale, scale);
}

// ------------------------------------------------------------------- glyph
//
// The Glyph is the system's one element. Painted at a point in space at
// time `born`, it starts fully bright and fades to nothing over LIFE
// seconds, at which point it is gone: nothing draws it, nothing keeps it.
// While it fades it reads the gongyō: every TICK seconds it becomes the next
// character of the text, starting from `start`.

struct Glyph {
  pos: vec3<f32>,   // in space, as above
  size: f32,        // its cell's height at the near plane, in screen heights
  born: f32,        // the time it was painted
  start: u32,       // its first character's place in the text
};

const LIFE: f32 = 4.0;           // seconds from painted to gone
const TICK: f32 = 0.75;          // seconds between characters
const GLYPH_SIZE: f32 = 0.0625;  // a glyph's cell height at the near plane, in screen heights

fn glyph_age(g: Glyph, t: f32) -> f32 { return t - g.born; }
fn glyph_alive(g: Glyph, t: f32) -> bool { let a = glyph_age(g, t); return a >= 0.0 && a < LIFE; }
fn glyph_opacity(g: Glyph, t: f32) -> f32 { return clamp(1.0 - glyph_age(g, t) / LIFE, 0.0, 1.0); }

// The character a glyph shows at time t: the text, read on from `start`.
fn glyph_char(g: Glyph, t: f32) -> u32 {
  let step = u32(floor(max(glyph_age(g, t), 0.0) / TICK));
  return text[(g.start + step) % arrayLength(&text)];
}

// A random number in 0..1 for an integer, stable from frame to frame.
fn hash(n: u32) -> f32 {
  var x = n * 747796405u + 2891336453u;
  x = ((x >> ((x >> 28u) + 4u)) ^ x) * 277803737u;
  x = (x >> 22u) ^ x;
  return f32(x) / 4294967295.0;
}

// The light a glyph casts on the screen point p (screen heights from the
// centre, y up), at pixel px, at time t. Nothing once it is gone.
fn paint(g: Glyph, p: vec2<f32>, px: vec2<f32>, t: f32) -> vec3<f32> {
  if (!glyph_alive(g, t)) { return vec3<f32>(0.0); }
  let pr = project(g.pos);
  let size = g.size * pr.scale;
  // The glyph's cell: uv 0..1 across it, y down, so the character reads upright.
  let rel = (p - pr.centre) / size;
  let uv = vec2<f32>(rel.x + 0.5, 0.5 - rel.y);
  if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) { return vec3<f32>(0.0); }
  return tube(glyph_char(g, t), uv, size * u.resolution.y, px, t) * glyph_opacity(g, t);
}

// ----------------------------------------------------------------- droplet
//
// The Droplet has no look of its own: it is a point that falls. Born at
// `origin` at time `born`, it descends at FALL screen heights per second,
// and every SPAWN seconds it paints a Glyph where it is, pinned there. The
// k-th glyph reads the text from `start + k`, so a column reads downward.

struct Droplet {
  origin: vec3<f32>,   // where it was born, in space
  born: f32,           // when
  start: u32,          // its first glyph's place in the text
};

const FALL: f32 = 0.125;         // screen heights per second: 8 seconds top to bottom
const SPAWN: f32 = 0.5;          // seconds between glyphs
const RUN: f32 = 1.0 / FALL;     // seconds a droplet lives: top to bottom of the near plane
const RUN_GLYPHS: u32 = u32(RUN / SPAWN);   // glyphs it paints in that time
const MARK: bool = true;         // for testing: show the droplet itself as a dot
const MARK_PX: f32 = 5.0;        // the dot's diameter, in drawn pixels

fn droplet_pos(d: Droplet, t: f32) -> vec3<f32> {
  return d.origin - vec3<f32>(0.0, FALL * max(t - d.born, 0.0), 0.0);
}

// The k-th glyph a droplet painted.
fn droplet_glyph(d: Droplet, k: u32) -> Glyph {
  let when = d.born + f32(k) * SPAWN;
  return Glyph(droplet_pos(d, when), GLYPH_SIZE, when, d.start + k);
}

// The light of every glyph a droplet has painted that is still alive: only
// the last LIFE / SPAWN of them can be.
fn droplet_paint(d: Droplet, p: vec2<f32>, px: vec2<f32>, t: f32) -> vec3<f32> {
  let age = t - d.born;
  if (age < 0.0) { return vec3<f32>(0.0); }
  // The droplet stops painting at the bottom; its glyphs fade on their own.
  let last = min(u32(floor(age / SPAWN)), RUN_GLYPHS);
  let first = u32(max(ceil((age - LIFE) / SPAWN), 0.0));
  if (first > last) { return vec3<f32>(0.0); }
  var col = vec3<f32>(0.0);
  for (var k = first; k <= last; k++) {
    col += paint(droplet_glyph(d, k), p, px, t);
  }
  if (MARK && age <= RUN) {
    let pr = project(droplet_pos(d, t));
    let dist_px = length(p - pr.centre) * u.resolution.y;
    col += vec3<f32>(1.0, 0.3, 0.3) * smoothstep(MARK_PX * 0.5 + 0.7, MARK_PX * 0.5 - 0.7, dist_px);
  }
  return min(col, vec3<f32>(1.0));
}

// ------------------------------------------------------------------- atlas
//
// The atlas holds one signed distance field per character in a grid of
// cells: 0.5 at the glyph's edge, above inside, below outside, the full
// range spanning SPREAD pixels of the cell either side of the edge.

const ATLAS_COLS: u32 = 64u;
const ATLAS_ROWS: u32 = 12u;
const ATLAS_CELL: f32 = 128.0;   // pixels per cell
const ATLAS_SPREAD: f32 = 20.0;  // pixels of the cell per 0.5 of SDF

fn glyph_sdf(code: u32, uv: vec2<f32>) -> f32 {
  let cell = vec2<f32>(f32(code % ATLAS_COLS), f32(code / ATLAS_COLS));
  // Stay half a texel inside the cell, so filtering never reaches the neighbours.
  let inset = 0.5 / ATLAS_CELL;
  let cuv = clamp(uv, vec2<f32>(inset), vec2<f32>(1.0 - inset));
  return textureSample(atlas, atlas_sampler, (cell + cuv) / vec2<f32>(f32(ATLAS_COLS), f32(ATLAS_ROWS))).r;
}

// Crisp coverage, antialiased over one screen pixel: `cell_px` is the
// cell's size on screen in pixels, from which the SDF's slope per pixel follows.
fn glyph(code: u32, uv: vec2<f32>, cell_px: f32) -> f32 {
  let d = glyph_sdf(code, uv);
  let aa = 0.7 * (ATLAS_CELL / cell_px) / (2.0 * ATLAS_SPREAD);
  return smoothstep(0.5 - aa, 0.5 + aa, d);
}

// --------------------------------------------------------------- tube look
//
// A phosphor tube: a sharp core over a scattered, smeared glow, both under
// an aperture grille of red, green and blue stripes and rolling scanlines,
// the light breathing a little.

const INK: vec3<f32> = vec3<f32>(0.6, 1.0, 0.65);
const STRIPE_PERIOD: f32 = 6.0;  // scanline spacing, in drawn pixels
const STRIPE_DEPTH: f32 = 0.55;  // how dark the gap between lines gets, 0..1
const ROLL: f32 = 18.0;          // scanline roll, in pixels per second
const GRILLE_PITCH: f32 = 3.0;   // width of one phosphor stripe, in drawn pixels
const GRILLE_FLOOR: f32 = 0.35;  // how much a stripe lets the other two colours through
const GRILLE_GAIN: f32 = 1.6;    // brightness back after the mask darkens things
const BREATHE: f32 = 0.1;        // brightness wobble, as a fraction
const EDGE_BLUR: f32 = 0.13;     // the glow's edge softness, in SDF units
const SCATTER: f32 = 0.8;        // the glow's halo strength
const SCATTER_REACH: f32 = 0.3;  // the halo's reach, in SDF units
const SMEAR: f32 = 0.045;        // downward smear of the glow, in cell units
const SMEAR_GAIN: f32 = 0.9;     // the smear's brightness relative to the body
const CORE: f32 = 0.85;          // the crisp core's brightness
const CORE_GRILLE: f32 = 0.5;    // how much the grille shows through the core, 0 none, 1 full
const BACK: f32 = 0.7;           // the glow's brightness

// The grille's colour mask at a pixel.
fn grille(px: vec2<f32>) -> vec3<f32> {
  let sub = u32(floor(px.x / GRILLE_PITCH)) % 3u;
  var mask = vec3<f32>(GRILLE_FLOOR);
  if (sub == 0u) { mask.r = 1.0; } else if (sub == 1u) { mask.g = 1.0; } else { mask.b = 1.0; }
  return mask * GRILLE_GAIN;
}

// The scanlines' brightness at a pixel: a raised cosine, dark between lines, rolling.
fn scanlines(px: vec2<f32>, t: f32) -> f32 {
  let phase = fract((px.y + t * ROLL) / STRIPE_PERIOD);
  let line = 0.5 + 0.5 * cos(phase * 6.2831853);
  return 1.0 - STRIPE_DEPTH * (1.0 - line);
}

// Light fades to nothing before the cell's edge, so no cell shows as a tile.
fn fade(uv: vec2<f32>) -> f32 {
  let e = 0.18;
  return smoothstep(0.0, e, uv.x) * smoothstep(0.0, e, 1.0 - uv.x) * smoothstep(0.0, e, uv.y) * smoothstep(0.0, e, 1.0 - uv.y);
}

// The glow: the glyph's light scattered, and smeared a little below itself.
fn glow(code: u32, uv: vec2<f32>) -> f32 {
  let d = glyph_sdf(code, uv);
  let smear = max(d, SMEAR_GAIN * glyph_sdf(code, uv - vec2<f32>(0.0, SMEAR)));
  let body = smoothstep(0.5 - EDGE_BLUR, 0.5 + EDGE_BLUR, smear);
  // The halo reaches zero where the distance field bottoms out, so nothing
  // spills past it into the cell.
  let halo = SCATTER * exp(-max(0.5 - smear, 0.0) / SCATTER_REACH * 3.0) * (1.0 - body) * smoothstep(0.0, 0.2, smear);
  return (body + halo) * fade(uv);
}

// The character `code` as seen on the tube, over its cell uv (cell_px pixels
// on screen), at pixel px.
fn tube(code: u32, uv: vec2<f32>, cell_px: f32, px: vec2<f32>, t: f32) -> vec3<f32> {
  let breathe = 1.0 - BREATHE + BREATHE * sin(t * 9.0) * sin(t * 2.3);
  let back = BACK * INK * glow(code, uv) * grille(px) * breathe;
  let front = CORE * INK * glyph(code, uv, cell_px) * mix(vec3<f32>(1.0), grille(px), CORE_GRILLE);
  return min((back + front) * scanlines(px, t), vec3<f32>(1.0));
}

// ------------------------------------------------------------------- frame

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
  // Screen space: the origin in the middle, one unit = screen height, y up.
  let p = (in.uv * u.resolution - 0.5 * u.resolution) / u.resolution.y * vec2<f32>(1.0, -1.0);
  let px = in.uv * u.resolution;
  // Droplets fall one after another from the top of the screen in the
  // middle, as near as anything can be, from the moment this shader was
  // loaded; each reads on in the text where the last stopped. The previous
  // one is painted too, for its glyphs still fading.
  let n = floor(max(u.time - u.loaded, 0.0) / RUN);
  let now = Droplet(vec3<f32>(0.0, 0.5, 0.0), u.loaded + n * RUN, u32(n) * (RUN_GLYPHS + 1u));
  let before = Droplet(vec3<f32>(0.0, 0.5, 0.0), u.loaded + (n - 1.0) * RUN, u32(max(n - 1.0, 0.0)) * (RUN_GLYPHS + 1u));
  var col = droplet_paint(now, p, px, u.time);
  if (n > 0.0) { col = min(col + droplet_paint(before, p, px, u.time), vec3<f32>(1.0)); }
  return vec4<f32>(col, 1.0);
}
