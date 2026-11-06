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
pub(super) fn sector(t: Tessellation, rot: &Rotation, size: f64, scale: f64, bodies: &[Tessellation]) -> Result<(Tessellation, u32, Vec<Cut>)> {
    let tol = 1e-9 * scale;
    let halves = [Half::new(rot, rot.start(), tol), Half::new(rot, rot.start() + rot.angle(), tol)];
    let mut t = with_bodies(t, bodies);
    // Inside the sector: past the first plane towards +φ, short of the second.
    for (f, h) in halves.iter().enumerate() {
        let n = h.normal();
        let sign = if f == 0 { 1.0 } else { -1.0 };
        let c = h.centre;
        let d = move |p: Point3| sign * ((p.x - c[0]) * n[0] + (p.y - c[1]) * n[1]);
        let hh = *h;
        clip(&mut t.surface, &d, &move |p: Point3| hh.point(hh.local(p)), tol);
    }
    if t.surface.triangles.iter().zip(&t.surface.tags).all(|(_, &g)| g >= BODY_TAG) {
        return Err(fail("nothing of the solid lies in the sector"));
    }
    close(t, rot, scale, Some(size), &[], bodies.len())
}

/// The tags from which a triangle is an inclusion's across the cut faces (body `tag − BODY_TAG`),
/// not the solid's.
const BODY_TAG: u32 = 1 << 30;

/// An inclusion across the sector's cut faces, cut with it: its surface inside the sector (the
/// interface with the solid, open on the cut planes, oriented out of it) and its two caps there
/// (each the other's turn, oriented out of the sector), one tag per triangle each.
#[derive(Clone, Debug, Default)]
pub(super) struct Cut {
    pub(super) open: Tessellation,
    pub(super) caps: Tessellation,
}

/// `t` with each of `bodies` appended, its triangles tagged `BODY_TAG + k`.
fn with_bodies(mut t: Tessellation, bodies: &[Tessellation]) -> Tessellation {
    for (k, b) in bodies.iter().enumerate() {
        let off = t.surface.positions.len() as u32;
        t.surface.positions.extend(&b.surface.positions);
        t.surface.triangles.extend(b.surface.triangles.iter().map(|tri| tri.map(|v| v + off)));
        t.surface.tags.extend(std::iter::repeat_n(BODY_TAG + k as u32, b.surface.triangles.len()));
    }
    t
}

/// Whether `c` lies inside the closed outline `segs` (the even–odd rule along +s).
fn inside(points: &[[f64; 2]], segs: &[[u32; 2]], c: [f64; 2]) -> bool {
    let mut odd = false;
    for e in segs {
        let (a, b) = (points[e[0] as usize], points[e[1] as usize]);
        if (a[1] > c[1]) != (b[1] > c[1]) {
            let s = a[0] + (c[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1]);
            if s > c[0] {
                odd = !odd;
            }
        }
    }
    odd
}

/// The triangles of `tris` (on vertices `positions`), with their tags, as a tessellation of its
/// own: only the vertices they use, one tag per triangle.
fn own_tessellation(positions: &[Point3], tris: &[[u32; 3]]) -> Tessellation {
    let mut id: BTreeMap<u32, u32> = BTreeMap::new();
    let mut t = Tessellation::default();
    for tri in tris {
        let ids = tri.map(|v| {
            *id.entry(v).or_insert_with(|| {
                t.surface.positions.push(positions.get(v as usize).copied().unwrap_or(Point3::ORIGIN));
                t.surface.positions.len() as u32 - 1
            })
        });
        t.surface.tags.push(t.surface.triangles.len() as u32);
        t.surface.triangles.push(ids);
    }
    t
}

