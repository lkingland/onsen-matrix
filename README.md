# onsen-matrix

A screensaver for Hyprland, written to have no ceiling: Rust on the Bevy
engine. The scene is described at a high level, a glyph here with this
look, a droplet there with this velocity, the camera with that one, and
Bevy does the 3D: transforms, projection, batching, bloom. It is being
built up one small requirement at a time; the aim is a true
three-dimensional rain of characters across three 4K displays, nearer the
film than any "matrix" screensaver so far.

## How it owns the display

A Bevy window, borderless fullscreen, with the app id `onsen-matrix`. On
Hyprland a window maps on the focused monitor, so the launcher focuses the
monitor it wants first, the way Omarchy's own screensaver launcher does.
Any key, click, wheel or pointer movement ends the run (`--hold` disables
that, for iterating). Three displays will be three windows, each a camera
into the same scene.

    cargo run --release

## The parts

- `src/scene.rs`: the scene. The rain, droplets, glyphs and the camera as
  entities with transforms and velocities, and the systems that move,
  stamp, fade and retire them. The coordinate system is documented at the
  top, and every tunable is a named constant there.
- `src/look.rs`: how a glyph looks: one material on one quad, shared by
  every glyph so they draw as instances; the per-glyph character,
  brightness and whiteness travel in the instance's tag.
- `assets/shaders/glyph.wesl`: the material's shader: the tube look and
  the aura. Bevy reloads it when the file changes.
- `src/atlas.rs`: the glyph atlas. Every character the shader may draw, as
  a signed distance field in a grid of cells, so character `n` is cell `n`.
  Distance fields stay crisp at any size and give glow, blur and outlines by
  thresholding.
- `src/host.rs`: ending the run on input, snapshots, frame-time logging.
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
set so that one unit, one unit ahead, spans the screen's height. The
camera starts at the origin and has a velocity: forward at `WALK`, up at
`RISE`.

**The rain** keeps `DROPLETS` droplets falling in a box that moves with the
camera: `RAIN_WIDTH` across, from `RAIN_NEAR` to `RAIN_FAR` ahead, born
`RAIN_TOP` above and gone below `RAIN_BOTTOM` or once behind the camera.
When one goes, another is born somewhere in the box, reading from a random
place in the text.

**The Droplet** is a point with a velocity, `FALL` units per second down,
reading the text, carrying its head as a child: a glyph at `HEAD`
brightness, `HEAD_SIZE` larger, pulled `HEAD_WHITE` toward white, showing
the character being read. Every `SPAWN` seconds it reads the next
character and stamps the one it is leaving as a Glyph where it is.

**The Glyph** is the one visible element: a quad with the look on it, at a
point in space, born at a brightness of its own between `TRAIL_MIN` and
`TRAIL_MAX`, fading exponentially to a floor and then to nothing at `LIFE`,
when it is despawned. As it fades, every `TICK` seconds it reads on to the
next character of the text.

**The look** draws a Glyph in three layers: the aura, a wide tinted repeat
of the character's shape beyond its cell; the tube's glow, the character's
light scattered and smeared downward; and the tube's core, the character
sharp. The glow and core sit under an aperture grille of red, green and
blue stripes and slowly rolling scanlines, the light breathing a little.
Glyphs are light and blend purely additively, so nothing a glyph draws can
hide what is behind it; the camera adds Bevy's bloom on top.

## Known

- The window runs at 60 fps on a 120 Hz display with the GPU mostly idle.
  A layer-shell surface on the same display gets 120, so the cap is in the
  window path (winit or Hyprland's handling of the fullscreen window), not
  in the scene. Being chased.

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
