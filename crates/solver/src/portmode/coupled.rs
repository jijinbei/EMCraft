//! Cross-sections with several conductors besides the ground (coupled strips): their quasi-TEM
//! modes, one per conductor, and each mode's impedance.
//!
//! In one homogeneous dielectric the TEM modes of N conductors share β = k₀√εr, so the
//! eigensolver returns some basis of their space. The basis a circuit needs is the one of the
//! line's impedance matrix Z (V = Z I): its eigenvectors are the modes whose conductor currents
//! are mutually orthogonal (for two like strips the even and the odd mode, currents (1, 1) and
//! (1, −1); C. R. Paul, *Analysis of Multiconductor Transmission Lines*, 2nd ed., Wiley 2008,
//! §§5.2, 7.2: L and C of a homogeneous line share their eigenvectors). With J the conductors'
//! currents of the solved modes and D their power form (D_ij = ∫ μr⁻¹ e_i · g_j), the combinations
//! q solve J ᵀJ q = λ D q: power-orthogonal (D-orthogonal) as the port's projection needs, their
//! currents J q orthogonal. Then 2P/|I|² is Z's eigenvalue, each mode's impedance per conductor
//! (Z₀ₑ and Z₀ₒ; S. B. Cohn, IRE Trans. MTT 3(5):29–38, 1955). Modes of distinct β (an
//! inhomogeneous line, a microstrip's c and π modes) are left as they are.

use emcraft_model::{Axis, Symmetry};

use super::{CrossSection, PortMode};
use crate::driven::{Result, invalid};

/// β² within this fraction of the largest are one degenerate cluster.
const DEGENERATE: f64 = 1e-8;
/// Sweeps of the Jacobi eigenvalue iteration.
const MAX_SWEEPS: usize = 64;

impl CrossSection {
    /// The `count` modes with the largest β² as [`CrossSection::modes`] finds them, each cluster
    /// of degenerate propagating modes on two or more conductors rotated to the impedance
    /// matrix's eigenvectors (see the module docs), the highest impedance first.
    pub fn modes_split(&self, k0: f64, count: usize) -> Result<Vec<PortMode>> {
        let mut modes = self.modes(k0, count)?;
        if self.conductors().len() < 2 {
            return Ok(modes);
        }
        let top = modes.iter().map(|m| m.beta2.abs()).fold(0.0, f64::max);
        let mut start = 0;
        while start < modes.len() {
            let mut end = start + 1;
            while end < modes.len() && (modes[end].beta2 - modes[start].beta2).abs() <= DEGENERATE * top {
                end += 1;
            }
            if end - start >= 2 && modes[start].beta2 > 0.0 {
                let split = self.split(&modes[start..end], k0)?;
                modes.splice(start..end, split);
            }
            start = end;
        }
        Ok(modes)
    }

