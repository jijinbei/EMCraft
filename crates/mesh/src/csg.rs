//! Boolean combinations of closed triangulated surfaces (union, difference, intersection), for
//! bodies built from primitives.
//!
//! The classic mesh Boolean in three steps:
//! 1. **Intersect**: every pair of triangles whose boxes overlap is tested with exact
//!    orientation predicates (Shewchuk's, from `emcraft_tetmesh::predicates`); a triangle edge
//!    crossing a triangle of the other surface gives a point, keyed by (edge, triangle) so the
//!    triangles that share the edge, and the triangle crossed, all use the same point. Each
//!    crossing pair of triangles gives the segment between its two points.
//! 2. **Split**: each triangle crossed is re-triangulated in its plane with the points on its
//!    edges and inside it, the segments as constraints (a small constrained triangulation:
//!    insertion, Lawson flips, Sloan's constraint recovery — S. W. Sloan, *A fast algorithm for
//!    generating constrained Delaunay triangulations*, Computers & Structures 47(3), 1993).
//! 3. **Classify**: each piece is inside or outside the other surface by the winding number of
//!    its centroid (the solid angles it subtends, A. Van Oosterom, J. Strackee, IEEE Trans.
//!    Biomed. Eng. 30(2), 1983), and kept or dropped (turned inside out for the tool of a
//!    difference).
//!
//! The surfaces must meet in general position: no coplanar overlapping faces and no vertex on
//! the other surface. The callers move a tool's faces that are flush with the body's off by a
//! small step first (`crate::solid`); what is still degenerate is refused, never guessed. A
//! vertex exactly on a plane is taken to lie on its positive side, and an edge exactly through
//! another's line to pass on the side its node order gives, the same from every triangle that
//! shares it, so the pieces still close up.

use std::collections::HashMap;

use emcraft_model::{Aabb, Point3};
use emcraft_tetmesh::predicates::{ExactPoint2, ExactPoint3 as Exact, Sign, orient2d, orient3d};

use crate::{MeshError, Result};

/// What to keep of two solids.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Union,
    /// The first minus the second.
    Difference,
    Intersection,
}

/// A closed, outward-oriented triangulated surface with a tag per triangle.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Surface {
    pub positions: Vec<Point3>,
    pub triangles: Vec<[u32; 3]>,
    pub tags: Vec<u32>,
}

fn fail(why: impl Into<String>) -> MeshError {
    MeshError::Invalid { what: "Boolean combination".into(), why: why.into() }
}

/// The two surfaces' points as exact-predicate points.
fn points(s: &Surface) -> Result<Vec<Exact>> {
    s.positions
        .iter()
        .map(|p| Exact::try_from(p.to_array().map(emcraft_tetmesh::predicates::snap_to_domain)).map_err(|e| fail(e.to_string())))
        .collect()
}

/// Which side of triangle (a, b, c)'s plane `p` lies on, by its right-hand normal; on the plane
/// counts as the positive side.
fn side(a: &Exact, b: &Exact, c: &Exact, p: &Exact) -> bool {
    // orient3d is positive when p lies below the plane (a, b, c counter-clockwise from above).
    orient3d(a, b, c, p) != Sign::Positive
}

/// The side of the line p→q that segment (a, b) passes (the sign of orient3d(p, q, a, b)),
/// an exact zero broken by the order of a and b (global ids), so the triangles on either side
/// of edge (a, b) see opposite signs.
fn around(p: &Exact, q: &Exact, a: (&Exact, u32), b: (&Exact, u32)) -> bool {
    match orient3d(p, q, a.0, b.0) {
        Sign::Positive => true,
        Sign::Negative => false,
        Sign::Zero => a.1 < b.1,
    }
}

