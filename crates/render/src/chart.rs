//! Line charts (S-parameters against frequency): series drawn as lines or markers, vertical
//! reference lines (cut-off frequencies), grid, axes and a legend.

use crate::axes::{self, Axis};
use crate::{Canvas, Rgb};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Line,
    Markers,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Series {
    pub label: String,
    pub color: Rgb,
    pub style: Style,
    pub points: Vec<[f64; 2]>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Chart {
    pub title: String,
    pub x: Axis,
    pub y: Axis,
    pub series: Vec<Series>,
    /// Vertical reference lines at these x values, with labels.
    pub marks: Vec<(f64, String)>,
}

/// The margins around a chart's plot area (pixels): left (the y axis' numbers and name), top
/// (the title), right, bottom (the x axis and the legend).
const LEFT: f64 = 90.0;
const TOP: f64 = 40.0;
const RIGHT: f64 = 30.0;
const BOTTOM: f64 = 100.0;
/// The least side of the plot area (pixels), however small the picture.
const MIN_SIDE: f64 = 50.0;

/// Where a chart drawn at some size puts its plot area: the picture's size, the area's
/// rectangle in it (pixels) and the axes' ranges.
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
    /// A chart `width` × `height` pixels over the axes' ranges `x` and `y`.
    pub fn new(width: usize, height: usize, x: (f64, f64), y: (f64, f64)) -> Layout {
        let pw = (width as f64 - LEFT - RIGHT).max(MIN_SIDE);
        let ph = (height as f64 - TOP - BOTTOM).max(MIN_SIDE);
        Layout { width, height, plane: [LEFT, TOP, pw, ph], x, y }
    }

    /// The layout of `chart` drawn `width` × `height` pixels.
    pub fn of(chart: &Chart, width: usize, height: usize) -> Layout {
        Layout::new(width, height, chart.x.span(), chart.y.span())
    }

    /// A point (axis units) in pixels.
    pub fn to_pixels(&self, p: [f64; 2]) -> [f64; 2] {
        let [x0, y0, pw, ph] = self.plane;
        [x0 + (p[0] - self.x.0) / (self.x.1 - self.x.0) * pw, y0 + ph - (p[1] - self.y.0) / (self.y.1 - self.y.0) * ph]
    }

    /// The point (axis units) under a pixel position; `None` off the plot area.
    pub fn to_axes(&self, px: [f64; 2]) -> Option<[f64; 2]> {
        let [x0, y0, pw, ph] = self.plane;
        let (u, v) = ((px[0] - x0) / pw, 1.0 - (px[1] - y0) / ph);
        ((0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v)).then_some([self.x.0 + u * (self.x.1 - self.x.0), self.y.0 + v * (self.y.1 - self.y.0)])
    }

    /// The x value (axis units) under the pixel column `px`; `None` left or right of the plot
    /// area.
    pub fn x_at(&self, px: f64) -> Option<f64> {
        let [x0, _, pw, _] = self.plane;
        let u = (px - x0) / pw;
        (0.0..=1.0).contains(&u).then_some(self.x.0 + u * (self.x.1 - self.x.0))
    }
}

/// A diamond marker of half-diagonal `r` about `q`.
fn diamond(c: &mut Canvas, q: [f64; 2], r: f64, color: Rgb) {
    c.triangle([[q[0], q[1] - r], [q[0] + r, q[1]], [q[0], q[1] + r]], color);
    c.triangle([[q[0], q[1] - r], [q[0], q[1] + r], [q[0] - r, q[1]]], color);
}

