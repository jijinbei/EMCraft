//! The cyclic Jacobi eigensolver of a small dense real symmetric matrix (G. H. Golub, C. F. Van
//! Loan, *Matrix Computations*, 4th ed., Johns Hopkins 2013, §8.5), shared by the solver's
//! subspace eigensolvers and coupled port modes and by the references' Galerkin problems
//! (`emcraft-theory::curved`). Matrices are row-major `n × n`; the sweeps are capped.
//!
//! ```
//! use emcraft_fem::jacobi::{JacobiStop, jacobi};
//! // [[2, 1], [1, 2]]: eigenvalues 1 and 3, eigenvectors (1, ∓1)/√2.
//! let (values, vectors) = jacobi(vec![2.0, 1.0, 1.0, 2.0], 2, JacobiStop::Relative);
//! let mut sorted = values.clone();
//! sorted.sort_by(f64::total_cmp);
//! assert!((sorted[0] - 1.0).abs() < 1e-14 && (sorted[1] - 3.0).abs() < 1e-14);
//! // Each column is a unit eigenvector.
//! for k in 0..2 {
//!     let (x, y) = (vectors[k], vectors[2 + k]);
//!     assert!((x * x + y * y - 1.0).abs() < 1e-14);
//!     assert!((2.0 * x + y - values[k] * x).abs() < 1e-14);
//! }
//! // Non-finite input does not loop: the sweeps are capped, the result is not finite.
//! let (bad, _) = jacobi(vec![f64::NAN, 1.0, 1.0, 0.0], 2, JacobiStop::Relative);
//! assert!(bad.iter().any(|v| !v.is_finite()));
//! ```

/// When [`jacobi`] stops: after a sweep cap, or once the off-diagonal part is small.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum JacobiStop {
    /// ‖off(A)‖_F ≤ 1e-15 ‖A₀‖_F (the starting matrix's norm), at most 100 sweeps.
    Relative,
    /// ‖off(A)‖²_F ≤ 1e-30 ‖diag(A)‖²_F or off(A) = 0, at most 64 sweeps.
    Diagonal,
}

/// The eigenvalues and eigenvectors (the columns of the row-major n × n result) of a dense
/// symmetric `a` (row-major, n × n) by cyclic Jacobi rotations (G. H. Golub, C. F. Van Loan,
/// *Matrix Computations*, 4th ed., §8.5), until `stop`.
pub fn jacobi(mut a: Vec<f64>, n: usize, stop: JacobiStop) -> (Vec<f64>, Vec<f64>) {
    let mut v = vec![0.0; n * n];
    for i in 0..n {
        v[i * n + i] = 1.0;
    }
    let off_squares =
        |a: &[f64]| -> f64 { (0..n).flat_map(|i| (0..n).filter(move |&j| j != i).map(move |j| (i, j))).map(|(i, j)| a[i * n + j].powi(2)).sum() };
    let norm: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    let sweeps = match stop {
        JacobiStop::Relative => 100,
        JacobiStop::Diagonal => 64,
    };
    for _ in 0..sweeps {
        let done = match stop {
            JacobiStop::Relative => off_squares(&a).sqrt() <= 1e-15 * norm,
            JacobiStop::Diagonal => {
                let off = off_squares(&a);
                let diag: f64 = (0..n).map(|i| a[i * n + i].powi(2)).sum();
                off <= 1e-30 * diag || off == 0.0
            }
        };
        if done {
            break;
        }
        for p in 0..n {
            for q in p + 1..n {
                let apq = a[p * n + q];
                if apq == 0.0 {
                    continue;
                }
                let theta = (a[q * n + q] - a[p * n + p]) / (2.0 * apq);
                let t = if theta == 0.0 { 1.0 } else { theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt()) };
                let c = 1.0 / (t * t + 1.0).sqrt();
                let s = t * c;
                for k in 0..n {
                    let (akp, akq) = (a[k * n + p], a[k * n + q]);
                    a[k * n + p] = c * akp - s * akq;
                    a[k * n + q] = s * akp + c * akq;
                }
                for k in 0..n {
                    let (apk, aqk) = (a[p * n + k], a[q * n + k]);
                    a[p * n + k] = c * apk - s * aqk;
                    a[q * n + k] = s * apk + c * aqk;
                }
                for k in 0..n {
                    let (vkp, vkq) = (v[k * n + p], v[k * n + q]);
                    v[k * n + p] = c * vkp - s * vkq;
                    v[k * n + q] = s * vkp + c * vkq;
                }
            }
        }
    }
    ((0..n).map(|i| a[i * n + i]).collect(), v)
}