/// Whether edge (p, q) of one surface crosses triangle t of the other.
fn crosses(p: &Exact, q: &Exact, t: [(&Exact, u32); 3]) -> bool {
    if side(t[0].0, t[1].0, t[2].0, p) == side(t[0].0, t[1].0, t[2].0, q) {
        return false;
    }
    let s0 = around(p, q, t[0], t[1]);
    s0 == around(p, q, t[1], t[2]) && s0 == around(p, q, t[2], t[0])
}

/// A point made by an edge of surface `edge_of` (nodes a < b) crossing triangle `tri` of the
/// other.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Key {
    edge_of: u8,
    a: u32,
    b: u32,
    tri: u32,
}

/// The surfaces' intersection: the crossing points, the segments in each triangle, and the
/// points on each edge.
#[derive(Default)]
struct Cut {
    /// Crossing points after both surfaces' vertices: id = offset + index.
    positions: Vec<Point3>,
    keys: HashMap<Key, u32>,
    /// Per surface, per triangle: the segments (point ids, global).
    segments: [HashMap<u32, Vec<[u32; 2]>>; 2],
    /// Per surface, per edge (a < b): the points on it.
    on_edge: [HashMap<[u32; 2], Vec<u32>>; 2],
    /// Per surface, per triangle: crossing points inside it.
    inside: [HashMap<u32, Vec<u32>>; 2],
}

/// Global vertex ids: surface 0's vertices, then surface 1's, then the crossings.
struct Ids {
    n0: u32,
    n1: u32,
}

impl Ids {
    fn of(&self, s: usize, v: u32) -> u32 {
        if s == 0 { v } else { self.n0 + v }
    }

    fn crossing(&self, k: u32) -> u32 {
        self.n0 + self.n1 + k
    }
}

/// Boxes of a surface's triangles binned on a uniform grid.
struct Grid {
    lo: Point3,
    step: f64,
    dims: [usize; 3],
    cells: HashMap<[usize; 3], Vec<u32>>,
}

fn tri_box(s: &Surface, t: usize) -> (Point3, Point3) {
    let b = Aabb::around(s.triangles[t].iter().map(|&v| s.positions[v as usize])).unwrap_or_default();
    (b.min, b.max)
}

impl Grid {
    fn new(s: &Surface) -> Grid {
        let Aabb { min: lo, max: hi } = Aabb::around(s.positions.iter().copied()).unwrap_or_default();
        let ext = (hi - lo).iter().fold(0.0, f64::max).max(f64::MIN_POSITIVE);
        let per_side = (s.triangles.len().max(1) as f64).sqrt().ceil().clamp(1.0, 256.0);
        let step = ext / per_side;
        let dims = [0, 1, 2].map(|d| (((hi[d] - lo[d]) / step).floor() as usize + 1).min(1024));
        let mut g = Grid { lo, step, dims, cells: HashMap::new() };
        for t in 0..s.triangles.len() {
            let (a, b) = tri_box(s, t);
            let (ia, ib) = (g.cell(a), g.cell(b));
            for i in ia[0]..=ib[0] {
                for j in ia[1]..=ib[1] {
                    for k in ia[2]..=ib[2] {
                        g.cells.entry([i, j, k]).or_default().push(t as u32);
                    }
                }
            }
        }
        g
    }

    fn cell(&self, p: Point3) -> [usize; 3] {
        [0, 1, 2].map(|d| (((p[d] - self.lo[d]) / self.step).floor().max(0.0) as usize).min(self.dims[d] - 1))
    }

