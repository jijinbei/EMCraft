//! Unions whose faces share a plane with the region's: a stub standing on a guide's broad wall
//! with its sides in the guide's side walls (an E-plane tee built of two boxes), a box carrying
//! the guide on past its end. The two surfaces are coplanar there and the mesh Boolean
//! ([`crate::csg`]) needs them in general position, but moving either face (`crate::flush`)
//! would change the union: the stub's side reaches past the region's face.
//!
//! When both bodies lie wholly behind such a plane P (nothing of either beyond it), the union is
//! found in two Booleans in general position instead: both bodies grown past P by different
//! amounts — each vertex on P moved straight out of it, which keeps every face meeting P in its
//! plane when those faces are perpendicular to P (checked) — united, and the union cut back at P
//! by intersecting it with a box whose faces lie in the planes P and far beyond everything
//! elsewhere. Grown body ∩ half-space = body, so (A′ ∪ B′) ∩ H = A ∪ B exactly; the box's face
//! in P takes the region's face tag there, so the walls and ports on P keep their names.

use emcraft_model::{Aabb, Point3};

use crate::csg::Surface;
use crate::flush::Triangles;
use crate::shared::corners;

/// A plane across axis `axis` at `at` that both bodies lie behind, `outward` (±1) its normal
/// along the axis pointing away from them; `tag` the region's face tag on it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Bound {
    pub(crate) axis: usize,
    pub(crate) at: f64,
    pub(crate) outward: f64,
    pub(crate) tag: u32,
}

/// The triangles flat across an axis (corners within `tol` along it): (axis, plane, outward
/// sign, triangle).
pub(crate) fn flat(s: Triangles, tol: f64) -> Vec<(usize, f64, f64, usize)> {
    let mut out = Vec::new();
    for (i, t) in s.triangles.iter().enumerate() {
        let Some(q) = corners(s.positions, t) else { continue };
        let n = (q[1] - q[0]).cross(q[2] - q[0]);
        for d in 0..3 {
            let (lo, hi) = q.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p[d]), b.max(p[d])));
            if hi - lo <= tol && n[d] != 0.0 {
                out.push((d, q[0][d], n[d].signum(), i));
                break;
            }
        }
    }
    out
}

/// Whether every vertex of `s` lies behind the plane (within `tol`), and every triangle with a
/// corner on it either lies in it or is perpendicular to it.
fn behind(s: Triangles, axis: usize, at: f64, outward: f64, tol: f64) -> bool {
    if s.positions.iter().any(|p| outward * (p[axis] - at) > tol) {
        return false;
    }
    s.triangles.iter().all(|t| {
        let Some(q) = corners(s.positions, t) else { return false };
        let on = q.iter().filter(|p| (p[axis] - at).abs() <= tol).count();
        if on == 0 || on == 3 {
            return true;
        }
        let n = (q[1] - q[0]).cross(q[2] - q[0]);
        n[axis].abs() <= 1e-9 * n.norm()
    })
}

/// The planes in which `tool`'s faces lie in `main`'s with the same orientation and which both
/// lie behind (see the module notes), each with `main`'s smallest face tag there; at most one per
/// axis and side.
pub(crate) fn bounds(main: Triangles, main_tags: &[u32], tool: Triangles, tol: f64) -> Vec<Bound> {
    let (fm, ft) = (flat(main, tol), flat(tool, tol));
    let mut out: Vec<Bound> = Vec::new();
    for &(axis, at, outward, i) in &fm {
        if out.iter().any(|b| b.axis == axis && b.outward == outward) {
            continue;
        }
        if !ft.iter().any(|&(d, c, s, _)| d == axis && s == outward && (c - at).abs() <= tol) {
            continue;
        }
        if !(behind(main, axis, at, outward, tol) && behind(tool, axis, at, outward, tol)) {
            continue;
        }
        let tag = fm
            .iter()
            .filter(|&&(d, c, s, _)| d == axis && s == outward && (c - at).abs() <= tol)
            .filter_map(|&(_, _, _, j)| main_tags.get(j).copied())
            .min()
            .or_else(|| main_tags.get(i).copied())
            .unwrap_or(0);
        out.push(Bound { axis, at, outward, tag });
    }
    out
}

/// The positions with every vertex on a bound (within `tol`) moved `by` out of it.
pub(crate) fn grown(positions: &[Point3], bounds: &[Bound], by: f64, tol: f64) -> Vec<Point3> {
    positions
        .iter()
        .map(|p| {
            let mut q = *p;
            for b in bounds {
                if (p[b.axis] - b.at).abs() <= tol {
                    q[b.axis] = b.at + b.outward * by;
                }
            }
            q
        })
        .collect()
}

