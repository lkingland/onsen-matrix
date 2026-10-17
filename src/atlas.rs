//! The glyph atlas: one texture holding every character the shader may draw,
//! in a grid of equal cells, so a shader can show character `n` by sampling
//! cell `n`. The characters are those of the Lotus Sutra (Kumārajīva's
//! 妙法蓮華經, Taishō 262, from the CBETA text), chapters 2 and 16: the Hōben
//! and Juryō chapters recited in Nichiren Shoshu gongyō
//! (`assets/lotus-gongyo.txt`), numbered in order of first appearance, so
//! the text can be streamed by index.
//!
//! Each cell holds a signed distance field, not a picture: a texel's value
//! is its distance to the glyph's edge, inside positive, scaled so 0.5 is
//! the edge and the full range spans `SPREAD` pixels. Sampling it with
//! bilinear filtering and thresholding at 0.5 gives a crisp edge at any
//! size on screen, and thresholds either side of 0.5 give outlines, glow and
//! blur for free.

use fontdue::{Font, FontSettings};

pub const COLS: u32 = 64;
pub const CELL: u32 = 128;
/// Pixels of the cell that the distance range covers on either side of the edge.
pub const SPREAD: f32 = 20.0;

const FONT: &str = "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc";
const TEXT: &str = include_str!("../assets/lotus-gongyo.txt");

/// The characters, in order of first appearance.
pub fn charset() -> Vec<char> {
    let mut seen = std::collections::HashSet::new();
    TEXT.chars().filter(|c| seen.insert(*c)).collect()
}

/// The text as a sequence of atlas indices, so a shader can read it in order.
pub fn text_indices() -> Vec<u32> {
    let index: std::collections::HashMap<char, u32> = charset().into_iter().enumerate().map(|(i, c)| (c, i as u32)).collect();
    TEXT.chars().map(|c| index[&c]).collect()
}

/// Builds the atlas: one byte per pixel, and its width and height.
pub fn build() -> (Vec<u8>, u32, u32) {
    let chars = charset();
    let rows = (chars.len() as u32).div_ceil(COLS);
    let (w, h) = (COLS * CELL, rows * CELL);
    let mut pixels = vec![0u8; (w * h) as usize];
    let data = std::fs::read(FONT).expect("the atlas font");
    let font = Font::from_bytes(data, FontSettings { collection_index: 0, ..Default::default() }).expect("a parseable font");
    let px = CELL as f32 * 0.8;
    let n = (CELL * CELL) as usize;
    let mut coverage = vec![0u8; n];
    let started = std::time::Instant::now();
    for (i, &ch) in chars.iter().enumerate() {
        let (m, bitmap) = font.rasterize(ch, px);
        coverage.fill(0);
        // Centre the glyph's box in the cell, both ways.
        let left = (CELL as i32 - m.width as i32) / 2;
        let top = (CELL as i32 - m.height as i32) / 2;
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
        let cx = (i as u32 % COLS) * CELL;
        let cy = (i as u32 / COLS) * CELL;
        for y in 0..CELL {
            for x in 0..CELL {
                let d = sdf[(y * CELL + x) as usize];
                let v = (0.5 + d / (2.0 * SPREAD)).clamp(0.0, 1.0);
                pixels[((cy + y) * w + cx + x) as usize] = (v * 255.0).round() as u8;
            }
        }
    }
    log::info!("atlas: {} characters, {w}x{h}, built in {:.2}s", chars.len(), started.elapsed().as_secs_f32());
    // ONSEN_MATRIX_ATLAS=path writes the atlas as a PGM, to look at it.
    if let Ok(path) = std::env::var("ONSEN_MATRIX_ATLAS") {
        let mut out = format!("P5\n{w} {h}\n255\n").into_bytes();
        out.extend_from_slice(&pixels);
        std::fs::write(&path, out).expect("the atlas dump");
    }
    (pixels, w, h)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn charset_is_distinct_and_in_order_of_first_appearance() {
        let chars = charset();
        let set: std::collections::HashSet<_> = chars.iter().collect();
        assert_eq!(set.len(), chars.len());
        assert_eq!(chars[0], '方', "chapter 2 opens 方便品第二");
        assert!(chars.len() <= (COLS * 64) as usize);
    }

    #[test]
    fn text_indices_follow_the_text() {
        let chars = charset();
        let idx = text_indices();
        assert_eq!(idx.len(), TEXT.chars().count());
        let back: String = idx.iter().map(|&i| chars[i as usize]).collect();
        assert_eq!(back, TEXT);
        assert_eq!(&idx[..5], &[0, 1, 2, 3, 4], "the opening characters are new, so number themselves");
    }

    #[test]
    fn distance_of_a_disc_is_its_radius_minus_the_distance_from_centre() {
        let size = 64usize;
        let (cx, cy, r) = (32.0f32, 32.0f32, 12.0f32);
        let coverage: Vec<u8> = (0..size * size)
            .map(|i| {
                let (x, y) = ((i % size) as f32 + 0.5, (i / size) as f32 + 0.5);
                if ((x - cx).powi(2) + (y - cy).powi(2)).sqrt() <= r { 255 } else { 0 }
            })
            .collect();
        let sdf = signed_distance(&coverage, size);
        for &(x, y) in &[(32usize, 32usize), (32, 20), (32, 44), (40, 32), (32, 10), (32, 56), (5, 5)] {
            let want = r - (((x as f32 + 0.5) - cx).powi(2) + ((y as f32 + 0.5) - cy).powi(2)).sqrt();
            let got = sdf[y * size + x];
            assert!((got - want).abs() < 1.2, "at ({x},{y}): want {want}, got {got}");
        }
    }
}
