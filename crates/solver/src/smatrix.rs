//! The scattering matrix of a structure at one frequency, and what is read from it: its
//! reciprocity, its passivity, and its waves referred to other impedances.

use std::ops::{Index, IndexMut};

use crate::Complex64 as C;
use crate::dense::Lu;

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

    /// max |(SᴴS − 1)_ij|: zero for a lossless structure (S unitary).
    pub fn unitarity_error(&self) -> f64 {
        let n = self.n;
        (0..n * n)
            .map(|k| {
                let (i, j) = (k / n, k % n);
                let v: C = (0..n).map(|r| self[(r, i)].conj() * self[(r, j)]).sum();
                (v - if i == j { 1.0 } else { 0.0 }).norm()
            })
            .fold(0.0, f64::max)
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
            let norm = w.iter().map(num_complex::Complex::norm_sqr).sum::<f64>().sqrt();
            if !(norm > 0.0) {
                break;
            }
            let vn = v.iter().map(num_complex::Complex::norm_sqr).sum::<f64>().sqrt();
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
        let dt = Lu::new((0..n * n).map(|k| den[(k % n, k / n)]).collect(), n)?;
        let rows: Vec<Vec<C>> = (0..n).map(|i| dt.solve(&num[i])).collect();
        Some(SMatrix { n, entries: rows.concat() })
    }
}

impl SMatrix {
    /// S′ = (Q + P S)(P + Q S)⁻¹ for full P and Q (n × n, row-major): the waves a′ = P a + Q b,
    /// b′ = Q a + P b, mixing the ports (a terminal port's modes into its conductors' waves).
    /// Also (P + Q S)⁻¹, row-major, whose column k holds the old incident waves of a unit new
    /// incident wave at k (the others zero). `None` when P + Q S is singular or a size is wrong.
    pub fn transformed(&self, p: &[C], q: &[C]) -> Option<(SMatrix, Vec<C>)> {
        let n = self.n;
        if p.len() != n * n || q.len() != n * n {
            return None;
        }
        // D = P + Q S, N = Q + P S.
        let product = |l: &[C], i: usize, j: usize| (0..n).map(|k| l[i * n + k] * self[(k, j)]).sum::<C>();
        let den = Lu::new((0..n * n).map(|k| p[k] + product(q, k / n, k % n)).collect(), n)?;
        let num: Vec<C> = (0..n * n).map(|k| q[k] + product(p, k / n, k % n)).collect();
        // X = D⁻¹, column by column.
        let mut inv = vec![C::new(0.0, 0.0); n * n];
        for k in 0..n {
            let e: Vec<C> = (0..n).map(|i| C::new(if i == k { 1.0 } else { 0.0 }, 0.0)).collect();
            let col = den.solve(&e);
            for (i, x) in col.into_iter().enumerate() {
                inv[i * n + k] = x;
            }
        }
        let s = SMatrix::from_fn(n, |i, j| (0..n).map(|k| num[i * n + k] * inv[k * n + j]).sum());
        s.entries.iter().all(|c| c.re.is_finite() && c.im.is_finite()).then_some((s, inv))
    }
}

/// N D⁻¹ for n × n row-major N and D: Dᵀ Xᵀ = Nᵀ solved row by row of N. `None` when D is
/// singular or a size is wrong.
fn right_divide(num: &[C], den: &[C], n: usize) -> Option<Vec<C>> {
    if num.len() != n * n || den.len() != n * n {
        return None;
    }
    let dt = Lu::new((0..n * n).map(|k| den[(k % n) * n + k / n]).collect(), n)?;
    let x: Vec<C> = (0..n).flat_map(|i| dt.solve(&num[i * n..(i + 1) * n])).collect();
    x.iter().all(|c| c.re.is_finite() && c.im.is_finite()).then_some(x)
}

