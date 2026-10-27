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

/// Draws the chart.
pub fn draw(chart: &Chart, width: usize, height: usize) -> Canvas {
    let mut c = Canvas::new(width, height, Rgb::WHITE);
    let ink = Rgb(30, 30, 30);
    let (x0, y0) = (90.0, 40.0);
    let pw = (c.w as f64 - x0 - 30.0).max(50.0);
    let ph = (c.h as f64 - y0 - 100.0).max(50.0);
    let (xa, xb) = chart.x.span();
    let (ya, yb) = chart.y.span();
    let to_px = |p: [f64; 2]| [x0 + (p[0] - xa) / (xb - xa) * pw, y0 + ph - (p[1].clamp(ya - (yb - ya), yb + (yb - ya)) - ya) / (yb - ya) * ph];
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
                    let r = 4.0;
                    c.triangle([[q[0], q[1] - r], [q[0] + r, q[1]], [q[0], q[1] + r]], s.color);
                    c.triangle([[q[0], q[1] - r], [q[0], q[1] + r], [q[0] - r, q[1]]], s.color);
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
            Style::Markers => {
                let q = [lx + 13.0, ly + 7.0];
                c.triangle([[q[0], q[1] - 4.0], [q[0] + 4.0, q[1]], [q[0], q[1] + 4.0]], s.color);
                c.triangle([[q[0], q[1] - 4.0], [q[0], q[1] + 4.0], [q[0] - 4.0, q[1]]], s.color);
            }
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
    }
}
