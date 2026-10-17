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

## The picture so far

The shader is in sections: bindings; the camera and space; the Droplet; the
atlas; the tube look; the frame.

**Space** is measured in screen heights at the near plane: a thing at
(x, y, 0) of size s appears s screen heights tall. The camera sits
`CAMERA_DEPTH` behind the near plane and looks along +z, so a thing at depth
z is scaled by `CAMERA_DEPTH / (CAMERA_DEPTH + z)`. Nothing is nearer than
z = 0.

**The Droplet** is the system's one element: a glyph at a point in space
(`pos`, `size`, `seed`) that every `TICK` seconds becomes a different random
character from the pool. `CYCLE` holds it still on `STILL_CHAR`.

**The tube look** draws a character the way a phosphor tube would: a sharp
core over a scattered, downward-smeared glow, both under an aperture grille
of red, green and blue stripes and slowly rolling scanlines, the light
breathing a little. Every part of it is a named constant at the top of its
section.

**The frame** currently places one Droplet at the origin, as near as
anything can be, a sixteenth of the screen high.

## The characters

`assets/lotus-gongyo.txt` is chapters 2 and 16 of the Lotus Sutra in
Kumārajīva's Chinese (妙法蓮華經, Taishō 262): Hōben and Juryō, the chapters
recited in Nichiren Shoshu gongyō. It was taken from the CBETA edition's TEI
XML with notes and variant readings stripped. The atlas numbers its
characters in order of first appearance, 744 of them, rendered from Noto Sans
CJK. To use another text, replace the file and rebuild.
`assets/lotus-sutra.txt` is the whole sutra, prepared the same way from
如是我聞 on, kept for a later background layer; it is not in the atlas yet.
