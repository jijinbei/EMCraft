//! The sector a rotationally symmetric solid is meshed on ([`emcraft_model::Rotation`]): the
//! solid cut by a wedge between the rays at φ = 0 and φ = 2π/n about the axis, its two cut faces
//! then made each other's turn node for node, so the solver can tie each unknown on the second to
//! its image on the first (the cyclic condition, as the periodic boundaries tie translates).
//!
//! The cut leaves the two faces' outlines with their own points: those where the solid's
//! triangles crossed each plane. Their union — the first face's, and the second's turned back
//! onto the first plane — is triangulated on the first face (a planar straight-line graph,
//! refined to the element size), the second face is that triangulation turned, and the solid's
//! other triangles along each outline are split at the union's points they lack. Both faces are
//! then frozen for the tetrahedral mesher, which keeps their triangles as they are. A solid not
//! symmetric under the turn has outlines that do not agree, and is refused.

use std::collections::{BTreeMap, BTreeSet};

use emcraft_model::{Point3, Rotation};

use super::tessellation::Tessellation;
use crate::{MeshError, Result};

fn fail(why: impl Into<String>) -> MeshError {
    MeshError::invalid("rotational symmetry", why)
}

/// A cut face's plane: the ray's direction from the axis, and whether `p` lies on its half-plane.
#[derive(Clone, Copy)]
struct Half {
    centre: [f64; 2],
    dir: [f64; 2],
    tol: f64,
}

impl Half {
    fn new(rot: &Rotation, angle: f64, tol: f64) -> Half {
        Half { centre: rot.centre, dir: [angle.cos(), angle.sin()], tol }
    }

    /// (s, t): the distance from the axis along the ray and z.
    fn local(&self, p: Point3) -> [f64; 2] {
        [(p.x - self.centre[0]) * self.dir[0] + (p.y - self.centre[1]) * self.dir[1], p.z]
    }

    fn holds(&self, p: Point3) -> bool {
        let off = -(p.x - self.centre[0]) * self.dir[1] + (p.y - self.centre[1]) * self.dir[0];
        off.abs() <= self.tol && self.local(p)[0] >= -self.tol
    }

    /// The plane's normal, towards +φ.
    fn normal(&self) -> [f64; 2] {
        [-self.dir[1], self.dir[0]]
    }

    fn point(&self, q: [f64; 2]) -> Point3 {
        Point3::new(self.centre[0] + q[0] * self.dir[0], self.centre[1] + q[0] * self.dir[1], q[1])
    }
}

/// The distance from `q` to segment a–b, and the point of it nearest `q`.
fn nearest_on(a: [f64; 2], b: [f64; 2], q: [f64; 2]) -> (f64, [f64; 2]) {
    let d = [b[0] - a[0], b[1] - a[1]];
    let len2 = d[0] * d[0] + d[1] * d[1];
    let t = if len2 > 0.0 { (((q[0] - a[0]) * d[0] + (q[1] - a[1]) * d[1]) / len2).clamp(0.0, 1.0) } else { 0.0 };
    let p = [a[0] + t * d[0], a[1] + t * d[1]];
    ((p[0] - q[0]).hypot(p[1] - q[1]), p)
}

/// The edges used by one triangle of `tris` only (the face's outline), sorted.
fn outline(tris: &[[u32; 3]]) -> Vec<[u32; 2]> {
    let mut count: BTreeMap<[u32; 2], usize> = BTreeMap::new();
    for t in tris {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            *count.entry([a.min(b), a.max(b)]).or_insert(0) += 1;
        }
    }
    count.into_iter().filter(|(_, c)| *c == 1).map(|(e, _)| e).collect()
}

/// The points of `pts` strictly inside segment p–q (within `tol` of it), from p to q.
fn between(pts: &[[f64; 2]], p: [f64; 2], q: [f64; 2], tol: f64) -> Vec<usize> {
    let d = [q[0] - p[0], q[1] - p[1]];
    let len2 = d[0] * d[0] + d[1] * d[1];
    if len2 <= 0.0 {
        return Vec::new();
    }
    let mut on: Vec<(f64, usize)> = pts
        .iter()
        .enumerate()
        .filter_map(|(k, x)| {
            let w = [x[0] - p[0], x[1] - p[1]];
            let t = (w[0] * d[0] + w[1] * d[1]) / len2;
            let off = (w[0] * d[1] - w[1] * d[0]).abs() / len2.sqrt();
            let along = tol / len2.sqrt();
            (t > along && t < 1.0 - along && off <= tol).then_some((t, k))
        })
        .collect();
    on.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    on.into_iter().map(|(_, k)| k).collect()
}

