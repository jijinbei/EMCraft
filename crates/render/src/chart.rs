//! Line charts (S-parameters against frequency): series drawn as lines or markers, vertical
//! reference lines (cut-off frequencies), grid, axes (linear or logarithmic) and a legend under
//! the plot or in one of its corners; each series' colour and width may be its own.

use crate::axes::{self, Axis};
use crate::palette::{Palette, Pen};
use crate::{Canvas, Rgb};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Line,
    Markers,
    /// Markers joined by a thin line: data whose points are what was computed, the line only a
    /// guide to the eye between them.
    Joined,
    /// A dashed line: a curve estimated rather than computed (a surrogate's prediction).
    Dashed,
    /// Hollow markers: points estimated rather than computed.
    Hollow,
    /// A shaded band, its points the upper edge from left to right and then the lower edge back
    /// (a prediction ± σ); drawn under every other series.
    Band,
}

/// The dashes of [`Style::Dashed`]: drawn and skipped lengths in pixels.
const DASH: [f64; 2] = [9.0, 6.0];
/// How strongly a band tints the plot.
const BAND_ALPHA: f64 = 0.2;

#[derive(Clone, Debug, PartialEq)]
pub struct Series {
    /// What names the curve whatever its label says (`s21.fem`, `band1`): the pieces of one
    /// curve share it, and a chart's style finds the curve by it.
    pub key: String,
    /// Its name in the legend; empty for a piece after the curve's first.
    pub label: String,
    /// Its colour, by role ([`Palette::pen`]).
    pub pen: Pen,
    pub style: Style,
    /// A line's width or a marker's half-diagonal in pixels; the chart's usual when `None`.
    pub width: Option<f64>,
    pub points: Vec<[f64; 2]>,
}

impl Series {
    /// A series named `key`, drawn at the usual width.
    pub fn new(key: &str, label: impl Into<String>, pen: Pen, style: Style, points: Vec<[f64; 2]>) -> Series {
        Series { key: key.to_owned(), label: label.into(), pen, style, width: None, points }
    }

    /// The width it is drawn at: its own (0.5 to 12 pixels), or 2 for a line and 4 for markers.
    fn drawn_width(&self) -> f64 {
        let usual = match self.style {
            Style::Line | Style::Dashed | Style::Band => 2.0,
            Style::Markers | Style::Joined | Style::Hollow => 4.0,
        };
        self.width.filter(|w| w.is_finite()).map_or(usual, |w| w.clamp(0.5, 12.0))
    }
}

/// Where a chart's legend goes: under the plot (the usual), in a corner of the plot area, or
/// nowhere.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Legend {
    #[default]
    Below,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    None,
}

/// How a chart looks besides its data: the grid and where its legend goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Look {
    pub grid: bool,
    pub legend: Legend,
}

