//! The glyph atlas: one texture holding every character the shader may draw,
//! in a grid of equal cells, so a shader can show character `n` by sampling
//! cell `n`. Cells 0..128 are ASCII by codepoint; the shader reaches `A` as
//! 65. Kana and kanji will follow in later cells.
//!
//! Each cell holds a signed distance field, not a picture: a texel's value
//! is its distance to the glyph's edge, inside positive, scaled so 0.5 is
//! the edge and the full range spans `SPREAD` pixels. Sampling it with
//! bilinear filtering and thresholding at 0.5 gives a crisp edge at any
//! size on screen, and thresholds either side of 0.5 give outlines, glow and
//! blur for free.

use fontdue::{Font, FontSettings};

pub const COLS: u32 = 16;
pub const ROWS: u32 = 8;
pub const CELL: u32 = 256;
/// Pixels of the cell that the distance range covers on either side of the edge.
pub const SPREAD: f32 = 40.0;

const FONT: &str = "/usr/share/fonts/TTF/JetBrainsMonoNerdFont-Regular.ttf";

/// Builds the atlas and uploads it as an R8 texture.
pub fn upload(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::TextureView {
    let (w, h) = (COLS * CELL, ROWS * CELL);
    let mut pixels = vec![0u8; (w * h) as usize];
    let data = std::fs::read(FONT).expect("the atlas font");
    let font = Font::from_bytes(data, FontSettings::default()).expect("a parseable font");
    let px = CELL as f32 * 0.78;
    let baseline = (CELL as f32 * 0.76) as i32;
    let n = (CELL * CELL) as usize;
    let mut coverage = vec![0u8; n];
    for code in 32u32..127 {
        let ch = char::from_u32(code).unwrap();
        let (m, bitmap) = font.rasterize(ch, px);
        coverage.fill(0);
        let left = ((CELL as f32 - m.advance_width) * 0.5) as i32 + m.xmin;
        let top = baseline - (m.ymin + m.height as i32);
        for row in 0..m.height as i32 {
            for col in 0..m.width as i32 {
                let (x, y) = (left + col, top + row);
                if x < 0 || y < 0 || x >= CELL as i32 || y >= CELL as i32 {
                    continue;
                }
                coverage[(y * CELL as i32 + x) as usize] = bitmap[(row * m.width as i32 + col) as usize];
            }
        }
        let sdf = signed_distance(&coverage, CELL as usize);
        let cx = (code % COLS) * CELL;
        let cy = (code / COLS) * CELL;
        for y in 0..CELL {
            for x in 0..CELL {
                let d = sdf[(y * CELL + x) as usize];
                let v = (0.5 + d / (2.0 * SPREAD)).clamp(0.0, 1.0);
                pixels[((cy + y) * w + cx + x) as usize] = (v * 255.0).round() as u8;
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

/// Signed distance to the edge of the shape whose coverage is given, in
/// pixels, positive inside. The edge is taken at half coverage; the
/// antialiased coverage of edge pixels refines the distance by a fraction.
fn signed_distance(coverage: &[u8], size: usize) -> Vec<f32> {
    let inside: Vec<bool> = coverage.iter().map(|&c| c >= 128).collect();
    let outside: Vec<bool> = inside.iter().map(|&i| !i).collect();
    let d_out = distance_transform(&inside, size); // distance from outside points to the shape
    let d_in = distance_transform(&outside, size); // distance from inside points to the background
    (0..size * size)
        .map(|i| {
            // Fractional refinement from coverage at the edge: 0.5 coverage is distance 0.
            let frac = (coverage[i] as f32 / 255.0) - 0.5;
            if inside[i] { d_in[i].sqrt() - 0.5 + frac } else { -(d_out[i].sqrt() - 0.5) + frac }
        })
        .collect()
}

/// Squared Euclidean distance from every point to the nearest `true` point
/// (Felzenszwalb and Huttenlocher's two-pass transform), in f64 so the
/// large "far" value does not swallow the squared offsets.
fn distance_transform(set: &[bool], size: usize) -> Vec<f32> {
    const FAR: f64 = 1e9;
    let mut grid: Vec<f64> = set.iter().map(|&s| if s { 0.0 } else { FAR }).collect();
    let mut f = vec![0.0f64; size];
    let mut d = vec![0.0f64; size];
    let mut v = vec![0usize; size];
    let mut z = vec![0.0f64; size + 1];
    for x in 0..size {
        for y in 0..size {
            f[y] = grid[y * size + x];
        }
        edt1d(&f, &mut d, &mut v, &mut z);
        for y in 0..size {
            grid[y * size + x] = d[y];
        }
    }
    for y in 0..size {
        f.copy_from_slice(&grid[y * size..(y + 1) * size]);
        edt1d(&f, &mut d, &mut v, &mut z);
        grid[y * size..(y + 1) * size].copy_from_slice(&d);
    }
    grid.into_iter().map(|g| g as f32).collect()
}

fn edt1d(f: &[f64], d: &mut [f64], v: &mut [usize], z: &mut [f64]) {
    let n = f.len();
    let mut k = 0usize;
    v[0] = 0;
    z[0] = f64::NEG_INFINITY;
    z[1] = f64::INFINITY;
    for q in 1..n {
        let mut s;
        loop {
            let p = v[k];
            s = ((f[q] + (q * q) as f64) - (f[p] + (p * p) as f64)) / (2.0 * (q as f64 - p as f64));
            if s <= z[k] && k > 0 {
                k -= 1;
            } else {
                break;
            }
        }
        if s <= z[k] {
            // Only when k == 0: this parabola is lower everywhere; replace.
            v[0] = q;
        } else {
            k += 1;
            v[k] = q;
            z[k] = s;
        }
        z[k + 1] = f64::INFINITY;
    }
    k = 0;
    for q in 0..n {
        while z[k + 1] < q as f64 {
            k += 1;
        }
        let p = v[k];
        d[q] = (q as f64 - p as f64).powi(2) + f[p];
    }
}
