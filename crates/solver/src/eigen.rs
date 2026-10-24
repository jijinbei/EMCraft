//! Eigenmodes of a closed cavity: K x = k² M x with perfectly conducting walls, by shift-invert
//! subspace iteration with Rayleigh–Ritz (K.-J. Bathe, E. L. Wilson, *Large eigenvalue problems
//! in dynamic analysis*, J. Eng. Mech. Div. ASCE 98(6):1471–1485, 1972; K.-J. Bathe, *Finite
//! Element Procedures*, 2nd ed. 2014, §11.6).
//!
//! The curl–curl matrix of Nédélec elements vanishes on every discrete gradient, so k² = 0 is an
//! eigenvalue of multiplicity about the number of nodes (J.-M. Jin, *The Finite Element Method in
//! Electromagnetics*, 3rd ed., Wiley 2014, §8.3). With the shift σ = k_near², iteration on
//! (K − σM)⁻¹M converges to the eigenvalues nearest σ; physical modes in (0, 2σ) are nearer than
//! the gradients' 0, so they come first, and the few gradient vectors that enter the subspace
//! show k² ≈ 0 and are dropped.
//!
//! **Q factors** come by perturbation (D. M. Pozar, *Microwave Engineering*, 4th ed., §6.7):
//! with the lossless mode, Q_c = ωU/P_c from the walls' ½R_s∫|H_t|² and Q_d = ωU/P_d from the
//! dielectrics' ½ωε₀∫ε″|E|², 1/Q = 1/Q_c + 1/Q_d.

use std::f64::consts::PI;

use emcraft_fem::Order;
use emcraft_model::Model;
use emcraft_model::units::C0;
use num_complex::Complex64 as C;

use crate::driven::{Driven, Result, SolveError};
use crate::ldlt::Ldlt;

/// The most modes one analysis may ask for.
pub const MAX_MODES: usize = 50;
/// Iteration cap.
const MAX_ITERATIONS: usize = 200;

/// One eigenmode.
#[derive(Clone, Debug, PartialEq)]
pub struct Mode {
    /// Resonant frequency (Hz).
    pub frequency: f64,
    /// The field coefficients (real), of unit M-norm (xᵀMx = 1).
    pub vector: Vec<C>,
    /// Q of the walls and of the dielectrics (`None` when lossless).
    pub q_walls: Option<f64>,
    pub q_dielectric: Option<f64>,
}

impl Mode {
    /// The total Q, 1/Q = 1/Q_c + 1/Q_d.
    pub fn q(&self) -> Option<f64> {
        let inv = self.q_walls.map_or(0.0, |q| 1.0 / q) + self.q_dielectric.map_or(0.0, |q| 1.0 / q);
        (inv > 0.0).then(|| 1.0 / inv)
    }
}

/// The eigenmodes of a cavity.
#[derive(Clone, Debug)]
pub struct Eigen {
    pub driven: Driven,
    pub modes: Vec<Mode>,
    pub iterations: usize,
}

fn dot_real(a: &[C], b: &[C]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x.re * y.re).sum()
}

/// y = A x for a matrix with values `val` on `pattern` (real part only).
fn mul(pattern: &crate::sparse::Csr, val: impl Fn(usize) -> f64, x: &[C]) -> Vec<C> {
    (0..pattern.n)
        .map(|i| {
            let s: f64 = (pattern.ptr[i]..pattern.ptr[i + 1]).map(|k| val(k) * x[pattern.col[k]].re).sum();
            C::new(s, 0.0)
        })
        .collect()
}

