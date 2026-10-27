//! Adaptive refinement: which tetrahedra of a mesh to refine, from an error indicator per
//! tetrahedron, and the local sizes that refine them when the solid is meshed again.
//!
//! **Marking** (W. Dörfler, *A convergent adaptive algorithm for Poisson's equation*, SIAM J.
//! Numer. Anal. 33(3):1106–1124, 1996): the fewest tetrahedra whose squared indicators sum to at
//! least a fraction θ of the total, taken largest first ([`dorfler`]). Bulk marking of this kind
//! is what makes adaptive loops converge at the optimal rate (P. Morin, R. H. Nochetto, K. G.
//! Siebert, *Data oscillation and convergence of adaptive FEM*, SIAM J. Numer. Anal.
//! 38(2):466–488, 2000).
//!
//! **Refinement by remeshing.** Each marked tetrahedron K becomes a ball about its centroid that
//! covers it, with the size h_K/2 (h_K its mean edge length) and a floor ([`refinements`]); the
//! solid is meshed again with these balls as local sizes ([`crate::solid::solid_refined`]),
//! which the tetrahedral mesher bakes into its background grid and grades at its growth limit.
//! The loop keeps every pass's balls, so where an earlier pass refined the size stays as it was
//! (the previous mesh's size) and only the newly marked parts get finer.
//!
//! Everything is deterministic: ties in the indicator are broken by the tetrahedron's index, and
//! the sums run in that order.

use emcraft_model::{Mesh, Point3};

use crate::solid::Refinement;
use crate::{MeshError, Result};

/// The tetrahedra marked by Dörfler's bulk criterion: the fewest, largest first, whose
/// indicators `eta` (squared, ≥ 0, one per tetrahedron) sum to at least `theta` (in (0, 1]) of
/// the total. Ascending by index. None when the total is zero.
pub fn dorfler(eta: &[f64], theta: f64) -> Result<Vec<usize>> {
    if !(theta > 0.0 && theta <= 1.0) {
        return Err(MeshError::Invalid { what: "marking fraction".into(), why: format!("must be in (0, 1], got {theta}") });
    }
    if let Some(bad) = eta.iter().position(|e| !(e.is_finite() && *e >= 0.0)) {
        return Err(MeshError::Invalid { what: "error indicator".into(), why: format!("tetrahedron {bad}'s value is not a finite number ≥ 0") });
    }
    let mut order: Vec<usize> = (0..eta.len()).collect();
    // Largest first; equal values by index.
    order.sort_by(|&a, &b| eta[b].total_cmp(&eta[a]).then(a.cmp(&b)));
    let total: f64 = order.iter().map(|&t| eta[t]).sum();
    if !(total > 0.0) {
        return Ok(Vec::new());
    }
    let mut marked = Vec::new();
    let mut sum = 0.0;
    for t in order {
        if sum >= theta * total {
            break;
        }
        sum += eta[t];
        marked.push(t);
    }
    marked.sort_unstable();
    Ok(marked)
}

/// The local sizes that refine the `marked` tetrahedra of `mesh`: for each, a ball about its
/// centroid through its farthest corner, of size half its mean edge length, at least `floor`.
pub fn refinements(mesh: &Mesh, marked: &[usize], floor: f64) -> Result<Vec<Refinement>> {
    if !(floor.is_finite() && floor > 0.0) {
        return Err(MeshError::Invalid { what: "smallest refined size".into(), why: format!("must be a finite number > 0, got {floor}") });
    }
    marked
        .iter()
        .map(|&t| {
            let corners =
                mesh.corners(t).ok_or_else(|| MeshError::Invalid { what: "marked tetrahedron".into(), why: format!("no tetrahedron {t}") })?;
            let centre = Point3::centroid(corners).unwrap_or(corners[0]);
            let radius = corners.iter().map(|c| c.distance(centre)).fold(0.0, f64::max);
            let mut edges = 0.0;
            for (i, a) in corners.iter().enumerate() {
                for b in &corners[i + 1..] {
                    edges += a.distance(*b);
                }
            }
            let size = (edges / 12.0).max(floor);
            Ok(Refinement { center: centre, radius: radius.max(floor), size })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dorfler_marks_the_fewest_largest() {
        let eta = [1.0, 5.0, 2.0, 2.0, 0.0];
        // Total 10: 5 alone is half.
        assert_eq!(dorfler(&eta, 0.5).unwrap(), vec![1]);
        // 5 + 2 (the lower index of the tie) = 7.
        assert_eq!(dorfler(&eta, 0.6).unwrap(), vec![1, 2]);
        assert_eq!(dorfler(&eta, 1.0).unwrap(), vec![0, 1, 2, 3]);
        assert!(dorfler(&[0.0; 3], 0.5).unwrap().is_empty());
        assert!(dorfler(&[], 0.5).unwrap().is_empty());
    }

    #[test]
    fn hostile_marking_is_refused() {
        assert!(dorfler(&[1.0], 0.0).is_err());
        assert!(dorfler(&[1.0], 1.5).is_err());
        assert!(dorfler(&[1.0], f64::NAN).is_err());
        assert!(dorfler(&[f64::NAN], 0.5).is_err());
        assert!(dorfler(&[-1.0], 0.5).is_err());
        assert!(dorfler(&[f64::INFINITY], 0.5).is_err());
    }

    #[test]
    fn refinements_cover_their_tetrahedra() {
        let mesh = Mesh {
            nodes: vec![Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0), Point3::new(0.0, 2.0, 0.0), Point3::new(0.0, 0.0, 2.0)],
            tets: vec![[0, 1, 2, 3]],
            tet_material: vec![0],
            boundary: Vec::new(),
            grid: None,
            buckets: None,
            curved: Vec::new(),
        };
        let r = refinements(&mesh, &[0], 1e-3).unwrap();
        assert_eq!(r.len(), 1);
        assert!((r[0].center.x - 0.5).abs() < 1e-15);
        let mean = (3.0 * 2.0 + 3.0 * 8f64.sqrt()) / 6.0;
        assert!((r[0].size - mean / 2.0).abs() < 1e-12);
        assert!(mesh.nodes.iter().all(|n| n.distance(r[0].center) <= r[0].radius + 1e-12));
        assert_eq!(refinements(&mesh, &[0], 10.0).unwrap()[0].size, 10.0, "the floor");
        assert!(refinements(&mesh, &[1], 1e-3).is_err());
        assert!(refinements(&mesh, &[0], 0.0).is_err());
    }
}