/// The box bounded by the planes and reaching `margin` past `around` elsewhere: 12 triangles
/// facing out, each face tagged with its bound's tag (the faces in no bound with the first
/// bound's; they lie past everything and are cut away).
pub(crate) fn half_space(bounds: &[Bound], around: Aabb, margin: f64) -> Surface {
    let mut lo = around.min.to_array().map(|v| v - margin);
    let mut hi = around.max.to_array().map(|v| v + margin);
    let mut tag = [[None; 2]; 3];
    for b in bounds {
        if b.outward < 0.0 {
            lo[b.axis] = b.at;
            tag[b.axis][0] = Some(b.tag);
        } else {
            hi[b.axis] = b.at;
            tag[b.axis][1] = Some(b.tag);
        }
    }
    let other = bounds.first().map_or(0, |b| b.tag);
    let positions = (0..8).map(|i| Point3::new([lo[0], hi[0]][i & 1], [lo[1], hi[1]][(i >> 1) & 1], [lo[2], hi[2]][(i >> 2) & 1])).collect();
    // Faces x0, x1, y0, y1, z0, z1, counter-clockwise from outside.
    let quads = [[0, 4, 6, 2], [1, 3, 7, 5], [0, 1, 5, 4], [2, 6, 7, 3], [0, 2, 3, 1], [4, 5, 7, 6]];
    let mut s = Surface { positions, ..Surface::default() };
    for (f, q) in quads.iter().enumerate() {
        let t = tag[f / 2][f % 2].unwrap_or(other);
        s.triangles.extend([[q[0], q[1], q[2]], [q[0], q[2], q[3]]]);
        s.tags.extend([t, t]);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::csg::{Op, combine};

    fn cube(lo: [f64; 3], hi: [f64; 3], tag: u32) -> Surface {
        let mut s = half_space(&[], Aabb::new(Point3::from(lo), Point3::from(hi)), 0.0);
        s.tags = (0..12).map(|i| tag + i / 2).collect();
        s
    }

    fn volume(s: &Surface) -> f64 {
        s.triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|v| s.positions[v as usize]);
                emcraft_model::geom::triple(a.to_vec(), b.to_vec(), c.to_vec()) / 6.0
            })
            .sum()
    }

    /// A stub on a guide's roof, its sides in the guide's side walls: grown, united and cut back,
    /// the union exact and the side walls keeping the guide's tags.
    #[test]
    fn a_stub_flush_with_the_side_walls_unites_exactly() {
        let (a, b, l) = (2.0, 1.0, 4.0);
        let guide = cube([0.0; 3], [a, b, l], 0);
        let stub = cube([0.0, b, 1.5], [a, b + 2.0, 2.5], 100);
        let tri = Triangles::from;
        let found = bounds(tri(&guide), &guide.tags, tri(&stub), 1e-9);
        assert_eq!(found.iter().map(|b| (b.axis, b.at, b.outward, b.tag)).collect::<Vec<_>>(), vec![(0, 0.0, -1.0, 0), (0, a, 1.0, 1)]);
        let g = Surface { positions: grown(&guide.positions, &found, 0.3, 1e-9), ..guide.clone() };
        // The stub's foot into the guide (as `flush` moves it), its sides out by less.
        let mut s = Surface { positions: grown(&stub.positions, &found, 0.1, 1e-9), ..stub.clone() };
        for p in &mut s.positions {
            if p.y == b {
                p.y = b - 0.1;
            }
        }
        let u = combine(&g, &s, Op::Union, 0).unwrap();
        let around = Aabb::around(u.positions.iter().copied()).unwrap();
        let r = combine(&u, &half_space(&found, around, 1.0), Op::Intersection, 0).unwrap();
        assert!((volume(&r) - (a * b * l + a * 2.0)).abs() < 1e-12, "{}", volume(&r));
        // Everything on x = 0 is the guide's face 0, on x = a its face 1.
        for (t, tag) in r.triangles.iter().zip(&r.tags) {
            let q = t.map(|v| r.positions[v as usize]);
            if q.iter().all(|p| p.x == 0.0) {
                assert_eq!(*tag, 0);
            }
            if q.iter().all(|p| p.x == a) {
                assert_eq!(*tag, 1);
            }
        }
        // A body reaching past the plane, or a slanted face at it: no bound.
        let past = cube([-0.5, b, 1.5], [a, b + 2.0, 2.5], 100);
        assert!(bounds(tri(&guide), &guide.tags, tri(&past), 1e-9).iter().all(|b| b.outward > 0.0));
        let mut slanted = stub.clone();
        for p in &mut slanted.positions {
            if p.x == 0.0 && p.y > b {
                p.z += 0.2;
            }
        }
        assert!(bounds(tri(&guide), &guide.tags, tri(&slanted), 1e-9).iter().all(|b| b.outward > 0.0));
    }
}