/// Eigenvalues and vectors of a dense symmetric matrix (row-major, n × n) by cyclic Jacobi
/// rotations (Golub & Van Loan, *Matrix Computations*, 4th ed., §8.5).
fn jacobi(mut a: Vec<f64>, n: usize) -> (Vec<f64>, Vec<f64>) {
    let mut v = vec![0.0; n * n];
    for i in 0..n {
        v[i * n + i] = 1.0;
    }
    let norm: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    for _ in 0..100 {
        let off: f64 = (0..n).flat_map(|i| (0..n).filter(move |&j| j != i).map(move |j| (i, j))).map(|(i, j)| a[i * n + j].powi(2)).sum::<f64>().sqrt();
        if off <= 1e-15 * norm {
            break;
        }
        for p in 0..n {
            for q in p + 1..n {
                let apq = a[p * n + q];
                if apq == 0.0 {
                    continue;
                }
                let theta = (a[q * n + q] - a[p * n + p]) / (2.0 * apq);
                let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
                let t = if theta == 0.0 { 1.0 } else { t };
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

/// The `count` modes of the model's guide closed into a cavity nearest above and below
/// `near` (Hz): the lowest ones when `near` is a little above them.
pub fn eigenmodes(model: &Model, order: Order, count: usize, near: f64) -> Result<Eigen> {
    if count == 0 || count > MAX_MODES {
        return Err(SolveError::Invalid { what: "mode count".into(), why: format!("must be 1 to {MAX_MODES}") });
    }
    if !(near.is_finite() && near > 0.0) {
        return Err(SolveError::Invalid { what: "frequency".into(), why: format!("must be a finite number > 0, got {near}") });
    }
    let driven = Driven::cavity(model, order)?;
    let (pattern, stiffness, mass) = driven.matrices();
    let n = pattern.n;
    if n == 0 {
        return Err(SolveError::NotReady("the cavity has no unknowns".into()));
    }
    let k_near = 2.0 * PI * near / C0;
    let sigma = k_near * k_near;
    let mut a = pattern.clone();
    for (k, x) in a.val.iter_mut().enumerate() {
        *x = C::new(stiffness[k] - sigma * mass[k].re, 0.0);
    }
    let factor = Ldlt::factor(&a, &driven.dofs.points)?;
    let q = (count + count.max(8)).min(n);
    // Deterministic starting vectors.
    let mut state: u64 = 0x2545_f491_4f6c_dd1d;
    let mut rand = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 11) as f64 / (1u64 << 53) as f64 - 0.5
    };
    let mut x: Vec<Vec<C>> = (0..q).map(|_| (0..n).map(|_| C::new(rand(), 0.0)).collect()).collect();
    let kmul = |v: &[C]| mul(pattern, |k| stiffness[k], v);
    let mmul = |v: &[C]| mul(pattern, |k| mass[k].re, v);
    let null = 1e-6 * sigma;
    let mut previous: Vec<f64> = Vec::new();
    let mut lambdas = Vec::new();
    let mut iterations = 0;
    for it in 0..MAX_ITERATIONS {
        iterations = it + 1;
        // Y = (K − σM)⁻¹ M X.
        let mut y = Vec::with_capacity(q);
        for v in &x {
            let mut r = mmul(v);
            factor.solve(&mut r)?;
            r.iter_mut().for_each(|c| *c = C::new(c.re, 0.0));
            y.push(r);
        }
        let ky: Vec<Vec<C>> = y.iter().map(|v| kmul(v)).collect();
        let my: Vec<Vec<C>> = y.iter().map(|v| mmul(v)).collect();
        let mut kr = vec![0.0; q * q];
        let mut mr = vec![0.0; q * q];
        for i in 0..q {
            for j in i..q {
                kr[i * q + j] = dot_real(&y[i], &ky[j]);
                mr[i * q + j] = dot_real(&y[i], &my[j]);
                kr[j * q + i] = kr[i * q + j];
                mr[j * q + i] = mr[i * q + j];
            }
        }
        // Mr = L Lᵀ; C = L⁻¹ Kr L⁻ᵀ.
        let mut l: Vec<f64> = vec![0.0; q * q];
        for j in 0..q {
            let d = mr[j * q + j] - (0..j).map(|k| l[j * q + k].powi(2)).sum::<f64>();
            if !(d > 0.0) {
                return Err(SolveError::Invalid { what: "eigen iteration".into(), why: "the subspace lost its independence".into() });
            }
            l[j * q + j] = d.sqrt();
            for i in j + 1..q {
                l[i * q + j] = (mr[i * q + j] - (0..j).map(|k| l[i * q + k] * l[j * q + k]).sum::<f64>()) / l[j * q + j];
            }
        }
        // Z = L⁻¹ Kr, then C = L⁻¹ Zᵀ (C symmetric).
        let lsolve = |b: &[f64]| -> Vec<f64> {
            let mut z = vec![0.0; q];
            for i in 0..q {
                z[i] = (b[i] - (0..i).map(|k| l[i * q + k] * z[k]).sum::<f64>()) / l[i * q + i];
            }
            z
        };
        let mut z = vec![0.0; q * q];
        for j in 0..q {
            let col: Vec<f64> = (0..q).map(|i| kr[i * q + j]).collect();
            let s = lsolve(&col);
            for i in 0..q {
                z[i * q + j] = s[i];
            }
        }
        let mut cm = vec![0.0; q * q];
        for i in 0..q {
            let row: Vec<f64> = (0..q).map(|j| z[i * q + j]).collect();
            let s = lsolve(&row);
            for j in 0..q {
                cm[j * q + i] = s[j];
            }
        }
        let (lam, v) = jacobi(cm, q);
        // Φ = L⁻ᵀ V.
        let mut phi = vec![0.0; q * q];
        for c in 0..q {
            for i in (0..q).rev() {
                let s: f64 = (i + 1..q).map(|k| l[k * q + i] * phi[k * q + c]).sum();
                phi[i * q + c] = (v[i * q + c] - s) / l[i * q + i];
            }
        }
        // Order by distance to σ.
        let mut idx: Vec<usize> = (0..q).collect();
        idx.sort_by(|&p, &r| (lam[p] - sigma).abs().total_cmp(&(lam[r] - sigma).abs()));
        x = idx.iter().map(|&c| (0..n).map(|r| C::new((0..q).map(|k| y[k][r].re * phi[k * q + c]).sum(), 0.0)).collect()).collect();
        lambdas = idx.iter().map(|&c| lam[c]).collect::<Vec<f64>>();
        let wanted: Vec<f64> = lambdas.iter().copied().filter(|l| *l > null).take(count).collect();
        let converged = wanted.len() == count
            && previous.len() == count
            && wanted.iter().zip(&previous).all(|(a, b)| (a - b).abs() <= 1e-11 * a.abs());
        previous = wanted;
        if converged {
            break;
        }
    }
    let omega_of = |l: f64| l.sqrt() * C0;
    let mut modes = Vec::new();
    for (vec, lam) in x.into_iter().zip(&lambdas) {
        if *lam <= null || modes.len() == count {
            continue;
        }
        let frequency = omega_of(*lam) / (2.0 * PI);
        let u = driven.stored_energy(&vec, frequency)?;
        let omega = 2.0 * PI * frequency;
        let q_walls = match model.wall_conductivity {
            Some(s) => Some(omega * u / driven.wall_loss_perturbation(&vec, frequency, s)?),
            None => None,
        };
        let pd = driven.dielectric_loss(&vec, frequency)?;
        let q_dielectric = (pd > 0.0).then(|| omega * u / pd);
        modes.push(Mode { frequency, vector: vec, q_walls, q_dielectric });
    }
    modes.sort_by(|a, b| a.frequency.total_cmp(&b.frequency));
    Ok(Eigen { driven, modes, iterations })
}
