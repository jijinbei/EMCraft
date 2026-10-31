//! A face of the drawn scene, found from one of its triangles: the triangles of the same tag
//! that lie in its plane and join it edge to edge or corner to corner, with its outline, its
//! corners (where the outline turns), the straight edges between them and its centre — what the
//! 3D view's tools snap to and highlight.

use std::collections::{BTreeMap, BTreeSet};

use emcraft_model::geom::{Point3, Vec3};
use emcraft_render::view3d::{Scene, SceneIndex};

/// Most triangles a face is looked for among (a fine mesh's wall).
const MAX_TRIS: usize = 400_000;
/// Most corners kept (a fine circle's outline).
const MAX_CORNERS: usize = 256;

/// A face of the scene.
#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    pub tag: u32,
    /// The unit normal of the triangle it was found from.
    pub normal: [f64; 3],
    /// Its triangles' indices in the scene.
    pub tris: Vec<usize>,
    /// The edges only one of its triangles has.
    pub outline: Vec<([f64; 3], [f64; 3])>,
    /// Where the outline turns.
    pub corners: Vec<[f64; 3]>,
    /// The outline's straight runs from corner to corner.
    pub edges: Vec<([f64; 3], [f64; 3])>,
    /// Its area-weighted centroid.
    pub centre: [f64; 3],
    /// Its box.
    pub lo: [f64; 3],
    pub hi: [f64; 3],
}

/// The unit normal of a triangle (its corners' turn); `None` for a degenerate one.
pub(crate) fn unit_normal([a, b, c]: [Point3; 3]) -> Option<Vec3> {
    (b - a).cross(c - a).normalized()
}

/// A point welded to a grid of step `tol`: points closer than that share a key.
type Key = [i64; 3];

/// The welding grid.
#[derive(Clone, Copy)]
struct Weld(f64);

impl Weld {
    fn key(self, p: [f64; 3]) -> Key {
        p.map(|c| (c / self.0).round() as i64)
    }
}

impl Face {
    /// The face of `scene`'s triangle `index`; `None` for a degenerate or missing triangle.
    pub fn of(scene: &Scene, index: usize) -> Option<Face> {
        Face::of_indexed(scene, None, index)
    }

    /// [`Face::of`], the triangles in its plane looked up through the scene's `index` when given
    /// (the same face).
    pub fn of_indexed(scene: &Scene, index: Option<&SceneIndex>, at: usize) -> Option<Face> {
        let t0 = scene.tris.get(at)?;
        let n = unit_normal(t0.p.map(Point3::from))?;
        let (blo, bhi) = index.map_or_else(|| scene.bounds(), SceneIndex::bounds)?;
        let weld = Weld(1e-7 * Point3::from(blo).distance(Point3::from(bhi)).max(1e-12));
        let d = n.dot(Point3::from(t0.p[0]).to_vec());
        // Through the index, only the triangles near the plane are tried (in the scene's order,
        // as the scan tries them), while every triangle is within the scan's cap.
        let near = index.filter(|_| t0.tag != 0 && scene.tris.len() <= MAX_TRIS).and_then(|i| i.near_plane(scene, n.to_array(), d, weld.0 * 10.0));
        let tris = joined(scene, at, &in_plane(scene, near.as_deref(), t0.tag, n, d, weld), weld);
        let corners_of = |i: &usize| scene.tris.get(*i).map(|t| t.p);
        let (centre, lo, hi) = centre_and_box(tris.iter().filter_map(corners_of));
        let outline = outline(tris.iter().filter_map(corners_of), weld);
        let (corners, edges) = runs(&outline, weld);
        Some(Face { tag: t0.tag, normal: n.to_array(), tris, outline, corners, edges, centre: centre.unwrap_or(t0.p[0]), lo, hi })
    }

    /// Whether it lies in a plane z = const.
    pub fn across_z(&self) -> bool {
        self.axis() == Some(emcraft_model::Axis::Z)
    }

    /// The axis it lies across, when it is flat and in a plane x, y or z = const.
    pub fn axis(&self) -> Option<emcraft_model::Axis> {
        let size = Point3::from(self.lo).distance(Point3::from(self.hi));
        (0..3)
            .find(|&k| self.normal[k].abs() > 1.0 - 1e-9 && (self.hi[k] - self.lo[k]).abs() <= 1e-9 * size)
            .and_then(emcraft_model::Axis::from_index)
    }
}

