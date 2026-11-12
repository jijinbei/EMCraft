//! Line charts (S-parameters against frequency): series drawn as lines or markers, vertical
//! reference lines (cut-off frequencies), grid, axes (linear or logarithmic) and a legend under
//! the plot or in one of its corners; each series' colour and width may be its own.

use crate::axes::{self, Axis};
use crate::palette::{Palette, Pen};
use crate::shapes::{Align, Drawing, Shape};
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
    /// One unit as long along x as along y (a Smith chart's circles round): the shorter
    /// side's range widened about its middle to match the picture.
    pub equal: bool,
}

impl Default for Look {
    fn default() -> Look {
        Look { grid: true, legend: Legend::Below, equal: false }
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
    let log = [chart.x.is_log(), chart.y.is_log()];
    let mut at = Layout { log, ..layout(width, height, chart.x.span(), chart.y.span()) };
    if chart.look.equal && !log[0] && !log[1] {
        let [_, _, pw, ph] = at.plane;
        // Units per pixel along each axis; the larger taken by both.
        let per = ((at.x.1 - at.x.0) / pw).max((at.y.1 - at.y.0) / ph);
        let widened = |(a, b): (f64, f64), side: f64| {
            let (mid, half) = (0.5 * (a + b), 0.5 * per * side);
            (mid - half, mid + half)
        };
        at.x = widened(at.x, pw);
        at.y = widened(at.y, ph);
    }
    at
}

/// A line from `a` to `b`.
fn line(a: [f64; 2], b: [f64; 2], width: f64, color: Rgb) -> Shape {
    Shape::Line { a, b, width, color }
}

/// A diamond marker of half-diagonal `r` about `q`.
fn diamond(q: [f64; 2], r: f64, color: Rgb) -> [Shape; 2] {
    [
        Shape::Triangle { p: [[q[0], q[1] - r], [q[0] + r, q[1]], [q[0], q[1] + r]], color },
        Shape::Triangle { p: [[q[0], q[1] - r], [q[0], q[1] + r], [q[0] - r, q[1]]], color },
    ]
}

/// A diamond's outline of half-diagonal `r` about `q`.
fn hollow_diamond(q: [f64; 2], r: f64, color: Rgb) -> Vec<Shape> {
    let corners = [[q[0], q[1] - r], [q[0] + r, q[1]], [q[0], q[1] + r], [q[0] - r, q[1]]];
    (0..4).map(|k| line(corners[k], corners[(k + 1) % 4], (r * 0.35).max(1.0), color)).collect()
}

/// A dashed polyline through `pts`, the dashes running on across its corners; the dashes with
/// an end `keep` takes.
fn dashed(pts: &[[f64; 2]], width: f64, color: Rgb, keep: impl Fn([f64; 2]) -> bool) -> Vec<Shape> {
    let mut out = Vec::new();
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
                out.push(line(at(t), at(t + step), width, color));
            }
            t += step;
            phase = (phase + step) % period;
        }
    }
    out
}

/// A band's shape: `pts` the upper edge left to right and then the lower edge back, within the
/// plot `area`.
fn band(pts: Vec<[f64; 2]>, area: [f64; 4], color: Rgb) -> Shape {
    Shape::Band { edge: pts, clip: area, color, alpha: BAND_ALPHA }
}

/// The legend's sample of a series at `[x, y]` (the row's top left).
fn sample(s: &Series, at: [f64; 2], color: Rgb) -> Vec<Shape> {
    let [lx, ly] = at;
    let (a, b, mid) = ([lx, ly + 7.0], [lx + 26.0, ly + 7.0], [lx + 13.0, ly + 7.0]);
    match s.style {
        Style::Line => vec![line(a, b, s.drawn_width(), color)],
        Style::Markers => diamond(mid, s.drawn_width(), color).to_vec(),
        Style::Joined => std::iter::once(line(a, b, 1.0, color)).chain(diamond(mid, s.drawn_width(), color)).collect(),
        Style::Dashed => dashed(&[a, b], s.drawn_width(), color, |_| true),
        Style::Hollow => hollow_diamond(mid, s.drawn_width(), color),
        Style::Band => vec![band(vec![[lx, ly + 2.0], [lx + 26.0, ly + 2.0], [lx + 26.0, ly + 12.0], [lx, ly + 12.0]], [lx, ly, 26.0, 14.0], color)],
    }
}

