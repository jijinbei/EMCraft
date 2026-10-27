//! Text in pictures: DejaVu Sans, its outlines rasterised with anti-aliasing (the exact area
//! of each pixel the outline covers, `ab_glyph`'s rasterizer), kerned. The 5 × 7 bitmap font
//! ([`crate::font`]) draws only if the face cannot be read.
//!
//! Labels may hold TeX math between `$` signs ([`crate::math`]): scripts, fractions, roots,
//! Greek letters.
//!
//! A label's size is its `scale`, as for the bitmap font it replaces: the text's box is about
//! 7 × `scale` pixels high, from its top at the y given down to the baseline, descenders below.

use std::sync::OnceLock;

use ab_glyph::{Font, FontRef, GlyphId, PxScale, ScaleFont, point};

use crate::{font, math};

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

/// A glyph or a rule (a fraction's bar, a root's overline) placed from the text's origin on
/// its baseline: `x` along, `y` down.
#[derive(Clone, Copy, Debug)]
enum Mark {
    Glyph { id: GlyphId, x: f32, y: f32, size: f32 },
    Rule { x: f32, y: f32, w: f32, h: f32 },
}

/// Laid-out text: its marks, its width and how far it reaches above and below the baseline.
#[derive(Clone, Debug, Default)]
struct Boxed {
    marks: Vec<Mark>,
    width: f32,
    ascent: f32,
    descent: f32,
}

impl Boxed {
    /// Appends `other` moved by (dx, dy).
    fn place(&mut self, other: Boxed, dx: f32, dy: f32) {
        self.marks.extend(other.marks.into_iter().map(|m| match m {
            Mark::Glyph { id, x, y, size } => Mark::Glyph { id, x: x + dx, y: y + dy, size },
            Mark::Rule { x, y, w, h } => Mark::Rule { x: x + dx, y: y + dy, w, h },
        }));
        self.ascent = self.ascent.max(other.ascent - dy);
        self.descent = self.descent.max(other.descent + dy);
    }
}

/// The smallest script size, in pixels.
const MIN_SCRIPT: f32 = 7.0;

/// Sets `nodes` at em `size` (TeX's rules much simplified: scripts at 70 %, raised by 0.42 em
/// or lowered by 0.2 em; fractions at 80 % about the math axis at 0.28 em; a root's overline
/// over its body).
fn set(face: &FontRef<'static>, nodes: &[math::Node], size: f32) -> Boxed {
    use math::Node;
    let mut out = Boxed::default();
    for n in nodes {
        let at = out.width;
        match n {
            Node::Text(s) => {
                let b = run(face, s, size);
                out.width += b.width;
                out.place(b, at, 0.0);
            }
            Node::Space(w) => out.width += w * size,
            Node::Scripts { base, sup, sub } => {
                let base = set(face, base, size);
                let small = (0.7 * size).max(MIN_SCRIPT.min(size));
                let x = at + base.width;
                let tall = base.ascent;
                out.width = x;
                out.place(base, at, 0.0);
                let mut reach = 0.0f32;
                if let Some(sub) = sub {
                    let b = set(face, sub, small);
                    reach = reach.max(b.width);
                    out.place(b, x, if sup.is_some() { 0.28 * size } else { 0.2 * size });
                }
                if let Some(sup) = sup {
                    let b = set(face, sup, small);
                    reach = reach.max(b.width + 0.05 * size);
                    out.place(b, x + 0.05 * size, -(0.42 * size).max(tall - 0.45 * small));
                }
                out.width = x + reach;
            }
            Node::Frac { num, den } => {
                let small = (0.8 * size).max(MIN_SCRIPT.min(size));
                let (num, den) = (set(face, num, small), set(face, den, small));
                let (axis, gap, rule) = (0.28 * size, 0.12 * size, (0.06 * size).max(1.0));
                let w = num.width.max(den.width) + 0.2 * size;
                let (nw, nd, da) = (num.width, num.descent, den.ascent);
                out.place(num, at + (w - nw) / 2.0, -(axis + gap + nd));
                let dw = den.width;
                out.place(den, at + (w - dw) / 2.0, -axis + gap + da);
                out.marks.push(Mark::Rule { x: at + 0.05 * size, y: -axis - rule / 2.0, w: w - 0.1 * size, h: rule });
                out.width = at + w;
            }
            Node::Sqrt(body) => {
                let radical = run(face, "√", size);
                let body = set(face, body, size);
                let top = body.ascent.max(0.72 * size) + 0.06 * size;
                let rule = (0.06 * size).max(1.0);
                let x = at + radical.width;
                let bw = body.width;
                out.place(radical, at, 0.0);
                out.place(body, x, 0.0);
                out.marks.push(Mark::Rule { x: x - 0.02 * size, y: -top, w: bw + 0.08 * size, h: rule });
                out.ascent = out.ascent.max(top + rule);
                out.width = x + bw + 0.1 * size;
            }
        }
    }
    out
}

