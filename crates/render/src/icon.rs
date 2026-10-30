//! EMCraft's app icon, drawn in code so it is reproducible: a blue rounded square with a white
//! sine wave (a field travelling along a guide) over an amber axis. The amber axis is left out
//! below 32 pixels, where it would blur the wave. The window takes [`rgba`]; the Windows
//! executable and installer take the `.ico` file [`ico`] writes (`cargo xtask icon`, checked
//! in as `apps/emcraft/assets/emcraft.ico`).
//!
//! Each pixel's coverage comes from the signed distance of its centre to the shape (as in
//! [`crate::canvas`]), so one sample per pixel draws smooth edges at every size.
//!
//! ```
//! let px = emcraft_render::icon::rgba(32);
//! assert_eq!(px.len(), 32 * 32 * 4);
//! assert_eq!(emcraft_render::icon::rgba(0).len(), 4); // sides are clamped to 1..=1024
//! let ico = emcraft_render::icon::ico().unwrap();
//! assert_eq!(&ico[..4], &[0, 0, 1, 0]); // an icon directory
//! ```

use crate::canvas::Rgb;

/// The sizes in the `.ico` file: Windows' small, list, large and jumbo icons at 100–200 % scale.
pub const ICO_SIDES: [u32; 9] = [16, 20, 24, 32, 40, 48, 64, 128, 256];

/// The largest side [`rgba`] draws.
pub const MAX_SIDE: u32 = 1024;

// The icon's colours are the app's mark, the same in every theme (like the colour maps'), so
// they are written here rather than in a theme file.
const TOP: Rgb = Rgb(36, 99, 200);
const BOTTOM: Rgb = Rgb(16, 52, 120);
const WAVE: Rgb = Rgb(255, 255, 255);
const AXIS: Rgb = Rgb(255, 184, 48);

/// Points of the wave in the unit square (y down): one period of a sine.
fn wave() -> Vec<(f64, f64)> {
    const N: usize = 96;
    (0..=N)
        .map(|i| {
            let t = i as f64 / N as f64;
            (0.16 + 0.68 * t, 0.5 - 0.22 * (std::f64::consts::TAU * t).sin())
        })
        .collect()
}

/// Distance from `p` to the segment `a`–`b`.
fn to_segment(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
    (p.0 - a.0 - t * dx).hypot(p.1 - a.1 - t * dy)
}

/// Signed distance from `p` to a square of half-side `h` centred on (0.5, 0.5) with corners
/// rounded by `r` (negative inside).
fn rounded_square(p: (f64, f64), h: f64, r: f64) -> f64 {
    let (qx, qy) = ((p.0 - 0.5).abs() - h + r, (p.1 - 0.5).abs() - h + r);
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r
}

/// The icon at `side` × `side` pixels (clamped to 1..=[`MAX_SIDE`]): RGBA, rows top to bottom,
/// alpha not premultiplied.
pub fn rgba(side: u32) -> Vec<u8> {
    let side = side.clamp(1, MAX_SIDE);
    let s = f64::from(side);
    let px = 1.0 / s;
    // Strokes no thinner than ~1.7 pixels stay solid at 16 px.
    let wave_half = (0.06f64).max(0.85 * px);
    let axis_half = (0.022f64).max(0.5 * px);
    let points = wave();
    let coverage = |d: f64| (0.5 - d / px).clamp(0.0, 1.0);
    let mut out = Vec::with_capacity((side * side * 4) as usize);
    for y in 0..side {
        for x in 0..side {
            let p = ((f64::from(x) + 0.5) * px, (f64::from(y) + 0.5) * px);
            let alpha = coverage(rounded_square(p, 0.5 - 0.5 * px, 0.2));
            let mut c = TOP.mix(BOTTOM, p.1);
            if side >= 32 {
                let d = to_segment(p, (0.07, 0.5), (0.93, 0.5)) - axis_half;
                c = c.mix(AXIS, coverage(d));
            }
            let d = points.windows(2).map(|w| to_segment(p, w[0], w[1])).fold(f64::INFINITY, f64::min) - wave_half;
            c = c.mix(WAVE, coverage(d));
            out.extend_from_slice(&[c.0, c.1, c.2, (alpha * 255.0).round() as u8]);
        }
    }
    out
}

