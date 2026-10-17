// The picture. Edit and save: the running host reloads this file on the next
// frame. Errors go to the host's log and the previous picture stays up.
//
// Sections: the bindings; the camera and space; the Glyph; the Droplet; the
// glyph atlas; the aura and the tube look; the frame.

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
// The Glyph is the system's one visible element: a character painted at a
// point in space at time `born`, which fades from a brightness of its own,
// somewhere between TRAIL_MIN and TRAIL_MAX, to nothing over LIFE seconds, at which point it is gone: nothing draws it,
// nothing keeps it. Whoever painted it chose where in the text it starts;
// as it fades, every TICK seconds it reads on to the next character. (The
// droplet's own head is drawn with the same look, at full brightness.)

struct Glyph {
  pos: vec3<f32>,   // in space, as above
  size: f32,        // its cell's height at the near plane, in screen heights
  born: f32,        // the time it was painted
  start: u32,       // its first character's place in the text
};

const LIFE: f32 = 10.0;          // seconds from painted to gone: longer than a run, so a trail spans the screen
const TRAIL_MIN: f32 = 0.1;      // a painted glyph's brightness at birth is random, between these
const TRAIL_MAX: f32 = 0.7;
const DECAY: f32 = 6.0;          // e-folds of fading across a life: bright at first, quickly down to the floor
const FLOOR: f32 = 0.08;         // the fraction of birth brightness the tail holds at, until LIFE ends it
const TICK: f32 = 0.75;          // seconds between characters as it fades
const GLYPH_SIZE: f32 = 0.03125; // a glyph's cell height at the near plane, in screen heights

fn glyph_age(g: Glyph, t: f32) -> f32 { return t - g.born; }
fn glyph_alive(g: Glyph, t: f32) -> bool { let a = glyph_age(g, t); return a >= 0.0 && a < LIFE; }
// A random number in 0..1 for an integer, stable from frame to frame.
fn hash(n: u32) -> f32 {
  var x = n * 747796405u + 2891336453u;
  x = ((x >> ((x >> 28u) + 4u)) ^ x) * 277803737u;
  x = (x >> 22u) ^ x;
  return f32(x) / 4294967295.0;
}

// Each glyph's own brightness at birth, from its start and its moment.
fn glyph_bright(g: Glyph) -> f32 {
  return mix(TRAIL_MIN, TRAIL_MAX, hash(g.start * 7919u + u32(g.born * 997.0)));
}

// Exponential decay down to a floor: most of the light goes early, the rest
// holds as a tail, and a linear term takes it exactly to nothing at LIFE.
fn glyph_opacity(g: Glyph, t: f32) -> f32 {
  let a = clamp(glyph_age(g, t) / LIFE, 0.0, 1.0);
  return glyph_bright(g) * max(exp(-DECAY * a), FLOOR) * (1.0 - a);
}

// The character a glyph shows at time t: the text, read on from its start.
fn glyph_char(g: Glyph, t: f32) -> u32 {
  let step = u32(floor(max(glyph_age(g, t), 0.0) / TICK));
  return text[(g.start + step) % arrayLength(&text)];
}

// The light a glyph casts on the screen point p (screen heights from the
// centre, y up), at pixel px, at time t. Nothing once it is gone.
fn paint(g: Glyph, p: vec2<f32>, px: vec2<f32>, t: f32) -> vec3<f32> {
  if (!glyph_alive(g, t)) { return vec3<f32>(0.0); }
  return paint_at(g, p, px, t, glyph_opacity(g, t));
}

