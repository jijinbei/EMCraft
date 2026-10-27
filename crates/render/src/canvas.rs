//! An RGBA canvas with anti-aliased lines and triangles (coverage from the pixel centre's
//! distance to the shape's edges) and anti-aliased text ([`crate::text`]).

use crate::text;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub const WHITE: Rgb = Rgb(255, 255, 255);
    pub const BLACK: Rgb = Rgb(0, 0, 0);

    /// Linear interpolation towards `other`.
    pub fn mix(self, other: Rgb, t: f64) -> Rgb {
        let t = t.clamp(0.0, 1.0);
        let f = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * t).round() as u8;
        Rgb(f(self.0, other.0), f(self.1, other.1), f(self.2, other.2))
    }
}

/// The largest canvas side, in pixels.
pub const MAX_SIDE: usize = 8192;

#[derive(Clone, Debug, PartialEq)]
pub struct Canvas {
    pub w: usize,
    pub h: usize,
    /// RGBA, rows top to bottom.
    pub rgba: Vec<u8>,
}

impl Canvas {
    /// A canvas filled with `bg`; sides are clamped to 1..[`MAX_SIDE`].
    pub fn new(w: usize, h: usize, bg: Rgb) -> Canvas {
        let (w, h) = (w.clamp(1, MAX_SIDE), h.clamp(1, MAX_SIDE));
        let mut rgba = Vec::with_capacity(w * h * 4);
        for _ in 0..w * h {
            rgba.extend_from_slice(&[bg.0, bg.1, bg.2, 255]);
        }
        Canvas { w, h, rgba }
    }

    pub fn pixel(&self, x: usize, y: usize) -> Option<Rgb> {
        let i = (y.checked_mul(self.w)?.checked_add(x)?) * 4;
        if x >= self.w || y >= self.h {
            return None;
        }
        Some(Rgb(*self.rgba.get(i)?, *self.rgba.get(i + 1)?, *self.rgba.get(i + 2)?))
    }

    /// Blends `c` over pixel (x, y) with coverage `a`.
    pub fn blend(&mut self, x: i64, y: i64, c: Rgb, a: f64) {
        if x < 0 || y < 0 || x as usize >= self.w || y as usize >= self.h || !(a > 0.0) {
            return;
        }
        let a = a.min(1.0);
        let i = (y as usize * self.w + x as usize) * 4;
        if let Some(px) = self.rgba.get_mut(i..i + 3) {
            for (p, v) in px.iter_mut().zip([c.0, c.1, c.2]) {
                *p = (*p as f64 * (1.0 - a) + v as f64 * a).round() as u8;
            }
        }
    }

    pub fn set(&mut self, x: i64, y: i64, c: Rgb) {
        self.blend(x, y, c, 1.0);
    }

    pub fn rect(&mut self, x: f64, y: f64, w: f64, h: f64, c: Rgb) {
        if ![x, y, w, h].iter().all(|v| v.is_finite()) {
            return;
        }
        let (x0, y0) = (x.round() as i64, y.round() as i64);
        let (x1, y1) = ((x + w).round() as i64, (y + h).round() as i64);
        for py in y0.max(0)..y1.min(self.h as i64) {
            for px in x0.max(0)..x1.min(self.w as i64) {
                self.set(px, py, c);
            }
        }
    }

    /// An anti-aliased segment `width` pixels wide with round caps.
    pub fn line(&mut self, a: [f64; 2], b: [f64; 2], width: f64, c: Rgb) {
        if !a.iter().chain(&b).all(|v| v.is_finite() && v.abs() < 1e7) || !(width > 0.0) {
            return;
        }
        let r = width / 2.0;
        let (x0, x1) = ((a[0].min(b[0]) - r - 1.0).floor() as i64, (a[0].max(b[0]) + r + 1.0).ceil() as i64);
        let (y0, y1) = ((a[1].min(b[1]) - r - 1.0).floor() as i64, (a[1].max(b[1]) + r + 1.0).ceil() as i64);
        let d = [b[0] - a[0], b[1] - a[1]];
        let len2 = d[0] * d[0] + d[1] * d[1];
        for py in y0.max(0)..=y1.min(self.h as i64 - 1) {
            for px in x0.max(0)..=x1.min(self.w as i64 - 1) {
                let p = [px as f64 + 0.5, py as f64 + 0.5];
                let t = if len2 > 0.0 { (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / len2).clamp(0.0, 1.0) } else { 0.0 };
                let q = [a[0] + t * d[0] - p[0], a[1] + t * d[1] - p[1]];
                let dist = (q[0] * q[0] + q[1] * q[1]).sqrt();
                self.blend(px, py, c, r + 0.5 - dist);
            }
        }
    }

