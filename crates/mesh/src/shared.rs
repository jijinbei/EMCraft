//! Small helpers the surface algorithms share (`csg`, `flush`, `cavity`): a disjoint-set forest
//! and the winding number of a closed triangulated surface.

use emcraft_model::Point3;

/// A disjoint-set forest with path halving; a union roots the pair at the smaller index, so the
/// sets and their roots depend only on the sequence of unions (deterministic).
pub(crate) struct DisjointSets(Vec<usize>);

impl DisjointSets {
    /// `n` singletons.
    pub(crate) fn new(n: usize) -> Self {
        DisjointSets((0..n).collect())
    }

    /// The root of `i`'s set (`i < n`).
    pub(crate) fn find(&mut self, mut i: usize) -> usize {
        while self.0[i] != i {
            self.0[i] = self.0[self.0[i]];
            i = self.0[i];
        }
        i
    }

    /// Joins the sets of `a` and `b`.
    pub(crate) fn union(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            self.0[a.max(b)] = a.min(b);
        }
    }
}

/// A triangle's corners; `None` when one is missing.
pub(crate) fn corners(positions: &[Point3], t: &[u32; 3]) -> Option<[Point3; 3]> {
    Some([*positions.get(t[0] as usize)?, *positions.get(t[1] as usize)?, *positions.get(t[2] as usize)?])
}

/// The winding number about `p` of the closed surface made of `triangles` (the solid angles
/// they subtend, summed in their order, over 4π — A. Van Oosterom, J. Strackee, IEEE Trans.
/// Biomed. Eng. 30(2), 1983). A triangle with a missing corner is skipped.
pub(crate) fn winding<'a>(positions: &[Point3], triangles: impl IntoIterator<Item = &'a [u32; 3]>, p: Point3) -> f64 {
    triangles.into_iter().filter_map(|t| corners(positions, t)).map(|q| emcraft_model::solid_angle(q.map(|x| x - p))).sum::<f64>()
        / (4.0 * std::f64::consts::PI)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unions_root_at_the_smaller_index() {
        let mut s = DisjointSets::new(5);
        s.union(4, 2);
        s.union(3, 4);
        assert_eq!((s.find(2), s.find(3), s.find(4), s.find(0)), (2, 2, 2, 0));
        s.union(1, 3);
        assert_eq!(s.find(4), 1);
    }

    #[test]
    fn a_tetrahedron_winds_once_about_its_inside() {
        let p = [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0), Point3::new(0.0, 0.0, 1.0)];
        let t = [[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]];
        assert!((winding(&p, &t, Point3::new(0.1, 0.1, 0.1)) - 1.0).abs() < 1e-12);
        assert!(winding(&p, &t, Point3::new(2.0, 2.0, 2.0)).abs() < 1e-12);
        // A missing corner skips its triangle, never panics.
        assert!(winding(&p, &[[0, 1, 9]], Point3::ORIGIN).is_finite());
    }
}