/// The solid's triangles `t` kept on the side `d(p) ≥ 0` of a plane: a triangle across it cut
/// along it, each crossing edge's point made once (from its ends in a fixed order, so the
/// triangles on both sides of the edge share it); a corner within `tol` of the plane kept and put
/// on it.
fn clip(t: &mut crate::csg::Surface, d: &dyn Fn(Point3) -> f64, onto: &dyn Fn(Point3) -> Point3, tol: f64) {
    let dist: Vec<f64> = t.positions.iter().map(|p| d(*p)).collect();
    for (p, &x) in t.positions.iter_mut().zip(&dist) {
        if x.abs() <= tol {
            *p = onto(*p);
        }
    }
    let side = |v: u32| {
        let x = dist[v as usize];
        if x > tol {
            1
        } else if x < -tol {
            -1
        } else {
            0
        }
    };
    let mut crossing: BTreeMap<[u32; 2], u32> = BTreeMap::new();
    let mut triangles = Vec::with_capacity(t.triangles.len());
    let mut tags = Vec::with_capacity(t.triangles.len());
    for (i, tri) in t.triangles.clone().into_iter().enumerate() {
        let tag = t.tags.get(i).copied().unwrap_or(0);
        if tri.iter().all(|&v| side(v) >= 0) {
            triangles.push(tri);
            tags.push(tag);
            continue;
        }
        if tri.iter().all(|&v| side(v) <= 0) {
            continue;
        }
        let mut ring: Vec<u32> = Vec::with_capacity(4);
        for k in 0..3 {
            let (a, b) = (tri[k], tri[(k + 1) % 3]);
            if side(a) >= 0 {
                ring.push(a);
            }
            if side(a) * side(b) < 0 {
                let key = [a.min(b), a.max(b)];
                let id = *crossing.entry(key).or_insert_with(|| {
                    let (p, q) = (t.positions[key[0] as usize], t.positions[key[1] as usize]);
                    let (dp, dq) = (dist[key[0] as usize], dist[key[1] as usize]);
                    t.positions.push(onto(p + (q - p) * (dp / (dp - dq))));
                    t.positions.len() as u32 - 1
                });
                ring.push(id);
            }
        }
        for k in 1..ring.len().saturating_sub(1) {
            triangles.push([ring[0], ring[k], ring[k + 1]]);
            tags.push(tag);
        }
    }
    t.triangles = triangles;
    t.tags = tags;
}

/// The solid's tessellation cut down to its sector, the two cut faces triangulated as each
/// other's turn, and the first of their tags for the mesher to freeze: every triangle of the
/// cut faces has a tag of its own (so the mesher's boundary recovery does not join a face's
/// triangles into one facet and triangulate it afresh), the first face's `base..base + T`, the
/// second's `base + T..base + 2T`, triangle i of the one the turn of triangle i of the other.
/// `size` is the element size, `scale` the model's (its box's diagonal).
pub(super) fn sector(t: Tessellation, rot: &Rotation, size: f64, scale: f64) -> Result<(Tessellation, u32)> {
    let tol = 1e-9 * scale;
    let halves = [Half::new(rot, rot.start(), tol), Half::new(rot, rot.start() + rot.angle(), tol)];
    let mut t = t;
    // Inside the sector: past the first plane towards +φ, short of the second.
    for (f, h) in halves.iter().enumerate() {
        let n = h.normal();
        let sign = if f == 0 { 1.0 } else { -1.0 };
        let c = h.centre;
        let d = move |p: Point3| sign * ((p.x - c[0]) * n[0] + (p.y - c[1]) * n[1]);
        let hh = *h;
        clip(&mut t.surface, &d, &move |p: Point3| hh.point(hh.local(p)), tol);
    }
    if t.surface.triangles.is_empty() {
        return Err(fail("nothing of the solid lies in the sector"));
    }
    close(t, rot, scale, Some(size), &[])
}