/// The triangles tagged `tag` that lie in the plane n · p = `d`, by their welded corners: of
/// `near` (in order) when given, else of the whole scene.
fn in_plane(scene: &Scene, near: Option<&[usize]>, tag: u32, n: Vec3, d: f64, weld: Weld) -> BTreeMap<Key, Vec<usize>> {
    let mut by_vertex: BTreeMap<Key, Vec<usize>> = BTreeMap::new();
    let tried: Box<dyn Iterator<Item = (usize, &emcraft_render::view3d::Tri)>> = match near {
        Some(near) => Box::new(near.iter().filter_map(|&i| scene.tris.get(i).map(|t| (i, t)))),
        None => Box::new(scene.tris.iter().enumerate()),
    };
    for (i, t) in tried.filter(|(_, t)| t.tag == tag).take(MAX_TRIS) {
        let flat = t.p.iter().all(|p| (n.dot(Point3::from(*p).to_vec()) - d).abs() <= weld.0 * 10.0);
        if flat && unit_normal(t.p.map(Point3::from)).is_some() {
            for p in t.p {
                by_vertex.entry(weld.key(p)).or_default().push(i);
            }
        }
    }
    by_vertex
}

/// The triangles of `by_vertex` joined to triangle `start` by shared corners, in order.
fn joined(scene: &Scene, start: usize, by_vertex: &BTreeMap<Key, Vec<usize>>, weld: Weld) -> Vec<usize> {
    let mut seen = BTreeSet::from([start]);
    let mut todo = vec![start];
    while let Some(i) = todo.pop() {
        let Some(t) = scene.tris.get(i) else { continue };
        for p in t.p {
            for &j in by_vertex.get(&weld.key(p)).into_iter().flatten() {
                if seen.insert(j) {
                    todo.push(j);
                }
            }
        }
    }
    seen.into_iter().collect()
}

/// The area-weighted centroid of triangles (`None` for no area) and their box.
fn centre_and_box(tris: impl Iterator<Item = [[f64; 3]; 3]>) -> (Option<[f64; 3]>, [f64; 3], [f64; 3]) {
    let (mut area, mut c) = (0.0, Vec3::ZERO);
    let (mut lo, mut hi) = (Point3::from([f64::INFINITY; 3]), Point3::from([f64::NEG_INFINITY; 3]));
    for p in tris {
        let [a, b, q] = p.map(Point3::from);
        let w = 0.5 * (b - a).cross(q - a).norm();
        area += w;
        c += (a.to_vec() + b.to_vec() + q.to_vec()) * w / 3.0;
        lo = lo.min(a).min(b).min(q);
        hi = hi.max(a).max(b).max(q);
    }
    ((area > 0.0).then(|| (c / area).to_array()), lo.to_array(), hi.to_array())
}

/// The edges only one of these triangles has.
fn outline(tris: impl Iterator<Item = [[f64; 3]; 3]>, weld: Weld) -> Vec<([f64; 3], [f64; 3])> {
    let mut count: BTreeMap<(Key, Key), ([f64; 3], [f64; 3], u32)> = BTreeMap::new();
    for p in tris {
        for (a, b) in [(p[0], p[1]), (p[1], p[2]), (p[2], p[0])] {
            let (ka, kb) = (weld.key(a), weld.key(b));
            let k = if ka <= kb { (ka, kb) } else { (kb, ka) };
            count.entry(k).or_insert((a, b, 0)).2 += 1;
        }
    }
    count.into_values().filter(|e| e.2 == 1).map(|(a, b, _)| (a, b)).collect()
}

