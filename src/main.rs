//! onsen-matrix: a screensaver that owns each output through a layer-shell
//! overlay surface and draws it with wgpu.
//!
//! The host (this program) does the fixed work: surfaces, the GPU, input,
//! frame pacing, the glyph atlas. The picture is `shaders/matrix.wgsl`,
//! reloaded whenever the file changes, so it can be iterated on without a
//! restart. Any key or pointer movement ends the run, unless `--hold`.

mod app;
mod atlas;
mod gpu;

use std::path::PathBuf;

/// The command line.
pub struct Args {
    /// `--output NAME`: only this output (e.g. HDMI-A-1); all of them otherwise.
    pub output: Option<String>,
    /// `--hold`: never end on input, and take no keyboard, so the other
    /// screens stay usable while this one holds the picture.
    pub hold: bool,
    /// `--height N`: lines drawn per output, scaled up by the compositor to
    /// fill it; 0 draws every output pixel. Default 1440.
    pub height: u32,
    /// `--shader PATH`: the picture. Default `shaders/matrix.wgsl` in the source tree.
    pub shader: PathBuf,
    /// `ONSEN_MATRIX_SNAP=PATH`: while that file exists, the next frame is
    /// written to the file it names (as PPM) and the request removed.
    pub snap: Option<PathBuf>,
}

impl Args {
    fn parse() -> Self {
        let value = |name: &str| std::env::args().skip_while(|a| a != name).nth(1);
        Args {
            output: value("--output"),
            hold: std::env::args().any(|a| a == "--hold"),
            height: value("--height").and_then(|h| h.parse().ok()).unwrap_or(1440),
            shader: value("--shader").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("shaders/matrix.wgsl")),
            snap: std::env::var("ONSEN_MATRIX_SNAP").ok().map(PathBuf::from),
        }
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("onsen_matrix=info")).init();
    app::run(Args::parse());
}