/// Plain characters at em `size`, kerned.
fn run(face: &FontRef<'static>, text: &str, size: f32) -> Boxed {
    let scaled = face.as_scaled(PxScale::from(size));
    let mut caret = 0.0f32;
    let mut previous: Option<GlyphId> = None;
    let mut marks = Vec::with_capacity(text.len());
    for ch in text.chars() {
        let id = face.glyph_id(ch);
        if let Some(p) = previous {
            caret += scaled.kern(p, id);
        }
        marks.push(Mark::Glyph { id, x: caret, y: 0.0, size });
        caret += scaled.h_advance(id);
        previous = Some(id);
    }
    Boxed { marks, width: caret, ascent: 0.76 * size, descent: 0.24 * size }
}

/// Most characters of one label laid out (a label is a line).
const MAX_CHARS: usize = 4096;

fn laid(face: &FontRef<'static>, text: &str, scale: usize) -> Boxed {
    let text: String = text.chars().take(MAX_CHARS).collect();
    set(face, &math::parse(&text), em(scale))
}

/// Width in pixels of `text` at `scale`.
pub fn width(text: &str, scale: usize) -> f64 {
    match face() {
        Some(f) => f64::from(laid(f, text, scale).width),
        None => (math::to_unicode(text).chars().count() * font::ADVANCE * scale.clamp(1, 16)) as f64,
    }
}

/// Calls `plot(u, v, coverage)` for every pixel `text` covers, `u` along the text from its
/// start and `v` down from the top of its box, in whole pixels. `$…$` is TeX math
/// ([`crate::math`]).
pub fn rasterize(text: &str, scale: usize, mut plot: impl FnMut(i64, i64, f32)) {
    let Some(face) = face() else {
        bitmap(&math::to_unicode(text), scale, plot);
        return;
    };
    let baseline = (CAP_HEIGHT * em(scale)).round();
    for mark in laid(face, text, scale).marks {
        match mark {
            Mark::Glyph { id, x, y, size } => {
                let glyph = id.with_scale_and_position(PxScale::from(size), point(x, baseline + y));
                if let Some(outline) = face.outline_glyph(glyph) {
                    let b = outline.px_bounds();
                    let (u0, v0) = (b.min.x as i64, b.min.y as i64);
                    outline.draw(|gx, gy, c| plot(u0 + i64::from(gx), v0 + i64::from(gy), c));
                }
            }
            Mark::Rule { x, y, w, h } => rule(x, baseline + y, w, h, &mut plot),
        }
    }
}

/// A filled rectangle, each pixel covered by the area it shares with it.
fn rule(x: f32, y: f32, w: f32, h: f32, plot: &mut impl FnMut(i64, i64, f32)) {
    if !(w > 0.0 && h > 0.0 && x.is_finite() && y.is_finite() && w < 1e5 && h < 1e5) {
        return;
    }
    let overlap = |a0: f32, a1: f32, p: f32| (a1.min(p + 1.0) - a0.max(p)).max(0.0);
    for v in y.floor() as i64..(y + h).ceil() as i64 {
        for u in x.floor() as i64..(x + w).ceil() as i64 {
            let c = overlap(x, x + w, u as f32) * overlap(y, y + h, v as f32);
            if c > 0.0 {
                plot(u, v, c);
            }
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

    #[test]
    fn math_sets_scripts_small_and_off_the_baseline() {
        // A superscript inks above the capitals' top, a subscript below the baseline.
        let rows = |s: &str| {
            let (mut top, mut bottom) = (i64::MAX, i64::MIN);
            rasterize(s, 2, |_, v, c| {
                if c > 0.5 {
                    top = top.min(v);
                    bottom = bottom.max(v);
                }
            });
            (top, bottom)
        };
        let (plain_top, plain_bottom) = rows("H");
        assert!(rows("$H^2$").0 < plain_top - 2, "{:?}", rows("$H^2$"));
        assert!(rows("$H_2$").1 > plain_bottom + 2, "{:?}", rows("$H_2$"));
        // Scripts are smaller: `x^2` narrower than `x2`.
        assert!(width("$x^2$", 2) < width("x2", 2));
        // A fraction is as wide as its wider part, not both side by side.
        assert!(width("$\\frac{1}{2}$", 2) < width("12", 2) * 0.9);
        // Commands become their symbols.
        assert_eq!(width("$\\omega$", 2), width("ω", 2));
        // Rules (the fraction's bar) draw.
        let mut ink = 0.0;
        rasterize("$\\frac{a}{b}$", 2, |_, _, c| ink += c);
        assert!(ink > 0.0);
    }
}
