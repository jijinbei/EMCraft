//! Field plots on a plane: a scalar field as a colour map (a component at one phase, or a
//! magnitude) with vector fields drawn over it as arrows, the plane's outlines, a colour bar
//! and a legend. Coordinates are in display units; the plane keeps its true aspect ratio.

use crate::axes::{self, Axis};
use crate::{Canvas, Rgb, colormap};

/// How a scalar maps to colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScalarKind {
    /// Signed values, symmetric about zero (diverging map).
    Signed,
    /// Magnitudes from zero (sequential map).
    Magnitude,
}

/// A scalar field sampled on a grid spanning the plot's axes: `nx × ny` values, row-major,
/// row 0 at the y axis' minimum. NaN marks points outside the domain, drawn as metal.
#[derive(Clone, Debug, PartialEq)]
pub struct Scalar {
    pub label: String,
    pub nx: usize,
    pub ny: usize,
    pub values: Vec<f64>,
    pub kind: ScalarKind,
    /// The value drawn at full colour; the largest |value| when `None`.
    pub range: Option<f64>,
}

impl Scalar {
    fn at(&self, i: usize, j: usize) -> f64 {
        self.values.get(j * self.nx + i).copied().unwrap_or(f64::NAN)
    }

    /// Bilinear interpolation at fractional grid coordinates.
    fn sample(&self, u: f64, v: f64) -> f64 {
        if self.nx < 2 || self.ny < 2 {
            return self.values.first().copied().unwrap_or(f64::NAN);
        }
        let x = u.clamp(0.0, 1.0) * (self.nx - 1) as f64;
        let y = v.clamp(0.0, 1.0) * (self.ny - 1) as f64;
        let (i, j) = ((x.floor() as usize).min(self.nx - 2), (y.floor() as usize).min(self.ny - 2));
        let (fx, fy) = (x - i as f64, y - j as f64);
        let a = self.at(i, j) * (1.0 - fx) + self.at(i + 1, j) * fx;
        let b = self.at(i, j + 1) * (1.0 - fx) + self.at(i + 1, j + 1) * fx;
        a * (1.0 - fy) + b * fy
    }

    fn scale(&self) -> f64 {
        let r = self.range.unwrap_or_else(|| self.values.iter().filter(|v| v.is_finite()).fold(0.0f64, |m, v| m.max(v.abs())));
        if r > 0.0 && r.is_finite() { r } else { 1.0 }
    }
}

/// Arrows of one vector field: positions and in-plane vectors (any unit).
#[derive(Clone, Debug, PartialEq)]
pub struct Arrows {
    pub label: String,
    pub color: Rgb,
    pub arrows: Vec<([f64; 2], [f64; 2])>,
    /// The magnitude drawn at the plot's full arrow length; the largest one when `None`.
    pub scale: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FieldPlot {
    pub title: String,
    pub subtitle: String,
    pub x: Axis,
    pub y: Axis,
    pub scalar: Option<Scalar>,
    pub arrows: Vec<Arrows>,
    /// Lines in the plane (section boundaries), in axis units.
    pub lines: Vec<[[f64; 2]; 2]>,
    /// The length of a full arrow, in axis units.
    pub arrow_length: f64,
}

/// The colour of points outside the domain (conductor).
pub const METAL: Rgb = Rgb(150, 150, 155);

const LEFT: f64 = 80.0;
const RIGHT: f64 = 130.0;
const TOP: f64 = 62.0;
/// Pixels from one header line to the next.
const LINE: f64 = 22.0;

/// Splits `text` into lines at most `room` pixels wide at `scale`, at spaces outside TeX math
/// (`$…$` stays whole); a word wider than the room stands alone.
fn wrap(text: &str, room: f64, scale: usize) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    let (mut word, mut math) = (String::new(), false);
    for ch in text.chars() {
        if ch == '$' {
            math = !math;
        }
        if ch == ' ' && !math {
            words.push(std::mem::take(&mut word));
        } else {
            word.push(ch);
        }
    }
    words.push(word);
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for w in words.into_iter().filter(|w| !w.is_empty()) {
        let longer = if line.is_empty() { w.clone() } else { format!("{line} {w}") };
        if line.is_empty() || crate::text::width(&longer, scale) <= room {
            line = longer;
        } else {
            lines.push(std::mem::replace(&mut line, w));
        }
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines
}

/// The header of a plot drawn `width` pixels wide: the title's lines, the subtitle's.
fn header(plot: &FieldPlot, width: f64) -> (Vec<String>, Vec<String>) {
    let room = (width - LEFT - 10.0).max(100.0);
    (wrap(&plot.title, room, 2), wrap(&plot.subtitle, room, 2))
}

/// The plane's top below a header of `lines` lines (two fit the usual margin).
fn top(lines: usize) -> f64 {
    TOP + LINE * lines.saturating_sub(2) as f64
}
const BOTTOM: f64 = 90.0;

/// Where a plot drawn at some width puts its plane: the picture's size and the plane's
/// rectangle in it (pixels), and the axes' ranges.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub width: usize,
    pub height: usize,
    /// The plane's rectangle: left, top, width, height.
    pub plane: [f64; 4],
    pub x: (f64, f64),
    pub y: (f64, f64),
}

