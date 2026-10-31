//! Prisms: a simple polygon of the plane z = 0 extruded along z, as a closed triangulated surface
//! ([`Shape::Surface`]) whose faces are its two caps and one rectangle per side. Bends, mitred
//! corners and junctions of rectangular guides are prisms of their outline in the plane of the
//! bend.
//!
//! The caps are triangulated by ear clipping: every simple polygon of four or more corners has
//! two non-overlapping ears (G. H. Meisters, *Polygons have ears*, Amer. Math. Monthly 82(6),
//! 1975, 648–651), so cutting one off at a time ends in triangles (D. Eberly, *Triangulation by
//! Ear Clipping*, Geometric Tools, 2002). The mesher refines the faces to its size.
//!
//! ```
//! use emcraft_model::Shape;
//! // An L: 3 × 1 along x, then up 2 more along y at its end.
//! let l = Shape::prism(&[[0.0, 0.0], [3.0, 0.0], [3.0, 3.0], [2.0, 3.0], [2.0, 1.0], [0.0, 1.0]], 0.5)?;
//! assert!((l.volume() - 2.5).abs() < 1e-12);
//! assert!(l.contains(emcraft_model::Point3::new(2.5, 2.5, 0.25)));
//! assert!(!l.contains(emcraft_model::Point3::new(1.0, 2.0, 0.25)));
//! // Self-crossing outlines and too few corners are refused.
//! assert!(Shape::prism(&[[0.0, 0.0], [1.0, 1.0], [1.0, 0.0], [0.0, 1.0]], 1.0).is_err());
//! assert!(Shape::prism(&[[0.0, 0.0], [1.0, 0.0]], 1.0).is_err());
//! # Ok::<(), emcraft_model::ModelError>(())
//! ```

use crate::error::{Result, invalid, positive};
use crate::geom::{Point3, Vec3};
use crate::shape::{FaceSurface, Shape};

/// The most corners a prism's polygon may have.
pub const MAX_PRISM_POINTS: usize = 256;

