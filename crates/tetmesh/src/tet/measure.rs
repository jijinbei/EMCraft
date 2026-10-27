//! Tetrahedron measures for the quality stage, on floating-point coordinates.
//!
//! Decisions use only `+ − × ÷ √` (correctly rounded in IEEE 754), so they are the same on every
//! platform: the minimum sine of the dihedral angles (the improvement objective, Klingner and
//! Shewchuk 2007, §2), the circumcentre and the radius–edge ratio (the refinement criterion,
//! Shewchuk 1998). Angles in degrees are for reports only.
//!
//! Sources: J. R. Shewchuk, *What is a good linear finite element? Interpolation, conditioning,
//! anisotropy, and quality measures*, 2002 (the dihedral-angle sine formula
//! `sin θ_ij = 3 V ℓ_ij / (2 A_k A_l)` and the volume–length and radius-ratio measures);
//! B. M. Klingner, J. R. Shewchuk, *Aggressive Tetrahedral Mesh Improvement*, IMR 2007, §2.

use crate::{Point3, Vec3};

/// Six times the signed volume (positive in tet4 order).
pub fn volume6(x: &[Point3; 4]) -> f64 {
    let [a, b, c, d] = *x;
    (b - a).dot((c - a).cross(d - a))
}

/// The cross product of two edges of the face opposite vertex `k` (its length is twice the
/// face's area).
fn face_cross(x: &[Point3; 4], k: usize) -> Vec3 {
    let [a, b, c] = match k {
        0 => [x[1], x[2], x[3]],
        1 => [x[0], x[2], x[3]],
        2 => [x[0], x[1], x[3]],
        _ => [x[0], x[1], x[2]],
    };
    (b - a).cross(c - a)
}

/// Twice the area of the face opposite vertex `k`.
fn face_area2(x: &[Point3; 4], k: usize) -> f64 {
    face_cross(x, k).norm()
}

/// The edges as `(i, j, k, l)`: edge `ij`, and the two other vertices (whose opposite faces meet
/// at the edge).
const EDGE_FACES: [(usize, usize, usize, usize); 6] = [(0, 1, 2, 3), (0, 2, 1, 3), (0, 3, 1, 2), (1, 2, 0, 3), (1, 3, 0, 2), (2, 3, 0, 1)];

/// The sines of the six dihedral angles, signed by orientation (all negative for an inverted
/// tetrahedron), in [`EDGE_FACES`] order; zeros for a degenerate one.
pub fn dihedral_sines(x: &[Point3; 4]) -> [f64; 6] {
    let v6 = volume6(x);
    let areas = [0, 1, 2, 3].map(|k| face_area2(x, k));
    EDGE_FACES.map(|(i, j, k, l)| {
        let len = x[i].distance(x[j]);
        // sin θ = 3 V ℓ / (2 A_k A_l) with V = v6 / 6 and A = area2 / 2.
        let den = areas[k] * areas[l];
        let s = v6 * len / den;
        if s.is_finite() { s } else { 0.0 }
    })
}

/// The cosines of the six dihedral angles (from the outward face normals).
pub fn dihedral_cosines(x: &[Point3; 4]) -> [f64; 6] {
    let normal = |k: usize| -> Vec3 {
        let f: Vec<Point3> = (0..4).filter(|&i| i != k).filter_map(|i| x.get(i).copied()).collect();
        match f.as_slice() {
            [a, b, c] => {
                let n = (*b - *a).cross(*c - *a);
                // Point it away from vertex k.
                let o = x.get(k).copied().unwrap_or(*a);
                if n.dot(o - *a) > 0.0 { n * -1.0 } else { n }
            }
            _ => Vec3::ZERO,
        }
    };
    let n = [0, 1, 2, 3].map(normal);
    EDGE_FACES.map(|(_, _, k, l)| {
        let c = -n[k].dot(n[l]) / (n[k].norm() * n[l].norm());
        if c.is_finite() { c.clamp(-1.0, 1.0) } else { 1.0 }
    })
}