/// The sector's open surface `t` (its cut faces missing) closed by two faces each other's turn:
/// triangulated from the union of the two outlines' points and `inner` (points inside the
/// first face, on its plane's (s, t)), refined to `refine` when given, else taken as they are.
fn close(t: Tessellation, rot: &Rotation, scale: f64, refine: Option<f64>, inner: &[[f64; 2]]) -> Result<(Tessellation, u32)> {
    let tol = 1e-9 * scale;
    let merge = 1e-7 * scale;
    let halves = [Half::new(rot, rot.start(), tol), Half::new(rot, rot.start() + rot.angle(), tol)];
    let base = t.surface.tags.iter().map(|g| g + 1).max().unwrap_or(0).max(t.analytic.len() as u32);
    let size = refine.unwrap_or(scale);
    let pos = t.surface.positions.clone();
    // The open outline the cut left on each plane, with the axis' stretches inside the solid
    // (where the two faces meet) closing it.
    let open = outline(&t.surface.triangles);
    let on = |h: &Half, v: u32| pos.get(v as usize).is_some_and(|p| h.holds(*p));
    let mut outlines: [Vec<[u32; 2]>; 2] = [0, 1].map(|f| open.iter().copied().filter(|e| on(&halves[f], e[0]) && on(&halves[f], e[1])).collect());
    let axis = |v: u32| pos.get(v as usize).is_some_and(|p| halves[0].local(*p)[0].abs() <= merge);
    for f in 0..2 {
        let mut degree: BTreeMap<u32, usize> = BTreeMap::new();
        for e in &outlines[f] {
            for v in e {
                *degree.entry(*v).or_insert(0) += 1;
            }
        }
        let mut ends: Vec<u32> = degree.into_iter().filter(|(v, d)| *d % 2 == 1 && axis(*v)).map(|(v, _)| v).collect();
        ends.sort_by(|a, b| pos[*a as usize].z.total_cmp(&pos[*b as usize].z).then(a.cmp(b)));
        if ends.len() % 2 == 1 {
            return Err(fail("the cut's outline does not close on the axis"));
        }
        for pair in ends.chunks(2) {
            outlines[f].push([pair[0].min(pair[1]), pair[0].max(pair[1])]);
        }
    }
    if outlines.iter().any(Vec::is_empty) {
        return Err(fail("the axis misses the solid: the sector's planes cut nothing"));
    }
    // The second face's outline put on the first's turned (a tessellated round wall crosses the
    // two planes a facet's sag apart): each of its points to the nearest on the first outline,
    // no further than a twentieth of the size, else the solid is not symmetric.
    let first: Vec<([f64; 2], [f64; 2])> =
        outlines[0].iter().map(|e| (halves[0].local(pos[e[0] as usize]), halves[0].local(pos[e[1] as usize]))).collect();
    let mut pos = pos;
    // A corner of an outline (where it turns by more than 10°) goes to the first's nearest
    // corner, any other point onto its nearest segment.
    let corners = |f: usize| -> BTreeMap<u32, [f64; 2]> {
        let mut nb: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        for e in &outlines[f] {
            nb.entry(e[0]).or_default().push(e[1]);
            nb.entry(e[1]).or_default().push(e[0]);
        }
        nb.into_iter()
            .filter_map(|(v, n)| {
                let [a, b] = n[..] else { return None };
                let (p, q, r) = (halves[f].local(pos[a as usize]), halves[f].local(pos[v as usize]), halves[f].local(pos[b as usize]));
                let (u, w) = ([q[0] - p[0], q[1] - p[1]], [r[0] - q[0], r[1] - q[1]]);
                let turn = (u[0] * w[1] - u[1] * w[0]).abs() / (u[0].hypot(u[1]) * w[0].hypot(w[1])).max(f64::MIN_POSITIVE);
                (turn > 10f64.to_radians().sin()).then_some((v, q))
            })
            .collect()
    };
    let first_corners: Vec<[f64; 2]> = corners(0).into_values().collect();
    let second_corners = corners(1);
    let seconds: BTreeSet<u32> = outlines[1].iter().flatten().copied().collect();
    for v in seconds {
        let q = halves[1].local(pos[v as usize]);
        let (gap, near) = if second_corners.contains_key(&v) {
            first_corners
                .iter()
                .map(|&c| ((c[0] - q[0]).hypot(c[1] - q[1]), c))
                .fold((f64::INFINITY, q), |best, x| if x.0 < best.0 { x } else { best })
        } else {
            first.iter().map(|&(a, b)| nearest_on(a, b, q)).fold((f64::INFINITY, q), |best, x| if x.0 < best.0 { x } else { best })
        };
        if gap > 0.05 * size {
            return Err(fail(format!("the sector's two cut faces differ by {gap:.3e} m: the solid is not symmetric under the turn")));
        }
        pos[v as usize] = halves[1].point(near);
    }
    // The union of the outlines' points on the first plane, each with the vertex it was on each
    // face.
    let mut union: Vec<[f64; 2]> = Vec::new();
    let mut vertex: Vec<[Option<u32>; 2]> = Vec::new();
    let mut index: BTreeMap<[i64; 2], usize> = BTreeMap::new();
    let key = |q: [f64; 2]| [(q[0] / merge).round() as i64, (q[1] / merge).round() as i64];
    let mut point_of: [BTreeMap<u32, usize>; 2] = [BTreeMap::new(), BTreeMap::new()];
    for f in 0..2 {
        let vs: BTreeSet<u32> = outlines[f].iter().flatten().copied().collect();
        for v in vs {
            let q = halves[f].local(pos[v as usize]);
            let k = (-1..=1)
                .flat_map(|i| (-1..=1).map(move |j| (i, j)))
                .find_map(|(i, j)| {
                    let mut kk = key(q);
                    kk[0] += i;
                    kk[1] += j;
                    index.get(&kk).copied()
                })
                .unwrap_or_else(|| {
                    union.push(q);
                    vertex.push([None, None]);
                    index.insert(key(q), union.len() - 1);
                    union.len() - 1
                });
            vertex[k][f] = Some(v);
            point_of[f].insert(v, k);
        }
    }
    let outline_points = union.len();
    // The outline segments with every union point on them, from each face; the faces must agree.
    let segments = |f: usize| -> BTreeSet<[u32; 2]> {
        let mut out = BTreeSet::new();
        for e in &outlines[f] {
            let (a, b) = (point_of[f][&e[0]], point_of[f][&e[1]]);
            let mut chain = vec![a];
            chain.extend(between(&union[..outline_points], union[a], union[b], merge));
            chain.push(b);
            for w in chain.windows(2) {
                out.insert([w[0].min(w[1]) as u32, w[0].max(w[1]) as u32]);
            }
        }
        out
    };
    let (first, second) = (segments(0), segments(1));
    if first != second {
        return Err(fail("the sector's two cut faces differ: the solid is not symmetric under the turn"));
    }
    let segs: Vec<[u32; 2]> = first.into_iter().collect();
    // A mesher's points are its own rounding (about 1e-7 of the size) off ours: one within 1e-6
    // of a point already there or of the outline is that point, or the outline's.
    let near = 1e-6 * scale;
    for &q in inner {
        let taken = union.iter().any(|u| (u[0] - q[0]).hypot(u[1] - q[1]) <= near);
        let on_outline = segs.iter().any(|e| nearest_on(union[e[0] as usize], union[e[1] as usize], q).0 <= near);
        if q[0] > near && !taken && !on_outline {
            union.push(q);
            vertex.push([None, None]);
        }
    }
    // Refined to the size, or (taking a mesher's points back) only as fine as the points given.
    let fine = if refine.is_some() { size } else { 4.0 * union.iter().fold(0.0f64, |m, q| m.max(q[0].abs()).max(q[1].abs())) };
    let (points, tris) = crate::planar::pslg(&union, &segs, fine)?;
    // The faces' vertices: those the outlines had, put exactly on the triangulation's points, and
    // new ones; a point on the axis is one vertex of both faces.
    let mut positions = pos.clone();
    let mut ids: [Vec<u32>; 2] = [Vec::new(), Vec::new()];
    for (k, q) in points.iter().enumerate() {
        let axis = q[0].abs() <= merge;
        let mut both: Option<u32> = None;
        for f in 0..2 {
            let given = vertex.get(k).and_then(|v| v[f]);
            let id = match (given, both) {
                (_, Some(v)) if axis => v,
                (Some(v), _) => {
                    positions[v as usize] = halves[f].point(*q);
                    v
                }
                (None, _) => {
                    positions.push(halves[f].point(*q));
                    positions.len() as u32 - 1
                }
            };
            if axis {
                both = Some(id);
                positions[id as usize] = halves[0].point([0.0, q[1]]);
            }
            ids[f].push(id);
        }
    }
    // Each outline edge's points in order, by its two vertices on that face.
    let mut inserts: BTreeMap<[u32; 2], Vec<u32>> = BTreeMap::new();
    for f in 0..2 {
        for e in &outlines[f] {
            let (a, b) = (point_of[f][&e[0]], point_of[f][&e[1]]);
            let mid: Vec<u32> = between(&points, points[a], points[b], merge).into_iter().map(|k| ids[f][k]).collect();
            if !mid.is_empty() {
                inserts.insert(*e, mid);
            }
        }
    }
    let mut triangles = Vec::with_capacity(t.surface.triangles.len() + 2 * tris.len());
    let mut out_tags = Vec::with_capacity(triangles.capacity());
    for (i, tri) in t.surface.triangles.iter().enumerate() {
        let tag = t.surface.tags.get(i).copied().unwrap_or(0);
        // The triangle's corners with the points added on its edges, in order round it.
        let mut ring = Vec::new();
        let mut split = 0;
        for k in 0..3 {
            let (a, b) = (tri[k], tri[(k + 1) % 3]);
            ring.push(a);
            if let Some(mid) = inserts.get(&[a.min(b), a.max(b)]) {
                split += 1;
                if a < b {
                    ring.extend(mid);
                } else {
                    ring.extend(mid.iter().rev());
                }
            }
        }
        match split {
            0 => triangles.push(*tri),
            1 => fan_from_opposite(tri, &inserts, &mut triangles),
            _ => {
                // A fan from the middle, a new vertex.
                let c = ring.iter().fold(Point3::ORIGIN.to_vec(), |s, &v| s + positions[v as usize].to_vec()) / ring.len() as f64;
                positions.push(Point3::ORIGIN + c);
                let m = positions.len() as u32 - 1;
                for k in 0..ring.len() {
                    triangles.push([m, ring[k], ring[(k + 1) % ring.len()]]);
                }
            }
        }
        out_tags.resize(triangles.len(), tag);
    }
    push_faces((&mut triangles, &mut out_tags), &positions, &halves, &tris, &ids, base);
    // Only the vertices the triangles use.
    let mut id = vec![u32::MAX; positions.len()];
    let mut kept = Vec::new();
    for v in triangles.iter_mut().flatten() {
        let slot = &mut id[*v as usize];
        if *slot == u32::MAX {
            *slot = kept.len() as u32;
            kept.push(positions[*v as usize]);
        }
        *v = *slot;
    }
    let mut out = t;
    out.surface = crate::csg::Surface { positions: kept, triangles, tags: out_tags };
    out.analytic.resize(base as usize + 2 * tris.len(), None);
    Ok((out, base))
}

