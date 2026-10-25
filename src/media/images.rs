//! The panel's pictures: the play and pause marks, drawn here rather than
//! taken from a font (no font has both at a matching weight), and the
//! thumbnail, read from the file the player named.

use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

/// The marks' texture side, in pixels.
const MARK_PIXELS: u32 = 64;

/// The play mark (a triangle) or the pause mark (two bars), white on
/// nothing, to be tinted: `MARK_PIXELS` square, with the edges softened
/// over a pixel.
pub fn mark(playing: bool) -> Image {
    let n = MARK_PIXELS;
    // Whether a point of the unit square (y down) is on the mark.
    let inside = |x: f32, y: f32| -> bool {
        if playing {
            // The triangle: left edge at 0.18, point at 0.92 on the middle line.
            let (left, right) = (0.18, 0.92);
            let half = 0.42 * (right - x) / (right - left);
            x >= left && x <= right && (y - 0.5).abs() <= half
        } else {
            let bar = |x0: f32| x >= x0 && x <= x0 + 0.24;
            (0.12..=0.88).contains(&y) && (bar(0.18) || bar(0.58))
        }
    };
    let mut pixels = Vec::with_capacity((n * n * 4) as usize);
    for py in 0..n {
        for px in 0..n {
            // Four by four samples across the pixel, for the soft edge.
            let mut hits = 0;
            for sy in 0..4 {
                for sx in 0..4 {
                    let x = (px as f32 + (sx as f32 + 0.5) / 4.0) / n as f32;
                    let y = (py as f32 + (sy as f32 + 0.5) / 4.0) / n as f32;
                    hits += inside(x, y) as u32;
                }
            }
            pixels.extend_from_slice(&[255, 255, 255, (hits * 255 / 16) as u8]);
        }
    }
    let size = Extent3d { width: n, height: n, depth_or_array_layers: 1 };
    let mut image = Image::new(size, TextureDimension::D2, pixels, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::RENDER_WORLD);
    image.sampler = ImageSampler::linear();
    image
}

/// The thumbnail file into an image. Browsers write PNG; a JPEG would need
/// Bevy's `jpeg` feature. A file still being written reads as not an
/// image yet, which is worth another try.
pub fn thumbnail(path: &std::path::Path) -> Result<Image, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let ext = match bytes.get(..4) {
        Some([0x89, b'P', b'N', b'G']) => "png",
        Some([0xFF, 0xD8, ..]) => "jpg",
        _ => return Err("not an image (yet)".into()),
    };
    Image::from_buffer(&bytes, ImageType::Extension(ext), CompressedImageFormats::NONE, true, ImageSampler::linear(), RenderAssetUsages::RENDER_WORLD).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The alpha of the mark at a point of the unit square.
    fn alpha_at(image: &Image, x: f32, y: f32) -> u8 {
        let n = MARK_PIXELS as f32;
        let i = ((y * n) as u32 * MARK_PIXELS + (x * n) as u32) as usize * 4 + 3;
        image.data.as_ref().expect("pixels")[i]
    }

    #[test]
    fn the_marks_are_where_they_should_be() {
        let play = mark(true);
        assert_eq!(alpha_at(&play, 0.3, 0.5), 255, "inside the triangle");
        assert_eq!(alpha_at(&play, 0.8, 0.1), 0, "outside, above the point");
        assert_eq!(alpha_at(&play, 0.05, 0.5), 0, "before the left edge");
        let pause = mark(false);
        assert_eq!(alpha_at(&pause, 0.3, 0.5), 255, "on the first bar");
        assert_eq!(alpha_at(&pause, 0.5, 0.5), 0, "in the gap");
        assert_eq!(alpha_at(&pause, 0.7, 0.5), 255, "on the second bar");
        assert_eq!(alpha_at(&pause, 0.3, 0.02), 0, "above the bars");
    }

    #[test]
    fn a_half_written_thumbnail_is_not_an_image_yet() {
        let dir = std::env::temp_dir().join(format!("onsen-matrix-thumb-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("art");
        std::fs::write(&path, b"\x89PN").unwrap();
        assert!(thumbnail(&path).unwrap_err().contains("yet"));
        assert!(thumbnail(&dir.join("missing")).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