/// The outline's corners (where it turns, or branches) and its straight runs between them.
#[allow(clippy::type_complexity)]
fn runs(outline: &[([f64; 3], [f64; 3])], weld: Weld) -> (Vec<[f64; 3]>, Vec<([f64; 3], [f64; 3])>) {
    let mut at: BTreeMap<Key, ([f64; 3], Vec<Key>)> = BTreeMap::new();
    for &(a, b) in outline {
        let (ka, kb) = (weld.key(a), weld.key(b));
        at.entry(ka).or_insert((a, Vec::new())).1.push(kb);
        at.entry(kb).or_insert((b, Vec::new())).1.push(ka);
    }
    let point = |k: &Key| at.get(k).map(|v| v.0);
    let is_corner = |k: &Key| {
        let Some((p, next)) = at.get(k) else { return false };
        let [a, b] = next[..] else { return true };
        let (Some(pa), Some(pb)) = (point(&a), point(&b)) else { return true };
        let p = Point3::from(*p);
        let (u, v) = (Point3::from(pa) - p, p - Point3::from(pb));
        u.cross(v).norm() > 1e-6 * u.norm() * v.norm()
    };
    let corners: Vec<Key> = at.keys().filter(|k| is_corner(k)).take(MAX_CORNERS).copied().collect();
    let mut edges = Vec::new();
    let mut done = BTreeSet::new();
    for c in &corners {
        let Some((start, next)) = at.get(c) else { continue };
        for first in next {
            // Along the outline until the next corner.
            let (mut prev, mut cur) = (*c, *first);
            for _ in 0..outline.len() {
                if is_corner(&cur) {
                    break;
                }
                let Some((_, n)) = at.get(&cur) else { break };
                let Some(&step) = n.iter().find(|k| **k != prev) else { break };
                (prev, cur) = (cur, step);
            }
            let pair = if *c <= cur { (*c, cur) } else { (cur, *c) };
            if done.insert(pair)
                && let Some(end) = point(&cur)
            {
                edges.push((*start, end));
            }
        }
    }
    (corners.iter().filter_map(point).collect(), edges)
}
#[cfg(test)]
mod tests {
    use super::*;
    use emcraft_render::Rgb;

    #[test]
    fn a_box_face_is_its_two_triangles_with_four_corners_four_edges_and_its_middle() {
        let mut s = Scene::default();
        s.tagged(5, |s| s.cuboid([0.0, 0.0, 0.0], [2.0, 1.0, 4.0], Rgb(1, 2, 3), 255));
        // Another tag in the same plane is another face.
        s.tagged(6, |s| s.quad([[3.0, 0.0, 0.0], [4.0, 0.0, 0.0], [4.0, 1.0, 0.0], [3.0, 1.0, 0.0]], Rgb(1, 2, 3), 255));
        let z0 = s.tris.iter().position(|t| t.tag == 5 && t.p.iter().all(|p| p[2] == 0.0)).unwrap();
        let f = Face::of(&s, z0).unwrap();
        assert_eq!((f.tag, f.tris.len(), f.outline.len(), f.corners.len(), f.edges.len()), (5, 2, 4, 4, 4));
        assert!(f.across_z());
        assert!(Point3::from(f.centre).distance(Point3::new(1.0, 0.5, 0.0)) < 1e-12);
        assert_eq!((f.lo, f.hi), ([0.0, 0.0, 0.0], [2.0, 1.0, 0.0]));
        // A side face is not across z.
        let side = s.tris.iter().position(|t| t.tag == 5 && t.p.iter().all(|p| p[0] == 0.0)).unwrap();
        assert!(!Face::of(&s, side).unwrap().across_z());
    }

    #[test]
    fn a_split_edge_is_one_straight_run() {
        // A square of four triangles round its centre and two more along one side: the side's
        // middle vertex is no corner.
        let mut s = Scene::default();
        let c = Rgb(1, 1, 1);
        s.tagged(1, |s| {
            s.tri([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], c, 255);
            s.tri([1.0, 0.0, 0.0], [2.0, 0.0, 0.0], [1.0, 1.0, 0.0], c, 255);
            s.tri([2.0, 0.0, 0.0], [2.0, 2.0, 0.0], [1.0, 1.0, 0.0], c, 255);
            s.tri([2.0, 2.0, 0.0], [0.0, 2.0, 0.0], [1.0, 1.0, 0.0], c, 255);
            s.tri([0.0, 2.0, 0.0], [0.0, 0.0, 0.0], [1.0, 1.0, 0.0], c, 255);
        });
        let f = Face::of(&s, 0).unwrap();
        assert_eq!((f.tris.len(), f.outline.len(), f.corners.len(), f.edges.len()), (5, 5, 4, 4));
        assert!(f.edges.iter().any(|(a, b)| (a[1] == 0.0 && b[1] == 0.0) && (a[0] - b[0]).abs() == 2.0));
        assert!(Face::of(&s, 99).is_none());
    }
}