/// The sector's surface as the tetrahedral mesher left it (boundary recovery adds points, even
/// on frozen faces, and on the outline they share with the solid's other faces), its cut faces
/// made each other's turn again: the other faces as meshed, the cut faces' points (the second's
/// turned back onto the first) triangulated with the outlines' as they now are. `None` when the
/// mesher changed nothing on them. `base` is the first cut-face tag, `t` the surface meshed.
pub(super) fn refreeze(mesh: &super::mesher::RegionMesh, t: &Tessellation, rot: &Rotation, base: u32, scale: f64) -> Result<Option<Tessellation>> {
    let tol = 1e-6 * scale;
    let halves = [Half::new(rot, rot.start(), tol), Half::new(rot, rot.start() + rot.angle(), tol)];
    let p = |v: u32| mesh.points.get(v as usize).copied().unwrap_or(Point3::ORIGIN);
    // The mesh's boundary: the solid's other faces, outward, and the cut faces' points.
    let mut index: BTreeMap<u32, u32> = BTreeMap::new();
    let mut open = crate::csg::Surface::default();
    let mut inner: Vec<[f64; 2]> = Vec::new();
    let mut cut_triangles = 0usize;
    for f in &mesh.boundary {
        if f.tag >= base {
            cut_triangles += 1;
            for &v in &f.nodes {
                let q = p(v);
                if let Some(h) = halves.iter().find(|h| h.holds(q)) {
                    inner.push(h.local(q));
                }
            }
            continue;
        }
        let Some(tv) = mesh.tets.get(f.tet as usize) else { continue };
        let Some(&apex) = tv.iter().find(|v| !f.nodes.contains(v)) else { continue };
        let (a, b, c, d) = (p(f.nodes[0]), p(f.nodes[1]), p(f.nodes[2]), p(apex));
        let tri = if (b - a).cross(c - a).dot(d - a) > 0.0 { [f.nodes[0], f.nodes[2], f.nodes[1]] } else { f.nodes };
        let ids = tri.map(|v| {
            *index.entry(v).or_insert_with(|| {
                open.positions.push(p(v));
                open.positions.len() as u32 - 1
            })
        });
        open.triangles.push(ids);
        open.tags.push(f.tag);
    }
    // Nothing to do when the mesh's two cut faces are each other's turn already: the points of
    // the second, turned back, those of the first (the solid's other faces may have changed —
    // a curved wall is refined to its curvature — without touching them).
    let key = |q: [f64; 2]| [(q[0] / tol).round() as i64, (q[1] / tol).round() as i64];
    let mut sides: [BTreeSet<[i64; 2]>; 2] = [BTreeSet::new(), BTreeSet::new()];
    for f in mesh.boundary.iter().filter(|f| f.tag >= base) {
        for &v in &f.nodes {
            let q = p(v);
            for (k, h) in halves.iter().enumerate() {
                if h.holds(q) {
                    sides[k].insert(key(h.local(q)));
                }
            }
        }
    }
    let agree = sides[1].iter().all(|k| (-1..=1).any(|i| (-1..=1).any(|j| sides[0].contains(&[k[0] + i, k[1] + j]))));
    if cut_triangles > 0 && sides[0].len() == sides[1].len() && agree {
        return Ok(None);
    }
    let mut analytic = t.analytic.clone();
    analytic.truncate(base as usize);
    let walled = Tessellation { surface: open, analytic, ..t.clone() };
    close(walled, rot, scale, None, &inner).map(|(t, _)| Some(t))
}