    /// The triangles whose boxes may overlap the box (lo, hi), each once.
    fn near(&self, lo: Point3, hi: Point3) -> Vec<u32> {
        let (ia, ib) = (self.cell(lo), self.cell(hi));
        let mut out = Vec::new();
        for i in ia[0]..=ib[0] {
            for j in ia[1]..=ib[1] {
                for k in ia[2]..=ib[2] {
                    if let Some(c) = self.cells.get(&[i, j, k]) {
                        out.extend_from_slice(c);
                    }
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }
}

fn intersect(s: [&Surface; 2], p: [&[Exact]; 2], ids: &Ids) -> Result<Cut> {
    let mut cut = Cut::default();
    let grid = Grid::new(s[1]);
    let tri = |k: usize, t: u32| -> [(&Exact, u32); 3] { s[k].triangles[t as usize].map(|v| (&p[k][v as usize], v)) };
    let point = |cut: &mut Cut, edge_of: usize, a: u32, b: u32, t: u32| -> u32 {
        let (a, b) = (a.min(b), a.max(b));
        let key = Key { edge_of: edge_of as u8, a, b, tri: t };
        if let Some(&id) = cut.keys.get(&key) {
            return id;
        }
        // The crossing by the signed distances of a and b to the triangle's plane.
        let other = 1 - edge_of;
        let [u, v, w] = s[other].triangles[t as usize].map(|i| s[other].positions[i as usize]);
        let n = (v - u).cross(w - u);
        let (pa, pb) = (s[edge_of].positions[a as usize], s[edge_of].positions[b as usize]);
        let (da, db) = (n.dot(pa - u), n.dot(pb - u));
        let f = if da == db { 0.5 } else { (da / (da - db)).clamp(0.0, 1.0) };
        cut.positions.push(pa.lerp(pb, f));
        let id = ids.crossing((cut.positions.len() - 1) as u32);
        cut.keys.insert(key, id);
        cut.on_edge[edge_of].entry([a, b]).or_default().push(id);
        cut.inside[other].entry(t).or_default().push(id);
        id
    };
    for ta in 0..s[0].triangles.len() as u32 {
        let (lo, hi) = tri_box(s[0], ta as usize);
        for tb in grid.near(lo, hi) {
            let (ra, rb) = (tri(0, ta), tri(1, tb));
            // Quick rejection: one triangle wholly on one side of the other's plane.
            let sides_b = rb.map(|q| side(ra[0].0, ra[1].0, ra[2].0, q.0));
            let sides_a = ra.map(|q| side(rb[0].0, rb[1].0, rb[2].0, q.0));
            if sides_b.iter().all(|x| *x == sides_b[0]) || sides_a.iter().all(|x| *x == sides_a[0]) {
                continue;
            }
            let mut ends = Vec::with_capacity(2);
            for (k, (from, to)) in [(ra, rb), (rb, ra)].into_iter().enumerate() {
                let t_other = if k == 0 { tb } else { ta };
                for (i, j) in [(0, 1), (1, 2), (2, 0)] {
                    if crosses(from[i].0, from[j].0, to) {
                        ends.push(point(&mut cut, k, from[i].1, from[j].1, t_other));
                    }
                }
            }
            match ends.len() {
                0 => {}
                2 => {
                    cut.segments[0].entry(ta).or_default().push([ends[0], ends[1]]);
                    cut.segments[1].entry(tb).or_default().push([ends[0], ends[1]]);
                }
                n => {
                    return Err(fail(format!(
                        "two triangles meet in {n} crossing points, not a segment (the surfaces touch degenerately); move or resize a body slightly"
                    )));
                }
            }
        }
    }
    Ok(cut)
}

/// A planar triangulation being built: 2D points (local), triangles counter-clockwise, and the
/// edges that may not be flipped.
struct Plane {
    p: Vec<ExactPoint2>,
    tris: Vec<[usize; 3]>,
    fixed: std::collections::HashSet<[usize; 2]>,
}

fn edge(a: usize, b: usize) -> [usize; 2] {
    [a.min(b), a.max(b)]
}

impl Plane {
    fn orient(&self, a: usize, b: usize, c: usize) -> Sign {
        orient2d(&self.p[a], &self.p[b], &self.p[c])
    }

    /// The triangles holding edge (a, b), with the corner opposite it.
    fn holding(&self, a: usize, b: usize) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for (i, t) in self.tris.iter().enumerate() {
            for k in 0..3 {
                let (u, v) = (t[k], t[(k + 1) % 3]);
                if (u, v) == (a, b) || (u, v) == (b, a) {
                    out.push((i, t[(k + 2) % 3]));
                }
            }
        }
        out
    }

    /// Splits edge (a, b) at point m in every triangle holding it.
    fn split_edge(&mut self, a: usize, b: usize, m: usize) {
        for (i, c) in self.holding(a, b) {
            let t = self.tris[i];
            // Keep the triangle's orientation: replace it by (u, m, c) and (m, v, c) along its
            // own direction u→v of the edge.
            let k = (0..3).find(|&k| edge(t[k], t[(k + 1) % 3]) == edge(a, b)).unwrap_or(0);
            let (u, v) = (t[k], t[(k + 1) % 3]);
            self.tris[i] = [u, m, c];
            self.tris.push([m, v, c]);
        }
        if self.fixed.remove(&edge(a, b)) {
            self.fixed.insert(edge(a, m));
            self.fixed.insert(edge(m, b));
        }
    }

    /// Inserts point m (inside the triangulation): splits the triangle holding it, or the edge
    /// it lies on.
    fn insert(&mut self, m: usize) -> Result<()> {
        for i in 0..self.tris.len() {
            let [a, b, c] = self.tris[i];
            let o = [self.orient(a, b, m), self.orient(b, c, m), self.orient(c, a, m)];
            if o.contains(&Sign::Negative) {
                continue;
            }
            match o.iter().filter(|s| s.is_zero()).count() {
                0 => {
                    self.tris[i] = [a, b, m];
                    self.tris.push([b, c, m]);
                    self.tris.push([c, a, m]);
                }
                1 => {
                    let k = o.iter().position(|s| s.is_zero()).unwrap_or(0);
                    let (u, v) = ([a, b, c][k], [a, b, c][(k + 1) % 3]);
                    self.split_edge(u, v, m);
                }
                _ => return Err(fail("a crossing point coincides with a vertex of the triangle it lies in")),
            }
            return Ok(());
        }
        Err(fail("a crossing point fell outside the triangle it should lie in"))
    }

    /// Flips edge (a, b) shared by two triangles when their union is strictly convex; returns
    /// whether it did.
    fn flip(&mut self, a: usize, b: usize) -> bool {
        let h = self.holding(a, b);
        let [(i, c), (j, d)] = h[..] else { return false };
        // The quad a, d, b, c (or its mirror) must be strictly convex: c and d on opposite
        // sides of ab, a and b on opposite sides of cd.
        if self.orient(c, d, a) == self.orient(c, d, b) || self.orient(c, d, a).is_zero() || self.orient(c, d, b).is_zero() {
            return false;
        }
        // Orient the new triangles counter-clockwise.
        let ccw = |p: &Plane, t: [usize; 3]| if p.orient(t[0], t[1], t[2]) == Sign::Negative { [t[0], t[2], t[1]] } else { t };
        self.tris[i] = ccw(self, [c, d, a]);
        self.tris[j] = ccw(self, [d, c, b]);
        true
    }

    /// Lawson's flips to the Delaunay triangulation, the fixed edges kept.
    fn delaunay(&mut self) {
        for _ in 0..50 {
            let mut flipped = false;
            let edges: Vec<[usize; 2]> = self.tris.iter().flat_map(|t| [edge(t[0], t[1]), edge(t[1], t[2]), edge(t[2], t[0])]).collect();
            for e in edges {
                if self.fixed.contains(&e) {
                    continue;
                }
                let h = self.holding(e[0], e[1]);
                let [(i, c), (_, d)] = h[..] else { continue };
                let t = self.tris[i];
                // d inside the circumcircle of the triangle (a, b, c) taken counter-clockwise.
                if emcraft_tetmesh::predicates::incircle(&self.p[t[0]], &self.p[t[1]], &self.p[t[2]], &self.p[d]) == Sign::Positive
                    && d != c
                    && self.flip(e[0], e[1])
                {
                    flipped = true;
                }
            }
            if !flipped {
                return;
            }
        }
    }

    /// Makes (u, v) an edge by flipping the edges that cross it (Sloan 1993), then fixes it.
    fn recover(&mut self, u: usize, v: usize) -> Result<()> {
        for _ in 0..10_000 {
            if !self.holding(u, v).is_empty() {
                self.fixed.insert(edge(u, v));
                return Ok(());
            }
            let crossing: Vec<[usize; 2]> = self
                .tris
                .iter()
                .flat_map(|t| [edge(t[0], t[1]), edge(t[1], t[2]), edge(t[2], t[0])])
                .filter(|e| !e.contains(&u) && !e.contains(&v))
                .filter(|e| {
                    let (a, b) = (e[0], e[1]);
                    let (s1, s2) = (self.orient(u, v, a), self.orient(u, v, b));
                    let (s3, s4) = (self.orient(a, b, u), self.orient(a, b, v));
                    s1 != s2 && !s1.is_zero() && !s2.is_zero() && s3 != s4 && !s3.is_zero() && !s4.is_zero()
                })
                .collect();
            if crossing.is_empty() {
                return Err(fail("an intersection segment passes through a vertex of the triangle it splits"));
            }
            if crossing.iter().any(|e| self.fixed.contains(e)) {
                return Err(fail("two intersection curves cross inside one triangle (a surface intersects itself)"));
            }
            let mut any = false;
            for e in crossing {
                if self.flip(e[0], e[1]) {
                    any = true;
                    break;
                }
            }
            if !any {
                return Err(fail("could not recover an intersection segment"));
            }
        }
        Err(fail("could not recover an intersection segment"))
    }
}

/// Triangle t of surface k split along its segments: the pieces, as global vertex ids.
fn split(s: &Surface, k: usize, t: u32, cut: &Cut, ids: &Ids, all: &[Point3]) -> Result<Vec<[u32; 3]>> {
    let corners = s.triangles[t as usize];
    let gc = corners.map(|v| ids.of(k, v));
    let segments = cut.segments[k].get(&t);
    let interior = cut.inside[k].get(&t);
    let edge_points: Vec<(usize, &Vec<u32>)> = (0..3)
        .filter_map(|i| cut.on_edge[k].get(&[corners[i].min(corners[(i + 1) % 3]), corners[i].max(corners[(i + 1) % 3])]).map(|p| (i, p)))
        .collect();
    if segments.is_none() && interior.is_none() && edge_points.is_empty() {
        return Ok(vec![gc]);
    }
    // Local 2D coordinates: drop the normal's largest component, keeping counter-clockwise.
    let q = gc.map(|v| all[v as usize]);
    let n = (q[1] - q[0]).cross(q[2] - q[0]);
    let axis = (0..3).max_by(|&a, &b| n[a].abs().total_cmp(&n[b].abs())).unwrap_or(2);
    let (mut u, mut w) = ((axis + 1) % 3, (axis + 2) % 3);
    if n[axis] < 0.0 {
        std::mem::swap(&mut u, &mut w);
    }
    let mut local: Vec<u32> = gc.to_vec();
    let mut index: HashMap<u32, usize> = gc.iter().enumerate().map(|(i, v)| (*v, i)).collect();
    let to2 = |p: Point3| ExactPoint2::new(emcraft_tetmesh::predicates::snap_to_domain(p[u]), emcraft_tetmesh::predicates::snap_to_domain(p[w]));
    let mut plane = Plane { p: Vec::new(), tris: vec![[0, 1, 2]], fixed: Default::default() };
    for v in &local {
        plane.p.push(to2(all[*v as usize]).map_err(|e| fail(e.to_string()))?);
    }
    if plane.orient(0, 1, 2) != Sign::Positive {
        return Err(fail("a degenerate triangle in a surface"));
    }
    for i in 0..3 {
        plane.fixed.insert(edge(i, (i + 1) % 3));
    }
    let add = |plane: &mut Plane, local: &mut Vec<u32>, index: &mut HashMap<u32, usize>, v: u32| -> Result<usize> {
        if let Some(&i) = index.get(&v) {
            return Ok(i);
        }
        local.push(v);
        plane.p.push(to2(all[v as usize]).map_err(|e| fail(e.to_string()))?);
        let i = local.len() - 1;
        index.insert(v, i);
        Ok(i)
    };
    // Points on the edges, split in along each edge from its start.
    for (i, pts) in edge_points {
        let (a, b) = (i, (i + 1) % 3);
        let (pa, pb) = (q[a], q[b]);
        let len2 = (pb - pa).dot(pb - pa);
        let mut sorted: Vec<(f64, u32)> = pts.iter().map(|&v| ((all[v as usize] - pa).dot(pb - pa) / len2, v)).collect();
        sorted.sort_by(|x, y| x.0.total_cmp(&y.0));
        let mut prev = a;
        for (_, v) in sorted {
            let m = add(&mut plane, &mut local, &mut index, v)?;
            plane.split_edge(prev, b, m);
            prev = m;
        }
    }
    for &v in interior.into_iter().flatten() {
        let m = add(&mut plane, &mut local, &mut index, v)?;
        plane.insert(m)?;
    }
    plane.delaunay();
    for seg in segments.into_iter().flatten() {
        let (a, b) = (add(&mut plane, &mut local, &mut index, seg[0])?, add(&mut plane, &mut local, &mut index, seg[1])?);
        if a != b {
            plane.recover(a, b)?;
        }
    }
    Ok(plane.tris.iter().map(|t| t.map(|i| local[i])).collect())
}

/// The winding number of a closed surface about p.
fn winding(s: &Surface, p: Point3) -> f64 {
    s.triangles.iter().map(|t| emcraft_model::solid_angle(t.map(|v| s.positions[v as usize] - p))).sum::<f64>() / (4.0 * std::f64::consts::PI)
}

/// A disjoint-set forest with path halving (union by index, so the result does not depend on
/// the order of the unions' calls beyond their sequence).
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

/// A piece of a surface's triangle split along the intersection curve: its corners (ids into
/// the combined points) and the triangle it comes from.
#[derive(Clone, Copy, Debug)]
struct Piece {
    corners: [u32; 3],
    triangle: u32,
}

/// Which pieces of surface `k` (0: a, 1: b) of `a op b` are kept: those inside the other surface
/// or those outside it; and whether they are turned inside out (b of a difference).
fn kept(op: Op, k: usize) -> (bool, bool) {
    // Keep pieces outside the other (union; a of a difference) or inside it (intersection;
    // b of a difference, turned inside out).
    match (op, k) {
        (Op::Union, _) => (false, false),
        (Op::Intersection, _) => (true, false),
        (Op::Difference, 0) => (false, false),
        (Op::Difference, _) => (true, true),
    }
}

/// The pieces of surface `k`'s triangles split along the intersection curve, the flat ones left
/// out.
fn pieces(s: &Surface, k: usize, cut: &Cut, ids: &Ids, all: &[Point3]) -> Result<Vec<Piece>> {
    let mut out = Vec::new();
    for t in 0..s.triangles.len() as u32 {
        for corners in split(s, k, t, cut, ids, all)? {
            let q = corners.map(|v| all[v as usize]);
            if (q[1] - q[0]).cross(q[2] - q[0]).norm() != 0.0 {
                out.push(Piece { corners, triangle: t });
            }
        }
    }
    Ok(out)
}

/// Whether each piece of surface `k` lies inside the `other` surface: one inside-or-outside test
/// per connected region of them (pieces that share an edge not on the intersection curve lie on
/// the same side of the other surface), each region decided at its largest piece's centroid (the
/// farthest from the curve).
fn inside(pieces: &[Piece], k: usize, cut: &Cut, other: &Surface, all: &[Point3]) -> Vec<bool> {
    let curve: std::collections::HashSet<[u32; 2]> = cut.segments[k].values().flatten().map(|e| [e[0].min(e[1]), e[0].max(e[1])]).collect();
    let mut sets = Sets((0..pieces.len()).collect());
    let mut by_edge: HashMap<[u32; 2], usize> = HashMap::new();
    for (i, p) in pieces.iter().enumerate() {
        let p = p.corners;
        for (a, b) in [(p[0], p[1]), (p[1], p[2]), (p[2], p[0])] {
            let e = [a.min(b), a.max(b)];
            if curve.contains(&e) {
                continue;
            }
            if let Some(&j) = by_edge.get(&e) {
                sets.union(i, j);
            } else {
                by_edge.insert(e, i);
            }
        }
    }
    let area = |p: &Piece| {
        let q = p.corners.map(|v| all[v as usize]);
        (q[1] - q[0]).cross(q[2] - q[0]).norm()
    };
    let mut best: HashMap<usize, usize> = HashMap::new();
    for i in 0..pieces.len() {
        let r = sets.find(i);
        let e = best.entry(r).or_insert(i);
        if area(&pieces[i]) > area(&pieces[*e]) {
            *e = i;
        }
    }
    let mut region_inside: HashMap<usize, bool> = HashMap::new();
    for (&r, &i) in &best {
        let centroid = Point3::centroid(pieces[i].corners.map(|v| all[v as usize])).unwrap_or(Point3::ORIGIN);
        region_inside.insert(r, winding(other, centroid) > 0.5);
    }
    (0..pieces.len()).map(|i| region_inside.get(&sets.find(i)).copied().unwrap_or(false)).collect()
}

/// Whether a surface is closed: every edge held once each way.
fn closed(s: &Surface) -> bool {
    let mut half: HashMap<[u32; 2], i32> = HashMap::new();
    for t in &s.triangles {
        for (i, j) in [(0, 1), (1, 2), (2, 0)] {
            *half.entry([t[i], t[j]]).or_default() += 1;
        }
    }
    !half.iter().any(|(e, n)| *n != 1 || half.get(&[e[1], e[0]]) != Some(&1))
}

/// `a op b`; b's tags are offset by `b_tags` in the result.
pub fn combine(a: &Surface, b: &Surface, op: Op, b_tags: u32) -> Result<Surface> {
    let (pa, pb) = (points(a)?, points(b)?);
    let ids = Ids { n0: a.positions.len() as u32, n1: b.positions.len() as u32 };
    let cut = intersect([a, b], [&pa, &pb], &ids)?;
    let all: Vec<Point3> = a.positions.iter().chain(&b.positions).chain(&cut.positions).copied().collect();
    let mut out = Surface::default();
    let mut used: HashMap<u32, u32> = HashMap::new();
    for (k, (s, other)) in [(a, b), (b, a)].into_iter().enumerate() {
        let (keep_inside, flip) = kept(op, k);
        let pieces = pieces(s, k, &cut, &ids, &all)?;
        let inside = inside(&pieces, k, &cut, other, &all);
        for (piece, _) in pieces.iter().zip(inside).filter(|(_, inside)| *inside == keep_inside) {
            let p = piece.corners;
            let corners = if flip { [p[0], p[2], p[1]] } else { p };
            let ids3 = corners.map(|v| {
                *used.entry(v).or_insert_with(|| {
                    out.positions.push(all[v as usize]);
                    (out.positions.len() - 1) as u32
                })
            });
            out.triangles.push(ids3);
            out.tags.push(s.tags[piece.triangle as usize] + if k == 1 { b_tags } else { 0 });
        }
    }
    if !closed(&out) {
        return Err(fail("the combined surface does not close (the bodies meet degenerately); move or resize a body slightly"));
    }
    if out.triangles.is_empty() {
        return Err(fail("nothing is left"));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A box [lo, hi] as 12 outward triangles, tag per face.
    fn cube(lo: [f64; 3], hi: [f64; 3]) -> Surface {
        let mut s = Surface::default();
        for i in 0..8 {
            s.positions.push(Point3::new(
                if i & 1 == 0 { lo[0] } else { hi[0] },
                if i & 2 == 0 { lo[1] } else { hi[1] },
                if i & 4 == 0 { lo[2] } else { hi[2] },
            ));
        }
        let quads = [[0, 4, 6, 2], [1, 3, 7, 5], [0, 1, 5, 4], [2, 6, 7, 3], [0, 2, 3, 1], [4, 5, 7, 6]];
        for (f, q) in quads.iter().enumerate() {
            s.triangles.push([q[0], q[1], q[2]]);
            s.triangles.push([q[0], q[2], q[3]]);
            s.tags.extend([f as u32; 2]);
        }
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

    #[test]
    fn boxes_combine_to_their_volumes() {
        let a = cube([0.0, 0.0, 0.0], [2.0, 2.0, 2.0]);
        // Overlapping in general position: [1.1, 3.3] × [0.7, 1.6] × [-0.4, 2.9].
        let b = cube([1.1, 0.7, -0.4], [3.3, 1.6, 2.9]);
        let (va, vb) = (8.0, 2.2 * 0.9 * 3.3);
        let overlap = 0.9 * 0.9 * 2.0;
        let u = combine(&a, &b, Op::Union, 10).unwrap();
        assert!((volume(&u) - (va + vb - overlap)).abs() < 1e-12, "{}", volume(&u));
        let d = combine(&a, &b, Op::Difference, 10).unwrap();
        assert!((volume(&d) - (va - overlap)).abs() < 1e-12, "{}", volume(&d));
        let i = combine(&a, &b, Op::Intersection, 10).unwrap();
        assert!((volume(&i) - overlap).abs() < 1e-12, "{}", volume(&i));
        // The tool's faces carry its tags, offset.
        assert!(d.tags.iter().any(|t| *t >= 10) && d.tags.iter().any(|t| *t < 10));
        // Apart: a union keeps both, an intersection nothing.
        let far = cube([5.0, 5.0, 5.0], [6.0, 6.0, 6.0]);
        assert!((volume(&combine(&a, &far, Op::Union, 10).unwrap()) - 9.0).abs() < 1e-12);
        assert!(combine(&a, &far, Op::Intersection, 10).is_err());
    }

    #[test]
    fn a_hole_through_a_box() {
        // A bar through the box, its ends outside: a square hole.
        let a = cube([0.0, 0.0, 0.0], [2.0, 2.0, 2.0]);
        let bar = cube([0.6, -0.5, 0.7], [1.3, 2.5, 1.5]);
        let d = combine(&a, &bar, Op::Difference, 10).unwrap();
        assert!((volume(&d) - (8.0 - 0.7 * 2.0 * 0.8)).abs() < 1e-12, "{}", volume(&d));
    }

    #[test]
    fn coplanar_faces_are_refused_not_guessed() {
        let a = cube([0.0, 0.0, 0.0], [2.0, 2.0, 2.0]);
        let flush = cube([0.5, 0.5, 0.0], [1.5, 1.5, 3.0]);
        match combine(&a, &flush, Op::Difference, 10) {
            Ok(s) => assert!((volume(&s) - (8.0 - 2.0)).abs() < 1e-9, "{}", volume(&s)),
            Err(e) => assert!(e.to_string().contains("degenerate") || e.to_string().contains("close"), "{e}"),
        }
    }
}
