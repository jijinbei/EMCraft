//! Triangle mesh simplification, for drawing a large scene coarser while it moves (display
//! levels of detail). Display only: picking, measuring and every still picture read the full
//! scene.
//!
//! Edge collapse ordered by quadric error (Garland and Heckbert, "Surface Simplification Using
//! Quadric Error Metrics", SIGGRAPH 1997): each vertex carries the sum of the squared-distance
//! quadrics of the planes of its faces; the edge whose two ends merge at the point of least
//! summed squared distance goes first. A collapse that would fold a face over (its normal turning
//! by more than about 80°) or join two sheets of the surface (the ends sharing more neighbours
//! than the faces between them) is skipped. Open boundaries carry extra planes across them, so
//! they keep their place.
//!
//! [`levels`] simplifies one mesh; [`coarser`] a whole [`Scene`]: its triangles in groups of one
//! colour, opacity and tag (each group a mesh of its own, so colours never bleed and a group's
//! outline, where it meets the next, stays put), each brought down by the same share.
//!
//! Deterministic: welding by exact position, the cheapest collapse first with ties broken by its
//! ends, groups in the order they first appear. The work is bounded by the mesh: each collapse
//! removes a vertex, and only a collapse queues new candidates.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BinaryHeap};

use super::{MAX_PRIMITIVES, Scene, Tri};

/// Most triangles [`levels`] simplifies (more: no levels).
pub const MAX_TRIANGLES: usize = MAX_PRIMITIVES;
/// Groups of fewer triangles than this are kept as they are by [`coarser`].
const SMALL_GROUP: usize = 64;

/// How much more an open boundary's planes count than a face's.
const BOUNDARY_WEIGHT: f64 = 10.0;
/// A collapse may turn a face's normal by at most this (cosine): about 80°.
const MIN_TURN_COS: f64 = 0.2;

/// A simplified copy of a mesh.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Level {
    pub positions: Vec<[f64; 3]>,
    pub triangles: Vec<[u32; 3]>,
}

/// The mesh (`positions`, and `triangles` indexing them; corners at the same position are one
/// vertex) simplified to each of `targets` triangles in turn (largest first), one [`Level`] per
/// target reached. Simplification stops early when no edge can collapse any more; the levels made
/// by then are returned, but none that is not at least a fifth smaller than the one before it.
/// Triangles with an index out of range or a point not finite are left out; an empty or oversized
/// mesh ([`MAX_TRIANGLES`]) has no levels.
///
/// ```
/// use emcraft_render::view3d::simplify::levels;
/// // A flat 20 × 20 grid of squares, two triangles each (m).
/// let at = |i: u32, j: u32| [f64::from(i) * 1e-3, f64::from(j) * 1e-3, 0.0];
/// let (mut pos, mut tris) = (Vec::new(), Vec::new());
/// for i in 0..20 {
///     for j in 0..20 {
///         for t in [[at(i, j), at(i + 1, j), at(i + 1, j + 1)], [at(i, j), at(i + 1, j + 1), at(i, j + 1)]] {
///             tris.push([0, 1, 2].map(|k| pos.len() as u32 + k));
///             pos.extend(t);
///         }
///     }
/// }
/// let made = levels(&pos, &tris, &[200, 50]);
/// assert_eq!(made.len(), 2);
/// assert!(made[1].triangles.len() <= 50);
/// // Still flat, still the same square.
/// assert!(made[1].positions.iter().all(|p| p[2] == 0.0 && p[0] <= 0.02 + 1e-12));
/// // Hostile input: no levels, no panic.
/// assert!(levels(&[[f64::NAN; 3]; 3], &[[0, 1, 2]], &[0]).is_empty());
/// assert!(levels(&pos, &[[0, 1, u32::MAX]], &[0]).is_empty());
/// ```
pub fn levels(positions: &[[f64; 3]], triangles: &[[u32; 3]], targets: &[usize]) -> Vec<Level> {
    if triangles.is_empty() || triangles.len() > MAX_TRIANGLES {
        return Vec::new();
    }
    let Some(mut m) = Collapser::new(positions, triangles) else { return Vec::new() };
    let mut out: Vec<Level> = Vec::new();
    let mut previous = m.live_triangles;
    for &target in targets {
        if target >= previous {
            continue;
        }
        m.collapse_to(target);
        if m.live_triangles * 5 > previous * 4 {
            break;
        }
        previous = m.live_triangles;
        out.push(m.level());
    }
    out
}