/// The two cut faces' triangles: `tris` on the first face's vertices `ids[0]` and the second's
/// `ids[1]`, each oriented out of the sector, triangle i of each tagged `base + i` and
/// `base + T + i`.
fn push_faces(
    (triangles, tags): (&mut Vec<[u32; 3]>, &mut Vec<u32>),
    positions: &[Point3],
    halves: &[Half; 2],
    tris: &[[u32; 3]],
    ids: &[Vec<u32>; 2],
    base: u32,
) {
    let count = tris.len() as u32;
    for f in 0..2 {
        for (i, tri) in tris.iter().enumerate() {
            let mut c = tri.map(|k| ids[f][k as usize]);
            let (a, b, d) = (positions[c[0] as usize], positions[c[1] as usize], positions[c[2] as usize]);
            let n = (b - a).cross(d - a);
            let h = halves[f].normal();
            // Out of the sector: back across the first plane, on across the second.
            if (n.x * h[0] + n.y * h[1] > 0.0) != (f == 1) {
                c.swap(1, 2);
            }
            triangles.push(c);
            tags.push(base + f as u32 * count + i as u32);
        }
    }
}

/// `tri` split along its one edge with points added, as a fan from the corner across it.
fn fan_from_opposite(tri: &[u32; 3], inserts: &BTreeMap<[u32; 2], Vec<u32>>, out: &mut Vec<[u32; 3]>) {
    for k in 0..3 {
        let (a, b, c) = (tri[k], tri[(k + 1) % 3], tri[(k + 2) % 3]);
        let Some(mid) = inserts.get(&[a.min(b), a.max(b)]) else { continue };
        let along: Vec<u32> = if a < b { mid.clone() } else { mid.iter().rev().copied().collect() };
        let mut chain = vec![a];
        chain.extend(along);
        chain.push(b);
        for w in chain.windows(2) {
            out.push([w[0], w[1], c]);
        }
        return;
    }
    out.push(*tri);
}