impl Layout {
    /// The plot of `plot` drawn `width` pixels wide (the height follows the plane's aspect
    /// ratio).
    pub fn of(plot: &FieldPlot, width: usize) -> Layout {
        let (x, y) = (plot.x.span(), plot.y.span());
        let pw = (width as f64 - LEFT - RIGHT).max(100.0);
        let ph = (pw * (y.1 - y.0) / (x.1 - x.0)).clamp(60.0, 3000.0);
        let pw = if ph >= 3000.0 { 3000.0 * (x.1 - x.0) / (y.1 - y.0) } else { pw };
        let (title, subtitle) = header(plot, pw + LEFT + RIGHT);
        let top = top(title.len() + subtitle.len());
        Layout { width: (pw + LEFT + RIGHT).ceil() as usize, height: (ph + top + BOTTOM).ceil() as usize, plane: [LEFT, top, pw, ph], x, y }
    }

    /// A point of the plane (axis units) in pixels.
    pub fn to_pixels(&self, p: [f64; 2]) -> [f64; 2] {
        let [x0, y0, pw, ph] = self.plane;
        [x0 + (p[0] - self.x.0) / (self.x.1 - self.x.0) * pw, y0 + ph - (p[1] - self.y.0) / (self.y.1 - self.y.0) * ph]
    }

    /// The point of the plane (axis units) under a pixel position; `None` off the plane.
    pub fn to_axes(&self, px: [f64; 2]) -> Option<[f64; 2]> {
        let [x0, y0, pw, ph] = self.plane;
        let (u, v) = ((px[0] - x0) / pw, 1.0 - (px[1] - y0) / ph);
        ((0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v)).then_some([self.x.0 + u * (self.x.1 - self.x.0), self.y.0 + v * (self.y.1 - self.y.0)])
    }
}

