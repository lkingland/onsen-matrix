# onsen-matrix

A screensaver for Hyprland, written to have no ceiling: Rust on the Bevy
engine. The scene is described at a high level, a glyph here with this
look, a droplet there with this velocity, the camera with that one, and
Bevy does the 3D: transforms, projection, batching, bloom. It is being
built up one small requirement at a time; the aim is a true
three-dimensional rain of characters across three 4K displays, nearer the
film than any "matrix" screensaver so far.

## How it owns the displays

All the displays are one canvas. The canvas is an image as wide as every
chosen display side by side and as tall as the tallest, and the scene is
rendered into it once per frame by one camera, bloom and all, so the
displays show one picture with no seam. Each display gets a borderless
fullscreen Bevy window, placed on its monitor by name, showing its own
slice of the canvas scaled to fill it. Any key, click, wheel or pointer
movement ends the run (`--hold` disables that, for iterating).

    cargo run --release                              every display
    cargo run --release -- --outputs HDMI-A-1,DP-3   these displays, left to right by position
    onsen-matrix --fps 60                            installed, holding 60 fps

## The parts

- `src/scene.rs`: the scene. The rain, the droplets and the camera as
  entities with transforms and velocities, and the systems that move them,
  stamp glyphs and retire droplets. The coordinate system is documented at
  the top, and every tunable is a named constant there.
- `src/look.rs`: how glyphs are drawn, all at once. Every glyph is a
  record in one storage buffer, and one mesh holds a quad per record, so
  the whole rain is one draw with one material. The CPU writes a record
  only when a droplet stamps or moves its head.
- `assets/shaders/glyph.wesl`: the material's shader. The vertex shader
  places each quad from its record and the clock, working out its fade and
  which character it shows; the fragment shader draws the tube look and
  the aura. Bevy reloads it when the file changes.
- `src/atlas.rs`: the glyph atlas. Every character the shader may draw, as
  a signed distance field in a grid of cells, so character `n` is cell `n`.
  Distance fields stay crisp at any size and give glow, blur and outlines by
  thresholding.
- `src/host.rs`: the canvas and the displays, ending the run on input,
  snapshots, frame-time logging.
- `bin/dev`: the iteration loop.

## Iterating

    bin/dev            build, then hold the picture on display 2 (HDMI-A-1)
    bin/dev snap       write the current frame to target/snap.png
    bin/dev stop       take it down

While it holds, input does not end it, and the launcher hands focus back to
the monitor you were on, so the other screens work as usual. Edit
`assets/shaders/glyph.wesl` and save: Bevy reloads it. Shader errors go to
`target/dev.log` and the previous look stays. Changes to `src/` need
`bin/dev` again, which rebuilds in a few seconds and restarts.
`ONSEN_MATRIX_MONITOR=DP-2 bin/dev` picks another output.
`ONSEN_MATRIX_ATLAS=path` writes the atlas as a PGM at startup. Frame
times are logged to `target/dev.log` every second or so.

## The picture so far: the rain, in depth

**Space** is in units, y up, ahead is -z. The camera's field of view is
set so that one unit, one unit ahead, spans the canvas's height, however
many displays wide the canvas is. The camera starts at the origin and has
a velocity: forward at `WALK`, up at `RISE`.

**The rain** keeps `DROPLETS` droplets falling in the camera's view: from
`RAIN_NEAR` to `RAIN_FAR` ahead, and at each depth as wide as the view is
there, born just above the top of the view at that depth and gone below
its bottom, or once behind the camera. When one goes, another is born
somewhere in the view, reading from a random place in the text.

**The Droplet** is a point with a velocity, `FALL` units per second down,
reading the text, carrying its head as a child: a glyph at `HEAD`
brightness, `HEAD_SIZE` larger, pulled `HEAD_WHITE` toward white, showing
the character being read. Every `SPAWN` seconds it reads the next
character and stamps the one it is leaving as a Glyph where it is.

**The Glyph** is the one visible element: a quad with the look on it, at a
point in space, born at a brightness of its own between `TRAIL_MIN` and
`TRAIL_MAX`, fading exponentially to a floor and then to nothing at `LIFE`.
As it fades, every `TICK` seconds it reads on to the next character of the
text. It is a record in the rain's buffer; the GPU draws it from the clock,
and its droplet's block of records is reused once its trail has faded.

**The look** draws a Glyph in three layers: the aura, a wide tinted repeat
of the character's shape beyond its cell; the tube's glow, the character's
light scattered and smeared downward; and the tube's core, the character
sharp. The glow and core sit under an aperture grille of red, green and
blue stripes and slowly rolling scanlines, the light breathing a little.
Glyphs are light and blend purely additively, so nothing a glyph draws can
hide what is behind it; the camera adds Bevy's bloom on top.

## Packaging

    makepkg -D pkg -si

builds the package from this checkout and installs it: `/usr/bin/onsen-matrix`
and `/usr/share/onsen-matrix/assets`, nothing else. The binary looks for
its assets at `ONSEN_MATRIX_ASSETS`, then the installed location, then the
source tree it was built from. It depends on a Vulkan driver, Wayland,
Noto Sans CJK and Hyprland, and touches no system configuration.

In the Onsen edition it is opt-in: an "Onsen Matrix" entry under System in
the Super+Esc menu (a menu extension in the edition's config), shown when
the package is installed. The stock screensaver stays the default and runs
on idle; this one runs only when chosen. `--app-id` sets the windows'
class; the default `onsen-matrix` keeps clear of Omarchy's window rules
for its own screensaver class, which would float the windows.

## Performance

1200 droplets on two 4K displays at 120 fps take about 60% of the GPU and
60% of one core; `--fps 60` halves both. The CPU cost is the droplets
(1200 entities) and one buffer upload a frame; the GPU cost is the
fragments, which scale with the glyphs' area on screen, so density and
glyph size are the levers.

## Known

- Hyprland gives a window on an unfocused monitor 60 fps; the same window
  gets 120 while its monitor is focused. The dev loop hands focus back to
  your monitor, so it always shows 60. In use the screensaver's windows
  hold focus. Accepted. `ONSEN_MATRIX_PRESENT=novsync|immediate|mailbox`,
  `ONSEN_MATRIX_LATENCY=N` and `ONSEN_MATRIX_WINDOWED=1` exist for pacing
  experiments, and `--fps N` holds a rate.

## Before Bevy

v0.1 was a hand-written host: a layer-shell overlay per output drawn by
wgpu with one full-screen shader computing everything per pixel. It is the
`v1` bookmark in the history. It taught the loop and the look; the scene
model is the same, now in entities.

## The characters

`assets/lotus-gongyo.txt` is chapters 2 and 16 of the Lotus Sutra in
Kumārajīva's Chinese (妙法蓮華經, Taishō 262): Hōben and Juryō, the chapters
recited in Nichiren Shoshu gongyō. It was taken from the CBETA edition's TEI
XML with notes and variant readings stripped. The atlas numbers its
characters in order of first appearance, 744 of them, rendered from Noto Sans
CJK. To use another text, replace the file and rebuild.
`assets/lotus-sutra.txt` is the whole sutra, prepared the same way from
如是我聞 on, kept for a later background layer; it is not in the atlas yet.