#[cfg(test)]
mod tests {
    use emcraft_model::{Boundary, Metres, Model, Point3, Rotation, Shape, Solid};

    /// A cylinder about z meshed on a quarter: its volume a quarter of the whole, and every node on
    /// the second cut face the turn of one on the first (the same nodes, node for node), the
    /// boundary faces on the two planes matching triangle for triangle.
    #[test]
    fn a_quarter_of_a_cylinder_has_cut_faces_that_are_each_others_turn() {
        let (r, l) = (0.01, 0.015);
        let shape = Shape::Cylinder { radius: Metres::new(r).unwrap(), length: Metres::new(l).unwrap() };
        matched(shape, Rotation::new(4, 0, [0.0, 0.0]).unwrap(), std::f64::consts::PI * r * r * l);
    }

    /// A square box about its middle on a quarter (C4), and a cylinder on a fifth (C5).
    #[test]
    fn a_square_box_and_a_cylinder_on_other_sectors() {
        let size = emcraft_model::BoxSize::new(emcraft_model::Vec3::new(0.02, 0.02, 0.01)).unwrap();
        matched(Shape::Box { size }, Rotation::new(4, 1, [0.01, 0.01]).unwrap(), 0.02 * 0.02 * 0.01);
        let (r, l) = (0.01, 0.015);
        let shape = Shape::Cylinder { radius: Metres::new(r).unwrap(), length: Metres::new(l).unwrap() };
        matched(shape, Rotation::new(5, 2, [0.0, 0.0]).unwrap(), std::f64::consts::PI * r * r * l);
    }

