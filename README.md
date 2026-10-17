# onsen-matrix

A screensaver for Hyprland, written to have no ceiling: Rust, with wgpu for
the GPU and smithay-client-toolkit for the Wayland surfaces. Each output gets
a layer-shell overlay surface with exclusive keyboard focus, which is how a
screensaver owns a display on Wayland; any key or pointer movement ends it.

    cargo run --release -- --output HDMI-A-1     one output
    cargo run --release                          every output

## Iterating

    bin/dev            build, then hold the picture on display 2 (HDMI-A-1)
    bin/dev stop       take it down

While it holds, input does not end it and it takes no keyboard, so the other
screens work as usual. Edit `shaders/matrix.wgsl` and save: the next frame
uses it. Shader errors go to `target/dev.log` and the previous picture stays.
Rust changes need `bin/dev` again. `ONSEN_MATRIX_MONITOR=DP-2 bin/dev` picks
another output; `ONSEN_MATRIX_HEIGHT=2160 bin/dev` draws at another size (the
default is 1440 lines per output, scaled up by the compositor; `--height 0`
draws every output pixel).
    bin/dev snap       write the current frame to target/snap.png (the host's own frame, not a screen capture)

## The characters

`assets/lotus-gongyo.txt` is chapters 2 and 16 of the Lotus Sutra in
Kumārajīva's Chinese (妙法蓮華經, Taishō 262): Hōben and Juryō, the chapters
recited in Nichiren Shoshu gongyō. It was taken from the CBETA edition's TEI
XML with notes and variant readings stripped. The atlas numbers its
characters in order of first appearance. To use another text, replace the
file and rebuild.
