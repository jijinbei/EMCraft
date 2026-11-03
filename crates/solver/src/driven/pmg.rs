//! Second-order systems solved iteratively, preconditioned by their first-order part: restarted
//! GMRES (Y. Saad, M. H. Schultz, *GMRES: a generalized minimal residual algorithm for solving
//! nonsymmetric linear systems*, SIAM J. Sci. Stat. Comput. 7(3):856–869, 1986) on the whole
//! system, right-preconditioned by a two-level p-multigrid cycle.
//!
//! The second-order basis is hierarchical (J. P. Webb, IEEE Trans. AP 47(8), 1999): it holds the
//! Whitney functions, whose block of the system is the first-order system itself. The cycle
//! (Sarkis' and Webb's p-version multilevel idea, here as in J.-F. Lee, D.-K. Sun, *p-Type
//! multiplicative Schwarz (pMUS) method with vector finite elements for modeling
//! three-dimensional waveguide discontinuities*, IEEE Trans. MTT 52(3):864–870, 2004): damped
//! Jacobi on the higher-order unknowns, the first-order block (with the ports' border) solved
//! exactly by its factorization, Jacobi again — so a second-order solve costs about what the
//! first-order factorization does, a sixth of the unknowns, and a few matrix products per
//! iteration. Right preconditioning keeps the residual GMRES minimises the system's own.
//!
//! Deterministic: every product sums each row in one fixed order (rows in parallel), the inner
//! products in index order; iterations are capped ([`MAX_ITERATIONS`]).

use num_complex::Complex64 as C;
use rayon::prelude::*;

use crate::ldlt::Ldlt;
use crate::sparse::Csr;

use super::{Driven, Result, invalid};

/// The Krylov space's dimension before GMRES restarts.
const RESTART: usize = 50;
/// The most iterations of a solve.
pub const MAX_ITERATIONS: usize = 1000;
/// The relative residual ‖b − A x‖/‖b‖ a solve stops at.
pub const TOLERANCE: f64 = 1e-6;
/// The relaxation of the higher-order unknowns' sweeps.
const DAMPING: f64 = 1.0;
/// Gauss–Seidel sweeps before and after the coarse solve.
const SWEEPS: usize = 1;

/// The two-level preconditioner of a system: its first-order unknowns (with the border) and
/// their factorization, the higher-order unknowns' inverse diagonal.
pub(crate) struct TwoLevel {
    coarse: Vec<usize>,
    factor: Ldlt,
    /// Per unknown: its inverse diagonal times the damping on a higher-order one, 0 on a
    /// first-order one.
    jacobi: Vec<C>,
}

/// y = A x, rows in parallel, each row summed in its own order.
fn product(a: &Csr, x: &[C]) -> Vec<C> {
    (0..a.n).into_par_iter().map(|i| a.entries(i).map(|(j, v)| v * x[j]).sum()).collect()
}

/// Σ conj(a) b in index order.
fn dot(a: &[C], b: &[C]) -> C {
    a.iter().zip(b).map(|(x, y)| x.conj() * y).sum()
}

fn norm(a: &[C]) -> f64 {
    a.iter().map(|x| x.norm_sqr()).sum::<f64>().sqrt()
}

impl Driven {
    /// Which of the system's `n + border` unknowns the coarse level takes: every edge's (the
    /// Whitney function and the edge's gradient) and the border's; the faces' are smoothed.
    fn first_order(&self, border: usize) -> Vec<bool> {
        let n = self.dofs.unknowns;
        let pe = self.order.per_edge();
        let mut out = vec![false; n + border];
        for g in 0..self.dofs.edges * pe {
            if let Some(u) = self.dofs.unknown_of(g) {
                out[u] = true;
            }
        }
        for b in out.iter_mut().skip(n) {
            *b = true;
        }
        out
    }

    /// The two-level preconditioner of `a` (a second-order system with `border` port unknowns
    /// last): its first-order block factored.
    pub(crate) fn two_level(&self, a: &Csr, border: usize) -> Result<TwoLevel> {
        let first = self.first_order(border);
        let coarse: Vec<usize> = (0..a.n).filter(|&i| first[i]).collect();
        let mut index = vec![usize::MAX; a.n];
        for (k, &i) in coarse.iter().enumerate() {
            index[i] = k;
        }
        let mut b = Csr::builder(coarse.len());
        for &i in &coarse {
            b.row(a.entries(i).filter(|(j, _)| first[*j]).map(|(j, v)| (index[j], v)));
        }
        let ac = b.finish();
        let n = self.dofs.unknowns;
        let points: Vec<_> = coarse.iter().map(|&i| self.dofs.points.get(i).copied().unwrap_or(emcraft_model::Point3::ORIGIN)).collect();
        let last = coarse.iter().filter(|&&i| i >= n).count();
        let factor = Ldlt::factor_bordered(&ac, &points, crate::ldlt::memory_budget(), last)?;
        let jacobi = (0..a.n)
            .map(|i| {
                let d = a.get(i, i);
                if first[i] || d.norm() == 0.0 { C::new(0.0, 0.0) } else { DAMPING / d }
            })
            .collect();
        Ok(TwoLevel { coarse, factor, jacobi })
    }
}

