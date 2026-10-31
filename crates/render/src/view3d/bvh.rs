//! A bounding volume hierarchy over a scene's triangles, for picking: the nearest tagged
//! triangle under a pixel ([`SceneIndex::hits`], the same answer as [`super::hits`]) and the
//! triangles near a plane ([`SceneIndex::near_plane`]), without looking at every triangle.
//!
//! Built once per scene (a median split on the widest axis of the boxes' centres, a few items
//! per leaf; Wald, Boulos and Shirley, "Ray tracing deformable scenes using dynamic bounding
//! volume hierarchies", ACM TOG 26(1), 2007, for the layout and the nearest-first traversal)
//! and only read afterwards; rays meet boxes by the slab test (Williams, Barrus, Morley and
//! Shirley, "An efficient and robust ray–box intersection algorithm", J. Graphics Tools 10(1),
//! 2005). Deterministic: the split's order is a total order on the centres,
//! ties by item.
//!
//! The nearest hit is the linear scan's, tie for tie: boxes are entered nearest first, one that
//! starts past the best hit so far is skipped, and equal distances go to the lowest triangle,
//! as a scan in scene order keeping strictly nearer hits answers. Triangle boxes are grown a
//! little so a hit the ray–triangle test admits at its rounding edge is never lost to the box
//! test; a triangle whose box is not finite is scanned rather than indexed.

use super::{Camera, Hit, Scene, hit};

/// Most items in a leaf.
const LEAF: usize = 4;

/// An axis-aligned box.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Aabb {
    lo: [f64; 3],
    hi: [f64; 3],
}

impl Aabb {
    const EMPTY: Aabb = Aabb { lo: [f64::INFINITY; 3], hi: [f64::NEG_INFINITY; 3] };

    fn add(&mut self, p: [f64; 3]) {
        for k in 0..3 {
            self.lo[k] = self.lo[k].min(p[k]);
            self.hi[k] = self.hi[k].max(p[k]);
        }
    }

    fn union(&self, o: &Aabb) -> Aabb {
        Aabb { lo: [0, 1, 2].map(|k| self.lo[k].min(o.lo[k])), hi: [0, 1, 2].map(|k| self.hi[k].max(o.hi[k])) }
    }

    fn centre(&self) -> [f64; 3] {
        [0, 1, 2].map(|k| 0.5 * (self.lo[k] + self.hi[k]))
    }

    fn finite(&self) -> bool {
        self.lo.iter().chain(&self.hi).all(|v| v.is_finite()) && (0..3).all(|k| self.lo[k] <= self.hi[k])
    }
}

#[derive(Clone, Copy, Debug)]
struct Node {
    bounds: Aabb,
    /// A leaf: the first of its `count` items in [`Bvh::order`]. Otherwise its children are
    /// `first` and `first + 1`.
    first: u32,
    count: u32,
}

/// A hierarchy over boxes; items are the boxes' indices.
#[derive(Clone, Debug, Default)]
struct Bvh {
    nodes: Vec<Node>,
    order: Vec<u32>,
}