/// `scene` drawn from about `target` triangles: its triangles (the first [`MAX_PRIMITIVES`])
/// grouped by colour, opacity and tag, each group of [`SMALL_GROUP`] or more brought down by the
/// share `target` is of them all, the others kept; its lines as they are. `None` when it has no
/// more than `target` triangles, or when that would save less than a fifth of them.
///
/// ```
/// use emcraft_render::view3d::{Scene, simplify::coarser};
/// use emcraft_render::Rgb;
/// // A sphere of 40 × 80 bands (m), tagged 3.
/// let mut s = Scene::default();
/// let at = |i: u32, j: u32| {
///     let (t, p) = (std::f64::consts::PI * f64::from(i) / 40.0, std::f64::consts::TAU * f64::from(j % 80) / 80.0);
///     // The poles and the seam exactly, so corners weld.
///     let st = if i == 0 || i == 40 { 0.0 } else { t.sin() };
///     [st * p.cos(), st * p.sin(), t.cos()]
/// };
/// s.tagged(3, |s| {
///     for i in 0..40 {
///         for j in 0..80 {
///             s.quad([at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)], Rgb(200, 0, 0), 255);
///         }
///     }
/// });
/// let c = coarser(&s, 1000).unwrap();
/// assert!(c.tris.len() <= 1000 && c.tris.len() > 500, "{}", c.tris.len());
/// assert!(c.tris.iter().all(|t| t.tag == 3 && t.color == Rgb(200, 0, 0)));
/// // Small enough already: nothing to do.
/// assert!(coarser(&s, 10_000).is_none());
/// ```
pub fn coarser(scene: &Scene, target: usize) -> Option<Scene> {
    let tris = scene.tris.get(..scene.tris.len().min(MAX_PRIMITIVES)).unwrap_or(&[]);
    if tris.len() <= target {
        return None;
    }
    // Groups in the order they first appear.
    let mut group_of: BTreeMap<(u8, u8, u8, u8, u32), usize> = BTreeMap::new();
    let mut groups: Vec<Vec<&Tri>> = Vec::new();
    for t in tris {
        let key = (t.color.0, t.color.1, t.color.2, t.alpha, t.tag);
        let next = groups.len();
        let g = *group_of.entry(key).or_insert(next);
        if g == next {
            groups.push(Vec::new());
        }
        if let Some(list) = groups.get_mut(g) {
            list.push(t);
        }
    }
    let share = target as f64 / tris.len() as f64;
    let mut out = Scene { tris: Vec::with_capacity(target + target / 4), lines: scene.lines.clone(), tag: scene.tag };
    for group in &groups {
        let Some(first) = group.first() else { continue };
        let want = (group.len() as f64 * share).ceil() as usize;
        let made = if group.len() >= SMALL_GROUP && want < group.len() {
            let positions: Vec<[f64; 3]> = group.iter().flat_map(|t| t.p).collect();
            let triangles: Vec<[u32; 3]> = (0..group.len() as u32).map(|i| [3 * i, 3 * i + 1, 3 * i + 2]).collect();
            levels(&positions, &triangles, &[want.max(4)]).pop()
        } else {
            None
        };
        match made {
            Some(l) => {
                let at = |i: u32| l.positions.get(i as usize).copied().unwrap_or_default();
                out.tris.extend(l.triangles.iter().map(|t| Tri { p: t.map(at), color: first.color, alpha: first.alpha, tag: first.tag }));
            }
            None => out.tris.extend(group.iter().map(|t| **t)),
        }
    }
    (out.tris.len() * 5 <= tris.len() * 4).then_some(out)
}

/// A symmetric 4 × 4 quadric: a a, a b, a c, a d, b b, b c, b d, c c, c d, d d.
#[derive(Clone, Copy, Default)]
struct Quadric([f64; 10]);

