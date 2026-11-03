//! Planar constrained Delaunay triangulation with Delaunay refinement to a size field, for the
//! common triangulation of a board's layers (adapted from FieldCraft's `quad/cdt.rs`, MIT OR
//! Apache-2.0: see ATTRIBUTION.md).
//!
//! - Insertion: Bowyer–Watson (Bowyer, Comput. J. 24(2), 1981; Watson, Comput. J. 24(2), 1981)
//!   inside a super-triangle, with exact `orient2d`/`incircle` (Shewchuk 1997). The cavity grows
//!   from the triangle containing the point and never crosses a constrained edge, which is the
//!   constrained Delaunay insertion of Shewchuk, *Delaunay refinement algorithms for triangular
//!   mesh generation*, CGTA 22:21–74, 2002, §2.
//! - Constraint recovery: Sloan, *A fast algorithm for generating constrained Delaunay
//!   triangulations*, Computers & Structures 47(3):441–450, 1993: flip the edges crossing a
//!   segment until it appears, then restore the Delaunay property with Lawson flips.
//! - Refinement: Ruppert, *A Delaunay refinement algorithm for quality 2-dimensional mesh
//!   generation*, J. Algorithms 18(3):548–585, 1995 (circumcentres of triangles too large for the
//!   size field or with a circumradius-to-shortest-edge ratio above √2), with one change: the
//!   segments arrive split to the size field already ([`super::split`]) and are never split here,
//!   so a circumcentre that falls outside the region, comes closer than a fraction of the local
//!   size to a vertex, or is refused by the caller's guard (clearance from the segments) is not
//!   inserted (a too-large triangle gets its centroid instead).
//!
//! The constraints are of two kinds: the region's outline, which separates the triangles kept
//! from those dropped, and the segments inside it (copper outlines), which only stay edges.
//! Every loop is bounded (walk steps, cavity rounds, flips, refinement passes and the triangle
//! count), and a cavity is checked before it is retriangulated: the new triangles all have
//! positive exact orientation and no constrained edge is removed.

use std::collections::VecDeque;

use emcraft_tetmesh::predicates::{ExactPoint2 as Point2, Sign, incircle, orient2d};

use crate::{MeshError, Result};

const NONE: u32 = u32::MAX;
/// Rounds of cavity shrinking before an insertion is rejected.
const MAX_CAVITY_ROUNDS: usize = 32;
/// Refinement passes over the triangles.
const MAX_REFINE_PASSES: usize = 64;
/// Steiner points keep this fraction of the local size from vertices (and, through the caller's
/// guard, from the segments).
pub(super) const CLEARANCE: f64 = 0.3;

/// What an edge is: free, a segment inside the region, or the region's outline.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Kind {
    #[default]
    Free,
    Segment,
    Outline,
}

impl Kind {
    fn fixed(self) -> bool {
        self != Kind::Free
    }
}

#[derive(Debug, Clone, Copy)]
struct Tri {
    /// Counterclockwise vertices.
    v: [u32; 3],
    /// `n[i]`: the neighbour across the edge opposite `v[i]` (from `v[i+1]` to `v[i+2]`).
    n: [u32; 3],
    /// What the edge opposite `v[i]` is.
    kind: [Kind; 3],
    inside: bool,
    alive: bool,
}

/// The triangles of the region: counterclockwise vertex triples into `points`.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Triangulation {
    pub points: Vec<[f64; 2]>,
    pub triangles: Vec<[u32; 3]>,
}

pub(super) struct Cdt {
    pts: Vec<[f64; 2]>,
    p2: Vec<Point2>,
    tris: Vec<Tri>,
    free: Vec<u32>,
    vt: Vec<u32>,
    last: u32,
    mark: Vec<u32>,
    epoch: u32,
    max_tris: usize,
    /// The largest coordinate magnitude a Steiner point may have.
    reach: f64,
}

fn fail(why: &str) -> MeshError {
    MeshError::invalid("planar triangulation", why)
}

/// The edge index `i` of `tri` that runs from `a` to `b` (opposite `v[i]`).
fn edge_index(tri: &Tri, a: u32, b: u32) -> Option<usize> {
    (0..3).find(|&i| tri.v[(i + 1) % 3] == a && tri.v[(i + 2) % 3] == b)
}