impl Bvh {
    /// The hierarchy over `boxes` (`None` and boxes not finite left out).
    fn new(boxes: &[Option<Aabb>]) -> Bvh {
        let item = |i: u32| boxes.get(i as usize).copied().flatten().unwrap_or(Aabb::EMPTY);
        let mut order: Vec<u32> = (0..boxes.len().min(u32::MAX as usize) as u32).filter(|&i| item(i).finite()).collect();
        let centres: Vec<[f64; 3]> = (0..boxes.len().min(u32::MAX as usize) as u32).map(|i| item(i).centre()).collect();
        let centre = |i: u32| centres.get(i as usize).copied().unwrap_or_default();
        if order.is_empty() {
            return Bvh::default();
        }
        let mut nodes = vec![Node { bounds: Aabb::EMPTY, first: 0, count: 0 }];
        // (node, start, end) in `order` still to be filled in.
        let mut todo = vec![(0usize, 0usize, order.len())];
        while let Some((n, start, end)) = todo.pop() {
            let Some(span) = order.get_mut(start..end) else { continue };
            let mut bounds = Aabb::EMPTY;
            let mut spread = Aabb::EMPTY;
            for &i in span.iter() {
                bounds = bounds.union(&item(i));
                spread.add(centre(i));
            }
            let size = [0, 1, 2].map(|k| spread.hi[k] - spread.lo[k]);
            let axis = if size[0] >= size[1] && size[0] >= size[2] {
                0
            } else if size[1] >= size[2] {
                1
            } else {
                2
            };
            if span.len() <= LEAF || !(size[axis] > 0.0) {
                if let Some(node) = nodes.get_mut(n) {
                    *node = Node { bounds, first: start as u32, count: span.len() as u32 };
                }
                continue;
            }
            let mid = span.len() / 2;
            // A total order (centre, then item), so the split never depends on the input's order.
            span.select_nth_unstable_by(mid, |a, b| centre(*a)[axis].total_cmp(&centre(*b)[axis]).then(a.cmp(b)));
            let left = nodes.len();
            nodes.push(Node { bounds: Aabb::EMPTY, first: 0, count: 0 });
            nodes.push(Node { bounds: Aabb::EMPTY, first: 0, count: 0 });
            if let Some(node) = nodes.get_mut(n) {
                *node = Node { bounds, first: left as u32, count: 0 };
            }
            todo.push((left, start, start + mid));
            todo.push((left + 1, start + mid, end));
        }
        Bvh { nodes, order }
    }

    /// Calls `item` with every item in a leaf whose box, and every box above it, `enter` accepts.
    fn visit(&self, mut enter: impl FnMut(&Aabb) -> bool, mut item: impl FnMut(u32)) {
        let mut stack = vec![0usize];
        while let Some(n) = stack.pop() {
            let Some(node) = self.nodes.get(n) else { continue };
            if !enter(&node.bounds) {
                continue;
            }
            if node.count > 0 {
                let (a, b) = (node.first as usize, node.first as usize + node.count as usize);
                for &i in self.order.get(a..b).unwrap_or(&[]) {
                    item(i);
                }
            } else {
                stack.push(node.first as usize + 1);
                stack.push(node.first as usize);
            }
        }
    }

    /// The nearest item along the ray from `o` along `d`: `hit` gives an item's distance along
    /// `d` (`None`: missed). Equal distances go to the lowest item.
    fn nearest(&self, o: [f64; 3], d: [f64; 3], mut hit: impl FnMut(u32) -> Option<f64>) -> Option<(f64, u32)> {
        if !o.iter().chain(&d).all(|v| v.is_finite()) {
            return None;
        }
        let inv = d.map(|c| 1.0 / c);
        let mut best: Option<(f64, u32)> = None;
        let root = self.nodes.first().and_then(|n| ray_box(o, d, inv, &n.bounds))?;
        let mut stack = vec![(0usize, root)];
        while let Some((n, t0)) = stack.pop() {
            if best.is_some_and(|(bt, _)| t0 > bt) {
                continue;
            }
            let Some(node) = self.nodes.get(n) else { continue };
            if node.count > 0 {
                let (a, b) = (node.first as usize, node.first as usize + node.count as usize);
                for &i in self.order.get(a..b).unwrap_or(&[]) {
                    if let Some(t) = hit(i) {
                        keep(&mut best, t, i);
                    }
                }
                continue;
            }
            let (l, r) = (node.first as usize, node.first as usize + 1);
            let tl = self.nodes.get(l).and_then(|c| ray_box(o, d, inv, &c.bounds));
            let tr = self.nodes.get(r).and_then(|c| ray_box(o, d, inv, &c.bounds));
            // The nearer child is popped first.
            match (tl, tr) {
                (Some(a), Some(b)) if a <= b => stack.extend([(r, b), (l, a)]),
                (Some(a), Some(b)) => stack.extend([(l, a), (r, b)]),
                (Some(a), None) => stack.push((l, a)),
                (None, Some(b)) => stack.push((r, b)),
                (None, None) => {}
            }
        }
        best
    }
}

