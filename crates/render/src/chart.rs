//! Line charts (S-parameters against frequency): series drawn as lines or markers, vertical
//! reference lines (cut-off frequencies), grid, axes and a legend.

use crate::axes::{self, Axis};
use crate::palette::{Palette, Pen};
use crate::{Canvas, Rgb};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Line,
    Markers,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Series {
    pub label: String,
    /// Its colour, by role ([`Palette::pen`]).
    pub pen: Pen,
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

pub use crate::axes::Layout;

/// A chart's layout `width` × `height` pixels over the axes' ranges `x` and `y`.
pub fn layout(width: usize, height: usize, x: (f64, f64), y: (f64, f64)) -> Layout {
    let pw = (width as f64 - LEFT - RIGHT).max(MIN_SIDE);
    let ph = (height as f64 - TOP - BOTTOM).max(MIN_SIDE);
    Layout { width, height, plane: [LEFT, TOP, pw, ph], x, y }
}

/// The layout of `chart` drawn `width` × `height` pixels.
pub fn layout_of(chart: &Chart, width: usize, height: usize) -> Layout {
    layout(width, height, chart.x.span(), chart.y.span())
}

/// A diamond marker of half-diagonal `r` about `q`.
fn diamond(c: &mut Canvas, q: [f64; 2], r: f64, color: Rgb) {
    c.triangle([[q[0], q[1] - r], [q[0] + r, q[1]], [q[0], q[1] + r]], color);
    c.triangle([[q[0], q[1] - r], [q[0], q[1] + r], [q[0] - r, q[1]]], color);
}

/// Draws the chart in `palette`'s colours.
pub fn draw(chart: &Chart, width: usize, height: usize, palette: &Palette) -> Canvas {
    let mut c = Canvas::new(width, height, palette.ground);
    let ink = palette.ink;
    let layout = layout_of(chart, c.w, c.h);
    let [x0, y0, pw, ph] = layout.plane;
    let (xa, xb) = layout.x;
    let (ya, yb) = layout.y;
    // Far points drawn at most a plot's height beyond it, so lines toward them keep their slope.
    let to_px = |p: [f64; 2]| layout.to_pixels([p[0], p[1].clamp(ya - (yb - ya), yb + (yb - ya))]);
    let inside = |q: [f64; 2]| q[0] >= x0 - 1.0 && q[0] <= x0 + pw + 1.0 && q[1] >= y0 - 1.0 && q[1] <= y0 + ph + 1.0;
    c.text(x0, 12.0, &chart.title, 2, ink);
    axes::frame(&mut c, [x0, y0, pw, ph], &chart.x, &chart.y, ink, Some(palette.grid));
    for (x, label) in &chart.marks {
        if !(x.is_finite() && *x > xa && *x < xb) {
            continue;
        }
        let px = to_px([*x, ya])[0];
        let mut y = y0;
        while y < y0 + ph {
            c.line([px, y], [px, (y + 6.0).min(y0 + ph)], 1.2, palette.mark);
            y += 10.0;
        }
        c.text(px + 4.0, y0 + 4.0, label, 2, palette.ink_dim);
    }
    for s in &chart.series {
        let color = palette.pen(s.pen);
        let pts: Vec<[f64; 2]> = s.points.iter().filter(|p| p[0].is_finite() && p[1].is_finite()).map(|p| to_px(*p)).collect();
        match s.style {
            Style::Line => {
                for w in pts.windows(2) {
                    if inside(w[0]) || inside(w[1]) {
                        c.line(w[0], w[1], 2.0, color);
                    }
                }
            }
            Style::Markers => {
                for q in pts.iter().filter(|q| inside(**q)) {
                    diamond(&mut c, *q, 4.0, color);
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
        let color = palette.pen(s.pen);
        match s.style {
            Style::Line => c.line([lx, ly + 7.0], [lx + 26.0, ly + 7.0], 2.0, color),
            Style::Markers => diamond(&mut c, [lx + 13.0, ly + 7.0], 4.0, color),
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
            series: vec![Series { label: "a".into(), pen: Pen::Series(1), style: Style::Line, points: vec![[0.0, -30.0], [10.0, 0.0]] }],
            marks: vec![(5.0, "fc".into())],
        };
        let c = draw(&chart, 500, 360, &crate::palette::samples::light());
        let red = |x: usize, y: usize| c.pixel(x, y).is_some_and(|p| p.0 > 150 && p.1 < 100);
        // The line runs from the bottom-left to the top-right corner of the plot area.
        let (x0, y0, pw, ph) = (90.0, 40.0, 500.0 - 120.0, 360.0 - 140.0);
        let on = |t: f64| red((x0 + t * pw) as usize, (y0 + ph - t * ph) as usize);
        assert!(on(0.25) && on(0.5) && on(0.75));
        assert_eq!(draw(&chart, 500, 360, &crate::palette::samples::light()), c);
        // In the dark palette the ground is dark.
        let dark = draw(&chart, 500, 360, &crate::palette::samples::dark());
        assert_eq!(dark.pixel(2, 2), Some(crate::palette::samples::dark().ground));
        // The layout says where: the plot area's corners, and back.
        let l = layout_of(&chart, 500, 360);
        assert_eq!(l.plane, [x0, y0, pw, ph]);
        assert_eq!(l.to_pixels([0.0, -30.0]), [x0, y0 + ph]);
        assert_eq!(l.to_axes([x0 + pw, y0]), Some([10.0, 0.0]));
        assert_eq!(l.x_at(x0 + 0.5 * pw), Some(5.0));
        assert_eq!((l.x_at(x0 - 1.0), l.to_axes([x0, y0 - 1.0])), (None, None));
        // A tiny picture keeps a plot area of the least side.
        assert_eq!(layout(10, 10, (0.0, 1.0), (0.0, 1.0)).plane, [90.0, 40.0, 50.0, 50.0]);
    }
}