/// Draws the plot `width` pixels wide (the height follows the plane's aspect ratio); see
/// [`Layout::of`].
pub fn draw(plot: &FieldPlot, width: usize) -> Canvas {
    let layout = Layout::of(plot, width);
    let [x0, y0, pw, ph] = layout.plane;
    let (xa, xb) = layout.x;
    let mut c = Canvas::new(layout.width, layout.height, Rgb::WHITE);
    let ink = Rgb(30, 30, 30);
    let to_px = |p: [f64; 2]| layout.to_pixels(p);

    // The title and the subtitle, wrapped to the picture's width.
    let (title, subtitle) = header(plot, layout.width as f64);
    for (i, (line, colour)) in title.iter().map(|l| (l, ink)).chain(subtitle.iter().map(|l| (l, Rgb(90, 90, 90)))).enumerate() {
        c.text(LEFT, 10.0 + LINE * i as f64, line, 2, colour);
    }

    if let Some(s) = &plot.scalar {
        let r = s.scale();
        for py in 0..ph.ceil() as usize {
            for px in 0..pw.ceil() as usize {
                let u = (px as f64 + 0.5) / pw;
                let v = 1.0 - (py as f64 + 0.5) / ph;
                let val = s.sample(u, v);
                if !val.is_finite() {
                    // Outside the domain: metal (an iris, a step's wall).
                    c.set((x0 + px as f64) as i64, (y0 + py as f64) as i64, METAL);
                    continue;
                }
                let col = match s.kind {
                    ScalarKind::Signed => colormap::diverging(val / r),
                    ScalarKind::Magnitude => colormap::sequential(val / r),
                };
                c.set((x0 + px as f64) as i64, (y0 + py as f64) as i64, col);
            }
        }
        // The colour bar.
        let (bx, by, bw, bh) = (x0 + pw + 22.0, y0, 18.0, ph);
        for k in 0..bh.ceil() as usize {
            let t = 1.0 - k as f64 / bh.max(1.0);
            let col = match s.kind {
                ScalarKind::Signed => colormap::diverging(2.0 * t - 1.0),
                ScalarKind::Magnitude => colormap::sequential(t),
            };
            c.rect(bx, by + k as f64, bw, 1.0, col);
        }
        for (a, b) in [([bx, by], [bx + bw, by]), ([bx + bw, by], [bx + bw, by + bh]), ([bx + bw, by + bh], [bx, by + bh]), ([bx, by + bh], [bx, by])]
        {
            c.line(a, b, 1.0, ink);
        }
        let marks: Vec<(f64, f64)> = match s.kind {
            ScalarKind::Signed => vec![(r, 0.0), (0.0, 0.5), (-r, 1.0)],
            ScalarKind::Magnitude => vec![(r, 0.0), (r / 2.0, 0.5), (0.0, 1.0)],
        };
        for (v, f) in marks {
            c.text(bx + bw + 5.0, by + f * bh - 7.0, &axes::format(v), 2, ink);
        }
        c.text(bx - 10.0, y0 + ph + 34.0, &s.label, 2, ink);
    }

    for l in &plot.lines {
        c.line(to_px(l[0]), to_px(l[1]), 1.5, Rgb(60, 60, 60));
    }

    let full = plot.arrow_length / (xb - xa) * pw;
    for set in &plot.arrows {
        let largest = set.arrows.iter().map(|(_, v)| (v[0] * v[0] + v[1] * v[1]).sqrt()).filter(|m| m.is_finite()).fold(0.0f64, f64::max);
        let scale = set.scale.filter(|s| *s > 0.0 && s.is_finite()).unwrap_or(largest);
        if !(scale > 0.0) {
            continue;
        }
        for (p, v) in &set.arrows {
            let q = to_px(*p);
            // Arrows above the scale (a source's spike) stop at the full length.
            let k = full / scale.max((v[0] * v[0] + v[1] * v[1]).sqrt());
            // Screen y points down.
            let d = [v[0] * k, -v[1] * k];
            if !(d[0].is_finite() && d[1].is_finite()) || (d[0] * d[0] + d[1] * d[1]).sqrt() < 2.5 {
                continue;
            }
            let a = [q[0] - d[0] / 2.0, q[1] - d[1] / 2.0];
            let b = [q[0] + d[0] / 2.0, q[1] + d[1] / 2.0];
            let head = (0.35 * (d[0] * d[0] + d[1] * d[1]).sqrt()).clamp(3.0, 9.0);
            c.arrow(a, b, 3.2, head + 1.5, Rgb::WHITE);
            c.arrow(a, b, 1.6, head, set.color);
        }
    }

    axes::frame(&mut c, [x0, y0, pw, ph], &plot.x, &plot.y, ink, None);

    // Legend: one sample arrow per vector field.
    let mut lx = LEFT;
    let ly = y0 + ph + 62.0;
    for set in &plot.arrows {
        c.arrow([lx, ly + 7.0], [lx + 28.0, ly + 7.0], 1.6, 8.0, set.color);
        c.text(lx + 36.0, ly, &set.label, 2, ink);
        lx += 46.0 + Canvas::text_width(&set.label, 2) as f64 + 20.0;
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plot() -> FieldPlot {
        let (nx, ny) = (20, 10);
        let values = (0..nx * ny).map(|k| ((k % nx) as f64 / (nx - 1) as f64 * std::f64::consts::PI).sin()).collect();
        FieldPlot {
            title: "TEST".into(),
            subtitle: "sub".into(),
            x: Axis::new("x (mm)", 0.0, 20.0),
            y: Axis::new("y (mm)", 0.0, 10.0),
            scalar: Some(Scalar { label: "Ey".into(), nx, ny, values, kind: ScalarKind::Signed, range: None }),
            arrows: vec![Arrows {
                label: "H".into(),
                color: Rgb(0, 0, 0),
                arrows: vec![([5.0, 5.0], [1.0, 0.0]), ([15.0, 5.0], [0.0, -1.0])],
                scale: None,
            }],
            lines: vec![[[10.0, 0.0], [10.0, 10.0]]],
            arrow_length: 2.0,
        }
    }

    #[test]
    fn colours_follow_the_field_and_arrows_point_their_way() {
        let c = draw(&plot(), 600);
        let pw = c.w as f64 - LEFT - RIGHT;
        let ph = c.h as f64 - TOP - BOTTOM;
        let at = |x: f64, y: f64| c.pixel((LEFT + x / 20.0 * pw) as usize, (TOP + ph - y / 10.0 * ph) as usize).unwrap();
        // sin(πx/a) peaks red in the middle, white at the walls.
        let mid = at(7.5, 8.5);
        assert!(mid.0 > 150 && mid.2 < 140, "{mid:?}");
        let wall = at(0.2, 8.5);
        assert!(wall.0 > 200 && wall.1 > 200 && wall.2 > 200, "{wall:?}");
        // The arrow at (5, 5) points +x: its head (dark) is right of the centre, not left.
        let dark = |c: Rgb| c.0 < 60 && c.1 < 60 && c.2 < 60;
        assert!(dark(at(5.7, 5.0)));
        // The aspect ratio is the plane's.
        assert!((pw / ph - 2.0).abs() < 0.05);
        // Pixels and axes map back and forth.
        let l = Layout::of(&plot(), 600);
        let q = l.to_axes(l.to_pixels([7.5, 2.5])).unwrap();
        assert!((q[0] - 7.5).abs() < 1e-9 && (q[1] - 2.5).abs() < 1e-9);
        assert_eq!(l.to_axes([0.0, 0.0]), None);
        assert_eq!((l.width, l.height), (c.w, c.h));
    }

    #[test]
    fn deterministic_and_hostile_inputs() {
        assert_eq!(draw(&plot(), 600), draw(&plot(), 600));
        let mut p = plot();
        p.x = Axis::new("x", f64::NAN, f64::INFINITY);
        p.scalar = Some(Scalar { label: "s".into(), nx: 0, ny: 0, values: vec![], kind: ScalarKind::Magnitude, range: Some(f64::NAN) });
        p.arrows[0].arrows.push(([f64::NAN, 0.0], [f64::INFINITY, 1.0]));
        let c = draw(&p, 0);
        assert!(c.png().is_some());
    }

    #[test]
    fn a_long_header_wraps_and_the_plane_moves_down() {
        let lines = wrap("Re(F e$^{j omega t}$) at one two three four five six seven", 200.0, 2);
        assert!(lines.len() > 1, "{lines:?}");
        assert!(lines.iter().any(|l| l.contains("$^{j omega t}$")), "math kept whole: {lines:?}");
        assert_eq!(top(2), TOP);
        assert!(top(4) > TOP);
    }
}
