//! Imported bodies whose flat faces lie flush with another surface: a metal plate standing on a
//! guide's wall, an iris touching both broad walls, the outer faces of a part on its bounding
//! box. The mesh Boolean ([`crate::csg`]) needs the two surfaces in general position, so such a
//! face is moved off the plane by a step first, in the direction that leaves the result the same
//! (what lies beyond it is dropped by the Boolean): outwards for a difference or an intersection
//! when both bodies lie behind the plane, away from the other surface for a difference when they
//! only touch from either side, into it for a union touching from outside, back into the body
//! for a union flush from inside (the region's face bounds the union there). Primitive features
//! get the same treatment in `solid::tessellation`; here it works on any triangulated body, face
//! by face.
//!
//! A face is a patch of triangles in one axis plane (x, y or z constant) joined by edges; it is
//! flush when one of its triangles overlaps (with area) a triangle of the other surface in the
//! same plane. Its vertices move along the plane's axis, and the faces meeting it keep their
//! planes: a perpendicular one (a box's, an extrusion's) by its corners sliding along it, a
//! slanted one (a wedge, a ramp, a chamfer) by each corner moving along the line where the planes
//! it keeps meet, so the slanted face reaches past the other surface as the flush face does. A
//! corner whose faces' planes do not meet in one line past it (more planes than a corner of a
//! polyhedron in general position keeps), or meet at too shallow an angle (a move longer than
//! ten steps), is refused; so is a slanted face that would reach past the other surface into
//! the region. A face parallel to the other surface's and near it without touching it is left
//! where it is: the gap is the mesher's to fill (`solid::narrow` refines it).
//!
//! Still refused: a flush face partly against the other surface and partly inside the region
//! (outside it for a union). There the other surface turns away along a line in the face's plane
//! (a step in the region's floor under a plate), and no move of the face keeps the result: the
//! part against the surface must move off it, the rest must not, and the face joining them
//! would lie in the other surface's turned face — a coplanar overlap again. Only a Boolean that
//! takes coplanar triangles itself could do it.

use std::collections::HashMap;

use emcraft_model::{Combine, Point3, Vec3};

use crate::csg::Surface;
use crate::shared::{DisjointSets, corners, winding};
use crate::{MeshError, Result};

fn fail(why: impl Into<String>) -> MeshError {
    MeshError::invalid("body", why)
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

/// How the faces of a body (`combine` with the other surface) are moved.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Gaps {
    /// How far a flush face moves.
    pub(crate) step: f64,
    /// Faces within this of the other surface's plane are flush.
    pub(crate) flush: f64,
    /// A length shown in messages: display millimetres.
    pub(crate) mm: f64,
}

/// A triangulated surface, borrowed: its corners and triangles.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Triangles<'a> {
    pub(crate) positions: &'a [Point3],
    pub(crate) triangles: &'a [[u32; 3]],
}

impl<'a> From<&'a Surface> for Triangles<'a> {
    fn from(s: &'a Surface) -> Self {
        Triangles { positions: &s.positions, triangles: &s.triangles }
    }
}

/// Whether a flat triangle of `body` still lies in a plane of `other`'s (within `flush`)
/// overlapping one of its triangles there: what the Boolean cannot take. A face moved into
/// the other surface can come to lie against another of its faces (a stub's foot pushed into
/// the floor brings its sides into the side walls' planes).
pub(crate) fn touching(body: Triangles, other: Triangles, flush: f64) -> bool {
    let mut planes: [Vec<Flat>; 3] = Default::default();
    for f in flats(other.positions, other.triangles, flush) {
        planes[f.axis].push(f);
    }
    for list in &mut planes {
        list.sort_by(|a, b| a.at.total_cmp(&b.at).then(a.tri.cmp(&b.tri)));
    }
    flats(body.positions, body.triangles, flush).iter().any(|f| {
        let list = &planes[f.axis];
        let from = list.partition_point(|o| o.at < f.at - flush);
        list[from..].iter().take_while(|o| o.at <= f.at + flush).any(|o| overlap(&f.uv, &o.uv, flush))
    })
}

