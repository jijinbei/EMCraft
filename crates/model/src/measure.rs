//! Measuring between two picked things: points, planes (a picked face: a point on it and its
//! normal) and lines (a picked straight edge: a point on it and its direction). Lengths in SI.
//!
//! The distance and its components are always those between the two picked points; where the
//! picks say more, the perpendicular distance between them (`gap`: two parallel faces, a point
//! and a face, a point and an edge, two parallel edges, an edge parallel to a face) and the
//! angle between them (two faces, two edges, an edge and a face) come too. Plain vector algebra;
//! no tolerances beyond the parallel test.

use crate::error::{Result, invalid};
use crate::geom::{Point3, Vec3};

/// Two directions closer than this (radians) count as parallel.
pub const PARALLEL: f64 = 1e-9;

/// What a pick is besides its point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Point,
    /// A plane through the point, its normal.
    Plane(Vec3),
    /// A line through the point, its direction.
    Line(Vec3),
}

/// A picked thing: a point, and the plane or line it lies on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mark {
    pub at: Point3,
    pub kind: Kind,
}

impl Kind {
    /// The kind with its direction made a unit one ([`unit`]).
    fn normalized(self, what: &str) -> Result<Kind> {
        Ok(match self {
            Kind::Point => Kind::Point,
            Kind::Plane(n) => Kind::Plane(unit(what, n)?),
            Kind::Line(d) => Kind::Line(unit(what, d)?),
        })
    }
}

impl Mark {
    pub fn point(at: Point3) -> Mark {
        Mark { at, kind: Kind::Point }
    }
}

/// What lies between two marks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Measure {
    /// The distance between the two points (m).
    pub distance: f64,
    /// The second point less the first (m).
    pub delta: Vec3,
    /// The perpendicular distance (m), where the marks define one.
    pub gap: Option<f64>,
    /// The angle between two planes, two lines or a line and a plane (degrees, 0 to 90).
    pub angle: Option<f64>,
}

/// The unit direction of a mark's plane or line; a direction of no length or not finite is
/// refused.
fn unit(what: &str, d: Vec3) -> Result<Vec3> {
    d.normalized().filter(|u| u.is_finite()).ok_or_else(|| invalid(what, "must be a finite direction of nonzero length"))
}

/// The angle (degrees, 0 to 90) between two unit directions taken as unsigned lines.
fn between(a: Vec3, b: Vec3) -> f64 {
    a.dot(b).abs().min(1.0).acos().to_degrees()
}

/// The distance of `v` from the line along unit `d`.
fn off_line(v: Vec3, d: Vec3) -> f64 {
    v.cross(d).norm()
}