    /// An anti-aliased filled triangle.
    pub fn triangle(&mut self, p: [[f64; 2]; 3], c: Rgb) {
        if !p.iter().flatten().all(|v| v.is_finite() && v.abs() < 1e7) {
            return;
        }
        let area = (p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[2][0] - p[0][0]) * (p[1][1] - p[0][1]);
        if area == 0.0 {
            return;
        }
        let s = area.signum();
        let xs = p.map(|q| q[0]);
        let ys = p.map(|q| q[1]);
        let fold = |v: [f64; 3], f: fn(f64, f64) -> f64| v.iter().copied().fold(v[0], f);
        let (x0, x1) = (fold(xs, f64::min).floor() as i64 - 1, fold(xs, f64::max).ceil() as i64 + 1);
        let (y0, y1) = (fold(ys, f64::min).floor() as i64 - 1, fold(ys, f64::max).ceil() as i64 + 1);
        for py in y0.max(0)..=y1.min(self.h as i64 - 1) {
            for px in x0.max(0)..=x1.min(self.w as i64 - 1) {
                let q = [px as f64 + 0.5, py as f64 + 0.5];
                let mut inside = f64::INFINITY;
                for k in 0..3 {
                    let (u, v) = (p[k], p[(k + 1) % 3]);
                    let e = [v[0] - u[0], v[1] - u[1]];
                    let len = (e[0] * e[0] + e[1] * e[1]).sqrt();
                    if len == 0.0 {
                        continue;
                    }
                    let signed = s * (e[0] * (q[1] - u[1]) - e[1] * (q[0] - u[0])) / len;
                    inside = inside.min(signed);
                }
                self.blend(px, py, c, inside + 0.5);
            }
        }
    }

    /// An arrow from `a` to `b`: a shaft and a filled head `head` pixels long.
    pub fn arrow(&mut self, a: [f64; 2], b: [f64; 2], width: f64, head: f64, c: Rgb) {
        let d = [b[0] - a[0], b[1] - a[1]];
        let len = (d[0] * d[0] + d[1] * d[1]).sqrt();
        if !(len > 0.5) {
            return;
        }
        let u = [d[0] / len, d[1] / len];
        let head = head.min(0.6 * len);
        let base = [b[0] - u[0] * head, b[1] - u[1] * head];
        let n = [-u[1] * head * 0.45, u[0] * head * 0.45];
        self.line(a, base, width, c);
        self.triangle([b, [base[0] + n[0], base[1] + n[1]], [base[0] - n[0], base[1] - n[1]]], c);
    }

    /// Width in pixels of `text` at `scale` ([`crate::text`]).
    pub fn text_width(text: &str, scale: usize) -> usize {
        text::width(text, scale).ceil() as usize
    }

    /// Draws `text` with the top-left corner of its box at (x, y).
    pub fn text(&mut self, x: f64, y: f64, text: &str, scale: usize, c: Rgb) {
        if !(x.is_finite() && y.is_finite() && x.abs() < 1e7 && y.abs() < 1e7) {
            return;
        }
        let (x0, y0) = (x.round() as i64, y.round() as i64);
        text::rasterize(text, scale, |u, v, a| self.blend(x0 + u, y0 + v, c, f64::from(a)));
    }

    /// Text drawn rotated a quarter turn anticlockwise, reading upwards from (x, y) (its
    /// bottom-left corner).
    pub fn text_up(&mut self, x: f64, y: f64, text: &str, scale: usize, c: Rgb) {
        if !(x.is_finite() && y.is_finite() && x.abs() < 1e7 && y.abs() < 1e7) {
            return;
        }
        let (x0, y0) = (x.round() as i64, y.round() as i64);
        text::rasterize(text, scale, |u, v, a| self.blend(x0 + v, y0 - u, c, f64::from(a)));
    }