/// Whether reference impedances `z0` are one finite positive resistance per port of `n`.
fn references_fit(z0: &[f64], n: usize) -> bool {
    z0.len() == n && z0.iter().all(|z| z.is_finite() && *z > 0.0)
}

/// The network matrices read from S (Pozar, Microwave Engineering, 4th ed., §4.3–4.4): each
/// port's reference impedance `z0` real and positive (Ω), its waves power waves on it.
impl SMatrix {
    /// The impedance matrix Z = √z₀ (I + S)(I − S)⁻¹ √z₀ (Ω), row-major; `None` when I − S is
    /// singular (a port open-circuited by the rest) or `z0` does not fit.
    ///
    /// ```
    /// use emcraft_solver::{Complex64 as C, SMatrix};
    ///
    /// // A 50 Ω line a quarter wave long, matched: Z11 = 0 and Z21 = −j 50 Ω (Z = −j Z₀ / sin θ).
    /// let s = SMatrix::from_fn(2, |p, q| if p == q { C::new(0.0, 0.0) } else { C::new(0.0, -1.0) });
    /// let z = s.impedance(&[50.0, 50.0]).unwrap();
    /// assert!(z[0].norm() < 1e-12 && (z[2] - C::new(0.0, -50.0)).norm() < 1e-12);
    /// // A thru (S21 = 1) has no Z; a negative reference is refused.
    /// assert!(SMatrix::from_fn(2, |p, q| C::new(if p == q { 0.0 } else { 1.0 }, 0.0)).impedance(&[50.0; 2]).is_none());
    /// assert!(s.impedance(&[50.0, -50.0]).is_none());
    /// ```
    pub fn impedance(&self, z0: &[f64]) -> Option<Vec<C>> {
        let n = self.n;
        if !references_fit(z0, n) {
            return None;
        }
        let eye = |i: usize, j: usize| C::new(if i == j { 1.0 } else { 0.0 }, 0.0);
        let plus: Vec<C> = (0..n * n).map(|k| eye(k / n, k % n) + self.entries[k]).collect();
        let minus: Vec<C> = (0..n * n).map(|k| eye(k / n, k % n) - self.entries[k]).collect();
        let zn = right_divide(&plus, &minus, n)?;
        Some((0..n * n).map(|k| zn[k] * (z0[k / n] * z0[k % n]).sqrt()).collect())
    }

    /// The admittance matrix Y = Z⁻¹ = (1/√z₀)(I − S)(I + S)⁻¹(1/√z₀) (S), row-major; `None`
    /// when I + S is singular (a port shorted by the rest) or `z0` does not fit.
    pub fn admittance(&self, z0: &[f64]) -> Option<Vec<C>> {
        let n = self.n;
        if !references_fit(z0, n) {
            return None;
        }
        let eye = |i: usize, j: usize| C::new(if i == j { 1.0 } else { 0.0 }, 0.0);
        let plus: Vec<C> = (0..n * n).map(|k| eye(k / n, k % n) + self.entries[k]).collect();
        let minus: Vec<C> = (0..n * n).map(|k| eye(k / n, k % n) - self.entries[k]).collect();
        let yn = right_divide(&minus, &plus, n)?;
        Some((0..n * n).map(|k| yn[k] / (z0[k / n] * z0[k % n]).sqrt()).collect())
    }