/// Every edge of `tri` keeps `p` on its left or on it (exact).
fn contains(tri: &Tri, p2: &[Point2], p: &Point2) -> bool {
    (0..3).all(|i| match (p2.get(tri.v[(i + 1) % 3] as usize), p2.get(tri.v[(i + 2) % 3] as usize)) {
        (Some(a), Some(b)) => orient2d(a, b, p) != Sign::Negative,
        _ => false,
    })
}

/// A Morton key of `p` in the box `lo`..`lo + span` (16 bits per axis), for insertion orders
/// with short walks.
fn morton(p: [f64; 2], lo: [f64; 2], span: [f64; 2]) -> u64 {
    let q = |x: f64, l: f64, s: f64| ((((x - l) / s) * 65535.0) as u32).min(65535);
    let (mut x, mut y) = (q(p[0], lo[0], span[0]), q(p[1], lo[1], span[1]));
    let mut k = 0u64;
    for bit in 0..16 {
        k |= u64::from(x & 1) << (2 * bit) | u64::from(y & 1) << (2 * bit + 1);
        x >>= 1;
        y >>= 1;
    }
    k
}

impl Cdt {
    fn t(&self, t: u32) -> Result<&Tri> {
        self.tris.get(t as usize).ok_or_else(|| fail("triangle index out of range"))
    }

    fn tm(&mut self, t: u32) -> Result<&mut Tri> {
        self.tris.get_mut(t as usize).ok_or_else(|| fail("triangle index out of range"))
    }

    fn p(&self, v: u32) -> Result<&Point2> {
        self.p2.get(v as usize).ok_or_else(|| fail("vertex index out of range"))
    }

    fn xy(&self, v: u32) -> Result<[f64; 2]> {
        self.pts.get(v as usize).copied().ok_or_else(|| fail("vertex index out of range"))
    }

