//! Imported bodies whose flat faces lie flush with another surface: a metal plate standing on a
//! guide's wall, an iris touching both broad walls, the outer faces of a part on its bounding
//! box. The mesh Boolean ([`crate::csg`]) needs the two surfaces in general position, so such a
//! face is moved off the plane by a step first, in the direction that leaves the result the same
//! (what lies beyond it is dropped by the Boolean): outwards for a difference or an intersection
//! when both bodies lie behind the plane, away from the other surface for a difference when they
//! only touch from either side, into it for a union. Primitive features get the same treatment in
//! `solid::tessellation`; here it works on any triangulated body, face by face.
//!
//! A face is a patch of triangles in one axis plane (x, y or z constant) joined by edges; it is
//! flush when one of its triangles overlaps (with area) a triangle of the other surface in the
//! same plane. Its vertices move along the plane's axis, so the faces meeting it must be
//! perpendicular to it (a box's, an extrusion's) for the body to stay the same shape: a slanted
//! one is refused. A face parallel to the other surface's and closer than `near` without
//! touching it (a sliver of air no mesh resolves) is refused too.

use std::collections::HashMap;

use emcraft_model::{Combine, Point3};

use crate::{MeshError, Result};

fn fail(why: impl Into<String>) -> MeshError {
    MeshError::Invalid { what: "body".into(), why: why.into() }
}

/// A triangle in an axis plane: its axis, the plane's coordinate, the sign of its outward normal
/// along the axis and its corners' other two coordinates.
#[derive(Clone, Copy, Debug)]
struct Flat {
    tri: usize,
    axis: usize,
    at: f64,
    outward: f64,
    uv: [[f64; 2]; 3],
}

/// The triangles of a surface that lie in an axis plane (their corners' coordinate along it
/// within `tol`).
fn flats(positions: &[Point3], triangles: &[[u32; 3]], tol: f64) -> Vec<Flat> {
    let mut out = Vec::new();
    for (i, t) in triangles.iter().enumerate() {
        let Some(q) = corners(positions, t) else { continue };
        let n = (q[1] - q[0]).cross(q[2] - q[0]);
        for d in 0..3 {
            let (lo, hi) = q.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p[d]), b.max(p[d])));
            if hi - lo <= tol && n[d] != 0.0 {
                let (u, v) = ((d + 1) % 3, (d + 2) % 3);
                out.push(Flat { tri: i, axis: d, at: 0.5 * (lo + hi), outward: n[d].signum(), uv: q.map(|p| [p[u], p[v]]) });
                break;
            }
        }
    }
    out
}

fn corners(positions: &[Point3], t: &[u32; 3]) -> Option<[Point3; 3]> {
    Some([*positions.get(t[0] as usize)?, *positions.get(t[1] as usize)?, *positions.get(t[2] as usize)?])
}

/// Whether two triangles of one plane overlap with area (deeper than `eps` along every
/// separating direction): the separating-axis test on their six edge normals.
fn overlap(a: &[[f64; 2]; 3], b: &[[f64; 2]; 3], eps: f64) -> bool {
    for tri in [a, b] {
        for k in 0..3 {
            let (p, q) = (tri[k], tri[(k + 1) % 3]);
            let n = [p[1] - q[1], q[0] - p[0]];
            let len = n[0].hypot(n[1]);
            if !(len > 0.0) {
                continue;
            }
            let proj = |s: &[[f64; 2]; 3]| {
                s.iter().map(|x| (x[0] * n[0] + x[1] * n[1]) / len).fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(v), hi.max(v)))
            };
            let ((alo, ahi), (blo, bhi)) = (proj(a), proj(b));
            if ahi.min(bhi) - alo.max(blo) <= eps {
                return false;
            }
        }
    }
    true
}

