//! The glyph atlas: one texture holding every character the shader may draw,
//! in a grid of equal cells, so a shader can show character `n` by sampling
//! cell `n`. Cells 0..128 are ASCII by codepoint; the shader reaches `A` as
//! 65. Kana and kanji will follow in later cells.

use fontdue::{Font, FontSettings};

pub const COLS: u32 = 16;
pub const ROWS: u32 = 8;
pub const CELL: u32 = 256;

const FONT: &str = "/usr/share/fonts/TTF/JetBrainsMonoNerdFont-Regular.ttf";

/// Rasterizes the atlas and uploads it as an R8 texture with mipmaps off;
/// the shader samples coverage from the red channel.
pub fn upload(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::TextureView {
    let (w, h) = (COLS * CELL, ROWS * CELL);
    let mut pixels = vec![0u8; (w * h) as usize];
    let data = std::fs::read(FONT).expect("the atlas font");
    let font = Font::from_bytes(data, FontSettings::default()).expect("a parseable font");
    let px = CELL as f32 * 0.78;
    let baseline = (CELL as f32 * 0.76) as i32;
    for code in 32u32..127 {
        let ch = char::from_u32(code).unwrap();
        let (m, bitmap) = font.rasterize(ch, px);
        let cx = ((code % COLS) * CELL) as i32;
        let cy = ((code / COLS) * CELL) as i32;
        let left = cx + ((CELL as f32 - m.advance_width) * 0.5) as i32 + m.xmin;
        let top = cy + baseline - (m.ymin + m.height as i32);
        for row in 0..m.height as i32 {
            for col in 0..m.width as i32 {
                let (x, y) = (left + col, top + row);
                if x < cx || y < cy || x >= cx + CELL as i32 || y >= cy + CELL as i32 {
                    continue;
                }
                pixels[(y as u32 * w + x as u32) as usize] = bitmap[(row * m.width as i32 + col) as usize];
            }
        }
    }
    // ONSEN_MATRIX_ATLAS=path writes the atlas as a PGM, to look at it.
    if let Ok(path) = std::env::var("ONSEN_MATRIX_ATLAS") {
        let mut out = format!("P5\n{w} {h}\n255\n").into_bytes();
        out.extend_from_slice(&pixels);
        std::fs::write(&path, out).expect("the atlas dump");
    }
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("atlas"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        &pixels,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w), rows_per_image: Some(h) },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    texture.create_view(&Default::default())
}