    /// The degenerate `cluster` rotated to orthogonal conductor currents.
    fn split(&self, cluster: &[PortMode], k0: f64) -> Result<Vec<PortMode>> {
        let m = cluster.len();
        let currents =
            cluster.iter().map(|md| self.currents(md, k0).map(|(i, _)| i.iter().map(|c| c.re).collect::<Vec<_>>())).collect::<Result<Vec<_>>>()?;
        // G = Jᵀ J and the power form D, both symmetric.
        let mut g = vec![0.0; m * m];
        let mut d = vec![0.0; m * m];
        for i in 0..m {
            for j in 0..m {
                g[i * m + j] = currents[i].iter().zip(&currents[j]).map(|(a, b)| a * b).sum();
                d[i * m + j] = self.power_form(&cluster[i], &cluster[j])?;
            }
        }
        for i in 0..m {
            for j in 0..i {
                let s = 0.5 * (d[i * m + j] + d[j * m + i]);
                (d[i * m + j], d[j * m + i]) = (s, s);
            }
        }
        // D = L Lᵀ; M = L⁻¹ G L⁻ᵀ; its eigenvectors y give q = L⁻ᵀ y.
        let l = cholesky(&d, m).ok_or_else(|| invalid("port modes", "degenerate modes without positive power"))?;
        let linv_g: Vec<Vec<f64>> = (0..m).map(|j| lower_solve(&l, m, &(0..m).map(|i| g[i * m + j]).collect::<Vec<_>>())).collect();
        // linv_g[j] is column j of L⁻¹G; M's column j is L⁻¹ (row j of L⁻¹G)ᵀ by symmetry of G.
        let rows: Vec<Vec<f64>> = (0..m).map(|i| (0..m).map(|j| linv_g[j][i]).collect()).collect();
        let mcols: Vec<Vec<f64>> = (0..m).map(|i| lower_solve(&l, m, &rows[i])).collect();
        let mut mat = vec![0.0; m * m];
        for i in 0..m {
            for j in 0..m {
                mat[i * m + j] = 0.5 * (mcols[i][j] + mcols[j][i]);
            }
        }
        let (values, vectors) = jacobi(mat, m);
        // λ = |I|²/(2P) up to a constant: the highest impedance (smallest λ) first.
        let mut order: Vec<usize> = (0..m).collect();
        order.sort_by(|a, b| values[*a].total_cmp(&values[*b]).then(a.cmp(b)));
        let beta2 = cluster.iter().map(|md| md.beta2).sum::<f64>() / m as f64;
        order
            .into_iter()
            .map(|k| {
                let y: Vec<f64> = (0..m).map(|i| vectors[i * m + k]).collect();
                let q = upper_solve(&l, m, &y);
                let n = cluster[0].x.len();
                let x: Vec<f64> = (0..n).map(|r| (0..m).map(|i| q[i] * cluster[i].x.get(r).copied().unwrap_or(0.0)).sum()).collect();
                self.finish(beta2, x)
            })
            .collect()
    }

    /// ∫ μr⁻¹ e_a · g_b dS of two modes.
    fn power_form(&self, a: &PortMode, b: &PortMode) -> Result<f64> {
        let mut sum = 0.0;
        for t in 0..self.tris.len() {
            let tri = self.triangle(t)?;
            for (l, w) in tri.rule() {
                let (e, _) = self.at(&tri, t, &a.x, l);
                let (_, g) = self.at(&tri, t, &b.x, l);
                sum += w * e.dot(g) / self.mu[t];
            }
        }
        Ok(sum)
    }

    /// The mode's impedance per conductor, Z = 2P / Σ|I_c|² (Ω), of the whole line when the
    /// cross-section is the part `planes` keep: each plane halves the power, and a conductor a
    /// plane cuts carries half its current (P_whole = 2ⁿ P, Σ_whole |I|² = Σ_c 2^{n + cuts_c}
    /// |I_c|²). `None` without conductors or for an evanescent mode. One conductor, no planes:
    /// [`CrossSection::impedance`].
    pub fn impedance_whole(&self, mode: &PortMode, k0: f64, planes: &[Symmetry]) -> Result<Option<f64>> {
        if mode.beta2 <= 0.0 {
            return Ok(None);
        }
        let (currents, power) = self.currents(mode, k0)?;
        let conductors = self.conductors();
        let (lo, hi) = self
            .nodes
            .iter()
            .fold(([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]), |(lo, hi), p| ([lo[0].min(p.x), lo[1].min(p.y)], [hi[0].max(p.x), hi[1].max(p.y)]));
        let mut sum = 0.0;
        for (i, nodes) in currents.iter().zip(&conductors) {
            let cuts = planes
                .iter()
                .filter(|pl| {
                    let d = match pl.axis {
                        Axis::X => 0,
                        Axis::Y => 1,
                        Axis::Z => return false,
                    };
                    let tol = 1e-9 * (hi[d] - lo[d]).abs().max(f64::MIN_POSITIVE);
                    nodes.iter().any(|&v| self.nodes.get(v as usize).is_some_and(|p| ((if d == 0 { p.x } else { p.y }) - pl.at).abs() <= tol))
                })
                .count();
            sum += 2f64.powi(cuts as i32) * i.norm_sqr();
        }
        Ok(if sum > 0.0 && sum.is_finite() { Some(2.0 * power / sum) } else { None })
    }
}

/// D = L Lᵀ for symmetric positive definite `d` (m × m, row-major); `None` otherwise.
fn cholesky(d: &[f64], m: usize) -> Option<Vec<f64>> {
    let mut l = vec![0.0; m * m];
    for j in 0..m {
        let s = d[j * m + j] - (0..j).map(|k| l[j * m + k] * l[j * m + k]).sum::<f64>();
        if !(s > 0.0 && s.is_finite()) {
            return None;
        }
        l[j * m + j] = s.sqrt();
        for i in j + 1..m {
            l[i * m + j] = (d[i * m + j] - (0..j).map(|k| l[i * m + k] * l[j * m + k]).sum::<f64>()) / l[j * m + j];
        }
    }
    Some(l)
}

