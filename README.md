# onsen-matrix

A screensaver for Hyprland, written to have no ceiling: Rust, with wgpu for
the GPU and smithay-client-toolkit for the Wayland surfaces. It is being
built up one small requirement at a time; the aim is a true three-dimensional
rain of characters across three 4K displays, nearer the film than any
"matrix" screensaver so far.

## How it owns the display

On Wayland a screensaver is a layer-shell surface on the overlay layer,
anchored to all four edges of an output, with exclusive keyboard focus: it
sits above every window and takes every key. The host makes one such surface
per output (or one, with `--output NAME`) and draws it with wgpu through a
Vulkan swapchain. A viewport lets the buffer be smaller than the output, so
the picture is drawn at `--height` lines (1440 by default) and the compositor
scales it up. Any key or pointer movement ends the run.

    cargo run --release -- --output HDMI-A-1     one output
    cargo run --release                          every output

## The parts

- `src/app.rs`: the Wayland side. Surfaces, the seat, frame pacing by the
  compositor's frame callbacks, the viewport.
- `src/gpu.rs`: the GPU side. One device, the atlas bound to it, and the
  pipeline built from the shader file, rebuilt whenever that file changes.
- `src/atlas.rs`: the glyph atlas. Every character the shader may draw, as a
  signed distance field in a grid of cells, so character `n` is cell `n`.
  Distance fields stay crisp at any size and give glow, blur and outlines by
  thresholding.
- `shaders/matrix.wgsl`: the picture. Everything visual lives here.
- `bin/dev`: the iteration loop.

## Iterating

    bin/dev            build, then hold the picture on display 2 (HDMI-A-1)
    bin/dev snap       write the current frame to target/snap.png
    bin/dev stop       take it down

While it holds, input does not end it and it takes no keyboard, so the other
screens work as usual. Edit `shaders/matrix.wgsl` and save: the next frame
uses it, within a tenth of a second. Shader errors go to `target/dev.log` and
the previous picture stays. Rust changes need `bin/dev` again, which
rebuilds in about a second and restarts. `ONSEN_MATRIX_MONITOR=DP-2 bin/dev`
picks another output; `ONSEN_MATRIX_HEIGHT=2160 bin/dev` draws at another
size (`--height 0` draws every output pixel).

`bin/dev snap` is the host's own frame read back from the GPU, not a screen
capture; on this machine grim loses part of the overlay, so judge by the
snapshot. `ONSEN_MATRIX_ATLAS=path` writes the atlas as a PGM at startup.

## The picture so far: v1 of the Droplet and the Glyph

The shader is in sections, in reading order: bindings; the camera and
space; the Glyph; the Droplet; the frame; then how a glyph is drawn: the
atlas, the tube look, the aura. Every tunable is a named constant at the
top of its section.

**Space** is measured in screen heights at the near plane, the glass of the
screen, z = 0: a thing at (x, y, 0) of size s appears s screen heights tall.
The camera is a pinhole `CAMERA_DEPTH` behind the glass looking straight in,
so a thing at depth z appears `1 / (1 + z)` as large and moves across the
screen `1 / (1 + z)` as fast, and the window the screen shows at depth z is
`1 + z` heights tall. Nothing is nearer than z = 0.

**The Glyph** is the one visible element: a character painted at a point
in space at a moment, at a brightness of its own between `TRAIL_MIN` and
`TRAIL_MAX`, fading exponentially to a floor and then to nothing at `LIFE`,
and then gone. Whoever painted it chose where in the text it starts; as it
fades, every `TICK` seconds it reads on to the next character.

**The Droplet** is a point that falls at `FALL` screen heights per second,
reading the text, carrying its head: a glyph at full brightness, larger and
whiter, showing the character being read. Every `SPAWN` seconds it reads
the next character and stamps the one it is leaving as a Glyph where it is,
pinned there, to fade: the trail. It stops at the bottom of the window at
its depth. `MARK` shows a droplet as a red dot, for watching.

**The look** draws a Glyph in three layers: the aura, a wide tinted repeat
of the character's shape beyond its cell; the tube's glow, the character's
light scattered and smeared downward; and the tube's core, the character
sharp. The glow and core sit under an aperture grille of red, green and
blue stripes and slowly rolling scanlines, the light breathing a little.

**The frame** is a list of columns, `COLUMNS`, each a place where droplets
fall one after another from the moment the shader was loaded, reading on in
the text. There are two: one at the glass, one two units in.

## Next: the third dimension

Everything so far is computed per pixel from the clock: a pixel asks every
column for every glyph that might be alive. That is fine for two columns
and is the wrong shape for a sky of them. The plan:

- **A scene on the host.** Droplets become a list the host owns, spawned
  and retired in Rust, with the same fields the shader's `Droplet` has. The
  glyphs they stamp become a list too, retired when their life ends, which
  is where "gone, no trace" is enforced.
- **One quad per glyph.** Instead of the full-screen pass, the host draws
  each live glyph as a small instanced quad placed by the vertex shader
  through `project`, and the fragment shader draws the look only where a
  glyph is. The cost becomes proportional to glyph area, not screen area
  times glyph count. Light is additive, so order does not matter.
- **Atmosphere.** Far things dimmer and bluer: one line per layer, from
  depth, once there is depth to see.
- **One space, three outputs.** The three displays are three windows onto
  the same space, side by side; each output's surface gets its own camera
  offset. The host already owns every output.
- **The walk.** The camera moving slowly forward, which with depth is what
  makes it a place rather than a picture.

## The characters

`assets/lotus-gongyo.txt` is chapters 2 and 16 of the Lotus Sutra in
Kumārajīva's Chinese (妙法蓮華經, Taishō 262): Hōben and Juryō, the chapters
recited in Nichiren Shoshu gongyō. It was taken from the CBETA edition's TEI
XML with notes and variant readings stripped. The atlas numbers its
characters in order of first appearance, 744 of them, rendered from Noto Sans
CJK. To use another text, replace the file and rebuild.
`assets/lotus-sutra.txt` is the whole sutra, prepared the same way from
如是我聞 on, kept for a later background layer; it is not in the atlas yet.