    /// A two-port's chain (ABCD) matrix [A, B, C, D] (B in Ω, C in S); `None` for another
    /// number of ports, S21 = 0 or `z0` that does not fit.
    ///
    /// ```
    /// use emcraft_solver::{Complex64 as C, SMatrix};
    ///
    /// // A matched line of electrical length θ: A = D = cos θ, B = j Z₀ sin θ, C = j sin θ / Z₀.
    /// let theta: f64 = 0.4;
    /// let s = SMatrix::from_fn(2, |p, q| if p == q { C::new(0.0, 0.0) } else { C::from_polar(1.0, -theta) });
    /// let [a, b, c, d] = s.chain([50.0, 50.0]).unwrap();
    /// assert!((a - C::new(theta.cos(), 0.0)).norm() < 1e-12 && (d - a).norm() < 1e-12);
    /// assert!((b - C::new(0.0, 50.0 * theta.sin())).norm() < 1e-12);
    /// assert!((c - C::new(0.0, theta.sin() / 50.0)).norm() < 1e-12);
    /// ```
    pub fn chain(&self, z0: [f64; 2]) -> Option<[C; 4]> {
        if self.n != 2 || !references_fit(&z0, 2) {
            return None;
        }
        let [s11, s12, s21, s22] = [self[(0, 0)], self[(0, 1)], self[(1, 0)], self[(1, 1)]];
        if s21.norm() == 0.0 {
            return None;
        }
        let one = C::new(1.0, 0.0);
        let (r1, r2) = (z0[0], z0[1]);
        let k = 2.0 * s21;
        let abcd = [
            ((one + s11) * (one - s22) + s12 * s21) / k * (r1 / r2).sqrt(),
            ((one + s11) * (one + s22) - s12 * s21) / k * (r1 * r2).sqrt(),
            ((one - s11) * (one - s22) - s12 * s21) / k / (r1 * r2).sqrt(),
            ((one - s11) * (one + s22) + s12 * s21) / k * (r2 / r1).sqrt(),
        ];
        abcd.iter().all(|c| c.re.is_finite() && c.im.is_finite()).then_some(abcd)
    }

    /// S from an impedance matrix `z` (Ω, row-major) on references `z0`:
    /// S = (z̄ − I)(z̄ + I)⁻¹ with z̄ = Z/√(z₀ᵢ z₀ⱼ). `None` when z̄ + I is singular or a size or
    /// reference does not fit.
    pub fn from_impedance(z: &[C], z0: &[f64]) -> Option<SMatrix> {
        let n = z0.len();
        if z.len() != n * n || !references_fit(z0, n) {
            return None;
        }
        let eye = |i: usize, j: usize| C::new(if i == j { 1.0 } else { 0.0 }, 0.0);
        let zn: Vec<C> = (0..n * n).map(|k| z[k] / (z0[k / n] * z0[k % n]).sqrt()).collect();
        let num: Vec<C> = (0..n * n).map(|k| zn[k] - eye(k / n, k % n)).collect();
        let den: Vec<C> = (0..n * n).map(|k| zn[k] + eye(k / n, k % n)).collect();
        Some(SMatrix { n, entries: right_divide(&num, &den, n)? })
    }

    /// S from an admittance matrix `y` (S, row-major) on references `z0`:
    /// S = (I − ȳ)(I + ȳ)⁻¹ with ȳ = Y √(z₀ᵢ z₀ⱼ).
    pub fn from_admittance(y: &[C], z0: &[f64]) -> Option<SMatrix> {
        let n = z0.len();
        if y.len() != n * n || !references_fit(z0, n) {
            return None;
        }
        let eye = |i: usize, j: usize| C::new(if i == j { 1.0 } else { 0.0 }, 0.0);
        let yn: Vec<C> = (0..n * n).map(|k| y[k] * (z0[k / n] * z0[k % n]).sqrt()).collect();
        let num: Vec<C> = (0..n * n).map(|k| eye(k / n, k % n) - yn[k]).collect();
        let den: Vec<C> = (0..n * n).map(|k| eye(k / n, k % n) + yn[k]).collect();
        Some(SMatrix { n, entries: right_divide(&num, &den, n)? })
    }