/// The quality objective of a valid tetrahedron: the smaller of its smallest dihedral sine and
/// its weighted scaled Jacobian ([`JACOBIAN_WEIGHT`]; the regular tetrahedron scores
/// √(8/9) ≈ 0.943). The dihedral sine catches slivers and caps; it does not see needles, whose
/// dihedral angles can all be fine while a corner is nearly flat (Shewchuk 2002), and the scaled
/// Jacobian `√2 · 6V / (ℓ₁ℓ₂ℓ₃)` at the worst corner, the measure analysts check (the skill's
/// limit, [`crate::quality`]), does. 0 for a flat tetrahedron; for an inverted one the negative
/// sine of largest magnitude, so worse inversions score lower. Extremes are found on squared
/// lengths and areas, with one square root each.
///
/// The six edge vectors are formed once and shared by the volume, the faces' cross products and
/// both measures. This is the same arithmetic as [`volume6`], [`face_cross`] and the corner
/// products taken separately, bit for bit: IEEE 754 rounding is symmetric, so `u − w = −(w − u)`,
/// `cross(−u, −w) = cross(u, w)`, `dot(−u, w) = −dot(u, w)` and `|−u|² = |u|²` exactly (Rust
/// contracts no multiply–add), and only the repeated work is gone (the improvement evaluates this
/// millions of times).
pub fn quality(x: &[Point3; 4]) -> f64 {
    let [x0, x1, x2, x3] = *x;
    // The edges in EDGE_FACES order, as x[i] − x[j].
    let e = [x0 - x1, x0 - x2, x0 - x3, x1 - x2, x1 - x3, x2 - x3];
    // face_cross(x, 1) = (x2 − x0) × (x3 − x0) = e02 × e03; volume6 = (x1 − x0) · that.
    let c1 = e[1].cross(e[2]);
    let v6 = -e[0].dot(c1);
    if v6 == 0.0 || !v6.is_finite() {
        return 0.0;
    }
    // face_cross(x, k) for k = 0, 1, 2, 3: (x2 − x1) × (x3 − x1), c1, (x1 − x0) × (x3 − x0),
    // (x1 − x0) × (x2 − x0).
    let area2sq = [e[3].cross(e[4]), c1, e[0].cross(e[2]), e[0].cross(e[1])].map(|a| a.dot(a));
    let l2 = e.map(|d| d.dot(d));
    let mut pick = if v6 > 0.0 { f64::INFINITY } else { 0.0 };
    for (&(_, _, k, l), &len2) in EDGE_FACES.iter().zip(&l2) {
        let r = len2 / (area2sq[k] * area2sq[l]);
        pick = if v6 > 0.0 { pick.min(r) } else { pick.max(r) };
    }
    let q = v6 * pick.sqrt();
    let q = if v6 > 0.0 {
        // The corner products of [`corner_products`], from the shared squared lengths in the
        // same order (corner c times its edges to the other corners, ascending; 1 × l = l).
        let [l01, l02, l03, l12, l13, l23] = l2;
        let corners = [l01 * l02 * l03, l01 * l12 * l13, l02 * l12 * l23, l03 * l13 * l23];
        let worst = corners.iter().fold(0.0_f64, |m, &p| m.max(p));
        q.min(JACOBIAN_WEIGHT * (std::f64::consts::SQRT_2 * v6 / worst.sqrt()))
    } else {
        q
    };
    if q.is_finite() { q } else { 0.0 }
}

/// [`quality`] as first written, term by term: the reference its shared-edge form must match bit
/// for bit.
#[cfg(test)]
pub(crate) fn quality_reference(x: &[Point3; 4]) -> f64 {
    let v6 = volume6(x);
    if v6 == 0.0 || !v6.is_finite() {
        return 0.0;
    }
    let area2sq = [0, 1, 2, 3].map(|k| {
        let a = face_cross(x, k);
        a.dot(a)
    });
    let mut pick = if v6 > 0.0 { f64::INFINITY } else { 0.0 };
    for (i, j, k, l) in EDGE_FACES {
        let e = x[i] - x[j];
        let r = e.dot(e) / (area2sq[k] * area2sq[l]);
        pick = if v6 > 0.0 { pick.min(r) } else { pick.max(r) };
    }
    let q = v6 * pick.sqrt();
    let q = if v6 > 0.0 { q.min(JACOBIAN_WEIGHT * scaled_jacobian(x, v6)) } else { q };
    if q.is_finite() { q } else { 0.0 }
}

/// The weight of the scaled Jacobian in [`quality`]: the meshing skill's limit, a scaled
/// Jacobian of 0.3, counts as much as a 30° dihedral angle (sin 30°, the improvement's target,
/// [`super::improve::ImproveOptions::target`]), so the Jacobian decides only for tetrahedra near
/// that limit (needles) and the dihedral sine everywhere else.
pub const JACOBIAN_WEIGHT: f64 = 0.5 / 0.3;

