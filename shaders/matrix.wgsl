// The picture. Edit and save: the running host reloads this file on the next
// frame. Errors are printed by the host and the previous shader stays up.

// Which character to draw: an ASCII code. 65 is A, 66 is B.
const LETTER: u32 = 65u;

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

// Coverage of character `code` at `uv` (0..1 across its cell, y down).
fn glyph(code: u32, uv: vec2<f32>) -> f32 {
  if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) { return 0.0; }
  let cell = vec2<f32>(f32(code % 16u), f32(code / 16u));
  return textureSample(atlas, atlas_sampler, (cell + uv) / vec2<f32>(ATLAS_COLS, ATLAS_ROWS)).r;
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

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
  // Screen space with the origin in the middle, one unit = screen height.
  let p = (in.uv * u.resolution - 0.5 * u.resolution) / u.resolution.y;
  let size = 0.25;                       // the character's cell, in screen heights
  let uv = p / size + vec2<f32>(0.5);
  let ink = glyph(LETTER, uv);
  return vec4<f32>(vec3<f32>(0.6, 1.0, 0.65) * ink, 1.0);
}
