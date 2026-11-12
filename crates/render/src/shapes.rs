//! A picture as a list of shapes in pixels — lines, triangles, rectangles, tinted bands and
//! labels — that any surface draws: this crate's canvas on the CPU ([`Drawing::canvas`]), or a
//! window's own vector canvas on its GPU. Charts and plot frames are made as such lists, so
//! every surface draws the same picture.
//!
//! ```
//! use emcraft_render::Rgb;
//! use emcraft_render::shapes::{Align, Drawing, Shape};
//! let d = Drawing {
//!     size: [40, 20],
//!     ground: Rgb::WHITE,
//!     shapes: vec![
//!         Shape::Line { a: [0.0, 10.0], b: [40.0, 10.0], width: 2.0, color: Rgb(0, 0, 0) },
//!         Shape::Text { at: [20.0, 0.0], text: "x".into(), scale: 1, color: Rgb(0, 0, 0), align: Align::Center, up: false },
//!     ],
//! };
//! let c = d.canvas();
//! assert_eq!((c.w, c.h), (40, 20));
//! assert_eq!(c.pixel(5, 10), Some(Rgb(0, 0, 0)));
//! ```

use crate::{Canvas, Rgb};

/// Where a label's anchor lies along its line: its start, middle or end (a label written
/// upwards: its bottom, middle or top).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// One shape, in pixels from the picture's top left.
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    /// A line `width` pixels wide.
    Line {
        a: [f64; 2],
        b: [f64; 2],
        width: f64,
        color: Rgb,
    },
    Triangle {
        p: [[f64; 2]; 3],
        color: Rgb,
    },
    Rect {
        at: [f64; 2],
        size: [f64; 2],
        color: Rgb,
    },
    /// A band: its upper edge left to right and then its lower edge back, tinting the area
    /// between them by `alpha`, within `clip` (x, y, width, height).
    Band {
        edge: Vec<[f64; 2]>,
        clip: [f64; 4],
        color: Rgb,
        alpha: f64,
    },
    /// A label of size `scale` ([`crate::text`]): its box's top at `at`'s y, placed along the
    /// line by `align` about `at`'s x; or (`up`) written upwards, its box's left at `at`'s x.
    Text {
        at: [f64; 2],
        text: String,
        scale: usize,
        color: Rgb,
        align: Align,
        up: bool,
    },
}

/// A picture as shapes: its size (pixels), its ground and the shapes in drawing order.
#[derive(Clone, Debug, PartialEq)]
pub struct Drawing {
    pub size: [usize; 2],
    pub ground: Rgb,
    pub shapes: Vec<Shape>,
}

impl Drawing {
    /// The picture drawn on the CPU.
    pub fn canvas(&self) -> Canvas {
        let mut c = Canvas::new(self.size[0], self.size[1], self.ground);
        paint(&mut c, &self.shapes);
        c
    }
}

/// How far before its anchor a label of `width` pixels starts.
fn lead(align: Align, width: f64) -> f64 {
    match align {
        Align::Left => 0.0,
        Align::Center => width / 2.0,
        Align::Right => width,
    }
}

/// Draws `shapes` on `c`, in order.
pub fn paint(c: &mut Canvas, shapes: &[Shape]) {
    for s in shapes {
        match s {
            Shape::Line { a, b, width, color } => c.line(*a, *b, *width, *color),
            Shape::Triangle { p, color } => c.triangle(*p, *color),
            Shape::Rect { at, size, color } => c.rect(at[0], at[1], size[0], size[1], *color),
            Shape::Band { edge, clip, color, alpha } => band(c, edge, *clip, *color, *alpha),
            Shape::Text { at, text, scale, color, align, up } => {
                let w = Canvas::text_width(text, *scale) as f64;
                if *up {
                    c.text_up(at[0], at[1] + lead(*align, w), text, *scale, *color);
                } else {
                    c.text(at[0] - lead(*align, w), at[1], text, *scale, *color);
                }
            }
        }
    }
}

/// A band: `pts` the upper edge left to right and then the lower edge back, tinting each pixel
/// column between the two edges within `clip` by `alpha`.
fn band(c: &mut Canvas, pts: &[[f64; 2]], clip: [f64; 4], color: Rgb, alpha: f64) {
    let m = pts.len() / 2;
    if m < 2 || pts.len() != 2 * m {
        return;
    }
    let (upper, lower): (Vec<[f64; 2]>, Vec<[f64; 2]>) = (pts[..m].to_vec(), pts[m..].iter().rev().copied().collect());
    let [x0, y0, pw, ph] = clip;
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
                c.blend(px, py, color, alpha);
            }
        }
    }
}
