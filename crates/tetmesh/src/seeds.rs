//! Edge seeds as the meshers take them: a design edge's polyline with the count, bias and
//! constraint the model's seed gives it (`crate::compat::seeds`).
//!
//! The meshers recover their own curves from the face-tagged boundary surface, so a seed is
//! matched to a mesher's curve by position: the curve's end points lie on the seed's end points
//! (either way round, which gives the orientation the bias is measured in) and each one's middle
//! lies on the other, within the tolerance of the two tessellations. The seed's node positions
//! are placed by arc length along the mesher's curve ([`place`]).

use crate::compat::{Bias, SeedConstraint, seeds::spacing};

use crate::Point3;
use crate::{Error, Result};

/// A seed on one design edge.
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeSeed {
    /// The edge's name, for messages.
    pub name: String,
    /// The edge's polyline, start to end.
    pub points: Vec<Point3>,
    /// How far the polyline may lie from the exact edge (its tessellation's chordal tolerance).
    pub tolerance: f64,
    /// The divisions the seed asks for.
    pub count: usize,
    pub bias: Option<Bias>,
    pub constraint: SeedConstraint,
}

impl EdgeSeed {
    /// `n + 1` node positions as fractions of the arc length from the seed's start (reversed:
    /// from its end).
    pub fn fractions(&self, n: usize, reversed: bool) -> Vec<f64> {
        let f = spacing(u32::try_from(n).unwrap_or(u32::MAX), self.bias);
        if reversed { f.iter().rev().map(|x| 1.0 - x).collect() } else { f }
    }

    /// The seed in words: `F2:a|F2:b (8 exact)`.
    pub fn describe(&self) -> String {
        format!("{} ({} {})", self.name, self.count, self.constraint.name())
    }
}

fn length(poly: &[Point3]) -> f64 {
    poly.windows(2).map(|w| w[0].distance(w[1])).sum()
}

/// The point at fraction `t` of a polyline's arc length.
fn at_fraction(poly: &[Point3], t: f64) -> Option<Point3> {
    let total = length(poly);
    let target = t.clamp(0.0, 1.0) * total;
    let mut walked = 0.0;
    for w in poly.windows(2) {
        let l = w[0].distance(w[1]);
        if walked + l >= target && l > 0.0 {
            return Some(w[0].lerp(w[1], ((target - walked) / l).clamp(0.0, 1.0)));
        }
        walked += l;
    }
    poly.last().copied()
}

fn point_segment(p: Point3, a: Point3, b: Point3) -> f64 {
    let ab = b - a;
    let l2 = ab.dot(ab);
    let t = if l2 > 0.0 { ((p - a).dot(ab) / l2).clamp(0.0, 1.0) } else { 0.0 };
    p.distance(a.lerp(b, t))
}

/// The distance from `p` to a polyline.
pub(crate) fn distance_to(poly: &[Point3], p: Point3) -> f64 {
    point_polyline(p, poly)
}

fn point_polyline(p: Point3, poly: &[Point3]) -> f64 {
    match poly {
        [] => f64::INFINITY,
        [a] => p.distance(*a),
        _ => poly.windows(2).map(|w| point_segment(p, w[0], w[1])).fold(f64::INFINITY, f64::min),
    }
}

/// The points at the given arc-length fractions along `poly` (its end points exact).
pub(crate) fn place(poly: &[Point3], fractions: &[f64]) -> Result<Vec<Point3>> {
    let (Some(&first), Some(&last)) = (poly.first(), poly.last()) else {
        return Err(Error::Invalid("empty polyline".into()));
    };
    let n = fractions.len();
    fractions
        .iter()
        .enumerate()
        .map(|(k, &t)| {
            if k == 0 {
                Ok(first)
            } else if k + 1 == n {
                Ok(last)
            } else {
                at_fraction(poly, t).ok_or_else(|| Error::Failed("seed spacing walked off the curve".into()))
            }
        })
        .collect()
}

/// The element size a seed asks for at each of its nodes (the mean length of the elements on
/// either side), for size fields.
pub fn node_sizes(seed: &EdgeSeed) -> Vec<(Point3, f64)> {
    let f = seed.fractions(seed.count, false);
    let total = length(&seed.points);
    let Ok(pts) = place(&seed.points, &f) else { return Vec::new() };
    pts.iter()
        .enumerate()
        .filter_map(|(k, p)| {
            let before = k.checked_sub(1).and_then(|j| Some(f.get(k)? - f.get(j)?));
            let after = f.get(k + 1).zip(f.get(k)).map(|(b, a)| b - a);
            let h = match (before, after) {
                (Some(x), Some(y)) => 0.5 * (x + y),
                (Some(x), None) | (None, Some(x)) => x,
                (None, None) => return None,
            } * total;
            (h > 0.0 && h.is_finite()).then_some((*p, h))
        })
        .collect()
}