impl Quadric {
    /// The squared distance to the plane n · p + d = 0 (`n` unit), times `w`.
    fn plane(n: [f64; 3], d: f64, w: f64) -> Quadric {
        let [a, b, c] = n;
        Quadric([a * a * w, a * b * w, a * c * w, a * d * w, b * b * w, b * c * w, b * d * w, c * c * w, c * d * w, d * d * w])
    }

    fn add(&mut self, o: &Quadric) {
        for (x, y) in self.0.iter_mut().zip(o.0) {
            *x += y;
        }
    }

    fn sum(&self, o: &Quadric) -> Quadric {
        let mut q = *self;
        q.add(o);
        q
    }

    /// The summed squared distance at `p`.
    fn at(&self, p: [f64; 3]) -> f64 {
        let [aa, ab, ac, ad, bb, bc, bd, cc, cd, dd] = self.0;
        let [x, y, z] = p;
        (aa * x * x
            + 2.0 * ab * x * y
            + 2.0 * ac * x * z
            + 2.0 * ad * x
            + bb * y * y
            + 2.0 * bc * y * z
            + 2.0 * bd * y
            + cc * z * z
            + 2.0 * cd * z
            + dd)
            .max(0.0)
    }

    /// The point of least summed squared distance, when it is well defined.
    fn best(&self) -> Option<[f64; 3]> {
        let [aa, ab, ac, ad, bb, bc, bd, cc, cd, _] = self.0;
        let det = aa * (bb * cc - bc * bc) - ab * (ab * cc - bc * ac) + ac * (ab * bc - bb * ac);
        let scale = (aa.abs() + bb.abs() + cc.abs()).powi(3);
        if !(det.abs() > 1e-12 * scale) {
            return None;
        }
        let (r0, r1, r2) = (-ad, -bd, -cd);
        let x = (r0 * (bb * cc - bc * bc) - ab * (r1 * cc - bc * r2) + ac * (r1 * bc - bb * r2)) / det;
        let y = (aa * (r1 * cc - bc * r2) - r0 * (ab * cc - bc * ac) + ac * (ab * r2 - r1 * ac)) / det;
        let z = (aa * (bb * r2 - r1 * bc) - ab * (ab * r2 - r1 * ac) + r0 * (ab * bc - bb * ac)) / det;
        [x, y, z].iter().all(|v| v.is_finite()).then_some([x, y, z])
    }
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn unit(a: [f64; 3]) -> Option<[f64; 3]> {
    let l = dot(a, a).sqrt();
    (l > 0.0 && l.is_finite()).then(|| [a[0] / l, a[1] / l, a[2] / l])
}

/// A candidate collapse: its cost, its ends, and their versions when it was costed (a later
/// change to either end makes it stale).
#[derive(Clone, Copy)]
struct Candidate {
    cost: f64,
    u: u32,
    v: u32,
    stamps: (u32, u32),
}

impl PartialEq for Candidate {
    fn eq(&self, o: &Self) -> bool {
        self.cmp(o) == Ordering::Equal
    }
}
impl Eq for Candidate {}
impl PartialOrd for Candidate {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Candidate {
    /// The cheapest first (the heap pops the greatest), ties by the ends, so the result never
    /// depends on the heap's order.
    fn cmp(&self, o: &Self) -> Ordering {
        o.cost.total_cmp(&self.cost).then_with(|| (o.u, o.v).cmp(&(self.u, self.v)))
    }
}

/// A quick hasher for welding by position (the keys are coordinates' bits).
#[derive(Default)]
struct BitsHasher(u64);

impl std::hash::Hasher for BitsHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.write_u64(u64::from(*b));
        }
    }
    fn write_u64(&mut self, x: u64) {
        self.0 = (self.0.rotate_left(5) ^ x).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
}

/// The lists a collapse works with, kept between collapses.
#[derive(Default)]
struct Scratch {
    at_u: Vec<u32>,
    at_v: Vec<u32>,
    shared: Vec<u32>,
    nu: Vec<u32>,
    nv: Vec<u32>,
}