// The same, at a given brightness.
fn paint_at(g: Glyph, p: vec2<f32>, px: vec2<f32>, t: f32, opacity: f32) -> vec3<f32> {
  let pr = project(g.pos);
  let size = g.size * pr.scale;
  // The glyph's cell: uv 0..1 across it, y down, so the character reads upright.
  let rel = (p - pr.centre) / size;
  let uv = vec2<f32>(rel.x + 0.5, 0.5 - rel.y);
  // The aura reaches beyond the cell; the tube stays within it.
  if (uv.x < -AURA_REACH || uv.x > 1.0 + AURA_REACH || uv.y < -AURA_REACH || uv.y > 1.0 + AURA_REACH) { return vec3<f32>(0.0); }
  let code = glyph_char(g, t);
  var col = aura(code, uv);
  if (uv.x >= 0.0 && uv.x <= 1.0 && uv.y >= 0.0 && uv.y <= 1.0) {
    col += tube(code, uv, size * u.resolution.y, px, t);
  }
  return min(col, vec3<f32>(1.0)) * opacity;
}

// ----------------------------------------------------------------- droplet
//
// The Droplet is a point that falls, reading the text, carrying its head: a
// glyph at full brightness that shows the character being read. Born at
// `origin` at time `born`, it descends at FALL screen heights per second,
// and every SPAWN seconds it reads the next character, and stamps the one
// it is leaving behind as a Glyph where it is, pinned there, to fade. The
// k-th stamp starts at text[start + k].

struct Droplet {
  origin: vec3<f32>,   // where it was born, in space
  born: f32,           // when
  start: u32,          // where in the text it begins reading
};

const FALL: f32 = 0.28125;       // screen heights per second: 3.6 seconds top to bottom
const SPAWN: f32 = 0.125;        // seconds between glyphs
const HEAD: f32 = 1.3;           // the head's brightness, over the full brightness of a fresh glyph
const HEAD_SIZE: f32 = 1.25;     // the head's size, relative to a stamped glyph
const HEAD_WHITE: f32 = 0.2;     // how far the head's colour is pulled toward white
const MARK: bool = false;        // for testing: show the droplet itself as a dot
const MARK_PX: f32 = 5.0;        // the dot's diameter, in drawn pixels

// The screen is one height tall at the near plane and (CAMERA_DEPTH + z) /
// CAMERA_DEPTH heights tall at depth z, so a droplet there has further to
// fall to cross it, at the same speed in space: it looks slower, as it should.
fn droplet_run(d: Droplet) -> f32 { return (CAMERA_DEPTH + d.origin.z) / CAMERA_DEPTH / FALL; }
fn droplet_run_glyphs(d: Droplet) -> u32 { return u32(droplet_run(d) / SPAWN); }
// Where to be born to start at the top of the screen at depth z.
fn droplet_top(x: f32, z: f32) -> vec3<f32> { return vec3<f32>(x, 0.5 * (CAMERA_DEPTH + z) / CAMERA_DEPTH, z); }

fn droplet_pos(d: Droplet, t: f32) -> vec3<f32> {
  return d.origin - vec3<f32>(0.0, FALL * max(t - d.born, 0.0), 0.0);
}

// The k-th glyph a droplet painted: where it was, when, and starting k
// characters on from the droplet's start in the text.
fn droplet_glyph(d: Droplet, k: u32) -> Glyph {
  let when = d.born + f32(k) * SPAWN;
  return Glyph(droplet_pos(d, when), GLYPH_SIZE, when, d.start + k);
}

