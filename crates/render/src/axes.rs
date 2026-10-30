//! Axes: ranges, "nice" tick values (1, 2 or 5 times a power of ten) and tick labels.

use crate::{Canvas, Rgb};

/// An axis: its label (with unit) and range, in display units.
#[derive(Clone, Debug, PartialEq)]
pub struct Axis {
    pub label: String,
    pub min: f64,
    pub max: f64,
}

impl Axis {
    pub fn new(label: &str, min: f64, max: f64) -> Axis {
        Axis { label: label.to_owned(), min, max }
    }

    /// A finite range of positive length (a unit range around a degenerate one).
    pub fn span(&self) -> (f64, f64) {
        let (a, b) = (self.min, self.max);
        if !(a.is_finite() && b.is_finite()) {
            return (0.0, 1.0);
        }
        if b > a {
            (a, b)
        } else if a > b {
            (b, a)
        } else {
            (a - 0.5, a + 0.5)
        }
    }
}

/// About `count` tick values inside [min, max].
pub fn ticks(min: f64, max: f64, count: usize) -> Vec<f64> {
    if !(min.is_finite() && max.is_finite() && max > min) {
        return Vec::new();
    }
    let raw = (max - min) / count.max(1) as f64;
    let mag = 10f64.powf(raw.log10().floor());
    let step = [1.0, 2.0, 5.0, 10.0].iter().map(|m| m * mag).find(|s| *s >= raw).unwrap_or(10.0 * mag);
    let first = (min / step - 1e-9).ceil() as i64;
    let last = (max / step + 1e-9).floor() as i64;
    if last - first > 100 {
        return Vec::new();
    }
    (first..=last).map(|i| i as f64 * step).map(|v| if v.abs() < step * 1e-9 { 0.0 } else { v }).collect()
}

/// A short label for a value: up to 4 significant figures, scientific outside 10⁻³..10⁵.
pub fn format(v: f64) -> String {
    if v == 0.0 {
        return "0".into();
    }
    let a = v.abs();
    if !(1e-3..1e5).contains(&a) {
        return format!("{v:.2e}");
    }
    let decimals = (3 - a.log10().floor() as i32).clamp(0, 6) as usize;
    let s = format!("{v:.decimals$}");
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_owned() } else { s }
}

/// Where a chart or a field plot drawn at some size puts its plot area: the picture's size, the
/// area's rectangle in it (pixels) and the axes' ranges (`chart::layout`, `plot::layout_of`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub width: usize,
    pub height: usize,
    /// The plot area: left, top, width, height.
    pub plane: [f64; 4],
    pub x: (f64, f64),
    pub y: (f64, f64),
}

impl Layout {
    /// A point (axis units) in pixels (y upwards).
    pub fn to_pixels(&self, p: [f64; 2]) -> [f64; 2] {
        let ([x0, y0, pw, ph], (x, y)) = (self.plane, (self.x, self.y));
        [x0 + (p[0] - x.0) / (x.1 - x.0) * pw, y0 + ph - (p[1] - y.0) / (y.1 - y.0) * ph]
    }

    /// The point (axis units) under a pixel position; `None` off the plot area.
    pub fn to_axes(&self, px: [f64; 2]) -> Option<[f64; 2]> {
        let ([x0, y0, pw, ph], (x, y)) = (self.plane, (self.x, self.y));
        let (u, v) = ((px[0] - x0) / pw, 1.0 - (px[1] - y0) / ph);
        ((0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v)).then_some([x.0 + u * (x.1 - x.0), y.0 + v * (y.1 - y.0)])
    }

    /// The x value (axis units) under the pixel column `px`; `None` left or right of the plot
    /// area.
    pub fn x_at(&self, px: f64) -> Option<f64> {
        let [x0, _, pw, _] = self.plane;
        let u = (px - x0) / pw;
        (0.0..=1.0).contains(&u).then_some(self.x.0 + u * (self.x.1 - self.x.0))
    }
}

/// Draws a frame, tick marks and labels for a plot area (x0, y0, w, h) in pixels.
pub fn frame(c: &mut Canvas, area: [f64; 4], x: &Axis, y: &Axis, ink: Rgb, grid: Option<Rgb>) {
    let [x0, y0, w, h] = area;
    let (xa, xb) = x.span();
    let (ya, yb) = y.span();
    let px = |v: f64| x0 + (v - xa) / (xb - xa) * w;
    let py = |v: f64| y0 + h - (v - ya) / (yb - ya) * h;
    let scale = 2;
    for t in ticks(xa, xb, 6) {
        let p = px(t);
        if let Some(g) = grid {
            c.line([p, y0], [p, y0 + h], 1.0, g);
        }
        c.line([p, y0 + h], [p, y0 + h + 5.0], 1.0, ink);
        let s = format(t);
        c.text(p - Canvas::text_width(&s, scale) as f64 / 2.0, y0 + h + 9.0, &s, scale, ink);
    }
    for t in ticks(ya, yb, 5) {
        let p = py(t);
        if let Some(g) = grid {
            c.line([x0, p], [x0 + w, p], 1.0, g);
        }
        c.line([x0 - 5.0, p], [x0, p], 1.0, ink);
        let s = format(t);
        c.text(x0 - 9.0 - Canvas::text_width(&s, scale) as f64, p - 7.0, &s, scale, ink);
    }
    for (a, b) in [([x0, y0], [x0 + w, y0]), ([x0 + w, y0], [x0 + w, y0 + h]), ([x0 + w, y0 + h], [x0, y0 + h]), ([x0, y0 + h], [x0, y0])] {
        c.line(a, b, 1.2, ink);
    }
    let lw = Canvas::text_width(&x.label, scale) as f64;
    c.text(x0 + w / 2.0 - lw / 2.0, y0 + h + 30.0, &x.label, scale, ink);
    let lh = Canvas::text_width(&y.label, scale) as f64;
    c.text_up(x0 - 62.0, y0 + h / 2.0 + lh / 2.0, &y.label, scale, ink);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nice_ticks() {
        assert_eq!(ticks(0.0, 10.0, 5), vec![0.0, 2.0, 4.0, 6.0, 8.0, 10.0]);
        assert_eq!(ticks(4.6, 9.1, 5), vec![5.0, 6.0, 7.0, 8.0, 9.0]);
        assert_eq!(ticks(-30.0, 0.0, 6), vec![-30.0, -25.0, -20.0, -15.0, -10.0, -5.0, 0.0]);
        assert!(ticks(f64::NAN, 1.0, 5).is_empty());
        assert!(ticks(1.0, 1.0, 5).is_empty());
    }

    #[test]
    fn labels() {
        assert_eq!(format(0.0), "0");
        assert_eq!(format(22.86), "22.86");
        assert_eq!(format(-5.0), "-5");
        assert_eq!(format(0.25), "0.25");
        assert_eq!(format(1.5e6), "1.50e6");
    }
}
