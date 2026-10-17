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
another output.