    /// The mixed-mode S of ports paired as `pairs` (each a positive and a negative port, every
    /// port in at most one pair): S_mm = M S Mᵀ (Bockelman & Eisenstadt, IEEE Trans. MTT 43(7),
    /// 1995), its entries ordered as [`mixed_order`] gives them — the differential waves
    /// (a₊ − a₋)/√2 of each pair, then their common waves (a₊ + a₋)/√2, then the unpaired
    /// ports. M is orthogonal, so the waves keep their power. `None` when a pair does not fit.
    ///
    /// ```
    /// use emcraft_solver::{Complex64 as C, SMatrix};
    ///
    /// // Two uncoupled thru lines 1→3 and 2→4 (the pairs (1, 2) and (3, 4)): the differential
    /// // and the common mode both go through whole, with no conversion between them.
    /// let s = SMatrix::from_fn(4, |p, q| C::new(if p == (q + 2) % 4 { 1.0 } else { 0.0 }, 0.0));
    /// let m = s.mixed_mode(&[(0, 1), (2, 3)]).unwrap();
    /// // Entries d1 d2 c1 c2: Sdd21 = Scc21 = 1, Scd21 = 0.
    /// assert!((m[(1, 0)] - C::new(1.0, 0.0)).norm() < 1e-12 && (m[(3, 2)] - C::new(1.0, 0.0)).norm() < 1e-12);
    /// assert!(m[(3, 0)].norm() < 1e-12);
    /// assert!(s.mixed_mode(&[(0, 0)]).is_none());
    /// ```
    pub fn mixed_mode(&self, pairs: &[(usize, usize)]) -> Option<SMatrix> {
        let n = self.n;
        let order = mixed_order(n, pairs)?;
        let h = std::f64::consts::FRAC_1_SQRT_2;
        // Row r of M: the new wave r in the old ports' waves.
        let mut m = vec![0.0; n * n];
        for (r, w) in order.iter().enumerate() {
            match *w {
                MixedWave::Differential(k) => {
                    m[r * n + pairs[k].0] = h;
                    m[r * n + pairs[k].1] = -h;
                }
                MixedWave::Common(k) => {
                    m[r * n + pairs[k].0] = h;
                    m[r * n + pairs[k].1] = h;
                }
                MixedWave::Single(p) => m[r * n + p] = 1.0,
            }
        }
        let ms: Vec<C> = (0..n * n).map(|k| (0..n).map(|j| self.entries[j * n + k % n] * m[(k / n) * n + j]).sum()).collect();
        Some(SMatrix::from_fn(n, |i, j| (0..n).map(|k| ms[i * n + k] * m[j * n + k]).sum()))
    }
}

/// A wave of the mixed-mode basis: a pair's differential or common wave, or an unpaired port's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MixedWave {
    /// Pair k's differential wave.
    Differential(usize),
    /// Pair k's common wave.
    Common(usize),
    /// Port p, unpaired.
    Single(usize),
}