    /// Triangulates `points` (distinct, their ids kept) and recovers `segments` as constrained
    /// edges of their kind; the triangles enclosed by the [`Kind::Outline`] segments are inside.
    /// The segments must not cross or pass through a point ([`super::split`] makes them so).
    pub fn build(points: &[Point2], segments: &[([u32; 2], Kind)], max_tris: usize) -> Result<Cdt> {
        let n = points.len();
        let pts: Vec<[f64; 2]> = points.iter().map(Point2::coords).collect();
        let (lo, hi) = pts.iter().fold(([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]), |(lo, hi), p| {
            ([lo[0].min(p[0]), lo[1].min(p[1])], [hi[0].max(p[0]), hi[1].max(p[1])])
        });
        if n < 3 || !(lo[0] <= hi[0] && lo[1] <= hi[1]) {
            return Err(fail("fewer than three points"));
        }
        // A super-triangle far around the points, its corners whole numbers (lattice points).
        let c = [(0.5 * (lo[0] + hi[0])).round(), (0.5 * (lo[1] + hi[1])).round()];
        let d = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1.0).ceil();
        let corners = [[c[0] - 20.0 * d, c[1] - 10.0 * d], [c[0] + 20.0 * d, c[1] - 10.0 * d], [c[0], c[1] + 20.0 * d]];
        let mut cdt = Cdt {
            vt: vec![NONE; n + 3],
            pts,
            p2: points.to_vec(),
            tris: vec![Tri { v: [n as u32, n as u32 + 1, n as u32 + 2], n: [NONE; 3], kind: [Kind::Free; 3], inside: false, alive: true }],
            free: Vec::new(),
            last: 0,
            mark: vec![0],
            epoch: 0,
            max_tris,
            reach: hi[0].abs().max(hi[1].abs()).max(lo[0].abs()).max(lo[1].abs()),
        };
        for k in corners {
            cdt.p2.push(Point2::new(k[0], k[1]).map_err(|e| fail(&e.to_string()))?);
            cdt.pts.push(k);
        }
        for v in n..n + 3 {
            if let Some(x) = cdt.vt.get_mut(v) {
                *x = 0;
            }
        }
        // Insert in Morton order (short walks), ties by id.
        let span = [(hi[0] - lo[0]).max(f64::MIN_POSITIVE), (hi[1] - lo[1]).max(f64::MIN_POSITIVE)];
        let mut order: Vec<(u64, u32)> = (0..n).map(|i| (morton(cdt.pts[i], lo, span), i as u32)).collect();
        order.sort_unstable();
        for (_, v) in order {
            if !cdt.insert(v, None)? {
                return Err(fail("a point could not be inserted"));
            }
        }
        for &([a, b], kind) in segments {
            cdt.recover(a, b, kind)?;
        }
        cdt.classify()?;
        #[cfg(test)]
        assert_eq!(cdt.verify(), Ok(()), "after build");
        Ok(cdt)
    }

    fn alloc(&mut self, tri: Tri) -> Result<u32> {
        if let Some(t) = self.free.pop() {
            *self.tm(t)? = tri;
            return Ok(t);
        }
        if self.tris.len() >= self.max_tris {
            return Err(fail(&format!("the triangulation exceeds {} triangles: use a larger element size or a smaller region", self.max_tris)));
        }
        self.tris.push(tri);
        self.mark.push(0);
        Ok(self.tris.len() as u32 - 1)
    }

    /// The triangle containing `p` (on its boundary counts), by a visibility walk from `start`
    /// with a linear scan as the fallback.
    fn locate(&self, p: &Point2, start: u32) -> Result<u32> {
        let mut t = if self.t(start).is_ok_and(|x| x.alive) { start } else { self.any_alive()? };
        let cap = 64 + 4 * self.tris.len();
        'walk: for step in 0..cap {
            let tri = *self.t(t)?;
            for k in 0..3 {
                let i = (k + step) % 3;
                let (a, b) = (tri.v[(i + 1) % 3], tri.v[(i + 2) % 3]);
                if orient2d(self.p(a)?, self.p(b)?, p) == Sign::Negative {
                    let next = tri.n[i];
                    if next == NONE {
                        return Err(fail("a point outside the super-triangle"));
                    }
                    t = next;
                    continue 'walk;
                }
            }
            return Ok(t);
        }
        self.tris.iter().position(|tri| tri.alive && contains(tri, &self.p2, p)).map(|i| i as u32).ok_or_else(|| fail("point location failed"))
    }

    fn any_alive(&self) -> Result<u32> {
        self.tris.iter().position(|t| t.alive).map(|i| i as u32).ok_or_else(|| fail("no triangles"))
    }

    /// Inserts vertex `v` (already in `pts`). Input points (`steiner == None`) must go in.
    /// Steiner points (`Some(min_distance)`) may be rejected (outside the region, closer than
    /// `min_distance` to a vertex, or no valid cavity), leaving the triangulation unchanged;
    /// returns whether the point went in.
    fn insert(&mut self, v: u32, steiner: Option<f64>) -> Result<bool> {
        let p = *self.p(v)?;
        let t0 = self.locate(&p, self.last)?;
        let tri0 = *self.t(t0)?;
        for &u in &tri0.v {
            if self.p(u)?.coords() == p.coords() {
                return match steiner {
                    Some(_) => Ok(false),
                    None => Err(fail("a repeated point")),
                };
            }
        }
        if steiner.is_some() && !tri0.inside {
            return Ok(false);
        }
        let mut excluded: Vec<u32> = Vec::new();
        for _ in 0..MAX_CAVITY_ROUNDS {
            let (cavity, edges) = self.cavity(t0, &p, &excluded)?;
            let mut bad: Vec<u32> = Vec::new();
            // A constrained edge inside the cavity: drop the triangle on the far side from p.
            for &t in &cavity {
                let tri = *self.t(t)?;
                for i in 0..3 {
                    if tri.kind[i].fixed() && tri.n[i] != NONE && self.marked(tri.n[i]) {
                        let (a, b) = (tri.v[(i + 1) % 3], tri.v[(i + 2) % 3]);
                        if orient2d(self.p(a)?, self.p(b)?, &p) != Sign::Positive {
                            bad.push(t);
                        }
                    }
                }
            }
            // Every new triangle (a, b, p) must be counterclockwise.
            for &(a, b, _, _, owner) in &edges {
                if orient2d(self.p(a)?, self.p(b)?, &p) != Sign::Positive {
                    bad.push(owner);
                }
            }
            if let Some(min_distance) = steiner {
                let pc = p.coords();
                for &(a, _, _, _, _) in &edges {
                    let pa = self.xy(a)?;
                    if (pa[0] - pc[0]).powi(2) + (pa[1] - pc[1]).powi(2) < min_distance * min_distance {
                        return Ok(false);
                    }
                }
            }
            if bad.is_empty() && edges.len() == cavity.len() + 2 {
                self.retriangulate(v, &cavity, &edges, tri0.inside)?;
                return Ok(true);
            }
            if bad.contains(&t0) || bad.is_empty() {
                break;
            }
            excluded.extend(bad);
            excluded.sort_unstable();
            excluded.dedup();
        }
        match steiner {
            Some(_) => Ok(false),
            None => Err(fail("no valid cavity for an input point")),
        }
    }

    fn marked(&self, t: u32) -> bool {
        self.mark.get(t as usize).is_some_and(|&m| m == self.epoch)
    }

    /// The Bowyer–Watson cavity of `p` from `t0` (never crossing constrained edges, skipping
    /// `excluded`), marked with the current epoch, and its boundary edges `(a, b, outside
    /// neighbour, kind, owner)` with the cavity on their left.
    #[allow(clippy::type_complexity)]
    fn cavity(&mut self, t0: u32, p: &Point2, excluded: &[u32]) -> Result<(Vec<u32>, Vec<(u32, u32, u32, Kind, u32)>)> {
        self.epoch = self.epoch.wrapping_add(1);
        if self.epoch == 0 {
            self.mark.iter_mut().for_each(|m| *m = 0);
            self.epoch = 1;
        }
        let epoch = self.epoch;
        let set = |mark: &mut Vec<u32>, t: u32| {
            if let Some(m) = mark.get_mut(t as usize) {
                *m = epoch;
            }
        };
        set(&mut self.mark, t0);
        let mut cavity = vec![t0];
        let mut k = 0;
        while let Some(&t) = cavity.get(k) {
            k += 1;
            let tri = *self.t(t)?;
            for i in 0..3 {
                let nb = tri.n[i];
                if nb == NONE || tri.kind[i].fixed() || self.marked(nb) || excluded.binary_search(&nb).is_ok() {
                    continue;
                }
                let o = *self.t(nb)?;
                if incircle(self.p(o.v[0])?, self.p(o.v[1])?, self.p(o.v[2])?, p) == Sign::Positive {
                    set(&mut self.mark, nb);
                    cavity.push(nb);
                }
            }
        }
        let mut edges = Vec::new();
        for &t in &cavity {
            let tri = *self.t(t)?;
            for i in 0..3 {
                let nb = tri.n[i];
                // Shared with another cavity triangle (a constrained one is reported by `insert`).
                if nb != NONE && self.marked(nb) {
                    continue;
                }
                edges.push((tri.v[(i + 1) % 3], tri.v[(i + 2) % 3], nb, tri.kind[i], t));
            }
        }
        Ok((cavity, edges))
    }

    fn retriangulate(&mut self, v: u32, cavity: &[u32], edges: &[(u32, u32, u32, Kind, u32)], inside: bool) -> Result<()> {
        for &t in cavity {
            self.tm(t)?.alive = false;
            self.free.push(t);
        }
        // (a, b, triangle)
        let mut made: Vec<(u32, u32, u32)> = Vec::with_capacity(edges.len());
        for &(a, b, nb, kind, _) in edges {
            let t = self.alloc(Tri { v: [a, b, v], n: [NONE, NONE, nb], kind: [Kind::Free, Kind::Free, kind], inside, alive: true })?;
            made.push((a, b, t));
            if nb != NONE {
                self.relink(nb, a, b, t)?;
            }
        }
        let mut by_a: Vec<(u32, u32)> = made.iter().map(|&(a, _, t)| (a, t)).collect();
        let mut by_b: Vec<(u32, u32)> = made.iter().map(|&(_, b, t)| (b, t)).collect();
        by_a.sort_unstable();
        by_b.sort_unstable();
        let find = |list: &[(u32, u32)], key: u32| -> Result<u32> {
            let i = list.binary_search_by_key(&key, |e| e.0).map_err(|_| fail("a cavity's boundary is not a closed loop"))?;
            list.get(i).map(|e| e.1).ok_or_else(|| fail("cavity lookup"))
        };
        for &(a, b, t) in &made {
            // The triangle (b, c, v) shares edge b–v; the triangle (z, a, v) shares edge v–a.
            let next = find(&by_a, b)?;
            let prev = find(&by_b, a)?;
            let tri = self.tm(t)?;
            tri.n[0] = next;
            tri.n[1] = prev;
            for u in [a, b, v] {
                if let Some(x) = self.vt.get_mut(u as usize) {
                    *x = t;
                }
            }
            self.last = t;
        }
        Ok(())
    }

    /// In triangle `t`, points the edge from `b` to `a` at `to`.
    fn relink(&mut self, t: u32, a: u32, b: u32, to: u32) -> Result<()> {
        let tri = self.tm(t)?;
        let i = edge_index(tri, b, a).ok_or_else(|| fail("a neighbour does not share the edge"))?;
        tri.n[i] = to;
        Ok(())
    }

    /// Visits the triangles around vertex `v` counterclockwise, each with `v`'s local index,
    /// until `visit` returns a value (or the rotation closes: `None`).
    fn around<R>(&self, v: u32, mut visit: impl FnMut(u32, &Tri, usize) -> Result<Option<R>>) -> Result<Option<R>> {
        let start = self.vt.get(v as usize).copied().ok_or_else(|| fail("vertex"))?;
        let mut t = start;
        for _ in 0..self.tris.len() + 1 {
            let tri = self.t(t)?;
            let k = tri.v.iter().position(|&x| x == v).ok_or_else(|| fail("the vertex map is stale"))?;
            if let Some(r) = visit(t, tri, k)? {
                return Ok(Some(r));
            }
            t = tri.n[(k + 1) % 3];
            if t == NONE || t == start {
                return Ok(None);
            }
        }
        Err(fail("a vertex rotation did not close"))
    }

    /// The triangle and local index of the directed edge `a → b` (the edge opposite `v[i]` runs
    /// from `v[i+1]` to `v[i+2]`).
    fn find_edge(&self, a: u32, b: u32) -> Result<Option<(u32, usize)>> {
        self.around(a, |t, tri, k| Ok((tri.v[(k + 1) % 3] == b).then_some((t, (k + 2) % 3))))
    }

    /// Flips the edge opposite `v[i]` of `t`; returns the new edge's end points.
    fn flip(&mut self, t: u32, i: usize) -> Result<(u32, u32)> {
        let a = *self.t(t)?;
        let (p, q, r) = (a.v[i], a.v[(i + 1) % 3], a.v[(i + 2) % 3]);
        let t2 = a.n[i];
        let b = *self.t(t2)?;
        let j = edge_index(&b, r, q).ok_or_else(|| fail("flip: neighbour mismatch"))?;
        let s = b.v[j];
        let (n_pq, k_pq) = (a.n[(i + 2) % 3], a.kind[(i + 2) % 3]);
        let (n_rp, k_rp) = (a.n[(i + 1) % 3], a.kind[(i + 1) % 3]);
        let (n_sr, k_sr) = (b.n[(j + 2) % 3], b.kind[(j + 2) % 3]);
        let (n_qs, k_qs) = (b.n[(j + 1) % 3], b.kind[(j + 1) % 3]);
        *self.tm(t)? = Tri { v: [p, q, s], n: [n_qs, t2, n_pq], kind: [k_qs, Kind::Free, k_pq], inside: a.inside, alive: true };
        *self.tm(t2)? = Tri { v: [p, s, r], n: [n_sr, n_rp, t], kind: [k_sr, k_rp, Kind::Free], inside: b.inside, alive: true };
        if n_qs != NONE {
            self.relink(n_qs, q, s, t)?;
        }
        if n_rp != NONE {
            self.relink(n_rp, r, p, t2)?;
        }
        for (u, x) in [(p, t), (q, t), (s, t2), (r, t2)] {
            if let Some(y) = self.vt.get_mut(u as usize) {
                *y = x;
            }
        }
        Ok((p, s))
    }

    /// The edge opposite `v[i]` of `t` with its far vertex, when it is flippable (strictly convex
    /// quadrilateral).
    fn flippable(&self, t: u32, i: usize) -> Result<Option<u32>> {
        let a = *self.t(t)?;
        let t2 = a.n[i];
        if t2 == NONE || a.kind[i].fixed() {
            return Ok(None);
        }
        let (p, q, r) = (a.v[i], a.v[(i + 1) % 3], a.v[(i + 2) % 3]);
        let b = *self.t(t2)?;
        let s = b.v[edge_index(&b, r, q).ok_or_else(|| fail("neighbour mismatch"))?];
        let (pp, ps) = (self.p(p)?, self.p(s)?);
        Ok((orient2d(pp, ps, self.p(q)?) * orient2d(pp, ps, self.p(r)?) == Sign::Negative).then_some(s))
    }

    /// Makes `a–b` an edge (Sloan 1993) and marks it with `kind`.
    fn recover(&mut self, a: u32, b: u32, kind: Kind) -> Result<()> {
        if self.find_edge(a, b)?.is_none() {
            let crossing = self.crossing_edges(a, b)?;
            let (pa, pb) = (*self.p(a)?, *self.p(b)?);
            let mut queue: VecDeque<(u32, u32)> = crossing.into_iter().collect();
            let cap = 1000 + 16 * queue.len() * queue.len();
            let mut fresh = Vec::new();
            let mut steps = 0usize;
            while let Some((u, w)) = queue.pop_front() {
                steps += 1;
                if steps > cap {
                    return Err(fail(&format!("recovering a segment did not finish in {cap} flips")));
                }
                let (t, i) = self.find_edge(u, w)?.ok_or_else(|| fail("a crossing edge vanished"))?;
                if self.t(t)?.kind[i].fixed() {
                    return Err(fail("two segments cross"));
                }
                match self.flippable(t, i)? {
                    Some(_) => {
                        let (x, y) = self.flip(t, i)?;
                        let crosses =
                            x != a && x != b && y != a && y != b && orient2d(&pa, &pb, self.p(x)?) * orient2d(&pa, &pb, self.p(y)?) == Sign::Negative;
                        if crosses { queue.push_back((x, y)) } else { fresh.push((x, y)) }
                    }
                    None => queue.push_back((u, w)),
                }
            }
            self.mark(a, b, kind)?;
            return self.lawson(fresh);
        }
        self.mark(a, b, kind)
    }

    /// Marks edge `a–b` (on both sides) with `kind`; an outline wins over a segment.
    fn mark(&mut self, a: u32, b: u32, kind: Kind) -> Result<()> {
        let (t, i) = self.find_edge(a, b)?.ok_or_else(|| fail("a recovered edge is missing"))?;
        let nb = self.t(t)?.n[i];
        let k = &mut self.tm(t)?.kind[i];
        *k = if *k == Kind::Outline { Kind::Outline } else { kind };
        if nb != NONE {
            let tri = self.tm(nb)?;
            if let Some(j) = edge_index(tri, b, a) {
                tri.kind[j] = if tri.kind[j] == Kind::Outline { Kind::Outline } else { kind };
            }
        }
        Ok(())
    }

    /// The edges crossed by the open segment `a–b`, from `a` to `b`.
    fn crossing_edges(&self, a: u32, b: u32) -> Result<Vec<(u32, u32)>> {
        let (pa, pb) = (*self.p(a)?, *self.p(b)?);
        let on_segment = || fail("a point lies on a segment");
        let found = self.around(a, |t, tri, k| {
            let (u, w) = (tri.v[(k + 1) % 3], tri.v[(k + 2) % 3]);
            let (ou, ow) = (orient2d(&pa, self.p(u)?, &pb), orient2d(&pa, self.p(w)?, &pb));
            if ou.is_zero() || ow.is_zero() {
                let x = if ou.is_zero() { u } else { w };
                let px = self.xy(x)?;
                let (qa, qb) = (pa.coords(), pb.coords());
                if (px[0] - qa[0]) * (qb[0] - qa[0]) + (px[1] - qa[1]) * (qb[1] - qa[1]) > 0.0 {
                    return Err(on_segment());
                }
            }
            Ok((ou == Sign::Positive && ow == Sign::Negative).then_some((t, u, w)))
        })?;
        let (mut t, mut right, mut left) = found.ok_or_else(|| fail("a segment's start was not found"))?;
        let mut out = vec![(right, left)];
        for _ in 0..self.tris.len() + 1 {
            let tri = *self.t(t)?;
            let i = tri.v.iter().position(|&x| x != right && x != left).ok_or_else(|| fail("walk"))?;
            let next = tri.n[i];
            let nt = *self.t(next)?;
            let d = *nt.v.iter().find(|&&x| x != right && x != left).ok_or_else(|| fail("walk"))?;
            if d == b {
                return Ok(out);
            }
            match orient2d(&pa, &pb, self.p(d)?) {
                Sign::Positive => left = d,
                Sign::Negative => right = d,
                Sign::Zero => return Err(on_segment()),
            }
            out.push((right, left));
            t = next;
        }
        Err(fail("a segment's walk did not reach its end"))
    }

    /// Lawson flips from the given edges until every unconstrained edge is locally Delaunay.
    fn lawson(&mut self, edges: Vec<(u32, u32)>) -> Result<()> {
        let mut stack = edges;
        let cap = 1000 + 64 * self.tris.len();
        let mut steps = 0usize;
        while let Some((u, w)) = stack.pop() {
            steps += 1;
            if steps > cap {
                return Err(fail("restoring the Delaunay property did not finish"));
            }
            let Some((t, i)) = self.find_edge(u, w)? else { continue };
            let tri = *self.t(t)?;
            if tri.kind[i].fixed() || tri.n[i] == NONE {
                continue;
            }
            let Some(s) = self.flippable(t, i)? else { continue };
            let [a, b, c] = tri.v;
            if incircle(self.p(a)?, self.p(b)?, self.p(c)?, self.p(s)?) != Sign::Positive {
                continue;
            }
            let (p, _) = self.flip(t, i)?;
            let (q, r) = (tri.v[(i + 1) % 3], tri.v[(i + 2) % 3]);
            stack.extend([(p, q), (q, s), (s, r), (r, p)]);
        }
        Ok(())
    }

    /// Marks the triangles inside the region: those not reached from the super-triangle's
    /// corner without crossing the outline.
    fn classify(&mut self) -> Result<()> {
        let n = self.tris.len();
        let mut outside = vec![false; n];
        let start = self.vt.get(self.pts.len().saturating_sub(1)).copied().ok_or_else(|| fail("super vertex"))?;
        let mut stack = vec![start];
        if let Some(o) = outside.get_mut(start as usize) {
            *o = true;
        }
        while let Some(t) = stack.pop() {
            let tri = *self.t(t)?;
            for i in 0..3 {
                let nb = tri.n[i];
                if nb == NONE || tri.kind[i] == Kind::Outline || outside.get(nb as usize).is_none_or(|&o| o) {
                    continue;
                }
                if let Some(o) = outside.get_mut(nb as usize) {
                    *o = true;
                }
                stack.push(nb);
            }
        }
        for (tri, o) in self.tris.iter_mut().zip(outside) {
            tri.inside = tri.alive && !o;
        }
        Ok(())
    }

    /// Tries to insert Steiner point `p` (located from `hint`), at least `min_distance` from the
    /// vertices of its cavity.
    fn try_steiner(&mut self, p: Point2, hint: u32, min_distance: f64) -> Result<bool> {
        let v = self.pts.len() as u32;
        self.pts.push(p.coords());
        self.p2.push(p);
        self.vt.push(NONE);
        self.last = hint;
        let ok = self.insert(v, Some(min_distance))?;
        if !ok {
            self.pts.pop();
            self.p2.pop();
            self.vt.pop();
        }
        Ok(ok)
    }

    /// Delaunay refinement of the inside triangles to the size field `size`. A Steiner point `p`
    /// for local size `h` is skipped when `allows(p, h)` refuses it (the caller's clearance from
    /// the segments) or when it comes closer than [`CLEARANCE`]` × h` to a vertex.
    pub fn refine(&mut self, size: &dyn Fn([f64; 2]) -> f64, allows: &dyn Fn([f64; 2], f64) -> bool) -> Result<()> {
        for _ in 0..MAX_REFINE_PASSES {
            let mut inserted = 0usize;
            let n = self.tris.len();
            for t in 0..n as u32 {
                let tri = *self.t(t)?;
                if !tri.alive || !tri.inside {
                    continue;
                }
                let [a, b, c] = [self.xy(tri.v[0])?, self.xy(tri.v[1])?, self.xy(tri.v[2])?];
                let centroid = [(a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0];
                let h = size(centroid);
                if !(h.is_finite() && h > 0.0) {
                    return Err(fail("the element size is not a positive number"));
                }
                let (bx, by, cx, cy) = (b[0] - a[0], b[1] - a[1], c[0] - a[0], c[1] - a[1]);
                let d = 2.0 * (bx * cy - by * cx);
                let (b2, c2) = (bx * bx + by * by, cx * cx + cy * cy);
                let (ux, uy) = ((cy * b2 - by * c2) / d, (bx * c2 - cx * b2) / d);
                let r = (ux * ux + uy * uy).sqrt();
                let shortest = b2.min(c2).min((cx - bx).powi(2) + (cy - by).powi(2)).sqrt();
                if !r.is_finite() || !(d > 0.0) {
                    continue;
                }
                let too_big = r > 0.75 * h;
                let poor = r > std::f64::consts::SQRT_2 * shortest;
                if !too_big && !poor {
                    continue;
                }
                let candidates = [Some([a[0] + ux, a[1] + uy]), too_big.then_some(centroid)];
                for c in candidates.into_iter().flatten() {
                    if let Some(p) = self.steiner_point(c).filter(|p| allows(p.coords(), h))
                        && self.try_steiner(p, t, CLEARANCE * h)?
                    {
                        inserted += 1;
                        break;
                    }
                }
            }
            if inserted == 0 {
                break;
            }
        }
        #[cfg(test)]
        assert_eq!(self.verify(), Ok(()), "after refinement");
        Ok(())
    }

    /// A constructed point, rounded to the lattice (whole numbers), or `None` when it is outside
    /// the points' reach.
    fn steiner_point(&self, c: [f64; 2]) -> Option<Point2> {
        let c = [c[0].round(), c[1].round()];
        if !(c[0].abs() <= self.reach && c[1].abs() <= self.reach) {
            return None;
        }
        Point2::new(c[0], c[1]).ok()
    }

    /// Consistency check for tests: every live triangle counterclockwise (exact), neighbours
    /// mutual, kinds matching on both sides, free edges locally Delaunay.
    #[cfg(test)]
    pub fn verify(&self) -> std::result::Result<(), String> {
        for (t, tri) in self.tris.iter().enumerate() {
            if !tri.alive {
                continue;
            }
            let [a, b, c] = tri.v;
            let o = orient2d(&self.p2[a as usize], &self.p2[b as usize], &self.p2[c as usize]);
            if o != Sign::Positive {
                return Err(format!("triangle {t} {:?} has orientation {o:?} (inside {})", tri.v, tri.inside));
            }
            for i in 0..3 {
                let nb = tri.n[i];
                if nb == NONE {
                    continue;
                }
                let o = &self.tris[nb as usize];
                let (p, q) = (tri.v[(i + 1) % 3], tri.v[(i + 2) % 3]);
                let back = (0..3).find(|&j| o.v[(j + 1) % 3] == q && o.v[(j + 2) % 3] == p);
                match back {
                    Some(j) if o.n[j] == t as u32 && o.alive && o.kind[j] == tri.kind[i] => {
                        let s = o.v[j];
                        if !tri.kind[i].fixed()
                            && incircle(&self.p2[a as usize], &self.p2[b as usize], &self.p2[c as usize], &self.p2[s as usize]) == Sign::Positive
                        {
                            return Err(format!("triangle {t} edge {i} is not locally Delaunay (inside {})", tri.inside));
                        }
                    }
                    _ => return Err(format!("triangle {t} edge {i}: neighbour {nb} does not match")),
                }
            }
        }
        Ok(())
    }

    /// The inside triangles. The points keep their ids: the input points, the super-triangle's
    /// three corners (used by no triangle), then the Steiner points.
    pub fn into_triangulation(self) -> Triangulation {
        let triangles = self.tris.iter().filter(|t| t.alive && t.inside).map(|t| t.v).collect();
        Triangulation { points: self.pts, triangles }
    }
}