/// Measures from `a` to `b`.
///
/// ```
/// use emcraft_model::measure::{Kind, Mark, measure};
/// use emcraft_model::{Point3, Vec3};
/// // Two faces of a 3 mm slab (z = 0 and z = 0.003), picked at points 4 mm apart across it.
/// let z = Vec3::new(0.0, 0.0, 1.0);
/// let a = Mark { at: Point3::new(0.0, 0.0, 0.0), kind: Kind::Plane(z) };
/// let b = Mark { at: Point3::new(0.004, 0.0, 0.003), kind: Kind::Plane(-z) };
/// let m = measure(a, b).unwrap();
/// assert!((m.distance - 0.005).abs() < 1e-15);
/// assert!((m.gap.unwrap() - 0.003).abs() < 1e-15 && m.angle == Some(0.0));
/// // A point that is not finite, or a face without a normal, is refused.
/// assert!(measure(Mark::point(Point3::new(f64::NAN, 0.0, 0.0)), b).is_err());
/// assert!(measure(Mark { kind: Kind::Plane(Vec3::default()), ..a }, b).is_err());
/// ```
pub fn measure(a: Mark, b: Mark) -> Result<Measure> {
    for (what, m) in [("from", a), ("to", b)] {
        if !m.at.is_finite() {
            return Err(invalid(what, "must be a finite point"));
        }
    }
    let (ka, kb) = (a.kind.normalized("from")?, b.kind.normalized("to")?);
    let delta = b.at - a.at;
    let parallel = |x: Vec3, y: Vec3| x.cross(y).norm() < PARALLEL;
    let across = |x: Vec3, y: Vec3| x.dot(y).abs() < PARALLEL;
    let (gap, angle) = match (ka, kb) {
        (Kind::Point, Kind::Point) => (None, None),
        (Kind::Plane(n), Kind::Point) | (Kind::Point, Kind::Plane(n)) => (Some(delta.dot(n).abs()), None),
        (Kind::Line(d), Kind::Point) | (Kind::Point, Kind::Line(d)) => (Some(off_line(delta, d)), None),
        (Kind::Plane(n), Kind::Plane(m)) => (parallel(n, m).then(|| delta.dot(n).abs()), Some(between(n, m))),
        (Kind::Line(d), Kind::Line(e)) => (parallel(d, e).then(|| off_line(delta, d)), Some(between(d, e))),
        (Kind::Plane(n), Kind::Line(d)) | (Kind::Line(d), Kind::Plane(n)) => (across(n, d).then(|| delta.dot(n).abs()), Some(90.0 - between(n, d))),
    };
    Ok(Measure { distance: delta.norm(), delta, gap, angle })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64, z: f64) -> Point3 {
        Point3::new(x, y, z)
    }

    fn v(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3::new(x, y, z)
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-12
    }

    #[test]
    fn two_points_give_their_distance_and_its_components() {
        let m = measure(Mark::point(p(1.0, 2.0, 3.0)), Mark::point(p(4.0, 6.0, 3.0))).unwrap();
        assert!(close(m.distance, 5.0));
        assert_eq!(m.delta, v(3.0, 4.0, 0.0));
        assert_eq!((m.gap, m.angle), (None, None));
    }

    #[test]
    fn faces_give_the_gap_when_parallel_and_the_angle_always() {
        let plane = |at: Point3, n: Vec3| Mark { at, kind: Kind::Plane(n) };
        // Parallel faces facing each other, picked anywhere on them: the gap is across.
        let m = measure(plane(p(0.0, 0.0, 0.0), v(0.0, 2.0, 0.0)), plane(p(7.0, 3.0, -1.0), v(0.0, -1.0, 0.0))).unwrap();
        assert!(close(m.gap.unwrap(), 3.0) && close(m.angle.unwrap(), 0.0));
        // Faces at right angles: no gap, 90°.
        let m = measure(plane(p(0.0, 0.0, 0.0), v(1.0, 0.0, 0.0)), plane(p(1.0, 1.0, 0.0), v(0.0, 1.0, 0.0))).unwrap();
        assert!(m.gap.is_none() && close(m.angle.unwrap(), 90.0));
        // A face and a point: the point's height over it.
        let m = measure(plane(p(0.0, 0.0, 2.0), v(0.0, 0.0, 1.0)), Mark::point(p(5.0, 5.0, 4.5))).unwrap();
        assert!(close(m.gap.unwrap(), 2.5) && m.angle.is_none());
    }

    #[test]
    fn edges_give_the_distance_between_them_when_parallel() {
        let line = |at: Point3, d: Vec3| Mark { at, kind: Kind::Line(d) };
        let m = measure(line(p(0.0, 0.0, 0.0), v(0.0, 0.0, 1.0)), line(p(3.0, 4.0, 9.0), v(0.0, 0.0, -2.0))).unwrap();
        assert!(close(m.gap.unwrap(), 5.0) && close(m.angle.unwrap(), 0.0));
        // A point off an edge.
        let m = measure(Mark::point(p(0.0, 2.0, 7.0)), line(p(0.0, 0.0, 0.0), v(1.0, 0.0, 0.0))).unwrap();
        assert!(close(m.gap.unwrap(), (4.0f64 + 49.0).sqrt()) && m.angle.is_none());
        // An edge along a face: its height over it; an edge through a face at 30°.
        let m = measure(line(p(0.0, 0.0, 1.0), v(1.0, 0.0, 0.0)), Mark { at: p(0.0, 0.0, 0.0), kind: Kind::Plane(v(0.0, 0.0, 1.0)) }).unwrap();
        assert!(close(m.gap.unwrap(), 1.0) && close(m.angle.unwrap(), 0.0));
        let tilted = v(3f64.sqrt(), 0.0, 1.0);
        let m = measure(line(p(0.0, 0.0, 1.0), tilted), Mark { at: p(0.0, 0.0, 0.0), kind: Kind::Plane(v(0.0, 0.0, 1.0)) }).unwrap();
        assert!(m.gap.is_none() && (m.angle.unwrap() - 30.0).abs() < 1e-9);
    }

    #[test]
    fn hostile_marks_are_refused() {
        let ok = Mark::point(p(0.0, 0.0, 0.0));
        for bad in [
            Mark::point(p(f64::INFINITY, 0.0, 0.0)),
            Mark { at: p(0.0, 0.0, 0.0), kind: Kind::Line(v(0.0, 0.0, 0.0)) },
            Mark { at: p(0.0, 0.0, 0.0), kind: Kind::Plane(v(f64::NAN, 1.0, 0.0)) },
            Mark { at: p(0.0, 0.0, 0.0), kind: Kind::Plane(v(1e308, 1e308, 0.0)) },
        ] {
            assert!(measure(ok, bad).is_err() && measure(bad, ok).is_err(), "{bad:?}");
        }
    }
}