/// The mixed-mode entries of `n` ports paired as `pairs`: every pair's differential wave, then
/// every pair's common wave, then the ports left unpaired in order. `None` when a pair names a
/// port twice or one out of range.
pub fn mixed_order(n: usize, pairs: &[(usize, usize)]) -> Option<Vec<MixedWave>> {
    let mut used = vec![false; n];
    for &(a, b) in pairs {
        if a == b || a >= n || b >= n || used[a] || used[b] {
            return None;
        }
        used[a] = true;
        used[b] = true;
    }
    let k = pairs.len();
    let singles = (0..n).filter(|p| !used[*p]).map(MixedWave::Single);
    Some((0..k).map(MixedWave::Differential).chain((0..k).map(MixedWave::Common)).chain(singles).collect())
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
        approx::assert_abs_diff_eq!(s.largest_singular_value(), 1.0, epsilon = 1e-12);
        assert_eq!(s.reciprocity_error(), 0.0);
        let mut lossy = s;
        lossy[(1, 0)] = t * 0.5;
        approx::assert_abs_diff_eq!(lossy.reciprocity_error(), 0.5, epsilon = 1e-12);
    }

    #[test]
    fn z_y_and_s_go_round_to_where_they_started() {
        // A lossy, non-reciprocal three-port on unequal references: Z then back, Y then back.
        let s = SMatrix::from_fn(3, |p, q| c(0.1 * (p as f64 + 1.0) - 0.05 * q as f64, 0.07 * (p as f64 - q as f64) + 0.02));
        let z0 = [50.0, 75.0, 25.0];
        let back = SMatrix::from_impedance(&s.impedance(&z0).unwrap(), &z0).unwrap();
        let again = SMatrix::from_admittance(&s.admittance(&z0).unwrap(), &z0).unwrap();
        for ((_, a), ((_, b), (_, d))) in s.entries().zip(back.entries().zip(again.entries())) {
            assert!((a - b).norm() < 1e-12 && (a - d).norm() < 1e-12);
        }
        // Y Z = I.
        let (z, y) = (s.impedance(&z0).unwrap(), s.admittance(&z0).unwrap());
        for i in 0..3 {
            for j in 0..3 {
                let e: C = (0..3).map(|k| y[i * 3 + k] * z[k * 3 + j]).sum();
                assert!((e - c(if i == j { 1.0 } else { 0.0 }, 0.0)).norm() < 1e-12);
            }
        }
    }

    #[test]
    fn a_shunt_resistor_reads_as_its_impedance() {
        // R across a 50 Ω line: S11 = −Z0/(2R + Z0), S21 = 2R/(2R + Z0) (Pozar Table 4.2's
        // ABCD [1, 0, 1/R, 1]); Z is R in every entry.
        let (r, z0): (f64, f64) = (100.0, 50.0);
        let (g, t) = (-z0 / (2.0 * r + z0), 2.0 * r / (2.0 * r + z0));
        let s = SMatrix::from_fn(2, |p, q| c(if p == q { g } else { t }, 0.0));
        let z = s.impedance(&[z0, z0]).unwrap();
        assert!(z.iter().all(|e| (e - c(r, 0.0)).norm() < 1e-9), "{z:?}");
        let [a, b, cc, d] = s.chain([z0, z0]).unwrap();
        assert!((a - c(1.0, 0.0)).norm() < 1e-12 && b.norm() < 1e-9 && (cc - c(1.0 / r, 0.0)).norm() < 1e-12 && (d - c(1.0, 0.0)).norm() < 1e-12);
    }

    #[test]
    fn a_pair_driven_apart_or_together_converts_by_its_imbalance() {
        // Two thru lines 1→3 and 2→4 of transmissions t and u: Sdd21 = Scc21 = (t + u)/2 and
        // the conversion Scd21 = (t − u)/2 (Bockelman & Eisenstadt 1995, eq. 13).
        let (t, u) = (C::from_polar(0.9, -1.0), C::from_polar(0.8, -1.2));
        let s = SMatrix::from_fn(4, |p, q| match (p, q) {
            (2, 0) | (0, 2) => t,
            (3, 1) | (1, 3) => u,
            _ => c(0.0, 0.0),
        });
        let m = s.mixed_mode(&[(0, 1), (2, 3)]).unwrap();
        assert!((m[(1, 0)] - (t + u) / 2.0).norm() < 1e-12);
        assert!((m[(3, 2)] - (t + u) / 2.0).norm() < 1e-12);
        assert!((m[(3, 0)] - (t - u) / 2.0).norm() < 1e-12);
        // Power kept: a lossless S stays unitary.
        let lossless = SMatrix::from_fn(4, |p, q| c(if p == (q + 2) % 4 { 1.0 } else { 0.0 }, 0.0));
        assert!(lossless.mixed_mode(&[(0, 1), (2, 3)]).unwrap().unitarity_error() < 1e-12);
        // An unpaired port keeps its place after the pairs' waves.
        assert_eq!(mixed_order(3, &[(0, 2)]), Some(vec![MixedWave::Differential(0), MixedWave::Common(0), MixedWave::Single(1)]));
        assert!(mixed_order(4, &[(0, 1), (1, 2)]).is_none());
        assert!(mixed_order(2, &[(0, 2)]).is_none());
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
