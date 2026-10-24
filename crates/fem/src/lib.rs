//! EMCraft's element kernels: Nédélec edge elements of orders 1 and 2 on tetrahedra
//! ([`nedelec`]) with exact element matrices ([`poly`]), quadrature rules ([`quadrature`]) and the
//! modes of rectangular and circular waveguide ports ([`modes`], with [`bessel`]). Pure functions, no solver loop.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod bessel;
pub mod modes;
pub mod nedelec;
pub mod poly;
pub mod quadrature;

pub use nedelec::{Element, ElementError, Order};

/// Solves a small dense system `a x = b` (row-major, n × n) by Gaussian elimination with
/// partial pivoting; `None` when it is singular.
pub fn dense_solve(mut a: Vec<f64>, mut b: Vec<f64>) -> Option<Vec<f64>> {
    let n = b.len();
    if a.len() != n * n {
        return None;
    }
    let scale = a.iter().fold(0.0f64, |m, x| m.max(x.abs()));
    for k in 0..n {
        let p = (k..n).max_by(|&i, &j| a[i * n + k].abs().total_cmp(&a[j * n + k].abs()))?;
        if !(a[p * n + k].abs() > 1e-14 * scale) {
            return None;
        }
        if p != k {
            for j in 0..n {
                a.swap(k * n + j, p * n + j);
            }
            b.swap(k, p);
        }
        for i in k + 1..n {
            let f = a[i * n + k] / a[k * n + k];
            for j in k..n {
                a[i * n + j] -= f * a[k * n + j];
            }
            b[i] -= f * b[k];
        }
    }
    let mut x = vec![0.0; n];
    for k in (0..n).rev() {
        let s: f64 = (k + 1..n).map(|j| a[k * n + j] * x[j]).sum();
        x[k] = (b[k] - s) / a[k * n + k];
    }
    Some(x)
}