    fn matched(shape: Shape, rot: Rotation, whole: f64) {
        let mut m = Model::new();
        m.set_solid(Solid { shape, material: "vacuum".into() }).unwrap();
        m.rotation = Some(rot);
        let mesh = crate::solid::solid(&m, Metres::new(0.003).unwrap()).unwrap();
        let v: f64 = crate::volumes(&mesh).iter().sum();
        let part = whole / f64::from(rot.n);
        assert!((v - part).abs() < 0.02 * part, "{v} against {part}");
        let r = 0.01;
        let tol = 1e-9 * r;
        let node = |n: u32| Point3::new(mesh.nodes[n as usize][0], mesh.nodes[n as usize][1], mesh.nodes[n as usize][2]);
        let c = rot.centre;
        let ray = |angle: f64| {
            move |p: Point3| {
                let (x, y) = (p.x - c[0], p.y - c[1]);
                (-x * angle.sin() + y * angle.cos()).abs() <= tol && x * angle.cos() + y * angle.sin() >= -tol
            }
        };
        let (on_a, on_b) = (ray(rot.start()), ray(rot.start() + rot.angle()));
        let key = |p: Point3| [(p.x / 1e-9).round() as i64, (p.y / 1e-9).round() as i64, (p.z / 1e-9).round() as i64];
        let a: std::collections::BTreeSet<[i64; 3]> =
            (0..mesh.nodes.len() as u32).map(node).filter(|p| on_a(*p)).map(|p| key(rot.turned(p))).collect();
        let b: std::collections::BTreeSet<[i64; 3]> = (0..mesh.nodes.len() as u32).map(node).filter(|p| on_b(*p)).map(key).collect();
        assert!(a.len() > 10 && a == b, "{} nodes on the first face, {} on the second", a.len(), b.len());
        let faces = |on: &dyn Fn(Point3) -> bool, turn: bool| -> std::collections::BTreeSet<[[i64; 3]; 3]> {
            mesh.boundary
                .iter()
                .filter(|f| f.tag == Boundary::Pec && f.nodes.iter().all(|&n| on(node(n))))
                .map(|f| {
                    let mut k = f.nodes.map(|n| key(if turn { rot.turned(node(n)) } else { node(n) }));
                    k.sort_unstable();
                    k
                })
                .collect()
        };
        let (fa, fb) = (faces(&on_a, true), faces(&on_b, false));
        assert!(!fa.is_empty() && fa == fb, "{} faces against {}", fa.len(), fb.len());
    }
}