/// (b − a) × (c − a): positive when a, b, c turn anticlockwise.
fn turn(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

/// Whether `c` lies in the box spanned by `a` and `b`.
fn in_box(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> bool {
    (0..2).all(|k| c[k] >= a[k].min(b[k]) && c[k] <= a[k].max(b[k]))
}

/// Whether two turns are strictly of opposite sense.
fn opposite(x: f64, y: f64) -> bool {
    (x > 0.0 && y < 0.0) || (x < 0.0 && y > 0.0)
}

/// Whether segments p–q and r–s meet (touching included).
fn meet(p: [f64; 2], q: [f64; 2], r: [f64; 2], s: [f64; 2]) -> bool {
    let (d1, d2, d3, d4) = (turn(r, s, p), turn(r, s, q), turn(p, q, r), turn(p, q, s));
    opposite(d1, d2) && opposite(d3, d4)
        || (d1 == 0.0 && in_box(r, s, p))
        || (d2 == 0.0 && in_box(r, s, q))
        || (d3 == 0.0 && in_box(p, q, r))
        || (d4 == 0.0 && in_box(p, q, s))
}

/// The polygon checked and tidied: finite, its corners distinct, straight corners dropped, simple
/// (no two sides meet but neighbours at their corner), anticlockwise.
fn simple(points: &[[f64; 2]]) -> Result<Vec<[f64; 2]>> {
    if points.len() > MAX_PRISM_POINTS {
        return Err(invalid("prism", format!("has {} corners; at most {MAX_PRISM_POINTS}", points.len())));
    }
    if !points.iter().flatten().all(|c| c.is_finite()) {
        return Err(invalid("prism", "has a corner that is not a finite number"));
    }
    let scale = points.iter().flatten().fold(0.0, |m: f64, c| m.max(c.abs())).max(f64::MIN_POSITIVE);
    let tol = 1e-12 * scale * scale;
    // Straight corners (and repeated ones) dropped until none is left.
    let mut p: Vec<[f64; 2]> = points.to_vec();
    loop {
        let n = p.len();
        if n < 3 {
            return Err(invalid("prism", "needs at least three corners not on one line"));
        }
        let Some(i) = (0..n).find(|&i| turn(p[(i + n - 1) % n], p[i], p[(i + 1) % n]).abs() <= tol) else { break };
        p.remove(i);
    }
    let n = p.len();
    for i in 0..n {
        for j in i + 1..n {
            if p[i] == p[j] {
                return Err(invalid("prism", "has a corner twice"));
            }
            // Sides i (p[i]–p[i+1]) and j, unless they are neighbours.
            if j == i + 1 || (i == 0 && j == n - 1) {
                continue;
            }
            if meet(p[i], p[(i + 1) % n], p[j], p[(j + 1) % n]) {
                return Err(invalid("prism", format!("its sides {} and {} cross or touch; the outline must be a simple polygon", i + 1, j + 1)));
            }
        }
    }
    let area: f64 = (0..n).map(|i| p[i][0] * p[(i + 1) % n][1] - p[(i + 1) % n][0] * p[i][1]).sum();
    if area < 0.0 {
        p.reverse();
    }
    Ok(p)
}

/// An anticlockwise simple polygon cut into triangles (corner indices, anticlockwise) by ear
/// clipping.
fn ears(p: &[[f64; 2]]) -> Result<Vec<[usize; 3]>> {
    let mut left: Vec<usize> = (0..p.len()).collect();
    let mut out = Vec::with_capacity(p.len().saturating_sub(2));
    // Each pass round the polygon cuts an ear (a simple polygon has two); the cap is for safety.
    let mut passes = 0;
    while left.len() > 3 {
        passes += 1;
        if passes > p.len() * p.len() {
            return Err(invalid("prism", "its outline could not be cut into triangles"));
        }
        let n = left.len();
        let ear = (0..n).find(|&k| {
            let (a, b, c) = (left[(k + n - 1) % n], left[k], left[(k + 1) % n]);
            turn(p[a], p[b], p[c]) > 0.0
                && left.iter().filter(|&&v| v != a && v != b && v != c).all(|&v| {
                    // No other corner inside the ear or on its sides.
                    !(turn(p[a], p[b], p[v]) >= 0.0 && turn(p[b], p[c], p[v]) >= 0.0 && turn(p[c], p[a], p[v]) >= 0.0)
                })
        });
        let Some(k) = ear else { return Err(invalid("prism", "its outline could not be cut into triangles")) };
        out.push([left[(k + n - 1) % n], left[k], left[(k + 1) % n]]);
        left.remove(k);
    }
    if let [a, b, c] = left[..] {
        out.push([a, b, c]);
    }
    Ok(out)
}

impl Shape {
    /// A prism: the simple polygon `points` (m, its corners in order either way round) of the
    /// plane z = 0 extruded to z = `length`. Face tags: 0 the bottom cap, 1 the top, 2 + i the
    /// side from corner i to the next (after straight corners are dropped and the outline is
    /// turned anticlockwise).
    pub fn prism(points: &[[f64; 2]], length: f64) -> Result<Shape> {
        positive("prism length", length)?;
        let p = simple(points)?;
        let n = p.len();
        let caps = ears(&p)?;
        let mut positions: Vec<Point3> = p.iter().map(|q| Point3::new(q[0], q[1], 0.0)).collect();
        positions.extend(p.iter().map(|q| Point3::new(q[0], q[1], length)));
        let (mut triangles, mut tags) = (Vec::with_capacity(2 * caps.len() + 2 * n), Vec::with_capacity(2 * caps.len() + 2 * n));
        for &[a, b, c] in &caps {
            // Outward: the bottom cap faces −z, the top +z.
            triangles.push([a as u32, c as u32, b as u32]);
            tags.push(0);
            triangles.push([(a + n) as u32, (b + n) as u32, (c + n) as u32]);
            tags.push(1);
        }
        let mut surfaces = vec![
            Some(FaceSurface::Plane { normal: Vec3::new(0.0, 0.0, -1.0), point: Point3::ORIGIN }),
            Some(FaceSurface::Plane { normal: Vec3::new(0.0, 0.0, 1.0), point: Point3::new(0.0, 0.0, length) }),
        ];
        for i in 0..n {
            let j = (i + 1) % n;
            let (bi, bj, ti, tj) = (i as u32, j as u32, (i + n) as u32, (j + n) as u32);
            triangles.push([bi, bj, tj]);
            triangles.push([bi, tj, ti]);
            let tag = (2 + i) as u32;
            tags.extend([tag, tag]);
            let (dx, dy) = (p[j][0] - p[i][0], p[j][1] - p[i][1]);
            let normal = Vec3::new(dy, -dx, 0.0) / dx.hypot(dy);
            surfaces.push(Some(FaceSurface::Plane { normal, point: positions[i] }));
        }
        let shape = Shape::Surface { name: "prism".into(), positions, triangles, tags, surfaces };
        shape.check()?;
        Ok(shape)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prisms_are_closed_and_hold_their_area() {
        // A tee: a bar 4 × 1 with a stem 1 × 2 below its middle, given clockwise.
        let tee = [[0.0, 3.0], [4.0, 3.0], [4.0, 2.0], [2.5, 2.0], [2.5, 0.0], [1.5, 0.0], [1.5, 2.0], [0.0, 2.0]];
        let mut cw = tee;
        cw.reverse();
        for pts in [tee, cw] {
            let s = Shape::prism(&pts, 2.0).unwrap();
            assert!((s.volume() - 12.0).abs() < 1e-12);
            let Shape::Surface { triangles, tags, .. } = &s else { panic!() };
            assert_eq!(triangles.len(), 2 * 6 + 2 * 8);
            assert_eq!(tags.iter().max(), Some(&9));
        }
        // A straight corner on a side is dropped.
        let s = Shape::prism(&[[0.0, 0.0], [1.0, 0.0], [2.0, 0.0], [2.0, 1.0], [0.0, 1.0]], 1.0).unwrap();
        let Shape::Surface { triangles, .. } = &s else { panic!() };
        assert_eq!(triangles.len(), 2 * 2 + 2 * 4);
        assert!(Shape::prism(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]], 0.0).is_err());
        assert!(Shape::prism(&[[0.0, 0.0], [1.0, 0.0], [f64::NAN, 1.0]], 1.0).is_err());
        assert!(Shape::prism(&[[0.0, 0.0], [1.0, 0.0], [2.0, 0.0]], 1.0).is_err());
        assert!(Shape::prism(&vec![[0.0, 0.0]; MAX_PRISM_POINTS + 1], 1.0).is_err());
        // Two sides touching at a point.
        assert!(Shape::prism(&[[0.0, 0.0], [2.0, 0.0], [1.0, 1.0], [2.0, 2.0], [0.0, 2.0], [1.0, 1.0]], 1.0).is_err());
    }
}