impl Scratch {
    fn clear(&mut self) {
        for l in [&mut self.at_u, &mut self.at_v, &mut self.shared, &mut self.nu, &mut self.nv] {
            l.clear();
        }
    }
}

struct Collapser {
    pos: Vec<[f64; 3]>,
    quadric: Vec<Quadric>,
    alive: Vec<bool>,
    stamp: Vec<u32>,
    tris: Vec<[u32; 3]>,
    tri_alive: Vec<bool>,
    /// The triangles at each vertex (dead ones are dropped lazily).
    around: Vec<Vec<u32>>,
    /// Lists a collapse reuses.
    scratch: Scratch,
    heap: BinaryHeap<Candidate>,
    live_triangles: usize,
}

impl Collapser {
    fn new(positions: &[[f64; 3]], triangles: &[[u32; 3]]) -> Option<Collapser> {
        // Weld corners at the same position.
        let mut index: std::collections::HashMap<[u64; 3], u32, std::hash::BuildHasherDefault<BitsHasher>> =
            std::collections::HashMap::with_capacity_and_hasher(positions.len() / 2, Default::default());
        let mut remap: Vec<Option<u32>> = Vec::with_capacity(positions.len());
        let mut pos: Vec<[f64; 3]> = Vec::new();
        for p in positions {
            if !p.iter().all(|v| v.is_finite()) {
                remap.push(None);
                continue;
            }
            let next = u32::try_from(pos.len()).ok()?;
            // `+ 0.0`: -0 and 0 are one position.
            let key = p.map(|v| (v + 0.0).to_bits());
            remap.push(Some(*index.entry(key).or_insert_with(|| {
                pos.push(*p);
                next
            })));
        }
        drop(index);
        let mut tris: Vec<[u32; 3]> = Vec::with_capacity(triangles.len());
        for t in triangles {
            let r = |i: u32| remap.get(i as usize).copied().flatten();
            let (Some(a), Some(b), Some(c)) = (r(t[0]), r(t[1]), r(t[2])) else { continue };
            if a != b && b != c && c != a {
                tris.push([a, b, c]);
            }
        }
        if tris.is_empty() {
            return None;
        }
        let n = pos.len();
        let mut quadric = vec![Quadric::default(); n];
        let mut around: Vec<Vec<u32>> = vec![Vec::new(); n];
        let mut sides: Vec<(u32, u32, u32)> = Vec::with_capacity(tris.len() * 3);
        for (ti, t) in tris.iter().enumerate() {
            let ti = u32::try_from(ti).ok()?;
            let [a, b, c] = t.map(|i| pos.get(i as usize).copied().unwrap_or_default());
            if let Some(nrm) = unit(cross(sub(b, a), sub(c, a))) {
                let q = Quadric::plane(nrm, -dot(nrm, a), 1.0);
                for &i in t {
                    if let Some(x) = quadric.get_mut(i as usize) {
                        x.add(&q);
                    }
                }
            }
            for (k, &i) in t.iter().enumerate() {
                if let Some(list) = around.get_mut(i as usize) {
                    list.push(ti);
                }
                let j = t[(k + 1) % 3];
                sides.push((i.min(j), i.max(j), ti));
            }
        }
        // Open boundaries (sides of one triangle only): a plane across each, weighted, so the
        // boundary keeps its place.
        sides.sort_unstable();
        let mut k = 0;
        while k < sides.len() {
            let (a, b, t) = sides[k];
            let mut e = k + 1;
            while e < sides.len() && sides[e].0 == a && sides[e].1 == b {
                e += 1;
            }
            if e - k == 1
                && let (Some(pa), Some(pb), Some(tri)) = (pos.get(a as usize), pos.get(b as usize), tris.get(t as usize))
            {
                let [p0, p1, p2] = tri.map(|i| pos.get(i as usize).copied().unwrap_or_default());
                let face = cross(sub(p1, p0), sub(p2, p0));
                if let Some(across) = unit(cross(sub(*pb, *pa), face)) {
                    let q = Quadric::plane(across, -dot(across, *pa), BOUNDARY_WEIGHT);
                    for i in [a, b] {
                        if let Some(x) = quadric.get_mut(i as usize) {
                            x.add(&q);
                        }
                    }
                }
            }
            k = e;
        }
        let live_triangles = tris.len();
        let mut m = Collapser {
            alive: vec![true; n],
            stamp: vec![0; n],
            tri_alive: vec![true; tris.len()],
            heap: BinaryHeap::with_capacity(sides.len() / 2 + 1),
            pos,
            quadric,
            tris,
            around,
            scratch: Scratch::default(),
            live_triangles,
        };
        let mut last = None;
        for &(a, b, _) in &sides {
            if last != Some((a, b)) {
                m.offer(a, b);
                last = Some((a, b));
            }
        }
        Some(m)
    }