/// Whether point `p` lies inside the triangle (on its edges counts).
fn covers(t: &[[f64; 2]; 3], p: [f64; 2]) -> bool {
    let cross = |a: [f64; 2], b: [f64; 2]| (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
    let s = [cross(t[0], t[1]), cross(t[1], t[2]), cross(t[2], t[0])];
    s.iter().all(|x| *x >= 0.0) || s.iter().all(|x| *x <= 0.0)
}

/// The winding number of a closed surface about `p`.
fn winding(positions: &[Point3], triangles: &[[u32; 3]], p: Point3) -> f64 {
    triangles.iter().filter_map(|t| corners(positions, t)).map(|q| emcraft_model::solid_angle(q.map(|x| x - p))).sum::<f64>()
        / (4.0 * std::f64::consts::PI)
}

/// A disjoint-set forest (union by the smaller index: deterministic).
struct Sets(Vec<usize>);

impl Sets {
    fn find(&mut self, mut i: usize) -> usize {
        while self.0[i] != i {
            self.0[i] = self.0[self.0[i]];
            i = self.0[i];
        }
        i
    }

    fn union(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            self.0[a.max(b)] = a.min(b);
        }
    }
}

/// How the faces of a body (`combine` with the other surface) are moved.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Gaps {
    /// How far a flush face moves.
    pub(crate) step: f64,
    /// Faces within this of the other surface's plane are flush.
    pub(crate) flush: f64,
    /// Parallel faces closer than this (and not flush) are refused.
    pub(crate) near: f64,
    /// A length shown in messages: display millimetres.
    pub(crate) mm: f64,
}