    /// The canvas as a PNG file.
    pub fn png(&self) -> Option<Vec<u8>> {
        let mut buf = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut buf, u32::try_from(self.w).ok()?, u32::try_from(self.h).ok()?);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut writer = enc.write_header().ok()?;
            writer.write_image_data(&self.rgba).ok()?;
        }
        Some(buf)
    }
}

/// An animated PNG (APNG) of `count` frames of one size, each shown for `delay_ms`, looping
/// forever. Frames are encoded as they come, so only one is held at a time. `None` when a frame
/// differs in size from the first or the encoder fails.
pub fn apng(frames: impl IntoIterator<Item = Canvas>, count: usize, delay_ms: u16) -> Option<Vec<u8>> {
    let mut frames = frames.into_iter();
    let first = frames.next()?;
    let (w, h) = (first.w, first.h);
    let mut buf = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut buf, u32::try_from(w).ok()?, u32::try_from(h).ok()?);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_animated(u32::try_from(count).ok()?, 0).ok()?;
        enc.set_frame_delay(delay_ms, 1000).ok()?;
        let mut writer = enc.write_header().ok()?;
        writer.write_image_data(&first.rgba).ok()?;
        let mut written = 1;
        for c in frames.take(count.saturating_sub(1)) {
            if (c.w, c.h) != (w, h) {
                return None;
            }
            writer.write_image_data(&c.rgba).ok()?;
            written += 1;
        }
        if written != count {
            return None;
        }
        writer.finish().ok()?;
    }
    Some(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_animation_holds_its_frames() {
        let frame = |c: Rgb| Canvas::new(8, 6, c);
        let a = apng([frame(Rgb::WHITE), frame(Rgb::BLACK), frame(Rgb(255, 0, 0))], 3, 40).unwrap();
        let mut d = png::Decoder::new(std::io::Cursor::new(a)).read_info().unwrap();
        let info = d.info().animation_control().copied().unwrap();
        assert_eq!((info.num_frames, info.num_plays), (3, 0));
        let mut buf = vec![0; d.output_buffer_size().unwrap()];
        for want in [255, 0, 255] {
            d.next_frame(&mut buf).unwrap();
            assert_eq!(buf[0], want);
        }
        // Frames of another size, or fewer than announced, are refused.
        assert!(apng([frame(Rgb::WHITE), Canvas::new(9, 6, Rgb::WHITE)], 2, 40).is_none());
        assert!(apng([frame(Rgb::WHITE)], 2, 40).is_none());
        assert!(apng(std::iter::empty(), 0, 40).is_none());
    }

    #[test]
    fn shapes_cover_their_pixels() {
        let mut c = Canvas::new(40, 30, Rgb::WHITE);
        c.line([5.0, 10.5], [35.0, 10.5], 3.0, Rgb::BLACK);
        assert_eq!(c.pixel(20, 10), Some(Rgb::BLACK));
        assert_eq!(c.pixel(20, 20), Some(Rgb::WHITE));
        c.triangle([[5.0, 15.0], [25.0, 15.0], [5.0, 29.0]], Rgb(255, 0, 0));
        assert_eq!(c.pixel(8, 18), Some(Rgb(255, 0, 0)));
        assert_eq!(c.pixel(30, 28), Some(Rgb::WHITE));
        // Hostile coordinates draw nothing and never panic.
        c.line([f64::NAN, 0.0], [1.0, 1.0], 2.0, Rgb::BLACK);
        c.triangle([[1e300, 0.0], [0.0, 1.0], [1.0, 1.0]], Rgb::BLACK);
        c.text(f64::INFINITY, 0.0, "x", 2, Rgb::BLACK);
        c.rect(-100.0, -100.0, 1e9, 3.0, Rgb::BLACK);
        assert!(c.png().is_some_and(|p| p.starts_with(&[0x89, b'P', b'N', b'G'])));
        assert_eq!(Canvas::new(0, usize::MAX, Rgb::WHITE).w, 1);
    }

    #[test]
    fn text_draws_inside_its_box() {
        let mut c = Canvas::new(60, 24, Rgb::WHITE);
        c.text(2.0, 2.0, "S21", 2, Rgb::BLACK);
        let w = Canvas::text_width("S21", 2);
        let dark = |x0: usize, x1: usize| (x0..x1).any(|x| (0..20).any(|y| c.pixel(x, y).is_some_and(|p| p.0 < 128)));
        assert!(dark(2, 2 + w));
        assert!(!dark(2 + w, 60));
    }
}