    /// Where `u` and `v` would merge, and its cost.
    fn target(&self, u: u32, v: u32) -> Option<([f64; 3], f64)> {
        let (qu, qv) = (self.quadric.get(u as usize)?, self.quadric.get(v as usize)?);
        let q = qu.sum(qv);
        let (pu, pv) = (*self.pos.get(u as usize)?, *self.pos.get(v as usize)?);
        let mid = [(pu[0] + pv[0]) * 0.5, (pu[1] + pv[1]) * 0.5, (pu[2] + pv[2]) * 0.5];
        // The best point when it lies near the edge; else the best of its ends and middle.
        let reach = dot(sub(pu, pv), sub(pu, pv)) * 4.0;
        let best = q.best().filter(|p| dot(sub(*p, mid), sub(*p, mid)) <= reach);
        best.into_iter().chain([pu, pv, mid]).map(|p| (p, q.at(p))).min_by(|a, b| a.1.total_cmp(&b.1))
    }

    fn offer(&mut self, u: u32, v: u32) {
        let Some((_, cost)) = self.target(u, v) else { return };
        let stamps = (self.stamp.get(u as usize).copied().unwrap_or(0), self.stamp.get(v as usize).copied().unwrap_or(0));
        self.heap.push(Candidate { cost, u, v, stamps });
    }

    /// The live vertices sharing a live triangle with `v`.
    fn neighbours(&self, v: u32, out: &mut Vec<u32>) {
        out.clear();
        out.extend(
            self.around
                .get(v as usize)
                .into_iter()
                .flatten()
                .filter(|t| self.tri_alive.get(**t as usize).copied().unwrap_or(false))
                .filter_map(|t| self.tris.get(*t as usize))
                .flat_map(|t| t.iter().copied())
                .filter(|i| *i != v),
        );
        out.sort_unstable();
        out.dedup();
    }

    fn collapse_to(&mut self, target: usize) {
        while self.live_triangles > target {
            let Some(c) = self.heap.pop() else { return };
            let current = |i: u32| self.alive.get(i as usize).copied().unwrap_or(false);
            let stamp = |i: u32| self.stamp.get(i as usize).copied().unwrap_or(u32::MAX);
            if !current(c.u) || !current(c.v) || (stamp(c.u), stamp(c.v)) != c.stamps {
                continue;
            }
            self.collapse(c.u, c.v);
        }
    }

    /// Merge `v` into `u` at their best point, unless that would fold a face over or join two
    /// sheets of the surface.
    fn collapse(&mut self, u: u32, v: u32) {
        let Some((p, _)) = self.target(u, v) else { return };
        let live = |t: &u32| self.tri_alive.get(*t as usize).copied().unwrap_or(false);
        let mut s = std::mem::take(&mut self.scratch);
        s.clear();
        s.at_v.extend(self.around.get(v as usize).into_iter().flatten().copied().filter(live));
        s.at_u.extend(self.around.get(u as usize).into_iter().flatten().copied().filter(live));
        s.shared.extend(s.at_v.iter().copied().filter(|t| self.tris.get(*t as usize).is_some_and(|tri| tri.contains(&u))));
        let done = self.collapse_with(u, v, p, &mut s);
        self.scratch = s;
        if done {
            self.reoffer(u);
        }
    }

