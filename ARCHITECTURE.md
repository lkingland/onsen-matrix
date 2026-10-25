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

While it runs it holds a Wayland idle inhibitor, as a video player does,
so the compositor's idle timers do not fire and Omarchy's own screensaver
and lock do not take over the display mid-run; the time limit ends the run
by running `run.then`: by default the session lock, a one-second settle,
then the display-off command, so the outputs go dark at once rather than
after the lock's own five seconds. Idle timers restart from zero when the inhibitor goes,
so without the hand-over the displays would stay lit until Omarchy's own
lock time. The inhibitor sits on a roleless surface of its own
connection, not on a window: Hyprland counts an inhibitor on a window only
while the window is visible, a window fading in is not yet visible, and it
re-evaluates only on map, unmap, focus and fullscreen changes, so an
inhibitor on a window evaluated during the fade stayed off for a whole run
(seen across three displays). An inhibitor on a surface with no window is
assumed visible and holds from creation to destruction. Because it is not
on the window, `hyprctl clients` does not show `inhibitingIdle`; the check
is an `ext_idle_notification_v1` that obeys inhibitors never reporting idle
while one that ignores them does.

The run ends on any key, click, wheel or pointer movement, except the few
keys the now-playing panel takes (below); on losing focus, which is what
the session lock does to every window; and at a time limit (30 minutes;
`--limit MINUTES`, 0 for none). None of that polls anything. `--hold`
disables the ending, for iterating; the panel's keys still work. The frame rate is
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
- `src/inhibit.rs`: the idle inhibitor, on a roleless surface of its own
  Wayland connection.
- `src/input.rs`: what a key or the pointer asks of the run, as one
  `Action` per press: the end, or one of the now-playing panel's. The key
  table is one function; the panel says whether it has the keys.
- `src/media/`: what is playing, at the bottom centre of the picture:
  the thumbnail, title, artist and position, in for a while after a track
  change or a key, with a fade. `mod.rs` is the Bevy side: the nodes, and
  the systems that follow the watcher, act on the keys, and paint;
  `presence.rs` is whether the panel is there, a state machine with a
  clock handed in; `images.rs` draws the play and pause marks and decodes
  the thumbnail. Bevy UI targeted at the scene's camera, so it is laid out
  over the whole canvas and drawn into it after bloom and tone mapping,
  sized by the canvas's height, set in the atlas's font. `[media]` in the
  config turns it off, sizes it and times it.
- `crates/nowplaying`: the library behind it, with no Bevy in it. It
  watches the session bus for media players the way the Omarchy bar does
  (every player speaks MPRIS, `org.mpris.MediaPlayer2.*` on D-Bus: Brave for
  the page that is playing, Spotify, mpv), by signal rather than polling,
  with the bar's rule for which player to show and the position carried
  forward from the last report. Its README has the details.
- `bin/dev`: the loop. `bin/dev` builds and holds the picture on one display
  (`ONSEN_MATRIX_MONITOR`, default HDMI-A-1); `bin/dev snap` writes the
  canvas to `target/snap.png`; `bin/dev stop`. `bin/burn-in` samples a long
  run into a CSV and `bin/burn-in report` summarises it. `bin/install`
  builds the package from the checkout and installs it: the only way a
  change reaches the installed binary the menu runs.

## The picture

**Space** is in units, y up, ahead is -z. The camera's field of view makes
one unit, one unit ahead, span the canvas's height. The camera starts at
the origin and has a velocity: forward at `camera.walk`, up at `camera.rise`.

**The rain** births droplets at `rain.rate` per second while fewer than
`rain.droplets` are falling, from the first frame, so the screen starts
empty and fills at that rate. They fall in the camera's view: from
`rain.near` to `rain.far` ahead, at each depth as wide as the view is
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

## What is playing

The media players on the session bus announce their track over MPRIS, and
the Omarchy bar shows the one that is playing. The screensaver shows the
same at the bottom centre: the thumbnail (a browser writes it to a file
under `/tmp` and names the file), the title, the artist, and the position,
which runs on from the last value the player reported without asking
again. It follows the bar's rule when several players are up: the one
playing, the one that started first among those, else one with a track
loaded. Nothing is polled; the panel redraws when the watcher's change
counter moves, once a frame checked. With no session bus, or the panel
off, nothing is drawn and nothing is watched.

The panel is not always there. It fades in when the track changes (or
first appears), stays `hold` seconds, and fades out; a fade takes `fade`
seconds. `[media]` in the config: `show`, `size` (the thumbnail's height
as a fraction of the screen's; the text scales with it), `margin` (the gap
below), `hold` and `fade`.

**The keys.** A screensaver ends on any key, and this one does, except
for the panel's keys, which depend on whether the panel is in:

| key         | panel out        | panel in                         |
|-------------|------------------|----------------------------------|
| Space       | brings it in     | play / pause                     |
| Shift+Space | nothing          | sends it out now                 |
| Right       | ends the run     | next track                       |
| Left        | ends the run     | start of the track, or previous  |
| anything else | ends the run   | ends the run                     |

A control keeps the panel in for another `hold`. Left goes back to the
start when more than three seconds in, else to the previous track, as a
CD player does. Modifier keys on their own do nothing, so a chord can be
pressed. Keys are read by what they mean (the logical key, after the
layout), and Shift from the key stream in order, so a chord reads the
same on any keyboard, virtual ones included. The table is `action` in
`src/input.rs`, and nothing else decides what a key does.

How the parts hang together: `input.rs` knows one thing about the panel,
whether it has the keys (`Focus`), and the panel knows one thing about the
input, the actions it listens for. The host ends the run on the End action
and knows nothing of the panel. The `nowplaying` crate knows nothing of
any of them.

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

`makepkg -D pkg -si`, or `bin/install`, builds the package from the
checkout and installs `/usr/bin/onsen-matrix` and
`/usr/share/onsen-matrix/assets`, nothing else. The package's version
names the commit (`pacman -Q onsen-matrix`), so what is running is never
in doubt. `bin/dev` runs the checkout's build and never touches the
installed one.
The binary finds its assets at `ONSEN_MATRIX_ASSETS`, the installed
location, or the source tree it was built from. It depends on a Vulkan
driver, Wayland, Noto Sans CJK and Hyprland, and touches no system
configuration. In Onsen (Omarchy: Onsen Edition) it is an opt-in "Onsen
Matrix" entry under System in the Super+Esc menu, a menu extension in
Onsen's config, run as `onsen-matrix --outputs 'desc:LG Electronics'`.

## Known

- Hyprland gives a window on an unfocused monitor 60 fps, and 120 while
  its monitor is focused; moot at the default 60.
- A 60 Hz display in the set paces every window at 60, which is one reason
  to select displays by make and model.