/// The corners of a tetrahedron and the other three corners of each.
#[cfg(test)]
const CORNERS: [(usize, [usize; 3]); 4] = [(0, [1, 2, 3]), (1, [0, 2, 3]), (2, [0, 1, 3]), (3, [0, 1, 2])];

/// The squared product of the three edge lengths at each corner.
#[cfg(test)]
fn corner_products(x: &[Point3; 4]) -> [f64; 4] {
    CORNERS.map(|(c, o)| {
        o.iter().fold(1.0, |m, &j| {
            let e = x[j] - x[c];
            m * e.dot(e)
        })
    })
}

/// The scaled Jacobian at the worst corner of a tetrahedron with six times the volume `v6`
/// (as [`crate::quality`] reports it).
#[cfg(test)]
fn scaled_jacobian(x: &[Point3; 4], v6: f64) -> f64 {
    let worst = corner_products(x).iter().fold(0.0_f64, |m, &p| m.max(p));
    std::f64::consts::SQRT_2 * v6 / worst.sqrt()
}

/// The smallest and largest dihedral angle in degrees (reports only).
pub fn dihedral_range(x: &[Point3; 4]) -> (f64, f64) {
    let (s, c) = (dihedral_sines(x), dihedral_cosines(x));
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for (sv, cv) in s.iter().zip(c) {
        let a = crate::vec3::atan2(sv.abs(), cv).to_degrees();
        lo = lo.min(a);
        hi = hi.max(a);
    }
    (lo, hi)
}

/// The circumcentre, or `None` for a flat tetrahedron.
pub fn circumcenter(x: &[Point3; 4]) -> Option<Point3> {
    let [a, b, c, d] = *x;
    let (b, c, d) = (b - a, c - a, d - a);
    let den = 2.0 * b.dot(c.cross(d));
    if den == 0.0 || !den.is_finite() {
        return None;
    }
    let num = c.cross(d) * b.dot(b) + d.cross(b) * c.dot(c) + b.cross(c) * d.dot(d);
    let o = a + num * (1.0 / den);
    o.is_finite().then_some(o)
}

/// The circumradius (infinite for a flat tetrahedron).
pub fn circumradius(x: &[Point3; 4]) -> f64 {
    circumcenter(x).map_or(f64::INFINITY, |o| o.distance(x[0]))
}

/// The shortest and longest edge.
pub fn edge_range(x: &[Point3; 4]) -> (f64, f64) {
    let mut lo = f64::INFINITY;
    let mut hi = 0.0_f64;
    for (i, j, _, _) in EDGE_FACES {
        let l = x.get(i).zip(x.get(j)).map_or(0.0, |(a, b)| a.distance(*b));
        lo = lo.min(l);
        hi = hi.max(l);
    }
    (lo, hi)
}

/// Circumradius over shortest edge (√6/4 ≈ 0.612 for the regular tetrahedron).
pub fn radius_edge(x: &[Point3; 4]) -> f64 {
    circumradius(x) / edge_range(x).0
}

/// Three times the inradius over the circumradius, signed (1 for the regular tetrahedron).
pub fn radius_ratio(x: &[Point3; 4]) -> f64 {
    let v6 = volume6(x);
    let area2: f64 = (0..4).map(|k| face_area2(x, k)).sum();
    // r = 3V / A_total = (v6 / 2) / (area2 / 2) = v6 / area2.
    let r = v6 / area2;
    let q = 3.0 * r / circumradius(x);
    if q.is_finite() { q } else { 0.0 }
}

/// The centroid.
pub fn centroid(x: &[Point3; 4]) -> Point3 {
    let s = x.iter().fold(Vec3::ZERO, |acc, p| acc + p.to_vec());
    (s * 0.25).to_point()
}

/// The circumcentre of a triangle (in its plane), or `None` when degenerate.
pub fn triangle_circumcenter(a: Point3, b: Point3, c: Point3) -> Option<Point3> {
    let (u, v) = (b - a, c - a);
    let w = u.cross(v);
    let den = 2.0 * w.dot(w);
    if den == 0.0 || !den.is_finite() {
        return None;
    }
    let num = w.cross(u) * v.dot(v) + v.cross(w) * u.dot(u);
    let o = a + num * (1.0 / den);
    o.is_finite().then_some(o)
}