/// Draws the chart in `palette`'s colours.
pub fn draw(chart: &Chart, width: usize, height: usize, palette: &Palette) -> Canvas {
    drawing(chart, width, height, palette).canvas()
}

/// The chart `width` × `height` pixels in `palette`'s colours, as shapes (any surface draws
/// them: [`draw`] on the CPU, a window's vector canvas on its GPU).
pub fn drawing(chart: &Chart, width: usize, height: usize, palette: &Palette) -> Drawing {
    let ink = palette.ink;
    let layout = layout_of(chart, width, height);
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
    let title = Shape::Text { at: [x0, 12.0], text: chart.title.clone(), scale: 2, color: ink, align: Align::Left, up: false };
    // The axes over the ranges laid out (an equal chart's widened).
    let (mut ax, mut ay) = (chart.x.clone(), chart.y.clone());
    if chart.look.equal {
        (ax.min, ax.max) = layout.x;
        (ay.min, ay.max) = layout.y;
    }
    let frame = axes::frame_shapes([x0, y0, pw, ph], &ax, &ay, ink, chart.look.grid.then_some(palette.grid));
    let marks = chart.marks.iter().filter(|(x, _)| x.is_finite() && *x > xa && *x < xb).flat_map(|(x, label)| {
        let px = to_px([*x, layout.y.0])[0];
        let dashes = std::iter::successors(Some(y0), |y| Some(y + 10.0)).take_while(move |y| *y < y0 + ph);
        let dashes = dashes.map(move |y| line([px, y], [px, (y + 6.0).min(y0 + ph)], 1.2, palette.mark));
        dashes.chain(std::iter::once(Shape::Text {
            at: [px + 4.0, y0 + 4.0],
            text: label.clone(),
            scale: 2,
            color: palette.ink_dim,
            align: Align::Left,
            up: false,
        }))
    });
    // Bands first, under the curves they go with.
    let bands = chart.series.iter().filter(|s| s.style == Style::Band).filter_map(|s| {
        let pts: Vec<[f64; 2]> = s.points.iter().map(|p| if placed(&p) { to_px(*p) } else { [f64::NAN; 2] }).collect();
        pts.iter().flatten().all(|v| v.is_finite()).then(|| band(pts, [x0, y0, pw, ph], palette.pen(s.pen)))
    });
    let curves = chart.series.iter().filter(|s| s.style != Style::Band).flat_map(|s| {
        let color = palette.pen(s.pen);
        let width = s.drawn_width();
        let pts: Vec<[f64; 2]> = s.points.iter().filter(placed).map(|p| to_px(*p)).collect();
        let joining = |w: f64| pts.windows(2).filter(|w| inside(w[0]) || inside(w[1])).map(|p| line(p[0], p[1], w, color)).collect::<Vec<_>>();
        let shown = pts.iter().filter(|q| inside(**q));
        match s.style {
            Style::Line => joining(width),
            Style::Markers => shown.flat_map(|q| diamond(*q, width, color)).collect(),
            Style::Joined => {
                // Many points close together: smaller markers, so the line still reads.
                let small = if pts.len() > 40 { width * 0.6 } else { width };
                joining((width * 0.3).max(1.0)).into_iter().chain(shown.flat_map(|q| diamond(*q, small, color))).collect()
            }
            Style::Dashed => dashed(&pts, width, color, inside),
            Style::Hollow => shown.flat_map(|q| hollow_diamond(*q, width, color)).collect(),
            Style::Band => Vec::new(),
        }
    });
    let shapes =
        std::iter::once(title).chain(frame).chain(marks).chain(bands).chain(curves).chain(legend(chart, [x0, y0, pw, ph], width, palette)).collect();
    Drawing { size: [width, height], ground: palette.ground, shapes }
}

