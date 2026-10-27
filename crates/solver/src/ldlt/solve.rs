//! The solves with the factor: L z = b, D w = z, Lᵀ x = w, supernode by supernode.

use num_complex::Complex64 as C;

use super::front::Pivot;
use super::{Ldlt, Supernode};
use crate::multifrontal::{FactorError, refine};
use crate::sparse::Csr;

impl Ldlt {
    /// Solves A x = b in place.
    pub fn solve(&self, b: &mut [C]) -> Result<(), FactorError> {
        let mut one = [b.to_vec()];
        self.solve_many(&mut one)?;
        b.copy_from_slice(&one[0]);
        Ok(())
    }

    /// Solves A X = B in place for several right-hand sides at once: each pass over the factor
    /// serves them all (the solve reads the whole factor and does little with each entry). Each
    /// column's arithmetic is the single solve's, in the same order.
    pub fn solve_many(&self, bs: &mut [Vec<C>]) -> Result<(), FactorError> {
        let r = bs.len();
        if r == 0 {
            return Ok(());
        }
        if bs.iter().any(|b| b.len() != self.n) {
            return Err(FactorError::Dimension(format!("right-hand sides for {} unknowns", self.n)));
        }
        // y[position · r + j].
        let mut y = vec![C::new(0.0, 0.0); self.n * r];
        for (pos, &u) in self.perm.iter().enumerate() {
            for (j, b) in bs.iter().enumerate() {
                y[pos * r + j] = b[u] * self.scale[u];
            }
        }
        let mut local: Vec<C> = Vec::new();
        for s in &self.supernodes {
            self.forward(s, &mut y, r, &mut local)?;
        }
        for s in &self.supernodes {
            diagonal(s, &mut y, r);
        }
        let mut acc = Vec::new();
        for s in self.supernodes.iter().rev() {
            self.backward(s, &mut y, r, &mut local, &mut acc)?;
        }
        for (pos, &u) in self.perm.iter().enumerate() {
            for (j, b) in bs.iter_mut().enumerate() {
                b[u] = y[pos * r + j] * self.scale[u];
            }
        }
        Ok(())
    }

    /// Supernode `s`'s part of L z = b on the `r` interleaved right-hand sides `y`, through the
    /// buffer `local`.
    fn forward(&self, s: &Supernode, y: &mut [C], r: usize, local: &mut Vec<C>) -> Result<(), FactorError> {
        let zero = C::new(0.0, 0.0);
        let cols = s.l.get(self.scratch.as_deref())?;
        let m = s.rows.len();
        gather(local, y, &s.rows, r);
        let mut p = 0;
        while p < s.k {
            let width = if matches!(s.pivots[p], Pivot::Two(..)) { 2 } else { 1 };
            for c in p..p + width {
                let xc: Vec<C> = local[c * r..c * r + r].to_vec();
                if xc.iter().all(|v| *v == zero) {
                    continue;
                }
                for i in p + width..m {
                    let l = cols[c * m + i];
                    if l == zero {
                        continue;
                    }
                    for (t, x) in local[i * r..i * r + r].iter_mut().zip(&xc) {
                        *t -= l * x;
                    }
                }
            }
            p += width;
        }
        scatter(local, y, &s.rows, r);
        Ok(())
    }

    /// Supernode `s`'s part of Lᵀ x = w, with the buffers `local` and `acc`.
    fn backward(&self, s: &Supernode, y: &mut [C], r: usize, local: &mut Vec<C>, acc: &mut Vec<C>) -> Result<(), FactorError> {
        let zero = C::new(0.0, 0.0);
        let cols = s.l.get(self.scratch.as_deref())?;
        let m = s.rows.len();
        gather(local, y, &s.rows, r);
        acc.clear();
        acc.resize(r, zero);
        let mut p = s.k;
        while p > 0 {
            let width = if p >= 2 && matches!(s.pivots[p - 2], Pivot::Two(..)) { 2 } else { 1 };
            let start = p - width;
            for c in start..p {
                acc.iter_mut().for_each(|a| *a = zero);
                for i in p..m {
                    let l = cols[c * m + i];
                    for (a, x) in acc.iter_mut().zip(&local[i * r..i * r + r]) {
                        *a += l * x;
                    }
                }
                for (t, a) in local[c * r..c * r + r].iter_mut().zip(acc.iter()) {
                    *t -= a;
                }
            }
            p = start;
        }
        scatter(local, y, &s.rows[..s.k], r);
        Ok(())
    }

    /// Solves A x = b with iterative refinement until the backward error
    /// ‖b − A x‖∞ / (‖A‖ ‖x‖∞ + ‖b‖∞) stops falling (at most `steps` corrections); returns x and
    /// that error.
    pub fn solve_refined(&self, a: &Csr, b: &[C], steps: usize) -> Result<(Vec<C>, f64), FactorError> {
        refine(a, b, steps, |x| self.solve(x))
    }
}

/// Supernode `s`'s part of D w = z.
fn diagonal(s: &Supernode, y: &mut [C], r: usize) {
    let mut p = 0;
    while p < s.k {
        match s.pivots[p] {
            Pivot::One(d) => {
                for v in &mut y[s.rows[p] * r..s.rows[p] * r + r] {
                    *v /= d;
                }
                p += 1;
            }
            Pivot::Two(a11, a21, a22) => {
                let (r0, r1) = (s.rows[p], s.rows[p + 1]);
                let det = a11 * a22 - a21 * a21;
                for j in 0..r {
                    let (b0, b1) = (y[r0 * r + j], y[r1 * r + j]);
                    y[r0 * r + j] = (a22 * b0 - a21 * b1) / det;
                    y[r1 * r + j] = (a11 * b1 - a21 * b0) / det;
                }
                p += 2;
            }
            Pivot::Second => p += 1,
        }
    }
}

/// `local` = the `r` values of each of `rows` in `y`.
fn gather(local: &mut Vec<C>, y: &[C], rows: &[usize], r: usize) {
    local.clear();
    for &row in rows {
        local.extend_from_slice(&y[row * r..row * r + r]);
    }
}

/// The values of `local` back to `rows` of `y`.
fn scatter(local: &[C], y: &mut [C], rows: &[usize], r: usize) {
    for (k, &row) in rows.iter().enumerate() {
        y[row * r..row * r + r].copy_from_slice(&local[k * r..k * r + r]);
    }
}