/// The unit normal of a triangle (zero when degenerate).
pub fn triangle_normal(a: Point3, b: Point3, c: Point3) -> Vec3 {
    let n = (b - a).cross(c - a);
    let l = n.norm();
    if l > 0.0 && l.is_finite() { n * (1.0 / l) } else { Vec3::ZERO }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::vec3::pts;

    const REGULAR: [[f64; 3]; 4] = [[1.0, 1.0, 1.0], [1.0, -1.0, -1.0], [-1.0, 1.0, -1.0], [-1.0, -1.0, 1.0]];

    fn positive(x: [[f64; 3]; 4]) -> [Point3; 4] {
        let mut x = pts(x);
        if volume6(&x) < 0.0 {
            x.swap(0, 1);
        }
        x
    }

    #[test]
    fn regular_tetrahedron() {
        let x = positive(REGULAR);
        assert!((quality(&x) - (8.0f64 / 9.0).sqrt()).abs() < 1e-12, "{}", quality(&x));
        assert!((radius_ratio(&x) - 1.0).abs() < 1e-12);
        assert!((radius_edge(&x) - 6f64.sqrt() / 4.0).abs() < 1e-12);
        let (lo, hi) = dihedral_range(&x);
        assert!((lo - 70.528_779).abs() < 1e-5 && (hi - lo).abs() < 1e-9, "{lo} {hi}");
        let o = circumcenter(&x).unwrap();
        assert!(o.to_vec().norm() < 1e-12);
    }

    #[test]
    fn right_corner_and_inverted() {
        let x = pts([[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
        assert!(volume6(&x) > 0.0);
        let (lo, hi) = dihedral_range(&x);
        assert!((hi - 90.0).abs() < 1e-9 && (lo - 54.735_610).abs() < 1e-5, "{lo} {hi}");
        let mut y = x;
        y.swap(1, 2);
        assert!(quality(&y) < 0.0);
        let flat = pts([[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0]]);
        assert_eq!(quality(&flat), 0.0);
        assert!(circumcenter(&flat).is_none());
    }

    /// A needle: every dihedral angle above 20°, yet one corner nearly flat; the objective sees
    /// it through the scaled Jacobian.
    #[test]
    fn a_needle_scores_low() {
        let x = positive([[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.5, 0.866, 0.0], [0.5, 0.29, 12.0]]);
        let (lo, hi) = dihedral_range(&x);
        assert!(lo > 20.0 && hi < 120.0, "{lo} {hi}");
        assert!(quality(&x) < 0.15 * JACOBIAN_WEIGHT, "{}", quality(&x));
    }

    #[test]
    fn sliver_scores_low() {
        let x = positive([[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.01]]);
        assert!(quality(&x) < 0.05 && quality(&x) > 0.0, "{}", quality(&x));
        assert!(radius_ratio(&x) < 0.1);
    }

    /// The shared-edge [`quality`] is the term-by-term formula bit for bit: on random
    /// tetrahedra of every shape and scale, inverted, flat and with repeated or huge corners.
    #[test]
    fn quality_matches_the_term_by_term_formula_bit_for_bit() {
        let mut s = 0x2545_f491_4f6c_dd1du64;
        let mut r = move || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (s >> 11) as f64 / (1u64 << 53) as f64 - 0.5
        };
        let mut seen = [0usize; 3];
        for n in 0..200_000 {
            let scale = [1e-9, 1e-3, 1.0, 1e3, 1e150][n % 5];
            let mut x = [Point3::ORIGIN; 4];
            for p in &mut x {
                for k in 0..3 {
                    p[k] = 0.3 + scale * r();
                }
            }
            match n % 7 {
                // Nearly flat: the fourth corner close to the plane of the others.
                0 => x[3] = scale_mid(x[0], x[1], x[2]) + Vec3::splat(scale * 1e-9 * r()),
                // A repeated corner, and an exactly flat one.
                1 => x[2] = x[1],
                2 => x[3] = x[0].midpoint(x[1]),
                _ => {}
            }
            let (a, b) = (quality(&x), quality_reference(&x));
            assert_eq!(a.to_bits(), b.to_bits(), "{x:?}: {a:e} {b:e}");
            seen[if a > 0.0 {
                0
            } else if a < 0.0 {
                1
            } else {
                2
            }] += 1;
        }
        assert!(seen.iter().all(|&k| k > 1000), "{seen:?}");
    }

    fn scale_mid(a: Point3, b: Point3, c: Point3) -> Point3 {
        ((a.to_vec() + b.to_vec() + c.to_vec()) * (1.0 / 3.0)).to_point()
    }
}
