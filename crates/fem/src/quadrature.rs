//! Quadrature: Gauss–Legendre rules on [0, 1] and collapsed (conical product) rules on
//! triangles and tetrahedra, which map the unit square or cube onto the simplex (A. H. Stroud,
//! *Approximate Calculation of Multiple Integrals*, Prentice-Hall 1971, §2.4; M. G. Duffy,
//! *Quadrature over a pyramid or cube of integrands with a singularity at a vertex*, SIAM J.
//! Numer. Anal. 19(6):1260–1262, 1982). An n-point rule per direction integrates polynomials of
//! degree 2n − 3 exactly on a tetrahedron and 2n − 2 on a triangle, with positive weights.
//!
//! Element matrices integrate exactly without quadrature ([`crate::poly`]); these rules are for
//! the port integrals of the (trigonometric) waveguide modes and for tests.

/// The n-point Gauss–Legendre rule on [0, 1]: nodes and weights. The nodes are the roots of
/// the Legendre polynomial P_n, found by Newton's method from Tricomi's initial guesses.
pub fn gauss_legendre(n: usize) -> (Vec<f64>, Vec<f64>) {
    let n = n.clamp(1, 64);
    let mut x = vec![0.0; n];
    let mut w = vec![0.0; n];
    for i in 0..n {
        let mut z = (std::f64::consts::PI * (i as f64 + 0.75) / (n as f64 + 0.5)).cos();
        let mut dp = 1.0;
        for _ in 0..100 {
            // P_n(z) and P_n'(z) by the three-term recurrence.
            let (mut p0, mut p1) = (1.0, z);
            for k in 2..=n {
                let p2 = ((2 * k - 1) as f64 * z * p1 - (k - 1) as f64 * p0) / k as f64;
                p0 = p1;
                p1 = p2;
            }
            let pn = if n == 1 { z } else { p1 };
            let pm = if n == 1 { 1.0 } else { p0 };
            dp = n as f64 * (z * pn - pm) / (z * z - 1.0);
            let dz = pn / dp;
            z -= dz;
            if dz.abs() < 1e-16 {
                break;
            }
        }
        x[i] = 0.5 * (1.0 - z);
        w[i] = 1.0 / ((1.0 - z * z) * dp * dp);
    }
    (x, w)
}

/// A rule on the tetrahedron with these corners: barycentric coordinates and weights (summing
/// to the volume), n points per direction.
pub fn tet_rule(corners: &[[f64; 3]; 4], n: usize) -> Vec<([f64; 4], f64)> {
    let volume = {
        let d = |k: usize| [corners[k][0] - corners[0][0], corners[k][1] - corners[0][1], corners[k][2] - corners[0][2]];
        let (a, b, c) = (d(1), d(2), d(3));
        (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0])).abs() / 6.0
    };
    let (x, w) = gauss_legendre(n);
    let mut out = Vec::with_capacity(x.len().pow(3));
    for (u, wu) in x.iter().zip(&w) {
        for (v, wv) in x.iter().zip(&w) {
            for (s, ws) in x.iter().zip(&w) {
                let l1 = *u;
                let l2 = v * (1.0 - u);
                let l3 = s * (1.0 - u) * (1.0 - v);
                let jac = (1.0 - u) * (1.0 - u) * (1.0 - v);
                out.push(([1.0 - l1 - l2 - l3, l1, l2, l3], wu * wv * ws * jac * 6.0 * volume));
            }
        }
    }
    out
}

/// A rule on the reference triangle: barycentric coordinates (λ₀, λ₁, λ₂) and weights summing
/// to 1 (multiply by the area).
pub fn triangle_rule(n: usize) -> Vec<([f64; 3], f64)> {
    let (x, w) = gauss_legendre(n);
    let mut out = Vec::with_capacity(x.len().pow(2));
    for (u, wu) in x.iter().zip(&w) {
        for (v, wv) in x.iter().zip(&w) {
            let l1 = *u;
            let l2 = v * (1.0 - u);
            out.push(([1.0 - l1 - l2, l1, l2], wu * wv * (1.0 - u) * 2.0));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::poly::{monomial, monomial_integral};

    #[test]
    fn gauss_legendre_integrates_polynomials() {
        for n in 1..12 {
            let (x, w) = gauss_legendre(n);
            assert!(x.windows(2).all(|p| p[0] < p[1]));
            for d in 0..2 * n {
                let s: f64 = x.iter().zip(&w).map(|(x, w)| w * x.powi(d as i32)).sum();
                assert!((s - 1.0 / (d + 1) as f64).abs() < 1e-14, "n {n} degree {d}: {s}");
            }
        }
    }

    #[test]
    fn simplex_rules_integrate_monomials() {
        let corners = [[0.0, 0.0, 0.0], [2.0, 0.1, 0.0], [0.3, 1.5, 0.2], [0.1, 0.2, 0.9]];
        let n = 4;
        let rule = tet_rule(&corners, n);
        let vol: f64 = rule.iter().map(|r| r.1).sum();
        for alpha in [[0, 0, 0, 0], [1, 2, 0, 1], [0, 0, 3, 2], [2, 1, 1, 1]] {
            let q: f64 = rule.iter().map(|(l, w)| w * monomial(alpha, *l)).sum();
            assert!((q - monomial_integral(alpha, vol)).abs() < 1e-14, "{alpha:?}");
        }
        // Triangle: ∫ λ₀^a λ₁^b λ₂^c = 2A a! b! c! / (a + b + c + 2)!.
        let rule = triangle_rule(5);
        let f = |k: u32| (1..=k).product::<u32>() as f64;
        for (a, b, c) in [(0, 0, 0), (2, 3, 1), (4, 0, 4)] {
            let q: f64 = rule.iter().map(|(l, w)| w * l[0].powi(a) * l[1].powi(b) * l[2].powi(c)).sum();
            let exact = 2.0 * f(a as u32) * f(b as u32) * f(c as u32) / f((a + b + c + 2) as u32);
            assert!((q - exact).abs() < 1e-14, "{a} {b} {c}: {q} {exact}");
        }
    }
}