    /// [`Collapser::collapse`] with its lists in `s`: whether it collapsed.
    fn collapse_with(&mut self, u: u32, v: u32, p: [f64; 3], s: &mut Scratch) -> bool {
        if s.shared.is_empty() {
            return false;
        }
        // Two sheets meeting only at these ends would be joined.
        self.neighbours(u, &mut s.nu);
        self.neighbours(v, &mut s.nv);
        let common = s.nu.iter().filter(|x| s.nv.binary_search(x).is_ok()).count();
        if common > s.shared.len() {
            return false;
        }
        // No face folds over.
        for &t in s.at_u.iter().chain(&s.at_v).filter(|t| !s.shared.contains(t)) {
            let Some(tri) = self.tris.get(t as usize) else { continue };
            let corners = tri.map(|i| self.pos.get(i as usize).copied().unwrap_or_default());
            let moved = tri.map(|i| if i == u || i == v { p } else { self.pos.get(i as usize).copied().unwrap_or_default() });
            let before = cross(sub(corners[1], corners[0]), sub(corners[2], corners[0]));
            let after = cross(sub(moved[1], moved[0]), sub(moved[2], moved[0]));
            let (lb, la) = (dot(before, before).sqrt(), dot(after, after).sqrt());
            if !(la > 0.0) || (lb > 0.0 && dot(before, after) < MIN_TURN_COS * lb * la) {
                return false;
            }
        }
        for t in &s.shared {
            if let Some(a) = self.tri_alive.get_mut(*t as usize) {
                *a = false;
            }
            self.live_triangles = self.live_triangles.saturating_sub(1);
        }
        let mut list = self.around.get_mut(u as usize).map(std::mem::take).unwrap_or_default();
        let tri_alive = &self.tri_alive;
        list.retain(|t| tri_alive.get(*t as usize).copied().unwrap_or(false));
        for &t in s.at_v.iter().filter(|t| !s.shared.contains(t)) {
            if let Some(tri) = self.tris.get_mut(t as usize) {
                for i in tri.iter_mut() {
                    if *i == v {
                        *i = u;
                    }
                }
                list.push(t);
            }
        }
        if let Some(slot) = self.around.get_mut(u as usize) {
            *slot = list;
        }
        if let Some(slot) = self.around.get_mut(v as usize) {
            *slot = Vec::new();
        }
        let qv = self.quadric.get(v as usize).copied().unwrap_or_default();
        if let Some(q) = self.quadric.get_mut(u as usize) {
            q.add(&qv);
        }
        if let Some(x) = self.pos.get_mut(u as usize) {
            *x = p;
        }
        if let Some(a) = self.alive.get_mut(v as usize) {
            *a = false;
        }
        for i in [u, v] {
            if let Some(stamp) = self.stamp.get_mut(i as usize) {
                *stamp = stamp.wrapping_add(1);
            }
        }
        true
    }

    /// Cost again every edge at `u` (it moved).
    fn reoffer(&mut self, u: u32) {
        let mut around = std::mem::take(&mut self.scratch.nu);
        self.neighbours(u, &mut around);
        for &w in &around {
            self.offer(u.min(w), u.max(w));
        }
        self.scratch.nu = around;
    }

    /// The mesh as it is now.
    fn level(&self) -> Level {
        let mut index: Vec<Option<u32>> = vec![None; self.pos.len()];
        let mut out = Level::default();
        out.triangles.reserve(self.live_triangles);
        for (t, alive) in self.tris.iter().zip(&self.tri_alive) {
            if !alive {
                continue;
            }
            let mut tri = [0u32; 3];
            for (k, &i) in t.iter().enumerate() {
                let Some(slot) = index.get_mut(i as usize) else { continue };
                tri[k] = *slot.get_or_insert_with(|| {
                    let p = self.pos.get(i as usize).copied().unwrap_or_default();
                    out.positions.push(p);
                    u32::try_from(out.positions.len() - 1).unwrap_or(u32::MAX)
                });
            }
            out.triangles.push(tri);
        }
        out
    }
}

#[cfg(test)]
#[path = "simplify_tests.rs"]
mod tests;
