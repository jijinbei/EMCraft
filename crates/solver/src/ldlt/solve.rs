//! The solves with the factor: L z = b, D w = z, Lᵀ x = w, supernode by supernode.

use faer::linalg::matmul::matmul;
use faer::{Accum, MatMut, MatRef, Par};
use num_complex::Complex64 as C;

use super::front::Pivot;
use super::{Ldlt, Supernode};
use crate::multifrontal::{FactorError, refine, refine_many};
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
    /// serves them all (the solve reads the whole factor and does little with each entry): each
    /// supernode's rows below its diagonal block as one matrix product with all of them. The
    /// products run on one thread, so the result does not depend on the machine's.
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
    /// buffer `local`: its diagonal block by columns, then the rows below it at once,
    /// local[k..] −= L₂₁ local[..k] (a matrix product: the factor is read once per pass however
    /// many right-hand sides share it).
    fn forward(&self, s: &Supernode, y: &mut [C], r: usize, local: &mut Vec<C>) -> Result<(), FactorError> {
        let zero = C::new(0.0, 0.0);
        let cols = s.l.get(self.scratch.as_deref())?;
        let (m, k) = (s.rows.len(), s.k);
        if cols.len() != m * k {
            return Err(FactorError::Dimension(format!("a supernode's columns hold {} entries, not {m} × {k}", cols.len())));
        }
        gather(local, y, &s.rows, r);
        let mut p = 0;
        while p < k {
            let width = if matches!(s.pivots[p], Pivot::Two(..)) { 2 } else { 1 };
            for c in p..p + width {
                let xc: Vec<C> = local[c * r..c * r + r].to_vec();
                if xc.iter().all(|v| *v == zero) {
                    continue;
                }
                for i in p + width..k {
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
        if m > k && r > 0 {
            let (top, below) = local.split_at_mut(k * r);
            let l21 = MatRef::from_column_major_slice(&cols, m, k).subrows(k, m - k);
            let x1 = MatRef::from_row_major_slice(top, k, r);
            let dst = MatMut::from_row_major_slice_mut(below, m - k, r);
            matmul(dst, Accum::Add, l21, x1, C::new(-1.0, 0.0), Par::Seq);
        }
        scatter(local, y, &s.rows, r);
        Ok(())
    }

    /// Supernode `s`'s part of Lᵀ x = w, with the buffers `local` and `acc`: the rows below the
    /// diagonal block at once, local[..k] −= L₂₁ᵀ local[k..], then the block by columns.
    fn backward(&self, s: &Supernode, y: &mut [C], r: usize, local: &mut Vec<C>, acc: &mut Vec<C>) -> Result<(), FactorError> {
        let zero = C::new(0.0, 0.0);
        let cols = s.l.get(self.scratch.as_deref())?;
        let (m, k) = (s.rows.len(), s.k);
        if cols.len() != m * k {
            return Err(FactorError::Dimension(format!("a supernode's columns hold {} entries, not {m} × {k}", cols.len())));
        }
        gather(local, y, &s.rows, r);
        if m > k && r > 0 {
            let (top, below) = local.split_at_mut(k * r);
            let l21 = MatRef::from_column_major_slice(&cols, m, k).subrows(k, m - k);
            let x2 = MatRef::from_row_major_slice(&*below, m - k, r);
            let dst = MatMut::from_row_major_slice_mut(top, k, r);
            matmul(dst, Accum::Add, l21.transpose(), x2, C::new(-1.0, 0.0), Par::Seq);
        }
        acc.clear();
        acc.resize(r, zero);
        let mut p = k;
        while p > 0 {
            let width = if p >= 2 && matches!(s.pivots[p - 2], Pivot::Two(..)) { 2 } else { 1 };
            let start = p - width;
            for c in start..p {
                acc.iter_mut().for_each(|a| *a = zero);
                for i in p..k {
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
        scatter(local, y, &s.rows[..k], r);
        Ok(())
    }

    /// Solves A x = b with iterative refinement until the backward error
    /// ‖b − A x‖∞ / (‖A‖ ‖x‖∞ + ‖b‖∞) stops falling (at most `steps` corrections); returns x and
    /// that error.
    pub fn solve_refined(&self, a: &Csr, b: &[C], steps: usize) -> Result<(Vec<C>, f64), FactorError> {
        refine(a, b, steps, |x| self.solve(x))
    }

    /// [`Ldlt::solve_refined`] for several right-hand sides, each pass over the factor serving
    /// them all ([`Ldlt::solve_many`]).
    pub fn solve_refined_many(&self, a: &Csr, bs: &[Vec<C>], steps: usize) -> Result<Vec<(Vec<C>, f64)>, FactorError> {
        refine_many(a, bs, steps, |xs| self.solve_many(xs))
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