/// The sector's open surface `t` (its cut faces missing) closed by two faces each other's turn:
/// triangulated from the union of the two outlines' points and `inner` (points inside the
/// first face, on its plane's (s, t)), refined to `refine` when given, else taken as they are.
fn close(
    t: Tessellation,
    rot: &Rotation,
    scale: f64,
    refine: Option<f64>,
    inner: &[[f64; 2]],
    bodies: usize,
) -> Result<(Tessellation, u32, Vec<Cut>)> {
    let tol = 1e-9 * scale;
    let merge = 1e-7 * scale;
    let halves = [Half::new(rot, rot.start(), tol), Half::new(rot, rot.start() + rot.angle(), tol)];
    // Cut first (not taking a mesher's points back): each outline thinned to the size.
    let t = match refine {
        Some(size) => thinned(t, &halves, 0.6 * size, merge),
        None => t,
    };
    let base = t.surface.tags.iter().filter(|&&g| g < BODY_TAG).map(|g| g + 1).max().unwrap_or(0).max(t.analytic.len() as u32);
    let size = refine.unwrap_or(scale);
    let pos = t.surface.positions.clone();
    // The open outline the cut left on each plane, with the axis' stretches inside the solid
    // (where the two faces meet) closing it.
    let open = outline(&t.surface.triangles);
    let on = |h: &Half, v: u32| pos.get(v as usize).is_some_and(|p| h.holds(*p));
    let mut outlines: [Vec<[u32; 2]>; 2] = [0, 1].map(|f| open.iter().copied().filter(|e| on(&halves[f], e[0]) && on(&halves[f], e[1])).collect());
    let axis = |v: u32| pos.get(v as usize).is_some_and(|p| halves[0].local(*p)[0].abs() <= merge);
    // Each vertex's body: the solid's (none) or an inclusion's across the cut.
    let mut owner: BTreeMap<u32, Option<usize>> = BTreeMap::new();
    for (tri, &g) in t.surface.triangles.iter().zip(&t.surface.tags) {
        for &v in tri {
            owner.insert(v, (g >= BODY_TAG).then(|| (g - BODY_TAG) as usize));
        }
    }
    let owner_of = |v: u32| owner.get(&v).copied().flatten();
    for f in 0..2 {
        let mut degree: BTreeMap<u32, usize> = BTreeMap::new();
        for e in &outlines[f] {
            for v in e {
                *degree.entry(*v).or_insert(0) += 1;
            }
        }
        // Each body's outline closed on the axis by its own stretches of it.
        let mut ends: BTreeMap<Option<usize>, Vec<u32>> = BTreeMap::new();
        for v in degree.into_iter().filter(|(v, d)| *d % 2 == 1 && axis(*v)).map(|(v, _)| v) {
            ends.entry(owner_of(v)).or_default().push(v);
        }
        for list in ends.values_mut() {
            list.sort_by(|a, b| pos[*a as usize].z.total_cmp(&pos[*b as usize].z).then(a.cmp(b)));
            if list.len() % 2 == 1 {
                return Err(fail("the cut's outline does not close on the axis"));
            }
            for pair in list.chunks(2) {
                outlines[f].push([pair[0].min(pair[1]), pair[0].max(pair[1])]);
            }
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
    // The first outline's points, and each second point's distance to its neighbours along its
    // own outline (before any moves).
    let first_points: Vec<[f64; 2]> = first.iter().flat_map(|&(a, b)| [a, b]).collect();
    let mut spacing: BTreeMap<u32, f64> = BTreeMap::new();
    let mut own: BTreeMap<u32, Vec<([f64; 2], [f64; 2])>> = BTreeMap::new();
    for e in &outlines[1] {
        let (a, b) = (halves[1].local(pos[e[0] as usize]), halves[1].local(pos[e[1] as usize]));
        let d = (a[0] - b[0]).hypot(a[1] - b[1]);
        for v in e {
            let s = spacing.entry(*v).or_insert(f64::INFINITY);
            *s = s.min(d);
            own.entry(*v).or_default().push((a, b));
        }
    }
    // The open surface's triangles at each of the second outline's points.
    let mut touching: BTreeMap<u32, Vec<[u32; 3]>> = BTreeMap::new();
    for tri in &t.surface.triangles {
        for &w in tri {
            if seconds.contains(&w) {
                touching.entry(w).or_default().push(*tri);
            }
        }
    }
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
        // A point of the first outline close by along it is taken instead: the union of two
        // outlines' crossings would otherwise hold pairs a sliver apart, which the mesher refines
        // down to. Moved along its own straight outline, by less than half the way to its
        // neighbours, the point keeps its triangles' shapes and sides.
        // (Taking a mesher's points back, none moves: the volume mesh holds them where they are.)
        let along = if refine.is_none() || second_corners.contains_key(&v) || q[0].abs() <= merge {
            near
        } else {
            let reach = (0.3 * size).min(0.4 * spacing.get(&v).copied().unwrap_or(0.0));
            first_points
                .iter()
                .map(|&c| ((c[0] - near[0]).hypot(c[1] - near[1]), c))
                // Not onto the axis (a point there is one vertex of both faces), and only along
                // its own segments, never round a bend onto another stretch of the outline.
                .filter(|(d, c)| *d < reach && c[0].abs() > merge)
                .filter(|(_, c)| own.get(&v).is_some_and(|segs| segs.iter().any(|&(a, b)| nearest_on(a, b, *c).0 <= merge)))
                .fold((f64::INFINITY, near), |best, x| if x.0 < best.0 { x } else { best })
                .1
        };
        // Only where every triangle at the point keeps its side and most of its area.
        let moved = halves[1].point(along);
        let keeps = touching.get(&v).is_none_or(|tris| {
            tris.iter().all(|tri| {
                let normal = |p: &dyn Fn(u32) -> Point3| {
                    let [a, b, c] = tri.map(p);
                    (b - a).cross(c - a)
                };
                let before = normal(&|w| pos[w as usize]);
                let after = normal(&|w| if w == v { moved } else { pos[w as usize] });
                after.dot(before) > 0.5 * before.norm_sqr()
            })
        });
        pos[v as usize] = if keeps { moved } else { halves[1].point(near) };
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
    // Points given on the axis (a mesher's, taken back) split its stretch of the outline: they
    // are the only ones there, no other face reaching the axis inside the solid.
    let near = 1e-6 * scale;
    for &q in inner {
        if q[0].abs() <= near && !union.iter().any(|u| (u[0] - q[0]).hypot(u[1] - q[1]) <= near) {
            union.push([0.0, q[1]]);
            vertex.push([None, None]);
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
    // Each body's own outline on the first face (the solid's, then each inclusion's), split at the
    // union's points as the faces' are.
    let owned = |who: Option<usize>| -> Vec<[u32; 2]> {
        let mut out = BTreeSet::new();
        for e in outlines[0].iter().filter(|e| owner_of(e[0]) == who && owner_of(e[1]) == who) {
            let (a, b) = (point_of[0][&e[0]], point_of[0][&e[1]]);
            let mut chain = vec![a];
            chain.extend(between(&union[..outline_points], union[a], union[b], merge));
            chain.push(b);
            for w in chain.windows(2) {
                out.insert([w[0].min(w[1]) as u32, w[0].max(w[1]) as u32]);
            }
        }
        out.into_iter().collect()
    };
    let host = owned(None);
    let rims: Vec<Vec<[u32; 2]>> = (0..bodies).map(|k| owned(Some(k))).collect();
    // A mesher's points are its own rounding (about 1e-7 of the size) off ours: one within 1e-6
    // of a point already there or of the outline is that point, or the outline's.
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
    // With inclusions across the cut, the solid's whole cut face is triangulated (their outlines
    // constraints in it) and each triangle inside one's outline is its cap.
    let (points, tris) = if bodies == 0 {
        crate::planar::pslg(&union, &segs, fine)?
    } else {
        let snapshot = union.clone();
        crate::planar::pslg_keep(&union, &segs, fine, &|c| inside(&snapshot, &host, c))?
    };
    let body_of = |tri: &[u32; 3]| -> Option<usize> {
        let c = tri.iter().fold([0.0; 2], |s, &k| [s[0] + points[k as usize][0] / 3.0, s[1] + points[k as usize][1] / 3.0]);
        rims.iter().position(|r| inside(&union, r, c))
    };
    let (tris, cap_tris): (Vec<[u32; 3]>, Vec<[u32; 3]>) = tris.into_iter().partition(|tri| body_of(tri).is_none());
    let cap_of: Vec<usize> = cap_tris.iter().map(|tri| body_of(tri).unwrap_or(0)).collect();
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
    let mut caps: Vec<Vec<[u32; 3]>> = vec![Vec::new(); bodies];
    for k in 0..bodies {
        let mine: Vec<[u32; 3]> = cap_tris.iter().zip(&cap_of).filter(|(_, b)| **b == k).map(|(tri, _)| *tri).collect();
        let (mut tri_k, mut tag_k) = (Vec::new(), Vec::new());
        push_faces((&mut tri_k, &mut tag_k), &positions, &halves, &mine, &ids, 0);
        caps[k] = tri_k;
    }
    // The solid's other faces' vertices on their exact surfaces: they are frozen for the mesher,
    // which puts the vertices of a face it may move on it but not these (the points added on the
    // outlines and in the splits lie on chords). A point on a cut plane stays only where its
    // projection does (a round wall about the axis moves it along the plane, outward).
    // A vertex moves only onto the one curved surface its triangles share, and only where it
    // stays on every plane it also lies on (a point where two curved faces meet, or a curved
    // face meets a plane it would leave, stays: moving it could fold a neighbour over).
    let mut touching: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
    for (tri, tag) in triangles.iter().zip(&out_tags) {
        for &v in tri {
            touching.entry(v).or_default().insert(*tag);
        }
    }
    let plane = |s: &emcraft_tetmesh::tet::geometry::AnalyticSurface| matches!(s, emcraft_tetmesh::tet::geometry::AnalyticSurface::Plane { .. });
    for (&v, tags) in &touching {
        // (A face without an exact surface is a facet the point may move within; a cut face's
        // point moves only along its plane, below.)
        let surfaces: Vec<&emcraft_tetmesh::tet::geometry::AnalyticSurface> =
            tags.iter().filter_map(|&g| t.analytic.get(g as usize).and_then(Option::as_ref)).collect();
        // (A curved face split into several tags is still one surface.)
        let mut curved = surfaces.iter().filter(|s| !plane(s));
        let Some(surface) = curved.next() else { continue };
        if curved.any(|s| s != surface) {
            continue;
        }
        let p = positions[v as usize];
        let Some(q) = surface.project(super::mesher::mesher_point(p)).map(super::mesher::model_point) else { continue };
        let stays = surfaces
            .iter()
            .filter(|s| plane(s))
            .all(|s| s.project(super::mesher::mesher_point(q)).map(super::mesher::model_point).is_some_and(|r| (r - q).norm() <= tol));
        let cut = halves.iter().any(|h| h.holds(p));
        if stays && (q - p).norm() <= 0.2 * size && (!cut || halves.iter().any(|h| h.holds(q))) {
            positions[v as usize] = q;
        }
    }
    // The inclusions' triangles out of the solid's: their open surfaces and caps their own.
    let mut cuts = Vec::with_capacity(bodies);
    if bodies > 0 {
        let mut opens: Vec<Vec<[u32; 3]>> = vec![Vec::new(); bodies];
        let (mut solid_tris, mut solid_tags) = (Vec::new(), Vec::new());
        for (tri, &g) in triangles.iter().zip(&out_tags) {
            if g >= BODY_TAG {
                if let Some(o) = opens.get_mut((g - BODY_TAG) as usize) {
                    o.push(*tri);
                }
            } else {
                solid_tris.push(*tri);
                solid_tags.push(g);
            }
        }
        for (o, c) in opens.iter().zip(&caps) {
            cuts.push(Cut { open: own_tessellation(&positions, o), caps: own_tessellation(&positions, c) });
        }
        triangles = solid_tris;
        out_tags = solid_tags;
    }
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
    Ok((out, base, cuts))
}

/// The open surface `t` with the points of each cut outline closer than `gap` to the one kept
/// before them along it collapsed onto it: the cut crosses the solid's tessellation wherever its
/// edges meet the plane, a hair apart at times, and the mesher keeps every point it is given (a
/// face it may remesh is still bound by them), refining the faces along the cut down to their
/// spacing. Corners (where an outline turns by more than 10° or branches) and points on the axis
/// stay; a point goes only where every triangle at it keeps its side and most of its area, the
/// triangles that held both points dropped.
fn thinned(mut t: Tessellation, halves: &[Half; 2], gap: f64, merge: f64) -> Tessellation {
    for h in halves {
        let pos = t.surface.positions.clone();
        let on = |v: u32| pos.get(v as usize).is_some_and(|p| h.holds(*p));
        let edges: Vec<[u32; 2]> = outline(&t.surface.triangles).into_iter().filter(|e| on(e[0]) && on(e[1])).collect();
        let mut nb: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        for e in &edges {
            nb.entry(e[0]).or_default().push(e[1]);
            nb.entry(e[1]).or_default().push(e[0]);
        }
        let local = |v: u32| h.local(pos[v as usize]);
        let corner = |v: u32| -> bool {
            let Some(n) = nb.get(&v) else { return true };
            let [a, b] = n[..] else { return true };
            if local(v)[0].abs() <= merge {
                return true;
            }
            let (p, q, r) = (local(a), local(v), local(b));
            let (u, w) = ([q[0] - p[0], q[1] - p[1]], [r[0] - q[0], r[1] - q[1]]);
            let turn = (u[0] * w[1] - u[1] * w[0]).abs() / (u[0].hypot(u[1]) * w[0].hypot(w[1])).max(f64::MIN_POSITIVE);
            turn > 10f64.to_radians().sin()
        };
        // Each chain from a corner to the next, walked once.
        let mut walked: std::collections::BTreeSet<[u32; 2]> = std::collections::BTreeSet::new();
        let mut onto: BTreeMap<u32, u32> = BTreeMap::new();
        let starts: Vec<u32> = nb.keys().copied().filter(|&v| corner(v)).collect();
        for &c in &starts {
            for &first in nb.get(&c).map_or(&[][..], Vec::as_slice) {
                if walked.contains(&[c.min(first), c.max(first)]) {
                    continue;
                }
                let mut chain = vec![c];
                let (mut prev, mut cur) = (c, first);
                loop {
                    walked.insert([prev.min(cur), prev.max(cur)]);
                    chain.push(cur);
                    if corner(cur) || chain.len() > edges.len() + 1 {
                        break;
                    }
                    let next = nb.get(&cur).and_then(|n| n.iter().copied().find(|&x| x != prev));
                    let Some(next) = next else { break };
                    (prev, cur) = (cur, next);
                }
                // Greedy along the chain, its last point (a corner) kept.
                let mut kept = chain[0];
                let end = *chain.last().unwrap_or(&kept);
                for &v in &chain[1..chain.len().saturating_sub(1)] {
                    let d = |a: u32, b: u32| {
                        let (p, q) = (local(a), local(b));
                        (p[0] - q[0]).hypot(p[1] - q[1])
                    };
                    if d(v, kept) < gap && d(v, end) >= 0.5 * gap {
                        onto.insert(v, kept);
                    } else {
                        kept = v;
                    }
                }
            }
        }
        // Collapse, each where its triangles stay sound.
        for (v, w) in onto {
            let at: Vec<usize> = (0..t.surface.triangles.len()).filter(|&i| t.surface.triangles[i].contains(&v)).collect();
            let normal = |tri: [u32; 3], p: &[Point3]| {
                let [a, b, c] = tri.map(|x| p[x as usize]);
                (b - a).cross(c - a)
            };
            let sound = at.iter().all(|&i| {
                let tri = t.surface.triangles[i];
                if tri.contains(&w) {
                    return true;
                }
                let after = tri.map(|x| if x == v { w } else { x });
                let (n0, n1) = (normal(tri, &t.surface.positions), normal(after, &t.surface.positions));
                n1.dot(n0) > 0.5 * n0.norm_sqr()
            });
            if !sound {
                continue;
            }
            let mut keep = vec![true; t.surface.triangles.len()];
            for &i in &at {
                let tri = &mut t.surface.triangles[i];
                if tri.contains(&w) {
                    keep[i] = false;
                } else {
                    for x in tri.iter_mut() {
                        if *x == v {
                            *x = w;
                        }
                    }
                }
            }
            let mut k = keep.iter();
            t.surface.triangles.retain(|_| k.next().copied().unwrap_or(true));
            let mut k = keep.iter();
            t.surface.tags.retain(|_| k.next().copied().unwrap_or(true));
        }
    }
    t
}

/// The sector's surface as the tetrahedral mesher left it (boundary recovery adds points, even
/// on frozen faces, and on the outline they share with the solid's other faces), its cut faces
/// made each other's turn again: the other faces as meshed, the cut faces' points (the second's
/// turned back onto the first) triangulated with the outlines' as they now are. `None` when the
/// mesher changed nothing on them. `base` is the first cut-face tag, `t` the surface meshed.
pub(super) fn refreeze(
    mesh: &super::mesher::RegionMesh,
    t: &Tessellation,
    rot: &Rotation,
    base: u32,
    scale: f64,
    bodies: &[Tessellation],
) -> Result<Option<(Tessellation, Vec<Cut>)>> {
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
    // Each cut face's triangles by their corners' places (s, t) on its plane, sorted: the two
    // agree when the second's are the first's (to a key's neighbour either way).
    let mut sides: [BTreeSet<[[i64; 2]; 3]>; 2] = [BTreeSet::new(), BTreeSet::new()];
    let mut loose: [Vec<[[f64; 2]; 3]>; 2] = [Vec::new(), Vec::new()];
    for f in mesh.boundary.iter().filter(|f| f.tag >= base) {
        let q = f.nodes.map(p);
        for (k, h) in halves.iter().enumerate() {
            if q.iter().all(|x| h.holds(*x)) {
                let mut c = q.map(|x| key(h.local(x)));
                c.sort_unstable();
                sides[k].insert(c);
                loose[k].push(q.map(|x| h.local(x)));
            }
        }
    }
    let same = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).abs() <= 2.0 * tol && (a[1] - b[1]).abs() <= 2.0 * tol;
    // By key first; a triangle whose corners round across a key's edge by the slower search.
    let agree = sides[0].len() == sides[1].len()
        && loose[1].iter().all(|t1| {
            let mut c = t1.map(key);
            c.sort_unstable();
            sides[0].contains(&c) || loose[0].iter().any(|t0| t1.iter().all(|x| t0.iter().any(|y| same(*x, *y))))
        });
    if cut_triangles > 0 && agree {
        return Ok(None);
    }
    let mut analytic = t.analytic.clone();
    analytic.truncate(base as usize);
    let walled = with_bodies(Tessellation { surface: open, analytic, ..t.clone() }, bodies);
    close(walled, rot, scale, None, &inner, bodies.len()).map(|(t, _, cuts)| Some((t, cuts)))
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

    /// A sector of a cylinder takes about its share of the whole's tetrahedra at the same size: the
    /// cut outlines thinned to the size and their crossings merged, the walls remeshed, so the
    /// mesher does not refine down to the tessellation's crossings along the cut lines.
    #[test]
    fn a_quarter_takes_a_fraction_of_the_whole_s_tetrahedra() {
        let shape = Shape::Cylinder { radius: Metres::new(0.01).unwrap(), length: Metres::new(0.03).unwrap() };
        let count = |rot: Option<Rotation>| {
            let mut m = Model::new();
            m.set_solid(Solid { shape: shape.clone(), material: "vacuum".into() }).unwrap();
            m.rotation = rot;
            crate::solid::solid(&m, Metres::new(0.003).unwrap()).unwrap().tets.len()
        };
        // 4 109 whole, 1 427 on a quarter, 867 on an eighth.
        let whole = count(None);
        for (n, share) in [(4, 0.4), (8, 0.25)] {
            let part = count(Some(Rotation::new(n, 0, [0.0, 0.0]).unwrap()));
            assert!((part as f64) < share * whole as f64, "{part} tetrahedra on 1/{n} against {whole} on the whole");
        }
    }

    /// The axis stays split to the element size through the mesher's rounds: taken back from it,
    /// the points on the axis (the cut faces' alone) are kept, not dropped into one long edge.
    #[test]
    fn the_axis_stays_split_to_the_size() {
        let shape = Shape::Cylinder { radius: Metres::new(0.01).unwrap(), length: Metres::new(0.03).unwrap() };
        let mut m = Model::new();
        m.set_solid(Solid { shape, material: "vacuum".into() }).unwrap();
        m.rotation = Some(Rotation::new(4, 1, [0.0, 0.0]).unwrap());
        let mesh = crate::solid::solid(&m, Metres::new(0.005).unwrap()).unwrap();
        let on_axis = mesh.nodes.iter().filter(|p| p[0].hypot(p[1]) < 1e-9).count();
        assert!(on_axis >= 6, "{on_axis} nodes on the 30 mm axis at 5 mm");
        matched_mesh(&mesh, m.rotation.unwrap());
    }

    fn matched(shape: Shape, rot: Rotation, whole: f64) {
        let mut m = Model::new();
        m.set_solid(Solid { shape, material: "vacuum".into() }).unwrap();
        m.rotation = Some(rot);
        let mesh = crate::solid::solid(&m, Metres::new(0.003).unwrap()).unwrap();
        let v: f64 = crate::volumes(&mesh).iter().sum();
        let part = whole / f64::from(rot.n);
        assert!((v - part).abs() < 0.02 * part, "{v} against {part}");
        matched_mesh(&mesh, rot);
    }

    fn matched_mesh(mesh: &emcraft_model::Mesh, rot: Rotation) {
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
