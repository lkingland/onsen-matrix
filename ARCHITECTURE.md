# Architecture

onsen-matrix is Rust on the Bevy engine. The scene is described at a high
level and Bevy does the 3D: transforms, projection, bloom, the windows.

## How it owns the displays

All the chosen displays are one canvas: an image as wide as they are side
by side and as tall as the tallest. One camera renders the scene into it
once per frame, bloom and all, so the displays show one picture with no
seam. Each display gets a borderless fullscreen Bevy window, placed on its
monitor by name, showing its own slice of the canvas scaled to fill it.
Outputs are chosen by selector: a connector name (`HDMI-A-1`) or `desc:`
and part of the make and model Hyprland reports (`desc:LG Electronics`), so
a display is chosen by what it is rather than where it is plugged in.

The settings live in `config.toml`: `ONSEN_MATRIX_CONFIG` if set, else
`~/.config/onsen-matrix/config.toml`; a setting left out keeps its default,
and `--outputs`, `--exclude`, `--fps` and `--limit` override the file. The
repository's `config.toml` lists every setting at its default and is
installed to `/usr/share/onsen-matrix/config.toml`.

While it runs it holds a Wayland idle inhibitor on its surface, as a video
player does, so the compositor's idle timers do not fire and Omarchy's own
screensaver and lock do not take over the display mid-run; the time limit
ends the run, and idle resumes. `hyprctl clients` shows the window as
`inhibitingIdle`.

The run ends on any key, click, wheel or pointer movement; on losing
focus, which is what the session lock does to every window; and at a time
limit (30 minutes; `--limit MINUTES`, 0 for none). None of that polls
anything. `--hold` disables all of it, for iterating. The frame rate is
held at 60 (`--fps N`, 0 for the display's rate). A launch without a
terminal logs to `~/.local/state/onsen-matrix/log`.

## The parts

- `src/scene.rs`: the scene. The rain, the droplets and the camera as
  entities with transforms and velocities, and the systems that move them,
  stamp glyphs and retire droplets. The coordinate system is documented at
  the top; every tunable is a named constant there.
- `src/look.rs`: how glyphs are drawn, all at once. Every glyph is a record
  in one storage buffer, and one mesh holds a quad per record, so the whole
  rain is one draw with one material. The CPU writes a record only when a
  droplet stamps or moves its head; the GPU computes each glyph's fade,
  character and place from the clock.
- `assets/shaders/glyph.wesl`: the material's shader: the vertex stage
  places each quad from its record, the fragment stage draws the tube look
  and the aura. Bevy reloads it on save.
- `src/atlas.rs`: the glyph atlas. Every character as a signed distance
  field in a grid of cells, rendered from Noto Sans CJK at startup, so
  character `n` is cell `n`; distance fields stay crisp at any size and give
  glow and blur by thresholding. Also the text as a sequence of indices.
- `src/config.rs`: the settings, read from `config.toml` into a resource.
- `src/host.rs`: the canvas and the displays, ending the run, snapshots,
  frame-time logging.
- `src/inhibit.rs`: the idle inhibitor, made on winit's own surface through
  its raw handles.
- `bin/dev`: the loop. `bin/dev` builds and holds the picture on one display
  (`ONSEN_MATRIX_MONITOR`, default HDMI-A-1); `bin/dev snap` writes the
  canvas to `target/snap.png`; `bin/dev stop`. `bin/burn-in` samples a long
  run into a CSV and `bin/burn-in report` summarises it.

## The picture

**Space** is in units, y up, ahead is -z. The camera's field of view makes
one unit, one unit ahead, span the canvas's height. The camera starts at
the origin and has a velocity: forward at `camera.walk`, up at `camera.rise`.

**The rain** keeps `rain.droplets` droplets falling in the camera's view:
from `rain.near` to `rain.far` ahead, at each depth as wide as the view is
there, born just above the top of the view at that depth and gone below its
bottom, or once behind the camera. When one goes, another is born somewhere
in the view, reading from a random place in the text.

**The Droplet** is a point falling at `droplet.fall` units per second,
reading the text, carrying its head: a glyph at `HEAD` brightness,
`HEAD_SIZE` larger, pulled `HEAD_WHITE` toward white, showing the character
being read. Every `droplet.spawn` seconds it reads the next character and
stamps the one it is leaving as a Glyph where it is.

**The Glyph** is the one visible element: a quad with the look on it, at a
point in space, born at a brightness of its own between `TRAIL_MIN` and
`TRAIL_MAX`, fading exponentially to a floor and then to nothing at
`glyph.life`. As it fades, every `glyph.tick` seconds it reads on to the
next character. It is
a record in the rain's buffer; its droplet's block of records is reused once
its trail has faded.

**The look** is three layers: the aura, a wide tinted repeat of the
character's shape beyond its cell; the tube's glow, the character's light
scattered and smeared downward; and the tube's core, the character sharp.
The glow and core sit under an aperture grille of red, green and blue
stripes and slowly rolling scanlines, the light breathing a little. Glyphs
are light and blend purely additively, so nothing a glyph draws can hide
what is behind it; the camera adds Bevy's bloom.

## The characters

`assets/lotus-gongyo.txt` is chapters 2 and 16 of the Lotus Sutra in
Kumārajīva's Chinese (妙法蓮華經, Taishō 262): Hōben and Juryō, the chapters
recited in Nichiren Shoshu gongyō, from the CBETA edition's TEI XML with
notes and variant readings stripped. The atlas numbers its characters in
order of first appearance, 744 of them. `assets/lotus-sutra.txt` is the
whole sutra, kept for a later background layer. To use another text,
replace the file and rebuild.

## Performance

1600 droplets (the default is now 1120) on two 4K displays at 60 fps: GPU 42 to 47% busy at a median
95 W and 58 °C, a third of a core, memory flat at about 660 MB, VRAM flat
at 5.75 GB, no dropped frames over 53 minutes. At 120 fps the same scene
drew a median 160 W with peaks to 424 W at 70 °C, which is why 60 is the
default. The CPU cost is the droplets and one buffer upload a frame; the
GPU cost is the fragments, which scale with the glyphs' area on screen, so
density and glyph size are the levers.

## Packaging

`makepkg -D pkg -si` builds the package from the checkout and installs
`/usr/bin/onsen-matrix` and `/usr/share/onsen-matrix/assets`, nothing else.
The binary finds its assets at `ONSEN_MATRIX_ASSETS`, the installed
location, or the source tree it was built from. It depends on a Vulkan
driver, Wayland, Noto Sans CJK and Hyprland, and touches no system
configuration. In Onsen (Omarchy: Onsen Edition) it is an opt-in "Onsen
Matrix" entry under System in the Super+Esc menu, a menu extension in
Onsen's config, run as `onsen-matrix --outputs 'desc:LG Electronics'`.

## Known

- Hyprland gives a window on an unfocused monitor 60 fps, and 120 while
  its monitor is focused; moot at the default 60. `ONSEN_MATRIX_PRESENT`,
  `ONSEN_MATRIX_LATENCY` and `ONSEN_MATRIX_WINDOWED` exist for pacing
  experiments.
- A 60 Hz display in the set paces every window at 60, which is one reason
  to select displays by make and model.