/// Keeps `(t, i)` in `best` when it is nearer, or as near and a lower item.
fn keep<I: Ord + Copy>(best: &mut Option<(f64, I)>, t: f64, i: I) {
    if best.is_none_or(|(bt, bi)| t < bt || (t == bt && i < bi)) {
        *best = Some((t, i));
    }
}

/// Where the ray from `o` along `d` (`inv` = 1 / `d` per axis) enters box `b`, from 0 on;
/// `None` when it misses or the box lies behind.
fn ray_box(o: [f64; 3], d: [f64; 3], inv: [f64; 3], b: &Aabb) -> Option<f64> {
    let (mut t0, mut t1) = (0.0f64, f64::INFINITY);
    for k in 0..3 {
        let (lo, hi) = (b.lo[k], b.hi[k]);
        if d[k] == 0.0 || !inv[k].is_finite() {
            if o[k] < lo || o[k] > hi {
                return None;
            }
            continue;
        }
        let (a, z) = ((lo - o[k]) * inv[k], (hi - o[k]) * inv[k]);
        let (a, z) = if a <= z { (a, z) } else { (z, a) };
        t0 = t0.max(a);
        t1 = t1.min(z);
        if t0 > t1 {
            return None;
        }
    }
    (t0 <= t1).then_some(t0)
}

/// A scene's tagged triangles in a hierarchy, for picking. Built from one scene and answers only
/// for it: asked about another (a different number of triangles), it scans as [`super::hits`].
///
/// ```
/// use emcraft_render::view3d::{Camera, Scene, SceneIndex, StandardView, hits};
/// use emcraft_render::Rgb;
/// let mut s = Scene::default();
/// // A grid of 40 × 40 tagged squares on z = 0 (m).
/// for i in 0..40 {
///     for j in 0..40 {
///         let (x, y) = (f64::from(i), f64::from(j));
///         s.tagged(1 + i * 40 + j, |s| s.quad([[x, y, 0.0], [x + 1.0, y, 0.0], [x + 1.0, y + 1.0, 0.0], [x, y + 1.0, 0.0]], Rgb(9, 9, 9), 255));
///     }
/// }
/// let index = SceneIndex::new(&s);
/// let mut cam = Camera::default();
/// cam.standard(StandardView::Top);
/// cam.fit([0.0, 0.0, 0.0], [40.0, 40.0, 0.0], 1.0);
/// // Every pixel picks what the scan of every triangle picks.
/// for (x, y) in [(50.0, 50.0), (13.0, 77.0), (0.5, 99.5), (500.0, 5.0)] {
///     assert_eq!(index.hits(&s, &cam, x, y, 100.0, 100.0), hits(&s, &cam, x, y, 100.0, 100.0));
/// }
/// ```
#[derive(Clone, Debug, Default)]
pub struct SceneIndex {
    bvh: Bvh,
    /// Tagged triangles whose box is not finite: scanned on every pick.
    loose: Vec<usize>,
    /// Triangles in the scene indexed.
    len: usize,
    /// The scene's [`Scene::bounds`].
    bounds: Option<([f64; 3], [f64; 3])>,
}

impl SceneIndex {
    /// The index of `scene`'s tagged triangles (those that can be picked).
    pub fn new(scene: &Scene) -> SceneIndex {
        let mut loose = Vec::new();
        let boxes: Vec<Option<Aabb>> = scene
            .tris
            .iter()
            .enumerate()
            .map(|(i, t)| {
                if t.tag == 0 {
                    return None;
                }
                let mut b = Aabb::EMPTY;
                t.p.iter().for_each(|p| b.add(*p));
                if !b.finite() || i >= u32::MAX as usize {
                    loose.push(i);
                    return None;
                }
                Some(grown(b))
            })
            .collect();
        SceneIndex { bvh: Bvh::new(&boxes), loose, len: scene.tris.len(), bounds: scene.bounds() }
    }