/// The body's corners with its flush faces moved off the `other` surface, for
/// `body combine other` — see the module notes. `name` names the body in messages.
pub(crate) fn moved(name: &str, body: Triangles, other: Triangles, combine: Combine, g: Gaps) -> Result<Vec<Point3>> {
    let flat = flats(body.positions, body.triangles, g.flush);
    if flat.is_empty() {
        return Ok(body.positions.to_vec());
    }
    // The other surface's flat triangles by axis, sorted by their plane (ties by triangle).
    let mut planes: [Vec<Flat>; 3] = Default::default();
    for f in flats(other.positions, other.triangles, g.flush) {
        planes[f.axis].push(f);
    }
    for list in &mut planes {
        list.sort_by(|a, b| a.at.total_cmp(&b.at).then(a.tri.cmp(&b.tri)));
    }
    let flush = Flush { name, body, other, combine, g, flat, planes };
    let contact = flush.contacts()?;
    if contact.iter().all(Option::is_none) {
        return Ok(body.positions.to_vec());
    }
    let mut faces = flush.faces();
    let face_move = flush.face_moves(&contact, &mut faces)?;
    flush.check_against(&mut faces, &face_move)?;
    flush.vertex_moves(&mut faces, &face_move)
}

/// One body's flush faces against the other surface: what [`moved`] works on.
struct Flush<'a> {
    name: &'a str,
    body: Triangles<'a>,
    other: Triangles<'a>,
    combine: Combine,
    g: Gaps,
    /// The body's flat triangles.
    flat: Vec<Flat>,
    /// The other surface's flat triangles by axis, sorted by their plane (ties by triangle).
    planes: [Vec<Flat>; 3],
}