/// The legend: a sample and the label of each named series, under the plot (wrapping to more
/// rows within a picture `width` pixels wide) or in a box in a corner of the plot area.
fn legend(chart: &Chart, area: [f64; 4], width: usize, palette: &Palette) -> Vec<Shape> {
    let [x0, y0, pw, ph] = area;
    let named: Vec<&Series> = chart.series.iter().filter(|s| !s.label.is_empty()).collect();
    let item = |s: &Series| 52.0 + Canvas::text_width(&s.label, 2) as f64;
    let label = |at: [f64; 2], s: &Series| Shape::Text { at, text: s.label.clone(), scale: 2, color: palette.ink, align: Align::Left, up: false };
    let row = |s: &Series, at: [f64; 2]| sample(s, at, palette.pen(s.pen)).into_iter().chain(std::iter::once(label([at[0] + 32.0, at[1]], s)));
    match chart.look.legend {
        Legend::None => Vec::new(),
        Legend::Below => {
            let mut out = Vec::new();
            let mut lx = x0;
            let mut ly = y0 + ph + 56.0;
            for s in named {
                // Wrap to a second row rather than run off the picture.
                if lx > x0 && lx + item(s) > width as f64 - 10.0 {
                    lx = x0;
                    ly += 20.0;
                }
                out.extend(row(s, [lx, ly]));
                lx += item(s);
            }
            out
        }
        _ if named.is_empty() => Vec::new(),
        corner => {
            // One row per series in a box on the plot's ground, framed.
            let w = named.iter().map(|s| item(s) - 12.0).fold(0.0, f64::max) + 8.0;
            let h = 20.0 * named.len() as f64 + 8.0;
            let left = matches!(corner, Legend::TopLeft | Legend::BottomLeft);
            let top = matches!(corner, Legend::TopLeft | Legend::TopRight);
            let bx = if left { x0 + 8.0 } else { x0 + pw - 8.0 - w };
            let by = if top { y0 + 8.0 } else { y0 + ph - 8.0 - h };
            let ground = Shape::Rect { at: [bx, by], size: [w, h], color: palette.ground };
            let sides = [([bx, by], [bx + w, by]), ([bx + w, by], [bx + w, by + h]), ([bx + w, by + h], [bx, by + h]), ([bx, by + h], [bx, by])]
                .map(|(a, b)| line(a, b, 1.0, palette.grid));
            let rows = named.iter().enumerate().flat_map(|(i, s)| row(s, [bx + 6.0, by + 5.0 + 20.0 * i as f64]));
            std::iter::once(ground).chain(sides).chain(rows).collect()
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
    fn an_equal_chart_draws_a_unit_as_long_either_way() {
        // A unit circle's chart in a wide picture: x widened, y kept, a unit the same pixels.
        let mut chart = Chart::new("Smith", Axis::new("x", -1.0, 1.0), Axis::new("y", -1.0, 1.0), Vec::new());
        chart.look.equal = true;
        let at = layout_of(&chart, 900, 500);
        let [_, _, pw, ph] = at.plane;
        approx::assert_abs_diff_eq!((at.x.1 - at.x.0) / pw, (at.y.1 - at.y.0) / ph, epsilon = 1e-12);
        assert_eq!(at.y, (-1.0, 1.0));
        assert!(at.x.0 < -1.0 && at.x.1 > 1.0);
        // Not equal: the ranges as given.
        chart.look.equal = false;
        assert_eq!(layout_of(&chart, 900, 500).x, (-1.0, 1.0));
        let _ = draw(&Chart { look: Look { equal: true, ..Look::default() }, ..chart }, 300, 600, &Palette::fallback());
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
        styled.look = Look { grid: false, legend: Legend::TopRight, equal: false };
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