/// Draws the chart.
pub fn draw(chart: &Chart, width: usize, height: usize) -> Canvas {
    let mut c = Canvas::new(width, height, Rgb::WHITE);
    let ink = Rgb(30, 30, 30);
    let layout = Layout::of(chart, c.w, c.h);
    let [x0, y0, pw, ph] = layout.plane;
    let (xa, xb) = layout.x;
    let (ya, yb) = layout.y;
    // Far points drawn at most a plot's height beyond it, so lines toward them keep their slope.
    let to_px = |p: [f64; 2]| layout.to_pixels([p[0], p[1].clamp(ya - (yb - ya), yb + (yb - ya))]);
    let inside = |q: [f64; 2]| q[0] >= x0 - 1.0 && q[0] <= x0 + pw + 1.0 && q[1] >= y0 - 1.0 && q[1] <= y0 + ph + 1.0;
    c.text(x0, 12.0, &chart.title, 2, ink);
    axes::frame(&mut c, [x0, y0, pw, ph], &chart.x, &chart.y, ink, Some(Rgb(225, 225, 225)));
    for (x, label) in &chart.marks {
        if !(x.is_finite() && *x > xa && *x < xb) {
            continue;
        }
        let px = to_px([*x, ya])[0];
        let mut y = y0;
        while y < y0 + ph {
            c.line([px, y], [px, (y + 6.0).min(y0 + ph)], 1.2, Rgb(120, 120, 120));
            y += 10.0;
        }
        c.text(px + 4.0, y0 + 4.0, label, 2, Rgb(90, 90, 90));
    }
    for s in &chart.series {
        let pts: Vec<[f64; 2]> = s.points.iter().filter(|p| p[0].is_finite() && p[1].is_finite()).map(|p| to_px(*p)).collect();
        match s.style {
            Style::Line => {
                for w in pts.windows(2) {
                    if inside(w[0]) || inside(w[1]) {
                        c.line(w[0], w[1], 2.0, s.color);
                    }
                }
            }
            Style::Markers => {
                for q in pts.iter().filter(|q| inside(**q)) {
                    diamond(&mut c, *q, 4.0, s.color);
                }
            }
        }
    }
    let mut lx = x0;
    let mut ly = y0 + ph + 56.0;
    for s in chart.series.iter().filter(|s| !s.label.is_empty()) {
        // Wrap to a second row rather than run off the canvas.
        let item = 52.0 + Canvas::text_width(&s.label, 2) as f64;
        if lx > x0 && lx + item > c.w as f64 - 10.0 {
            lx = x0;
            ly += 20.0;
        }
        match s.style {
            Style::Line => c.line([lx, ly + 7.0], [lx + 26.0, ly + 7.0], 2.0, s.color),
            Style::Markers => diamond(&mut c, [lx + 13.0, ly + 7.0], 4.0, s.color),
        }
        c.text(lx + 32.0, ly, &s.label, 2, ink);
        lx += item;
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rising_line_rises() {
        let chart = Chart {
            title: "S21".into(),
            x: Axis::new("f (GHz)", 0.0, 10.0),
            y: Axis::new("dB", -30.0, 0.0),
            series: vec![Series { label: "a".into(), color: Rgb(200, 0, 0), style: Style::Line, points: vec![[0.0, -30.0], [10.0, 0.0]] }],
            marks: vec![(5.0, "fc".into())],
        };
        let c = draw(&chart, 500, 360);
        let red = |x: usize, y: usize| c.pixel(x, y).is_some_and(|p| p.0 > 150 && p.1 < 80);
        // The line runs from the bottom-left to the top-right corner of the plot area.
        let (x0, y0, pw, ph) = (90.0, 40.0, 500.0 - 120.0, 360.0 - 140.0);
        let on = |t: f64| red((x0 + t * pw) as usize, (y0 + ph - t * ph) as usize);
        assert!(on(0.25) && on(0.5) && on(0.75));
        assert_eq!(draw(&chart, 500, 360), c);
        // The layout says where: the plot area's corners, and back.
        let l = Layout::of(&chart, 500, 360);
        assert_eq!(l.plane, [x0, y0, pw, ph]);
        assert_eq!(l.to_pixels([0.0, -30.0]), [x0, y0 + ph]);
        assert_eq!(l.to_axes([x0 + pw, y0]), Some([10.0, 0.0]));
        assert_eq!(l.x_at(x0 + 0.5 * pw), Some(5.0));
        assert_eq!((l.x_at(x0 - 1.0), l.to_axes([x0, y0 - 1.0])), (None, None));
        // A tiny picture keeps a plot area of the least side.
        assert_eq!(Layout::new(10, 10, (0.0, 1.0), (0.0, 1.0)).plane, [90.0, 40.0, 50.0, 50.0]);
    }
}