/// A Windows `.ico` file of the icon at [`ICO_SIDES`], each a 32-bit bitmap: read by every
/// Windows tool, and the same bytes in every build (a PNG entry would depend on the encoder's
/// features). `None` only when a size overflows the format.
pub fn ico() -> Option<Vec<u8>> {
    let images: Vec<Vec<u8>> = ICO_SIDES.iter().map(|&side| bitmap(side)).collect();
    let count = u16::try_from(images.len()).ok()?;
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]);
    out.extend_from_slice(&count.to_le_bytes());
    let mut offset = 6 + 16 * u32::from(count);
    for (&side, image) in ICO_SIDES.iter().zip(&images) {
        let dim = if side >= 256 { 0 } else { side as u8 };
        let len = u32::try_from(image.len()).ok()?;
        out.extend_from_slice(&[dim, dim, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset = offset.checked_add(len)?;
    }
    for image in images {
        out.extend(image);
    }
    Some(out)
}

/// An icon entry as a device-independent bitmap: the header, BGRA rows bottom to top, then the
/// 1-bit transparency mask (rows padded to 4 bytes).
fn bitmap(side: u32) -> Vec<u8> {
    let side = side.clamp(1, MAX_SIDE);
    let n = side as usize;
    let px = rgba(side);
    let mask_row = n.div_ceil(32) * 4;
    let mut out = Vec::with_capacity(40 + n * n * 4 + mask_row * n);
    let size_image = (n * n * 4 + mask_row * n) as u32;
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&side.to_le_bytes());
    out.extend_from_slice(&(2 * side).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&size_image.to_le_bytes());
    out.extend_from_slice(&[0; 16]);
    for row in px.chunks_exact(n * 4).rev() {
        for p in row.as_chunks::<4>().0 {
            out.extend_from_slice(&[p[2], p[1], p[0], p[3]]);
        }
    }
    for row in px.chunks_exact(n * 4).rev() {
        let mut bits = vec![0u8; mask_row];
        for (x, p) in row.as_chunks::<4>().0.iter().enumerate() {
            if p[3] == 0 {
                bits[x / 8] |= 0x80 >> (x % 8);
            }
        }
        out.extend(bits);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_icon_is_drawn_the_same_every_time() {
        assert_eq!(rgba(48), rgba(48));
        assert_eq!(ico(), ico());
    }

    #[test]
    fn corners_are_clear_and_the_middle_is_opaque() {
        for side in [16u32, 32, 256] {
            let px = rgba(side);
            let n = side as usize;
            let at = |x: usize, y: usize| &px[(y * n + x) * 4..(y * n + x) * 4 + 4];
            assert_eq!(at(0, 0)[3], 0, "{side}");
            assert_eq!(at(n / 2, n / 2)[3], 255, "{side}");
            // The wave crosses the middle in white.
            assert!(at(n / 2, n / 2)[..3].iter().all(|&c| c > 200), "{side}: {:?}", at(n / 2, n / 2));
        }
    }

    #[test]
    fn the_ico_directory_points_at_each_image() {
        let ico = ico().unwrap();
        let count = u16::from_le_bytes([ico[4], ico[5]]) as usize;
        assert_eq!(count, ICO_SIDES.len());
        for (i, &side) in ICO_SIDES.iter().enumerate() {
            let e = &ico[6 + 16 * i..6 + 16 * (i + 1)];
            assert_eq!(u32::from(e[0]), side % 256);
            let len = u32::from_le_bytes([e[8], e[9], e[10], e[11]]) as usize;
            let off = u32::from_le_bytes([e[12], e[13], e[14], e[15]]) as usize;
            let image = &ico[off..off + len];
            let n = side as usize;
            assert_eq!(len, 40 + n * n * 4 + n.div_ceil(32) * 4 * n);
            assert_eq!(u32::from_le_bytes([image[4], image[5], image[6], image[7]]), side);
            assert_eq!(u32::from_le_bytes([image[8], image[9], image[10], image[11]]), 2 * side);
        }
        let last = &ico[6 + 16 * (count - 1)..6 + 16 * count];
        let end = u32::from_le_bytes([last[12], last[13], last[14], last[15]]) + u32::from_le_bytes([last[8], last[9], last[10], last[11]]);
        assert_eq!(end as usize, ico.len());
    }
}