    /// Whether it was built from a scene of `scene`'s size (its answers are that scene's).
    fn of(&self, scene: &Scene) -> bool {
        scene.tris.len() == self.len
    }

    /// The scene's [`Scene::bounds`], as built.
    pub fn bounds(&self) -> Option<([f64; 3], [f64; 3])> {
        self.bounds
    }

    /// [`super::hits`] through the index: the same answer, tie for tie.
    pub fn hits(&self, scene: &Scene, camera: &Camera, x: f64, y: f64, w: f64, h: f64) -> Option<(Hit, Option<Hit>)> {
        if !self.of(scene) {
            return super::hits(scene, camera, x, y, w, h);
        }
        let (origin, dir) = camera.ray(x, y, w, h)?;
        let nearest = |solid: bool| {
            let at = |i: usize| scene.tris.get(i).filter(|t| t.tag != 0 && (!solid || t.alpha >= 200)).and_then(|t| hit(origin, dir, t));
            let mut best = self.bvh.nearest(origin, dir, |i| at(i as usize)).map(|(t, i)| (t, i as usize));
            for &i in &self.loose {
                if let Some(t) = at(i) {
                    keep(&mut best, t, i);
                }
            }
            best
        };
        let any = nearest(false)?;
        // The nearest of all, when solid, is the nearest solid one too.
        let solid = if scene.tris.get(any.1).is_some_and(|t| t.alpha >= 200) { Some(any) } else { nearest(true) };
        let at = |(d, i): (f64, usize)| {
            let tag = scene.tris.get(i).map_or(0, |t| t.tag);
            Hit { index: i, at: [origin[0] + dir[0] * d, origin[1] + dir[1] * d, origin[2] + dir[2] * d], tag }
        };
        Some((at(any), solid.map(at)))
    }

    /// [`super::pick`] through the index.
    pub fn pick(&self, scene: &Scene, camera: &Camera, x: f64, y: f64, w: f64, h: f64) -> Option<u32> {
        self.hits(scene, camera, x, y, w, h).map(|(any, solid)| solid.unwrap_or(any).tag)
    }

    /// Calls `f` with every tagged triangle of `scene` that may have all its corners within
    /// `tol` of the plane n · p = `d`, in the scene's order (each once). A superset: the caller
    /// tests each. `None` when the index is not `scene`'s (the caller scans).
    pub fn near_plane(&self, scene: &Scene, n: [f64; 3], d: f64, tol: f64) -> Option<Vec<usize>> {
        if !self.of(scene) || !n.iter().all(|v| v.is_finite()) || !d.is_finite() || !tol.is_finite() {
            return None;
        }
        // Twice the tolerance and a little: rounding in the box's projection never loses one.
        let reach = 2.0 * tol.abs() + 1e-12 * (d.abs() + 1.0);
        let mut out = self.loose.clone();
        self.bvh.visit(
            |b| {
                let c = b.centre();
                let r: f64 = (0..3).map(|k| n[k].abs() * 0.5 * (b.hi[k] - b.lo[k])).sum();
                let at = n[0] * c[0] + n[1] * c[1] + n[2] * c[2];
                (at - d).abs() <= r * (1.0 + 1e-12) + reach
            },
            |i| out.push(i as usize),
        );
        out.sort_unstable();
        Some(out)
    }
}

/// `b` grown on every side, so the box test never misses a hit the triangle test admits at its
/// rounding edge (a triangle square to an axis has a flat box).
fn grown(mut b: Aabb) -> Aabb {
    let scale = b.lo.iter().chain(&b.hi).fold(1.0f64, |m, v| m.max(v.abs()));
    let size = (0..3).map(|k| b.hi[k] - b.lo[k]).fold(0.0f64, f64::max);
    let e = scale * 1e-9 + size * 1e-6;
    for k in 0..3 {
        b.lo[k] -= e;
        b.hi[k] += e;
    }
    b
}

#[cfg(test)]
#[path = "tests/bvh.rs"]
mod tests;