impl Default for Look {
    fn default() -> Look {
        Look { grid: true, legend: Legend::Below }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Chart {
    pub title: String,
    pub x: Axis,
    pub y: Axis,
    pub series: Vec<Series>,
    /// Vertical reference lines at these x values, with labels.
    pub marks: Vec<(f64, String)>,
    pub look: Look,
}

impl Chart {
    /// A chart of `series` over the axes `x` and `y`, with no marks, a grid and the legend under
    /// it.
    pub fn new(title: impl Into<String>, x: Axis, y: Axis, series: Vec<Series>) -> Chart {
        Chart { title: title.into(), x, y, series, marks: Vec::new(), look: Look::default() }
    }

    /// The curves a legend names: each key once with its first label, in order.
    pub fn curves(&self) -> Vec<(&str, &str)> {
        let mut out: Vec<(&str, &str)> = Vec::new();
        for s in &self.series {
            if !out.iter().any(|(k, _)| *k == s.key) {
                out.push((s.key.as_str(), s.label.as_str()));
            }
        }
        out
    }
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

/// A chart's layout `width` × `height` pixels over the axes' linear ranges `x` and `y`.
pub fn layout(width: usize, height: usize, x: (f64, f64), y: (f64, f64)) -> Layout {
    let pw = (width as f64 - LEFT - RIGHT).max(MIN_SIDE);
    let ph = (height as f64 - TOP - BOTTOM).max(MIN_SIDE);
    Layout { width, height, plane: [LEFT, TOP, pw, ph], x, y, log: [false, false] }
}

/// The layout of `chart` drawn `width` × `height` pixels.
pub fn layout_of(chart: &Chart, width: usize, height: usize) -> Layout {
    Layout { log: [chart.x.is_log(), chart.y.is_log()], ..layout(width, height, chart.x.span(), chart.y.span()) }
}

/// A diamond marker of half-diagonal `r` about `q`.
fn diamond(c: &mut Canvas, q: [f64; 2], r: f64, color: Rgb) {
    c.triangle([[q[0], q[1] - r], [q[0] + r, q[1]], [q[0], q[1] + r]], color);
    c.triangle([[q[0], q[1] - r], [q[0], q[1] + r], [q[0] - r, q[1]]], color);
}

/// A diamond's outline of half-diagonal `r` about `q`.
fn hollow_diamond(c: &mut Canvas, q: [f64; 2], r: f64, color: Rgb) {
    let corners = [[q[0], q[1] - r], [q[0] + r, q[1]], [q[0], q[1] + r], [q[0] - r, q[1]]];
    for k in 0..4 {
        c.line(corners[k], corners[(k + 1) % 4], (r * 0.35).max(1.0), color);
    }
}

/// A dashed polyline through `pts`, the dashes running on across its corners.
fn dashed(c: &mut Canvas, pts: &[[f64; 2]], width: f64, color: Rgb, keep: impl Fn([f64; 2]) -> bool) {
    // Where along the pattern the next piece starts, from 0 to the pattern's length.
    let period = DASH[0] + DASH[1];
    let mut phase = 0.0;
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
        if !(len.is_finite() && len > 0.0) {
            continue;
        }
        let at = |t: f64| [a[0] + (b[0] - a[0]) * t / len, a[1] + (b[1] - a[1]) * t / len];
        let mut t = 0.0;
        // At most a dash and a gap per pattern along a piece no longer than a few plots.
        while t < len && t < 1e5 {
            let (on, left) = if phase < DASH[0] { (true, DASH[0] - phase) } else { (false, period - phase) };
            let step = left.min(len - t);
            if on && (keep(at(t)) || keep(at(t + step))) {
                c.line(at(t), at(t + step), width, color);
            }
            t += step;
            phase = (phase + step) % period;
        }
    }
}

/// A band: `pts` the upper edge left to right and then the lower edge back, tinting each pixel
/// column between the two edges within the plot `area`.
fn band(c: &mut Canvas, pts: &[[f64; 2]], area: [f64; 4], color: Rgb) {
    let m = pts.len() / 2;
    if m < 2 || pts.len() != 2 * m {
        return;
    }
    let (upper, lower): (Vec<[f64; 2]>, Vec<[f64; 2]>) = (pts[..m].to_vec(), pts[m..].iter().rev().copied().collect());
    let [x0, y0, pw, ph] = area;
    for i in 0..m - 1 {
        let (u0, u1, l0, l1) = (upper[i], upper[i + 1], lower[i], lower[i + 1]);
        let (xa, xb) = (u0[0].max(x0), u1[0].min(x0 + pw));
        if !(xb > xa) {
            continue;
        }
        for px in xa.floor() as i64..xb.ceil() as i64 {
            let x = px as f64 + 0.5;
            let t = if u1[0] > u0[0] { ((x - u0[0]) / (u1[0] - u0[0])).clamp(0.0, 1.0) } else { 0.0 };
            let (yu, yl) = (u0[1] + t * (u1[1] - u0[1]), l0[1] + t * (l1[1] - l0[1]));
            let (top, bottom) = (yu.min(yl).max(y0), yu.max(yl).min(y0 + ph));
            for py in top.floor() as i64..bottom.ceil() as i64 {
                c.blend(px, py, color, BAND_ALPHA);
            }
        }
    }
}

/// The legend's sample of a series at `[x, y]` (the row's top left).
fn sample(c: &mut Canvas, s: &Series, at: [f64; 2], color: Rgb) {
    let [lx, ly] = at;
    match s.style {
        Style::Line => c.line([lx, ly + 7.0], [lx + 26.0, ly + 7.0], s.drawn_width(), color),
        Style::Markers => diamond(c, [lx + 13.0, ly + 7.0], s.drawn_width(), color),
        Style::Joined => {
            c.line([lx, ly + 7.0], [lx + 26.0, ly + 7.0], 1.0, color);
            diamond(c, [lx + 13.0, ly + 7.0], s.drawn_width(), color);
        }
        Style::Dashed => dashed(c, &[[lx, ly + 7.0], [lx + 26.0, ly + 7.0]], s.drawn_width(), color, |_| true),
        Style::Hollow => hollow_diamond(c, [lx + 13.0, ly + 7.0], s.drawn_width(), color),
        Style::Band => band(c, &[[lx, ly + 2.0], [lx + 26.0, ly + 2.0], [lx + 26.0, ly + 12.0], [lx, ly + 12.0]], [lx, ly, 26.0, 14.0], color),
    }
}

/// Draws the chart in `palette`'s colours.
pub fn draw(chart: &Chart, width: usize, height: usize, palette: &Palette) -> Canvas {
    let mut c = Canvas::new(width, height, palette.ground);
    let ink = palette.ink;
    let layout = layout_of(chart, c.w, c.h);
    let [x0, y0, pw, ph] = layout.plane;
    let (xa, xb) = layout.x;
    // Far points drawn at most a plot's size beyond it, so lines toward them keep their slope.
    let to_px = |p: [f64; 2]| {
        let [x, y] = layout.to_pixels(p);
        [x.clamp(x0 - 2.0 * pw, x0 + 3.0 * pw), y.clamp(y0 - ph, y0 + 2.0 * ph)]
    };
    let inside = |q: [f64; 2]| q[0] >= x0 - 1.0 && q[0] <= x0 + pw + 1.0 && q[1] >= y0 - 1.0 && q[1] <= y0 + ph + 1.0;
    // A point a log axis cannot place (zero, negative) is left out.
    let placed = |p: &&[f64; 2]| p[0].is_finite() && p[1].is_finite() && (!layout.log[0] || p[0] > 0.0) && (!layout.log[1] || p[1] > 0.0);
    c.text(x0, 12.0, &chart.title, 2, ink);
    axes::frame(&mut c, [x0, y0, pw, ph], &chart.x, &chart.y, ink, chart.look.grid.then_some(palette.grid));
    for (x, label) in &chart.marks {
        if !(x.is_finite() && *x > xa && *x < xb) {
            continue;
        }
        let px = to_px([*x, layout.y.0])[0];
        let mut y = y0;
        while y < y0 + ph {
            c.line([px, y], [px, (y + 6.0).min(y0 + ph)], 1.2, palette.mark);
            y += 10.0;
        }
        c.text(px + 4.0, y0 + 4.0, label, 2, palette.ink_dim);
    }
    // Bands first, under the curves they go with.
    for s in chart.series.iter().filter(|s| s.style == Style::Band) {
        let pts: Vec<[f64; 2]> = s.points.iter().map(|p| if placed(&p) { to_px(*p) } else { [f64::NAN; 2] }).collect();
        if pts.iter().flatten().all(|v| v.is_finite()) {
            band(&mut c, &pts, [x0, y0, pw, ph], palette.pen(s.pen));
        }
    }
    for s in chart.series.iter().filter(|s| s.style != Style::Band) {
        let color = palette.pen(s.pen);
        let width = s.drawn_width();
        let pts: Vec<[f64; 2]> = s.points.iter().filter(placed).map(|p| to_px(*p)).collect();
        match s.style {
            Style::Line => {
                for w in pts.windows(2) {
                    if inside(w[0]) || inside(w[1]) {
                        c.line(w[0], w[1], width, color);
                    }
                }
            }
            Style::Markers => {
                for q in pts.iter().filter(|q| inside(**q)) {
                    diamond(&mut c, *q, width, color);
                }
            }
            Style::Joined => {
                for w in pts.windows(2) {
                    if inside(w[0]) || inside(w[1]) {
                        c.line(w[0], w[1], (width * 0.3).max(1.0), color);
                    }
                }
                // Many points close together: smaller markers, so the line still reads.
                let small = if pts.len() > 40 { width * 0.6 } else { width };
                for q in pts.iter().filter(|q| inside(**q)) {
                    diamond(&mut c, *q, small, color);
                }
            }
            Style::Dashed => dashed(&mut c, &pts, width, color, inside),
            Style::Hollow => {
                for q in pts.iter().filter(|q| inside(**q)) {
                    hollow_diamond(&mut c, *q, width, color);
                }
            }
            Style::Band => {}
        }
    }
    legend(&mut c, chart, [x0, y0, pw, ph], palette);
    c
}

/// The legend: a sample and the label of each named series, under the plot (wrapping to more
/// rows) or in a box in a corner of the plot area.
fn legend(c: &mut Canvas, chart: &Chart, area: [f64; 4], palette: &Palette) {
    let [x0, y0, pw, ph] = area;
    let named: Vec<&Series> = chart.series.iter().filter(|s| !s.label.is_empty()).collect();
    let item = |s: &Series| 52.0 + Canvas::text_width(&s.label, 2) as f64;
    match chart.look.legend {
        Legend::None => {}
        Legend::Below => {
            let mut lx = x0;
            let mut ly = y0 + ph + 56.0;
            for s in named {
                // Wrap to a second row rather than run off the canvas.
                if lx > x0 && lx + item(s) > c.w as f64 - 10.0 {
                    lx = x0;
                    ly += 20.0;
                }
                sample(c, s, [lx, ly], palette.pen(s.pen));
                c.text(lx + 32.0, ly, &s.label, 2, palette.ink);
                lx += item(s);
            }
        }
        corner => {
            if named.is_empty() {
                return;
            }
            // One row per series in a box on the plot's ground, framed.
            let w = named.iter().map(|s| item(s) - 12.0).fold(0.0, f64::max) + 8.0;
            let h = 20.0 * named.len() as f64 + 8.0;
            let left = matches!(corner, Legend::TopLeft | Legend::BottomLeft);
            let top = matches!(corner, Legend::TopLeft | Legend::TopRight);
            let bx = if left { x0 + 8.0 } else { x0 + pw - 8.0 - w };
            let by = if top { y0 + 8.0 } else { y0 + ph - 8.0 - h };
            c.rect(bx, by, w, h, palette.ground);
            for (a, b) in [([bx, by], [bx + w, by]), ([bx + w, by], [bx + w, by + h]), ([bx + w, by + h], [bx, by + h]), ([bx, by + h], [bx, by])] {
                c.line(a, b, 1.0, palette.grid);
            }
            for (i, s) in named.iter().enumerate() {
                let (lx, ly) = (bx + 6.0, by + 5.0 + 20.0 * i as f64);
                sample(c, s, [lx, ly], palette.pen(s.pen));
                c.text(lx + 32.0, ly, &s.label, 2, palette.ink);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rising_line_rises() {
        let mut chart = Chart::new(
            "S21",
            Axis::new("f (GHz)", 0.0, 10.0),
            Axis::new("dB", -30.0, 0.0),
            vec![Series::new("a", "a", Pen::Series(1), Style::Line, vec![[0.0, -30.0], [10.0, 0.0]])],
        );
        chart.marks = vec![(5.0, "fc".into())];
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

    #[test]
    fn looks_change_the_picture_and_nothing_breaks() {
        let light = crate::palette::samples::light();
        let base = Chart::new(
            "t",
            Axis::new("f", 1.0, 100.0),
            Axis::new("y", 0.0, 1.0),
            vec![
                Series::new("a", "a", Pen::Series(0), Style::Line, vec![[1.0, 0.0], [10.0, 0.5], [100.0, 1.0]]),
                Series::new("a", "", Pen::Series(0), Style::Line, vec![[0.0, 0.2], [-5.0, 0.3]]),
                Series::new("b", "b", Pen::Series(1), Style::Markers, vec![[50.0, 0.5]]),
            ],
        );
        assert_eq!(base.curves(), vec![("a", "a"), ("b", "b")]);
        let plain = draw(&base, 400, 300, &light);
        let mut styled = base.clone();
        styled.x.log = true;
        styled.look = Look { grid: false, legend: Legend::TopRight };
        styled.series[0].pen = Pen::Custom(crate::Rgb(10, 200, 10));
        styled.series[0].width = Some(5.0);
        let other = draw(&styled, 400, 300, &light);
        assert_ne!(plain, other);
        // On a log x axis the middle point (10) sits mid-plot: green there.
        let l = layout_of(&styled, 400, 300);
        let [px, py] = l.to_pixels([10.0, 0.5]);
        assert_eq!(other.pixel(px as usize, py as usize), Some(crate::Rgb(10, 200, 10)));
        // Predicted points: dashed, hollow, a band under them; hostile points left out.
        let mut predicted = base.clone();
        predicted.series = vec![
            Series::new("p", "", Pen::Series(1), Style::Band, vec![[1.0, 0.6], [100.0, 0.9], [100.0, 0.7], [1.0, 0.4]]),
            Series::new("p", "p", Pen::Series(1), Style::Dashed, vec![[1.0, 0.5], [100.0, 0.8], [f64::NAN, 0.1]]),
            Series::new("q", "q", Pen::Series(0), Style::Hollow, vec![[50.0, 0.5], [1e9, 1e9]]),
            Series::new("r", "r", Pen::Series(0), Style::Band, vec![[1.0, f64::NAN], [2.0, 0.1], [3.0, 0.2]]),
        ];
        let drawn = draw(&predicted, 400, 300, &light);
        let l = layout_of(&predicted, 400, 300);
        let tint = |p: [f64; 2]| {
            let [x, y] = l.to_pixels(p);
            drawn.pixel(x as usize, y as usize)
        };
        // Inside the band, tinted; outside it, the ground.
        assert!(tint([55.0, 0.65]).is_some_and(|c| c != light.ground && c != light.pen(Pen::Series(1))));
        assert_eq!(tint([55.0, 0.15]), Some(light.ground));
        assert_ne!(drawn, draw(&base, 400, 300, &light));
        assert_eq!(drawn, draw(&predicted, 400, 300, &light));
        for legend in [Legend::Below, Legend::TopRight] {
            predicted.look.legend = legend;
            assert!(draw(&predicted, 400, 300, &light).png().is_some());
        }
        // Every legend position draws; hostile widths are clamped.
        for legend in [Legend::Below, Legend::TopLeft, Legend::BottomLeft, Legend::BottomRight, Legend::None] {
            styled.look.legend = legend;
            styled.series[1].width = Some(f64::NAN);
            styled.series[2].width = Some(1e9);
            assert!(draw(&styled, 120, 80, &light).png().is_some());
        }
    }
}