/// L⁻¹ b.
fn lower_solve(l: &[f64], m: usize, b: &[f64]) -> Vec<f64> {
    let mut x = b.to_vec();
    for i in 0..m {
        x[i] = (x[i] - (0..i).map(|k| l[i * m + k] * x[k]).sum::<f64>()) / l[i * m + i];
    }
    x
}

/// L⁻ᵀ b.
fn upper_solve(l: &[f64], m: usize, b: &[f64]) -> Vec<f64> {
    let mut x = b.to_vec();
    for i in (0..m).rev() {
        x[i] = (x[i] - (i + 1..m).map(|k| l[k * m + i] * x[k]).sum::<f64>()) / l[i * m + i];
    }
    x
}

/// The eigenvalues and eigenvectors (columns of the row-major m × m result) of symmetric `a`
/// by cyclic Jacobi rotations (G. H. Golub, C. F. Van Loan, *Matrix Computations*, 4th ed.,
/// §8.5), at most [`MAX_SWEEPS`] sweeps.
fn jacobi(mut a: Vec<f64>, m: usize) -> (Vec<f64>, Vec<f64>) {
    let mut v = vec![0.0; m * m];
    (0..m).for_each(|i| v[i * m + i] = 1.0);
    for _ in 0..MAX_SWEEPS {
        let off: f64 = (0..m).flat_map(|i| (0..m).filter(move |j| *j != i).map(move |j| (i, j))).map(|(i, j)| a[i * m + j].powi(2)).sum();
        let diag: f64 = (0..m).map(|i| a[i * m + i].powi(2)).sum();
        if off <= 1e-30 * diag || off == 0.0 {
            break;
        }
        for p in 0..m {
            for q in p + 1..m {
                let apq = a[p * m + q];
                if apq == 0.0 {
                    continue;
                }
                let theta = (a[q * m + q] - a[p * m + p]) / (2.0 * apq);
                let sign = if theta >= 0.0 { 1.0 } else { -1.0 };
                let t = sign / (theta.abs() + (theta * theta + 1.0).sqrt());
                let c = 1.0 / (t * t + 1.0).sqrt();
                let s = t * c;
                for k in 0..m {
                    let (akp, akq) = (a[k * m + p], a[k * m + q]);
                    a[k * m + p] = c * akp - s * akq;
                    a[k * m + q] = s * akp + c * akq;
                }
                for k in 0..m {
                    let (apk, aqk) = (a[p * m + k], a[q * m + k]);
                    a[p * m + k] = c * apk - s * aqk;
                    a[q * m + k] = s * apk + c * aqk;
                }
                for k in 0..m {
                    let (vkp, vkq) = (v[k * m + p], v[k * m + q]);
                    v[k * m + p] = c * vkp - s * vkq;
                    v[k * m + q] = s * vkp + c * vkq;
                }
            }
        }
    }
    ((0..m).map(|i| a[i * m + i]).collect(), v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jacobi_diagonalises_a_symmetric_matrix() {
        let a = vec![4.0, 1.0, 0.5, 1.0, 3.0, 0.2, 0.5, 0.2, 1.0];
        let (values, v) = jacobi(a.clone(), 3);
        for k in 0..3 {
            for i in 0..3 {
                let av: f64 = (0..3).map(|j| a[i * 3 + j] * v[j * 3 + k]).sum();
                assert!((av - values[k] * v[i * 3 + k]).abs() < 1e-12);
            }
        }
        let l = cholesky(&a, 3).unwrap();
        let b = [1.0, 2.0, 3.0];
        let x = upper_solve(&l, 3, &lower_solve(&l, 3, &b));
        for i in 0..3 {
            let ax: f64 = (0..3).map(|j| a[i * 3 + j] * x[j]).sum();
            assert!((ax - b[i]).abs() < 1e-12);
        }
        assert!(cholesky(&[1.0, 2.0, 2.0, 1.0], 2).is_none());
    }
}