impl Flush<'_> {
    /// Per flat triangle of the body: touching the other surface with the same (true) or the
    /// opposite (false) orientation.
    fn contacts(&self) -> Result<Vec<Option<bool>>> {
        let (name, g) = (self.name, self.g);
        let eps = g.flush;
        let mut contact: Vec<Option<bool>> = vec![None; self.flat.len()];
        for (i, f) in self.flat.iter().enumerate() {
            let list = &self.planes[f.axis];
            let from = list.partition_point(|o| o.at < f.at - g.flush);
            for o in list[from..].iter().take_while(|o| o.at <= f.at + g.flush) {
                if !overlap(&f.uv, &o.uv, eps) {
                    continue;
                }
                let same = o.outward == f.outward;
                if contact[i].is_some_and(|c| c != same) {
                    return Err(fail(format!("`{name}`: a face touches a surface from both sides")));
                }
                contact[i] = Some(same);
            }
        }
        Ok(contact)
    }

    /// Faces: the flat triangles of one plane (and orientation) joined by edges.
    fn faces(&self) -> DisjointSets {
        let mut sets = DisjointSets::new(self.flat.len());
        let mut by_edge: HashMap<(usize, i8, [u32; 2]), usize> = HashMap::new();
        for (i, f) in self.flat.iter().enumerate() {
            let t = self.body.triangles[f.tri];
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                let key = (f.axis, f.outward as i8, [a.min(b), a.max(b)]);
                match by_edge.get(&key) {
                    Some(&j) if (self.flat[j].at - f.at).abs() <= self.g.flush => sets.union(i, j),
                    Some(_) => {}
                    None => {
                        by_edge.insert(key, i);
                    }
                }
            }
        }
        sets
    }

    /// Each face's move along its axis, by the face's root in `faces`.
    fn face_moves(&self, contact: &[Option<bool>], faces: &mut DisjointSets) -> Result<HashMap<usize, f64>> {
        let name = self.name;
        let mut face_move: HashMap<usize, f64> = HashMap::new();
        for (i, f) in self.flat.iter().enumerate() {
            let Some(same) = contact[i] else { continue };
            let out = f.outward * self.g.step;
            let by = match (self.combine, same) {
                (Combine::Subtract | Combine::Intersect, true) => out,
                (Combine::Subtract, false) => -out,
                (Combine::Add, false) => out,
                // A union flush with the region from inside: the region's face bounds the union
                // there, so the body's face draws back into the body (the sliver it leaves is the
                // region's already). Where the face is not against the region it would change the
                // union: refused in `check_against`.
                (Combine::Add, true) => -out,
                (Combine::Intersect, false) => return Err(fail(format!("`{name}`: its intersection with the region is only a face"))),
            };
            let r = faces.find(i);
            if face_move.get(&r).is_some_and(|m| *m != by) {
                return Err(fail(format!("`{name}`: one face touches surfaces on both of its sides")));
            }
            face_move.insert(r, by);
        }
        Ok(face_move)
    }

    /// Whether a point inside the other surface is what changes the result (a difference or an
    /// intersection; outside it for a union).
    fn inside_matters(&self) -> bool {
        !matches!(self.combine, Combine::Add)
    }

    /// A moved face must lie wholly against the other surface, or beyond it where it does not:
    /// the part of it that is not against it moves too, which changes the result when that part
    /// lies inside the other surface (for a difference or an intersection; outside it for a
    /// union). Checked at points of each triangle (its centroid, corners and edges' middles drawn
    /// a tenth towards it) that no coplanar triangle of the other surface covers.
    fn check_against(&self, faces: &mut DisjointSets, face_move: &HashMap<usize, f64>) -> Result<()> {
        let (name, g) = (self.name, self.g);
        let inside_matters = self.inside_matters();
        for (i, f) in self.flat.iter().enumerate() {
            if !face_move.contains_key(&faces.find(i)) {
                continue;
            }
            let Some(q) = corners(self.body.positions, &self.body.triangles[f.tri]) else { continue };
            let c = Point3::centroid(q).unwrap_or(q[0]);
            let toward = |p: Point3| p + (c - p) * 0.1;
            let samples =
                [c, toward(q[0]), toward(q[1]), toward(q[2]), toward(q[0].midpoint(q[1])), toward(q[1].midpoint(q[2])), toward(q[2].midpoint(q[0]))];
            let list = &self.planes[f.axis];
            let from = list.partition_point(|o| o.at < f.at - g.flush);
            let plane: Vec<&Flat> = list[from..].iter().take_while(|o| o.at <= f.at + g.flush).collect();
            let (u, v) = ((f.axis + 1) % 3, (f.axis + 2) % 3);
            for p in samples {
                if plane.iter().any(|o| covers(&o.uv, [p[u], p[v]])) {
                    continue;
                }
                if (winding(self.other.positions, self.other.triangles, p) > 0.5) == inside_matters {
                    let axis = ["x", "y", "z"][f.axis];
                    let side = if inside_matters { "inside" } else { "outside" };
                    return Err(fail(format!(
                        "`{name}`: not supported yet: its face at {axis} = {:.6} mm lies partly against another surface and partly {side} the region (let the bodies overlap instead of touching, or join them into one body in the CAD tool)",
                        f.at * g.mm
                    )));
                }
            }
        }
        Ok(())
    }

    /// The body's corners moved with their faces, the corners of slanted faces meeting a moved
    /// one slid along them.
    fn vertex_moves(&self, faces: &mut DisjointSets, face_move: &HashMap<usize, f64>) -> Result<Vec<Point3>> {
        let (name, g) = (self.name, self.g);
        let (positions, triangles) = (self.body.positions, self.body.triangles);
        // The vertices' moves, per axis; the triangles that move with each (its face's); whether
        // a vertex's move grows the body (into the space beyond the other surface).
        let mut vertex_move: Vec<[f64; 3]> = vec![[0.0; 3]; positions.len()];
        let mut vertex_grows: Vec<bool> = vec![false; positions.len()];
        let mut moved_with: Vec<Option<usize>> = vec![None; triangles.len()];
        for (i, f) in self.flat.iter().enumerate() {
            let Some(&by) = face_move.get(&faces.find(i)) else { continue };
            moved_with[f.tri] = Some(f.axis);
            // Growing: the move along the face's outward normal.
            let outward = by * f.outward > 0.0;
            for v in triangles[f.tri] {
                let m = vertex_move.get_mut(v as usize).ok_or_else(|| fail(format!("`{name}`: a triangle has a missing corner")))?;
                if m[f.axis] != 0.0 && m[f.axis] != by {
                    return Err(fail(format!("`{name}`: a corner lies on two flush faces moving apart")));
                }
                m[f.axis] = by;
                if let Some(gv) = vertex_grows.get_mut(v as usize) {
                    *gv |= outward;
                }
            }
        }
        // The faces meeting a moved one keep their planes: a face perpendicular to it already
        // does (its corners slide along it); at a corner of a slanted one the corner moves along
        // the line where the planes it must keep meet (`slide`), so the slanted face reaches past
        // the other surface too.
        let mut keep: Vec<Vec<Vec3>> = vec![Vec::new(); positions.len()];
        for (k, t) in triangles.iter().enumerate() {
            let Some(q) = corners(positions, t) else { continue };
            let n = (q[1] - q[0]).cross(q[2] - q[0]);
            for v in t {
                let m = vertex_move[*v as usize];
                if (0..3).any(|d| m[d] != 0.0 && moved_with[k] != Some(d) && n[d].abs() > 1e-6 * n.norm())
                    && let Some(u) = n.normalized()
                    && let Some(list) = keep.get_mut(*v as usize)
                {
                    // Every face of the corner that is not flush keeps its plane.
                    list.push(u);
                }
            }
        }
        let mut out: Vec<Point3> = positions.iter().zip(&vertex_move).map(|(p, m)| Point3::new(p.x + m[0], p.y + m[1], p.z + m[2])).collect();
        let inside_matters = self.inside_matters();
        for (v, slanted) in keep.iter().enumerate() {
            if slanted.is_empty() {
                continue;
            }
            let m = vertex_move[v];
            let p = positions[v];
            let axis = (0..3).find(|d| m[*d] != 0.0).unwrap_or(0);
            let at = |what: &str| {
                fail(format!(
                    "`{name}`: not supported yet: its face flush with another surface at {} = {:.6} mm meets {what} (let the body reach through the other surface instead)",
                    ["x", "y", "z"][axis],
                    p[axis] * g.mm
                ))
            };
            // The planes the corner keeps: its flush faces' (moved by their steps), and every
            // other face at it (the perpendicular ones too, so the corner slides along them).
            let mut rows: Vec<(Vec3, f64)> = (0..3).filter(|d| m[*d] != 0.0).map(|d| (Vec3::ZERO.with(d, 1.0), m[d])).collect();
            for (k, t) in triangles.iter().enumerate() {
                if !t.contains(&(v as u32)) || moved_with[k].is_some_and(|d| m[d] != 0.0) {
                    continue;
                }
                let Some(q) = corners(positions, t) else { continue };
                if let Some(u) = (q[1] - q[0]).cross(q[2] - q[0]).normalized() {
                    rows.push((u, 0.0));
                }
            }
            let by = slide(&rows, g.step).ok_or_else(|| at("faces whose planes do not meet in one line past it"))?;
            if by.norm() > 10.0 * g.step {
                return Err(at("a face at too shallow an angle to it"));
            }
            let q = p + by;
            // Grown past the other surface: the corner must land where the body's part beyond
            // it is dropped (outside the region for a difference or an intersection, inside for
            // a union).
            if vertex_grows[v] && (winding(self.other.positions, self.other.triangles, q) > 0.5) == inside_matters {
                return Err(at("a slanted face that would reach past the other surface into the region"));
            }
            out[v] = q;
        }
        // No triangle turned over by the moves.
        for t in triangles {
            let (Some(a), Some(b)) = (corners(positions, t), corners(&out, t)) else { continue };
            if (a[1] - a[0]).cross(a[2] - a[0]).dot((b[1] - b[0]).cross(b[2] - b[0])) <= 0.0 {
                return Err(fail(format!("`{name}`: a face flush with another surface is too close to an opposite face of the body to move")));
            }
        }
        Ok(out)
    }
}