impl TwoLevel {
    /// One cycle on `r`: a Gauss–Seidel sweep over the higher-order unknowns, the coarse
    /// block solved exactly for the residual left, a sweep back (symmetric, so the cycle is
    /// symmetric too).
    fn apply(&self, a: &Csr, r: &[C]) -> Result<Vec<C>> {
        let mut x: Vec<C> = vec![C::new(0.0, 0.0); r.len()];
        self.sweep(a, r, &mut x, false);
        let ax = product(a, &x);
        let mut rc: Vec<C> = self.coarse.iter().map(|&i| r[i] - ax[i]).collect();
        self.factor.solve(&mut rc)?;
        for (&i, v) in self.coarse.iter().zip(&rc) {
            x[i] += v;
        }
        self.sweep(a, r, &mut x, true);
        Ok(x)
    }

    /// Gauss–Seidel over the higher-order unknowns, in ascending order or back: each takes the
    /// residual of its row with the values as they now are.
    fn sweep(&self, a: &Csr, r: &[C], x: &mut [C], back: bool) {
        let n = x.len();
        for _ in 0..SWEEPS {
            for k in 0..n {
                let i = if back { n - 1 - k } else { k };
                let w = self.jacobi[i];
                if w != C::new(0.0, 0.0) {
                    let ax: C = a.entries(i).map(|(j, v)| v * x[j]).sum();
                    x[i] += w * (r[i] - ax);
                }
            }
        }
    }

    /// Solves `a x = b` by GMRES right-preconditioned by the cycle: the solution and its
    /// relative residual. Refused when it has not reached [`TOLERANCE`] in [`MAX_ITERATIONS`].
    pub(crate) fn solve(&self, a: &Csr, b: &[C]) -> Result<(Vec<C>, f64, usize)> {
        let bn = norm(b);
        let zero = C::new(0.0, 0.0);
        let mut x = vec![zero; b.len()];
        if bn == 0.0 {
            return Ok((x, 0.0, 0));
        }
        let mut iterations = 0;
        let mut residual = 1.0;
        while iterations < MAX_ITERATIONS {
            let ax = product(a, &x);
            let r: Vec<C> = b.iter().zip(&ax).map(|(bi, ai)| bi - ai).collect();
            let beta = norm(&r);
            residual = beta / bn;
            if residual <= TOLERANCE {
                return Ok((x, residual, iterations));
            }
            // Arnoldi with modified Gram–Schmidt; Givens rotations keep the least squares'
            // residual as it goes.
            let mut v: Vec<Vec<C>> = vec![r.iter().map(|ri| ri / beta).collect()];
            let mut h: Vec<Vec<C>> = Vec::new();
            let (mut cs, mut sn): (Vec<C>, Vec<C>) = (Vec::new(), Vec::new());
            let mut g = vec![C::new(beta, 0.0)];
            for j in 0..RESTART {
                iterations += 1;
                let mut w = product(a, &self.apply(a, &v[j])?);
                let mut hj = vec![zero; j + 2];
                for (i, vi) in v.iter().enumerate() {
                    hj[i] = dot(vi, &w);
                    for (wk, vk) in w.iter_mut().zip(vi) {
                        *wk -= hj[i] * vk;
                    }
                }
                let wn = norm(&w);
                hj[j + 1] = C::new(wn, 0.0);
                for i in 0..j {
                    let t = cs[i].conj() * hj[i] + sn[i].conj() * hj[i + 1];
                    hj[i + 1] = -sn[i] * hj[i] + cs[i] * hj[i + 1];
                    hj[i] = t;
                }
                let d = (hj[j].norm_sqr() + hj[j + 1].norm_sqr()).sqrt();
                let (c, s) = if d == 0.0 { (C::new(1.0, 0.0), zero) } else { (hj[j] / d, hj[j + 1] / d) };
                hj[j] = C::new(d, 0.0);
                hj[j + 1] = zero;
                g.push(-s * g[j]);
                g[j] = c.conj() * g[j];
                cs.push(c);
                sn.push(s);
                h.push(hj);
                residual = g[j + 1].norm() / bn;
                if residual <= TOLERANCE || wn == 0.0 || iterations >= MAX_ITERATIONS {
                    break;
                }
                v.push(w.iter().map(|wi| wi / wn).collect());
            }
            // Back substitution for y, then x += M⁻¹ V y (the cycle is linear: one more
            // application, rather than keeping each M⁻¹ v).
            let m = h.len();
            let mut y = vec![zero; m];
            for i in (0..m).rev() {
                let s: C = (i + 1..m).map(|k| h[k][i] * y[k]).sum();
                y[i] = if h[i][i].norm() == 0.0 { zero } else { (g[i] - s) / h[i][i] };
            }
            let mut vy = vec![zero; b.len()];
            for (yk, vk) in y.iter().zip(&v) {
                for (s, vi) in vy.iter_mut().zip(vk) {
                    *s += yk * vi;
                }
            }
            for (xi, zi) in x.iter_mut().zip(self.apply(a, &vy)?) {
                *xi += zi;
            }
        }
        let ax = product(a, &x);
        let r: Vec<C> = b.iter().zip(&ax).map(|(bi, ai)| bi - ai).collect();
        let final_residual = norm(&r) / bn;
        if final_residual <= TOLERANCE * 10.0 {
            return Ok((x, final_residual, iterations));
        }
        Err(invalid(
            "iterative solve",
            format!(
                "did not converge in {MAX_ITERATIONS} iterations (relative residual {final_residual:.1e}, was {residual:.1e}); solve directly or refine the mesh"
            ),
        ))
    }
}
