//! A face of the drawn scene, found from one of its triangles: the triangles of the same tag
//! that lie in its plane and join it edge to edge or corner to corner, with its outline, its
//! corners (where the outline turns), the straight edges between them and its centre — what the
//! 3D view's tools snap to and highlight.

use std::collections::BTreeMap;

use emcraft_render::view3d::Scene;

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

pub fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

pub fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

pub fn lerp(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// The unit normal of a triangle; `None` for a degenerate one.
fn normal_of(p: &[[f64; 3]; 3]) -> Option<[f64; 3]> {
    let n = cross(sub(p[1], p[0]), sub(p[2], p[0]));
    let l = norm(n);
    (l > 0.0 && l.is_finite()).then(|| n.map(|c| c / l))
}

impl Face {
    /// The face of `scene`'s triangle `index`; `None` for a degenerate or missing triangle.
    pub fn of(scene: &Scene, index: usize) -> Option<Face> {
        let t0 = scene.tris.get(index)?;
        let n = normal_of(&t0.p)?;
        let (blo, bhi) = scene.bounds()?;
        let size = norm(sub(bhi, blo)).max(1e-12);
        let tol = 1e-7 * size;
        let d = dot(n, t0.p[0]);
        let key = |p: [f64; 3]| p.map(|c| (c / tol).round() as i64);
        // The tag's triangles in the plane.
        let mut by_vertex: BTreeMap<[i64; 3], Vec<usize>> = BTreeMap::new();
        for (i, t) in scene.tris.iter().enumerate().filter(|(_, t)| t.tag == t0.tag).take(MAX_TRIS) {
            let flat = t.p.iter().all(|p| (dot(n, *p) - d).abs() <= tol * 10.0);
            if flat && normal_of(&t.p).is_some() {
                for p in t.p {
                    by_vertex.entry(key(p)).or_default().push(i);
                }
            }
        }
        // Those joined to the first, by shared corners.
        let mut seen = std::collections::BTreeSet::from([index]);
        let mut todo = vec![index];
        while let Some(i) = todo.pop() {
            let Some(t) = scene.tris.get(i) else { continue };
            for p in t.p {
                for &j in by_vertex.get(&key(p)).into_iter().flatten() {
                    if seen.insert(j) {
                        todo.push(j);
                    }
                }
            }
        }
        let tris: Vec<usize> = seen.into_iter().collect();
        let pts = |i: &usize| scene.tris.get(*i).map(|t| t.p);
        // The outline: edges one triangle has.
        let mut count: BTreeMap<([i64; 3], [i64; 3]), ([f64; 3], [f64; 3], u32)> = BTreeMap::new();
        let (mut area, mut c) = (0.0, [0.0; 3]);
        let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        for p in tris.iter().filter_map(pts) {
            for (a, b) in [(p[0], p[1]), (p[1], p[2]), (p[2], p[0])] {
                let (ka, kb) = (key(a), key(b));
                let k = if ka <= kb { (ka, kb) } else { (kb, ka) };
                count.entry(k).or_insert((a, b, 0)).2 += 1;
            }
            let w = 0.5 * norm(cross(sub(p[1], p[0]), sub(p[2], p[0])));
            area += w;
            for k in 0..3 {
                c[k] += w * (p[0][k] + p[1][k] + p[2][k]) / 3.0;
                lo[k] = lo[k].min(p[0][k]).min(p[1][k]).min(p[2][k]);
                hi[k] = hi[k].max(p[0][k]).max(p[1][k]).max(p[2][k]);
            }
        }
        let centre = if area > 0.0 { c.map(|v| v / area) } else { t0.p[0] };
        let outline: Vec<([f64; 3], [f64; 3])> = count.into_values().filter(|e| e.2 == 1).map(|(a, b, _)| (a, b)).collect();
        let (corners, edges) = runs(&outline, key);
        Some(Face { tag: t0.tag, normal: n, tris, outline, corners, edges, centre, lo, hi })
    }

    /// Whether it lies in a plane z = const.
    pub fn across_z(&self) -> bool {
        self.normal[2].abs() > 1.0 - 1e-9
    }
}

/// The outline's corners (where it turns, or branches) and its straight runs between them.
#[allow(clippy::type_complexity)]
fn runs(outline: &[([f64; 3], [f64; 3])], key: impl Fn([f64; 3]) -> [i64; 3]) -> (Vec<[f64; 3]>, Vec<([f64; 3], [f64; 3])>) {
    let mut at: BTreeMap<[i64; 3], ([f64; 3], Vec<[i64; 3]>)> = BTreeMap::new();
    for &(a, b) in outline {
        let (ka, kb) = (key(a), key(b));
        at.entry(ka).or_insert((a, Vec::new())).1.push(kb);
        at.entry(kb).or_insert((b, Vec::new())).1.push(ka);
    }
    let point = |k: &[i64; 3]| at.get(k).map(|v| v.0);
    let is_corner = |k: &[i64; 3]| {
        let Some((p, next)) = at.get(k) else { return false };
        let [a, b] = next[..] else { return true };
        let (Some(pa), Some(pb)) = (point(&a), point(&b)) else { return true };
        let (u, v) = (sub(pa, *p), sub(*p, pb));
        norm(cross(u, v)) > 1e-6 * norm(u) * norm(v)
    };
    let corners: Vec<[i64; 3]> = at.keys().filter(|k| is_corner(k)).take(MAX_CORNERS).copied().collect();
    let mut edges = Vec::new();
    let mut done = std::collections::BTreeSet::new();
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
        assert!(norm(sub(f.centre, [1.0, 0.5, 0.0])) < 1e-12);
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