/// The smallest move `δ` with `n·δ = c` for every row `(n, c)` (unit normals): the rows made
/// orthonormal one by one (modified Gram–Schmidt), a row that depends on the earlier ones
/// checked against them instead. `None` when the rows contradict each other (planes that do
/// not meet in a line or point), relative to `step`.
fn slide(rows: &[(Vec3, f64)], step: f64) -> Option<Vec3> {
    let mut basis: Vec<(Vec3, f64)> = Vec::with_capacity(3);
    for &(n, c) in rows {
        let (mut r, mut rhs) = (n, c);
        for &(q, qc) in &basis {
            let k = r.dot(q);
            r -= q * k;
            rhs -= k * qc;
        }
        let len = r.norm();
        if len > 1e-6 {
            basis.push((r / len, rhs / len));
        } else if rhs.abs() > 1e-6 * step {
            return None;
        }
    }
    Some(basis.iter().fold(Vec3::ZERO, |s, (q, c)| s + *q * *c))
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

    fn tri<'a>(positions: &'a [Point3], triangles: &'a [[u32; 3]]) -> Triangles<'a> {
        Triangles { positions, triangles }
    }

    const G: Gaps = Gaps { step: 0.1, flush: 1e-9, mm: 1.0 };

    #[test]
    fn a_plate_on_three_walls_moves_out_of_them() {
        let (gp, gt) = cube([0.0, 0.0, 0.0], [10.0, 5.0, 20.0]);
        let (pp, pt) = cube([0.0, 0.0, 8.0], [3.0, 5.0, 9.0]);
        let q = moved("plate", tri(&pp, &pt), tri(&gp, &gt), Combine::Subtract, G).unwrap();
        let lo = q.iter().fold([f64::INFINITY; 3], |m, p| [0, 1, 2].map(|d| m[d].min(p[d])));
        let hi = q.iter().fold([f64::NEG_INFINITY; 3], |m, p| [0, 1, 2].map(|d| m[d].max(p[d])));
        assert_eq!(lo, [-0.1, -0.1, 8.0]);
        assert_eq!(hi, [3.0, 5.1, 9.0]);
        // Inside, apart from every wall: unchanged.
        let (ip, it) = cube([1.0, 1.0, 8.0], [3.0, 4.0, 9.0]);
        assert_eq!(moved("inside", tri(&ip, &it), tri(&gp, &gt), Combine::Subtract, G).unwrap(), ip);
        // Touching from outside: moved away; a union into the region.
        let (op, ot) = cube([10.0, 1.0, 8.0], [12.0, 4.0, 9.0]);
        assert_eq!(moved("out", tri(&op, &ot), tri(&gp, &gt), Combine::Subtract, G).unwrap()[0].x, 10.1);
        assert_eq!(moved("out", tri(&op, &ot), tri(&gp, &gt), Combine::Add, G).unwrap()[0].x, 9.9);
        // A union flush from inside draws back into itself: the region's walls bound it.
        let q = moved("plate", tri(&pp, &pt), tri(&gp, &gt), Combine::Add, G).unwrap();
        let lo = q.iter().fold([f64::INFINITY; 3], |m, p| [0, 1, 2].map(|d| m[d].min(p[d])));
        let hi = q.iter().fold([f64::NEG_INFINITY; 3], |m, p| [0, 1, 2].map(|d| m[d].max(p[d])));
        assert_eq!((lo, hi), ([0.1, 0.1, 8.0], [3.0, 4.9, 9.0]));
        // Unless part of the face lies outside the region: the union's face there would move.
        let (wp, wt) = cube([0.0, 0.0, 8.0], [12.0, 5.0, 9.0]);
        let e = moved("wide", tri(&wp, &wt), tri(&gp, &gt), Combine::Add, G).unwrap_err().to_string();
        assert!(e.contains("partly outside"), "{e}");
    }

    /// The wedge with corners (1, 0, 8), (3, 0, 8), (1, 0, 9), (1, 2, 8): its face on y = 0, a
    /// slanted face over it.
    fn wedge() -> (Vec<Point3>, Vec<[u32; 3]>) {
        let positions = vec![Point3::new(1.0, 0.0, 8.0), Point3::new(3.0, 0.0, 8.0), Point3::new(1.0, 0.0, 9.0), Point3::new(1.0, 2.0, 8.0)];
        (positions, vec![[0, 1, 2], [0, 3, 1], [0, 2, 3], [1, 3, 2]])
    }

    fn plane_of(p: &[Point3], t: [u32; 3]) -> (Vec3, f64) {
        let [a, b, c] = t.map(|v| p[v as usize]);
        let n = (b - a).cross(c - a).normalized().unwrap();
        (n, n.dot(a.to_vec()))
    }

    #[test]
    fn slanted_faces_meeting_a_flush_one_keep_their_planes() {
        let (gp, gt) = cube([0.0, 0.0, 0.0], [10.0, 5.0, 20.0]);
        let (wp, wt) = wedge();
        let q = moved("wedge", tri(&wp, &wt), tri(&gp, &gt), Combine::Subtract, G).unwrap();
        // The foot moved 0.1 below the floor; every other face's plane kept.
        assert!(q.iter().take(3).all(|p| (p.y + 0.1).abs() < 1e-12), "{q:?}");
        assert_eq!(q[3], wp[3]);
        for t in &wt[1..] {
            let ((n0, d0), (n1, d1)) = (plane_of(&wp, *t), plane_of(&q, *t));
            assert!((n0 - n1).norm() < 1e-12 && (d0 - d1).abs() < 1e-12, "{t:?}");
        }
        // The slanted face's corner on the foot slid along it: (3, 0, 8) to (3.1, −0.1, 8).
        assert!((q[1] - Point3::new(3.1, -0.1, 8.0)).norm() < 1e-12, "{:?}", q[1]);
        // A ramp across the guide, flush with the floor and both side walls.
        let positions = vec![
            Point3::new(0.0, 0.0, 6.0),
            Point3::new(10.0, 0.0, 6.0),
            Point3::new(0.0, 0.0, 10.0),
            Point3::new(10.0, 0.0, 10.0),
            Point3::new(0.0, 2.0, 10.0),
            Point3::new(10.0, 2.0, 10.0),
        ];
        let triangles = vec![[0, 1, 3], [0, 3, 2], [0, 4, 5], [0, 5, 1], [2, 3, 5], [2, 5, 4], [0, 2, 4], [1, 5, 3]];
        let q = moved("ramp", tri(&positions, &triangles), tri(&gp, &gt), Combine::Subtract, G).unwrap();
        for t in [[0, 4, 5], [2, 3, 5]] {
            let ((n0, d0), (n1, d1)) = (plane_of(&positions, t), plane_of(&q, t));
            assert!((n0 - n1).norm() < 1e-12 && (d0 - d1).abs() < 1e-12, "{t:?}");
        }
        assert!((q[0] - Point3::new(-0.1, -0.1, 5.8)).norm() < 1e-12, "{:?}", q[0]);
    }

    #[test]
    fn nearly_flush_faces_stay_and_unkeepable_corners_are_refused() {
        let (gp, gt) = cube([0.0, 0.0, 0.0], [10.0, 5.0, 20.0]);
        // A face a hair off the wall is not flush: it stays (the gap is the mesher's to fill).
        let (pp, pt) = cube([0.001, 1.0, 8.0], [3.0, 4.0, 9.0]);
        assert_eq!(moved("plate", tri(&pp, &pt), tri(&gp, &gt), Combine::Subtract, G).unwrap(), pp);
        // A wedge whose slanted face rises at a hair's angle: its foot would slide far.
        let mut flat = wedge().0;
        flat[3] = Point3::new(1.0, 0.01, 8.0);
        let e = moved("sliver", tri(&flat, &wedge().1), tri(&gp, &gt), Combine::Subtract, G).unwrap_err().to_string();
        assert!(e.contains("shallow"), "{e}");
        // A pyramid on the floor: each corner of its foot meets two slanted faces, which slide
        // it along their common edge.
        let mut positions = vec![
            Point3::new(1.0, 0.0, 8.0),
            Point3::new(3.0, 0.0, 8.0),
            Point3::new(3.0, 0.0, 10.0),
            Point3::new(1.0, 0.0, 10.0),
            Point3::new(2.0, 0.0, 9.0),
            Point3::new(2.0, 1.0, 9.0),
        ];
        // The foot fanned about its middle (4): corner 0 meets two slanted faces and two of the
        // foot's triangles; the middle point keeps nothing but the floor.
        let mut triangles = vec![[0, 4, 1], [1, 4, 2], [2, 4, 3], [3, 4, 0], [0, 1, 5], [1, 2, 5], [2, 3, 5], [3, 0, 5]];
        let q = moved("pyramid", tri(&positions, &triangles), tri(&gp, &gt), Combine::Subtract, G).unwrap();
        assert!((q[4].y + 0.1).abs() < 1e-12 && (q[0] - Point3::new(0.9, -0.1, 7.9)).norm() < 1e-12, "{q:?}");
        // One side bent out of its plane at a point (6): corner 0 now meets three slanted planes,
        // which do not meet in a line through it.
        positions.push(Point3::new(1.2, 1.0 / 3.0, 9.0));
        triangles.pop();
        triangles.extend([[3, 0, 6], [0, 5, 6], [5, 3, 6]]);
        let e = moved("bent", tri(&positions, &triangles), tri(&gp, &gt), Combine::Subtract, G).unwrap_err().to_string();
        assert!(e.contains("do not meet"), "{e}");
        // The region's floor stepping down just past the wedge's foot: the slanted face's slid
        // corner would land in the region below the step.
        let low = cube([3.05, -1.0, -1.0], [11.0, 0.5, 21.0]);
        let stepped = crate::csg::combine(
            &crate::csg::Surface { positions: gp.clone(), triangles: gt.clone(), tags: vec![0; gt.len()] },
            &crate::csg::Surface { positions: low.0, triangles: low.1.clone(), tags: vec![1; low.1.len()] },
            crate::csg::Op::Union,
            1,
        )
        .unwrap();
        let (wp, wt) = wedge();
        let e = moved("wedge", tri(&wp, &wt), tri(&stepped.positions, &stepped.triangles), Combine::Subtract, G).unwrap_err().to_string();
        assert!(e.contains("into the region"), "{e}");
    }
}