/// The body's corners with its flush faces moved off the other surface (`other_positions`,
/// `other_triangles`), for `body combine other` — see the module notes. `name` names the body
/// in messages.
pub(crate) fn moved(
    name: &str,
    positions: &[Point3],
    triangles: &[[u32; 3]],
    other_positions: &[Point3],
    other_triangles: &[[u32; 3]],
    combine: Combine,
    g: Gaps,
) -> Result<Vec<Point3>> {
    let body = flats(positions, triangles, g.flush);
    if body.is_empty() {
        return Ok(positions.to_vec());
    }
    // The other surface's flat triangles by axis, sorted by their plane (ties by triangle).
    let mut other: [Vec<Flat>; 3] = Default::default();
    for f in flats(other_positions, other_triangles, g.flush) {
        other[f.axis].push(f);
    }
    for list in &mut other {
        list.sort_by(|a, b| a.at.total_cmp(&b.at).then(a.tri.cmp(&b.tri)));
    }
    let reach = g.near.max(g.flush);
    let eps = g.flush;
    // Per flat triangle of the body: touching the other surface with the same (true) or the
    // opposite (false) orientation.
    let mut contact: Vec<Option<bool>> = vec![None; body.len()];
    for (i, f) in body.iter().enumerate() {
        let list = &other[f.axis];
        let from = list.partition_point(|o| o.at < f.at - reach);
        for o in list[from..].iter().take_while(|o| o.at <= f.at + reach) {
            if !overlap(&f.uv, &o.uv, eps) {
                continue;
            }
            let gap = (o.at - f.at).abs();
            if gap <= g.flush {
                let same = o.outward == f.outward;
                if contact[i].is_some_and(|c| c != same) {
                    return Err(fail(format!("`{name}`: a face touches a surface from both sides")));
                }
                contact[i] = Some(same);
            } else {
                let axis = ["x", "y", "z"][f.axis];
                return Err(fail(format!(
                    "`{name}`: its face at {axis} = {:.6} mm lies {:.3e} mm from a parallel face — a gap narrower than {:.3} mm (a tenth of the element size) is not meshed; let them touch, keep them farther apart, or mesh finer",
                    f.at * g.mm,
                    gap * g.mm,
                    g.near * g.mm
                )));
            }
        }
    }
    if contact.iter().all(Option::is_none) {
        return Ok(positions.to_vec());
    }
    // Faces: the flat triangles of one plane (and orientation) joined by edges.
    let mut sets = Sets((0..body.len()).collect());
    let mut by_edge: HashMap<(usize, i8, [u32; 2]), usize> = HashMap::new();
    for (i, f) in body.iter().enumerate() {
        let t = triangles[f.tri];
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let key = (f.axis, f.outward as i8, [a.min(b), a.max(b)]);
            match by_edge.get(&key) {
                Some(&j) if (body[j].at - f.at).abs() <= g.flush => sets.union(i, j),
                Some(_) => {}
                None => {
                    by_edge.insert(key, i);
                }
            }
        }
    }
    // Each face's move along its axis.
    let mut face_move: HashMap<usize, f64> = HashMap::new();
    for (i, f) in body.iter().enumerate() {
        let Some(same) = contact[i] else { continue };
        let out = f.outward * g.step;
        let by = match (combine, same) {
            (Combine::Subtract | Combine::Intersect, true) => out,
            (Combine::Subtract, false) => -out,
            (Combine::Add, false) => out,
            (Combine::Add, true) => {
                return Err(fail(format!(
                    "`{name}`: not supported yet: a body of the region's material whose face is flush with the region's (let it stand out of the face, or reach into the body)"
                )));
            }
            (Combine::Intersect, false) => return Err(fail(format!("`{name}`: its intersection with the region is only a face"))),
        };
        let r = sets.find(i);
        if face_move.get(&r).is_some_and(|m| *m != by) {
            return Err(fail(format!("`{name}`: one face touches surfaces on both of its sides")));
        }
        face_move.insert(r, by);
    }
    // A moved face must lie wholly against the other surface, or beyond it where it does not:
    // the part of it that is not against it moves too, which changes the result when that part
    // lies inside the other surface (for a difference or an intersection; outside it for a
    // union). Checked at points of each triangle (its centroid, corners and edges' middles drawn
    // a tenth towards it) that no coplanar triangle of the other surface covers.
    let inside_matters = !matches!(combine, Combine::Add);
    for (i, f) in body.iter().enumerate() {
        if !face_move.contains_key(&sets.find(i)) {
            continue;
        }
        let Some(q) = corners(positions, &triangles[f.tri]) else { continue };
        let c = Point3::centroid(q).unwrap_or(q[0]);
        let toward = |p: Point3| p + (c - p) * 0.1;
        let samples =
            [c, toward(q[0]), toward(q[1]), toward(q[2]), toward(q[0].midpoint(q[1])), toward(q[1].midpoint(q[2])), toward(q[2].midpoint(q[0]))];
        let list = &other[f.axis];
        let from = list.partition_point(|o| o.at < f.at - g.flush);
        let plane: Vec<&Flat> = list[from..].iter().take_while(|o| o.at <= f.at + g.flush).collect();
        let (u, v) = ((f.axis + 1) % 3, (f.axis + 2) % 3);
        for p in samples {
            if plane.iter().any(|o| covers(&o.uv, [p[u], p[v]])) {
                continue;
            }
            if (winding(other_positions, other_triangles, p) > 0.5) == inside_matters {
                let axis = ["x", "y", "z"][f.axis];
                return Err(fail(format!(
                    "`{name}`: not supported yet: its face at {axis} = {:.6} mm lies partly against another surface and partly inside the region (let the bodies overlap instead of touching, or join them into one body in the CAD tool)",
                    f.at * g.mm
                )));
            }
        }
    }
    // The vertices' moves, per axis; the triangles that move with each (its face's).
    let mut vertex_move: Vec<[f64; 3]> = vec![[0.0; 3]; positions.len()];
    let mut moved_with: Vec<Option<usize>> = vec![None; triangles.len()];
    for (i, f) in body.iter().enumerate() {
        let Some(&by) = face_move.get(&sets.find(i)) else { continue };
        moved_with[f.tri] = Some(f.axis);
        for v in triangles[f.tri] {
            let m = vertex_move.get_mut(v as usize).ok_or_else(|| fail(format!("`{name}`: a triangle has a missing corner")))?;
            if m[f.axis] != 0.0 && m[f.axis] != by {
                return Err(fail(format!("`{name}`: a corner lies on two flush faces moving apart")));
            }
            m[f.axis] = by;
        }
    }
    // The faces meeting a moved one must be perpendicular to it.
    for (k, t) in triangles.iter().enumerate() {
        let Some(q) = corners(positions, t) else { continue };
        let n = (q[1] - q[0]).cross(q[2] - q[0]);
        for v in t {
            for d in 0..3 {
                if vertex_move[*v as usize][d] != 0.0 && moved_with[k] != Some(d) && n[d].abs() > 1e-6 * n.norm() {
                    let axis = ["x", "y", "z"][d];
                    return Err(fail(format!(
                        "`{name}`: not supported yet: its face flush with another surface at {axis} = {:.6} mm meets a face slanted to it (let the body reach through the other surface instead)",
                        positions[*v as usize][d] * g.mm
                    )));
                }
            }
        }
    }
    Ok(positions.iter().zip(&vertex_move).map(|(p, m)| Point3::new(p.x + m[0], p.y + m[1], p.z + m[2])).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A box [lo, hi] as 12 outward triangles.
    fn cube(lo: [f64; 3], hi: [f64; 3]) -> (Vec<Point3>, Vec<[u32; 3]>) {
        let positions = (0..8)
            .map(|i| {
                Point3::new(if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] })
            })
            .collect();
        let quads = [[0, 4, 6, 2], [1, 3, 7, 5], [0, 1, 5, 4], [2, 6, 7, 3], [0, 2, 3, 1], [4, 5, 7, 6]];
        let triangles = quads.iter().flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]]).collect();
        (positions, triangles)
    }

    const G: Gaps = Gaps { step: 0.1, flush: 1e-9, near: 0.1, mm: 1.0 };

    #[test]
    fn a_plate_on_three_walls_moves_out_of_them() {
        let (gp, gt) = cube([0.0, 0.0, 0.0], [10.0, 5.0, 20.0]);
        let (pp, pt) = cube([0.0, 0.0, 8.0], [3.0, 5.0, 9.0]);
        let q = moved("plate", &pp, &pt, &gp, &gt, Combine::Subtract, G).unwrap();
        let lo = q.iter().fold([f64::INFINITY; 3], |m, p| [0, 1, 2].map(|d| m[d].min(p[d])));
        let hi = q.iter().fold([f64::NEG_INFINITY; 3], |m, p| [0, 1, 2].map(|d| m[d].max(p[d])));
        assert_eq!(lo, [-0.1, -0.1, 8.0]);
        assert_eq!(hi, [3.0, 5.1, 9.0]);
        // Inside, apart from every wall: unchanged.
        let (ip, it) = cube([1.0, 1.0, 8.0], [3.0, 4.0, 9.0]);
        assert_eq!(moved("inside", &ip, &it, &gp, &gt, Combine::Subtract, G).unwrap(), ip);
        // Touching from outside: moved away; a union into the region.
        let (op, ot) = cube([10.0, 1.0, 8.0], [12.0, 4.0, 9.0]);
        assert_eq!(moved("out", &op, &ot, &gp, &gt, Combine::Subtract, G).unwrap()[0].x, 10.1);
        assert_eq!(moved("out", &op, &ot, &gp, &gt, Combine::Add, G).unwrap()[0].x, 9.9);
        assert!(moved("plate", &pp, &pt, &gp, &gt, Combine::Add, G).is_err());
    }

    #[test]
    fn nearly_flush_faces_and_slanted_neighbours_are_refused() {
        let (gp, gt) = cube([0.0, 0.0, 0.0], [10.0, 5.0, 20.0]);
        let (pp, pt) = cube([0.001, 1.0, 8.0], [3.0, 4.0, 9.0]);
        let e = moved("plate", &pp, &pt, &gp, &gt, Combine::Subtract, G).unwrap_err();
        assert!(e.to_string().contains("gap narrower"), "{e}");
        // A wedge on the floor: its slanted face meets the flush one.
        let positions = vec![Point3::new(1.0, 0.0, 8.0), Point3::new(3.0, 0.0, 8.0), Point3::new(1.0, 0.0, 9.0), Point3::new(1.0, 2.0, 8.0)];
        let triangles = vec![[0, 1, 2], [0, 3, 1], [0, 2, 3], [1, 3, 2]];
        let e = moved("wedge", &positions, &triangles, &gp, &gt, Combine::Subtract, G).unwrap_err();
        assert!(e.to_string().contains("slanted"), "{e}");
    }
}
