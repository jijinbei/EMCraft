//! Text in pictures: DejaVu Sans, its outlines rasterised with anti-aliasing (the exact area
//! of each pixel the outline covers, `ab_glyph`'s rasterizer), kerned. The 5 × 7 bitmap font
//! ([`crate::font`]) draws only if the face cannot be read.
//!
//! A label's size is its `scale`, as for the bitmap font it replaces: the text's box is about
//! 7 × `scale` pixels high, from its top at the y given down to the baseline, descenders below.

use std::sync::OnceLock;

use ab_glyph::{Font, FontRef, GlyphId, PxScale, ScaleFont, point};

use crate::font;

/// DejaVu Sans 2.37 (Bitstream Vera licence, crates/render/fonts/LICENSE-DejaVu.txt).
static FACE_BYTES: &[u8] = include_bytes!("../fonts/DejaVuSans.ttf");

/// The em of DejaVu Sans whose capitals stand `scale` × 7 pixels, about.
const EM_PER_SCALE: f32 = 9.0;

/// DejaVu Sans's capital height in ems (1493 of 2048 units).
const CAP_HEIGHT: f32 = 0.729;

fn face() -> Option<&'static FontRef<'static>> {
    static FACE: OnceLock<Option<FontRef<'static>>> = OnceLock::new();
    FACE.get_or_init(|| FontRef::try_from_slice(FACE_BYTES).ok()).as_ref()
}

fn em(scale: usize) -> f32 {
    EM_PER_SCALE * scale.clamp(1, 16) as f32
}

/// The glyphs of `text` with their pen positions along the baseline (pixels from the start).
fn layout(face: &FontRef<'static>, text: &str, scale: usize) -> (Vec<(GlyphId, f32)>, f32) {
    let scaled = face.as_scaled(PxScale::from(em(scale)));
    let mut caret = 0.0f32;
    let mut previous: Option<GlyphId> = None;
    let mut glyphs = Vec::with_capacity(text.len());
    for ch in text.chars() {
        let id = face.glyph_id(ch);
        if let Some(p) = previous {
            caret += scaled.kern(p, id);
        }
        glyphs.push((id, caret));
        caret += scaled.h_advance(id);
        previous = Some(id);
    }
    (glyphs, caret)
}

/// Width in pixels of `text` at `scale`.
pub fn width(text: &str, scale: usize) -> f64 {
    match face() {
        Some(f) => f64::from(layout(f, text, scale).1),
        None => (text.chars().count() * font::ADVANCE * scale.clamp(1, 16)) as f64,
    }
}

/// Calls `plot(u, v, coverage)` for every pixel `text` covers, `u` along the text from its
/// start and `v` down from the top of its box, in whole pixels.
pub fn rasterize(text: &str, scale: usize, mut plot: impl FnMut(i64, i64, f32)) {
    let Some(face) = face() else {
        bitmap(text, scale, plot);
        return;
    };
    let px = em(scale);
    let baseline = (CAP_HEIGHT * px).round();
    let (glyphs, _) = layout(face, text, scale);
    for (id, x) in glyphs {
        let glyph = id.with_scale_and_position(PxScale::from(px), point(x, baseline));
        if let Some(outline) = face.outline_glyph(glyph) {
            let b = outline.px_bounds();
            let (u0, v0) = (b.min.x as i64, b.min.y as i64);
            outline.draw(|gx, gy, c| plot(u0 + i64::from(gx), v0 + i64::from(gy), c));
        }
    }
}

/// The bitmap font's pixels, when the face is missing.
fn bitmap(text: &str, scale: usize, mut plot: impl FnMut(i64, i64, f32)) {
    let s = scale.clamp(1, 16) as i64;
    for (i, ch) in text.chars().enumerate() {
        for (ry, row) in font::cased(ch).iter().enumerate() {
            for rx in 0..font::W {
                if row & (0x10 >> rx) != 0 {
                    for dy in 0..s {
                        for dx in 0..s {
                            plot((i * font::ADVANCE + rx) as i64 * s + dx, ry as i64 * s + dy, 1.0);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_face_reads_and_covers_the_labels_marks() {
        let f = face().unwrap();
        for ch in "S21 |E| (V/m) εr ωt φ θ ° µ² ₁₀ — dBi λ/4 √".chars().filter(|c| !c.is_whitespace()) {
            assert_ne!(f.glyph_id(ch).0, 0, "{ch}");
        }
    }

    #[test]
    fn text_is_kerned_anti_aliased_and_sits_in_its_box() {
        let (mut full, mut partial, mut top, mut bottom) = (0, 0, i64::MAX, i64::MIN);
        rasterize("Hz", 2, |_, v, c| {
            if c >= 0.99 {
                full += 1;
            } else if c > 0.01 {
                partial += 1;
            }
            if c > 0.5 {
                top = top.min(v);
                bottom = bottom.max(v);
            }
        });
        assert!(full > 20 && partial > 10, "{full} {partial}");
        // Capitals from the top of the box to the baseline, 13 pixels at scale 2.
        assert!(top <= 2 && (11..=14).contains(&bottom), "{top} {bottom}");
        assert!(width("AV", 2) < width("AA", 2), "kerning");
        assert!(width("", 2) == 0.0 && width("mm", 2) > width("ii", 2));
    }
}
