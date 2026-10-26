//! The scattering matrix of a structure at one frequency, and what is read from it: its
//! reciprocity, its passivity, and its waves referred to other impedances.

use std::ops::{Index, IndexMut};

use crate::Complex64 as C;
use crate::driven::complex_solve;

/// S of `n` ports: `S[(p, q)]` (or `S[p][q]`) is the wave out of port p for a unit wave into
/// port q. Square by construction.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SMatrix {
    n: usize,
    /// Row-major.
    entries: Vec<C>,
}

impl SMatrix {
    /// The zero matrix of `n` ports.
    pub fn zeros(n: usize) -> SMatrix {
        SMatrix { n, entries: vec![C::new(0.0, 0.0); n * n] }
    }

    /// The matrix whose entry (p, q) is `f(p, q)`.
    pub fn from_fn(n: usize, mut f: impl FnMut(usize, usize) -> C) -> SMatrix {
        SMatrix { n, entries: (0..n * n).map(|k| f(k / n, k % n)).collect() }
    }

    /// The number of ports.
    pub fn ports(&self) -> usize {
        self.n
    }

    /// Entry (p, q), `None` outside the matrix.
    pub fn get(&self, p: usize, q: usize) -> Option<C> {
        (p < self.n && q < self.n).then(|| self.entries[p * self.n + q])
    }

    /// The rows, port 1's first.
    pub fn rows(&self) -> impl ExactSizeIterator<Item = &[C]> {
        self.entries.chunks_exact(self.n.max(1)).take(self.n)
    }

    /// Every entry with its ports (p, q), row by row.
    pub fn entries(&self) -> impl Iterator<Item = ((usize, usize), C)> + '_ {
        self.entries.iter().enumerate().map(|(k, s)| ((k / self.n, k % self.n), *s))
    }

    /// The transpose: S of the reverse directions.
    pub fn transpose(&self) -> SMatrix {
        SMatrix::from_fn(self.n, |p, q| self[(q, p)])
    }

    /// max |S − Sᵀ|: zero for a reciprocal structure.
    pub fn reciprocity_error(&self) -> f64 {
        self.entries().map(|((p, q), s)| (s - self[(q, p)]).norm()).fold(0.0, f64::max)
    }

    /// The largest singular value: ≤ 1 for a passive structure. By power iteration on SᴴS
    /// (`n` is a port count, so a few hundred products cost nothing).
    pub fn largest_singular_value(&self) -> f64 {
        let n = self.n;
        let mut v = vec![C::new(1.0, 0.0); n];
        let mut sigma2 = 0.0;
        for _ in 0..200 {
            let sv: Vec<C> = (0..n).map(|i| (0..n).map(|j| self[(i, j)] * v[j]).sum()).collect();
            let w: Vec<C> = (0..n).map(|j| (0..n).map(|i| self[(i, j)].conj() * sv[i]).sum()).collect();
            let norm = w.iter().map(|c| c.norm_sqr()).sum::<f64>().sqrt();
            if !(norm > 0.0) {
                break;
            }
            let vn = v.iter().map(|c| c.norm_sqr()).sum::<f64>().sqrt();
            sigma2 = norm / vn.max(f64::MIN_POSITIVE);
            v = w.into_iter().map(|c| c / norm).collect();
        }
        sigma2.sqrt()
    }

    /// S′ = (Q + P S)(P + Q S)⁻¹ for diagonal P and Q (each port's): the waves a′ = P a + Q b,
    /// b′ = Q a + P b. `None` when P + Q S is singular.
    pub fn renormalized(&self, p: &[f64], q: &[f64]) -> Option<SMatrix> {
        let n = self.n;
        let diagonal = |i: usize, j: usize, d: f64| if i == j { C::new(d, 0.0) } else { C::new(0.0, 0.0) };
        // N = Q + P S, D = P + Q S (row i scaled by port i's P, Q); S′ = N D⁻¹.
        let num = SMatrix::from_fn(n, |i, j| p[i] * self[(i, j)] + diagonal(i, j, q[i]));
        let den = SMatrix::from_fn(n, |i, j| q[i] * self[(i, j)] + diagonal(i, j, p[i]));
        // Solve S′ D = N, i.e. Dᵀ S′ᵀ = Nᵀ, column by column of S′ᵀ.
        let dt: Vec<Vec<C>> = (0..n).map(|i| (0..n).map(|j| den[(j, i)]).collect()).collect();
        let rows = (0..n).map(|i| complex_solve(dt.clone(), num[i].to_vec())).collect::<Option<Vec<Vec<C>>>>()?;
        Some(SMatrix { n, entries: rows.concat() })
    }
}

impl Index<(usize, usize)> for SMatrix {
    type Output = C;

    fn index(&self, (p, q): (usize, usize)) -> &C {
        &self.entries[p * self.n + q]
    }
}

impl IndexMut<(usize, usize)> for SMatrix {
    fn index_mut(&mut self, (p, q): (usize, usize)) -> &mut C {
        &mut self.entries[p * self.n + q]
    }
}

/// Row `p`: the waves out of port p.
impl Index<usize> for SMatrix {
    type Output = [C];

    fn index(&self, p: usize) -> &[C] {
        &self.entries[p * self.n..(p + 1) * self.n]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(re: f64, im: f64) -> C {
        C::new(re, im)
    }

    #[test]
    fn entries_read_by_pair_and_by_row() {
        let s = SMatrix::from_fn(2, |p, q| c(p as f64, q as f64));
        assert_eq!(s[(1, 0)], c(1.0, 0.0));
        assert_eq!(s[0][1], c(0.0, 1.0));
        assert_eq!(s.get(2, 0), None);
        assert_eq!(s.rows().count(), 2);
        assert_eq!(SMatrix::zeros(0).rows().count(), 0);
        assert_eq!(s.transpose()[(0, 1)], s[(1, 0)]);
    }

    #[test]
    fn a_lossless_reciprocal_two_port_is_unitary_and_symmetric() {
        // A line section: S11 = S22 = 0, S21 = S12 = e^{−jθ}.
        let t = C::from_polar(1.0, -0.7);
        let s = SMatrix::from_fn(2, |p, q| if p == q { c(0.0, 0.0) } else { t });
        assert!((s.largest_singular_value() - 1.0).abs() < 1e-12);
        assert_eq!(s.reciprocity_error(), 0.0);
        let mut lossy = s.clone();
        lossy[(1, 0)] = t * 0.5;
        assert!((lossy.reciprocity_error() - 0.5).abs() < 1e-12);
    }

    #[test]
    fn renormalizing_a_matched_port_gives_the_impedance_step() {
        // A port of line impedance Z matched (S = 0), referred to R: Γ = (Z − R)/(Z + R).
        let (z, r): (f64, f64) = (50.0, 75.0);
        let root = 2.0 * (z * r).sqrt();
        let s = SMatrix::zeros(1).renormalized(&[(z + r) / root], &[(z - r) / root]).unwrap();
        assert!((s[(0, 0)] - c((z - r) / (z + r), 0.0)).norm() < 1e-15);
        assert_eq!(SMatrix::zeros(1).renormalized(&[1.0], &[0.0]), Some(SMatrix::zeros(1)));
    }
}