// The light of the droplet's head, and of every glyph it has stamped that
// is still alive: only the last LIFE / SPAWN of them can be.
fn droplet_paint(d: Droplet, p: vec2<f32>, px: vec2<f32>, t: f32) -> vec3<f32> {
  let age = t - d.born;
  if (age < 0.0) { return vec3<f32>(0.0); }
  // The droplet stops painting at the bottom; its glyphs fade on their own.
  let last = min(u32(floor(age / SPAWN)), droplet_run_glyphs(d));
  let first = u32(max(ceil((age - LIFE) / SPAWN), 0.0));
  if (first > last) { return vec3<f32>(0.0); }
  var col = vec3<f32>(0.0);
  for (var k = first; k <= last; k++) {
    col += paint(droplet_glyph(d, k), p, px, t);
  }
  // The head: where the droplet is now, reading the character after its
  // latest stamp, at full brightness, until it reaches the bottom.
  if (age <= droplet_run(d)) {
    let head = Glyph(droplet_pos(d, t), GLYPH_SIZE * HEAD_SIZE, t, d.start + last + 1u);
    let light = paint_at(head, p, px, t, HEAD);
    col += mix(light, vec3<f32>(max(max(light.r, light.g), light.b)), HEAD_WHITE);
  }
  if (MARK && age <= droplet_run(d)) {
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

const INK: vec3<f32> = vec3<f32>(0.2, 1.0, 0.35);
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

// ---------------------------------------------------------------------- aura
//
// Behind the tube, a third layer: the character's light spread wide and
// soft, like the glow a bright screen leaves on the air around it. It is a
// heavy blur of the glyph's shape, tinted, reaching beyond the cell.

const AURA: f32 = 0.45;          // brightness
const AURA_REACH: f32 = 0.35;    // blur radius, in cell units
const AURA_TINT: vec3<f32> = vec3<f32>(0.2, 0.9, 0.5);

// The glyph's shape repeated over two rings of taps and the centre, each
// tap sharp enough to read as a ghost of the character: the first version,
// and the one that was liked.
fn aura(code: u32, uv: vec2<f32>) -> vec3<f32> {
  var acc = 0.0;
  for (var i = 0u; i < 8u; i++) {
    let a = f32(i) * 0.7853982;   // 45 degree steps
    let dir = vec2<f32>(cos(a), sin(a));
    acc += smoothstep(0.35, 0.65, glyph_sdf(code, uv + dir * AURA_REACH));
    acc += smoothstep(0.35, 0.65, glyph_sdf(code, uv + dir * AURA_REACH * 0.5));
  }
  acc += smoothstep(0.35, 0.65, glyph_sdf(code, uv));
  // Softly to nothing at the reach's edge, so the layer has no border.
  let edge = smoothstep(-AURA_REACH, 0.0, uv.x) * smoothstep(-AURA_REACH, 0.0, 1.0 - uv.x)
           * smoothstep(-AURA_REACH, 0.0, uv.y) * smoothstep(-AURA_REACH, 0.0, 1.0 - uv.y);
  return AURA_TINT * AURA * (acc / 17.0) * edge;
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

// A column: droplets born at `top`, one after another from `since`, each
// reading on in the text from `start`.
fn column(top: vec3<f32>, start: u32, p: vec2<f32>, px: vec2<f32>, t: f32) -> vec3<f32> {
  let probe = Droplet(top, 0.0, 0u);
  let run = droplet_run(probe);
  let per = droplet_run_glyphs(probe) + 1u;
  let n = floor(max(t - u.loaded, 0.0) / run);
  let now = Droplet(top, u.loaded + n * run, start + u32(n) * per);
  var col = droplet_paint(now, p, px, t);
  if (n > 0.0) {
    let before = Droplet(top, u.loaded + (n - 1.0) * run, start + u32(n - 1.0) * per);
    col += droplet_paint(before, p, px, t);
  }
  return col;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
  // Screen space: the origin in the middle, one unit = screen height, y up.
  let p = (in.uv * u.resolution - 0.5 * u.resolution) / u.resolution.y * vec2<f32>(1.0, -1.0);
  let px = in.uv * u.resolution;
  // Columns fall one droplet after another from the moment this shader was
  // loaded: one at the near plane, and one some distance away, which falls
  // the same speed in space and so looks slower and smaller. The previous
  // droplet of each is painted too, for its glyphs still fading.
  var col = column(droplet_top(0.0, 0.0), 0u, p, px, u.time);
  col += column(droplet_top(0.9, 2.0), 1000u, p, px, u.time);
  col = min(col, vec3<f32>(1.0));
  return vec4<f32>(col, 1.0);
}
