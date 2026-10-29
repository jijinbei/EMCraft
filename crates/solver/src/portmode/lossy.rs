//! Hybrid modes of cross-sections holding lossy or dispersive materials: a microstrip or a
//! stripline on FR-4, a guide loaded with a lossy slab.
//!
//! **Formulation.** The eigenproblem of [`crate::portmode`] holds unchanged with complex εr and
//! μr per triangle (J.-F. Lee, D.-K. Sun, Z. J. Cendes, IEEE Trans. MTT 39(8):1262–1271, 1991;
//! J.-M. Jin, *The Finite Element Method in Electromagnetics*, 3rd ed., Wiley 2014, §8.4):
//! A x = γ² B x with A = ∫ μr⁻¹ (∇×e)(∇×f) − k₀² εr e·f and B = ∫ μr⁻¹ (e + ∇φ)·(f + ∇ξ) −
//! k₀² εr φ ξ, now complex symmetric (not Hermitian), and γ² = −β² complex: each mode goes as
//! e^{−γs}, γ = α + jβ with Re γ ≥ 0. The shift-invert subspace iteration runs in complex
//! arithmetic (the complex symmetric LDLᵀ of A − σB, σ real below every Re γ²), each projected
//! problem a small complex one ([`crate::zdense`]: Hessenberg reduction and shifted QR,
//! G. H. Golub, C. F. Van Loan, *Matrix Computations*, 4th ed., §7.5).
//!
//! **Orthogonality and normalisation.** A and B are complex symmetric, so modes of distinct γ
//! are orthogonal in the unconjugated form xᵀB y — on the constraint every mode meets, xᵀB y =
//! ∫ μr⁻¹ e_y · g_x dS = D(x, y), the modes' unconjugated cross power (L. B. Felsen,
//! N. Marcuvitz, *Radiation and Scattering of Waves*, 1973, §2.2; R. B. Marks, D. F. Williams,
//! *A general waveguide circuit theory*, J. Res. NIST 97(5):533–562, 1992, §2): the
//! orthogonality the port condition projects the field by, with D = ∫ μr⁻¹ e · g dS complex. A
//! degenerate cluster is made orthogonal in that form by Gram–Schmidt. Each mode is scaled to
//! ∫ |e_t|² dS = 1 and turned so ∫ e_t · e_t dS is real and positive (the lossless modes' real
//! fields, continued), then signed as the lossless ones.

use emcraft_fem::tri::Triangle;
use emcraft_model::{CVec3, Point3, Vec3};
use num_complex::Complex64 as C;

use super::{CrossSection, MAX_ITERATIONS};
use crate::driven::{Result, Slot, invalid};
use crate::ldlt::Ldlt;
use crate::sparse::Csr;

/// Eigenvalues γ² within this fraction of the largest |γ²| asked for are one degenerate cluster.
const DEGENERATE: f64 = 1e-8;

/// A hybrid mode of a cross-section with lossy materials at one frequency.
#[derive(Clone, Debug, PartialEq)]
pub struct LossyPortMode {
    /// γ = α + jβ (1/m), Re γ ≥ 0: the mode goes as e^{−γs} along the line.
    pub gamma: C,
    /// The coefficients: transverse unknowns, then scalar ones, scaled so ∫ |e_t|² dS = 1 and
    /// ∫ e_t · e_t dS > 0.
    pub x: Vec<C>,
    /// D = ∫ μr⁻¹ e_t · (e_t + ∇φ) dS, unconjugated.
    pub d: C,
}

impl LossyPortMode {
    /// −Re γ² = β² − α² (rad²/m²): positive while the mode propagates (β > α).
    pub fn beta2(&self) -> f64 {
        -(self.gamma * self.gamma).re
    }
}

/// Σ conj(aᵢ) bᵢ.
fn hdot(a: &[C], b: &[C]) -> C {
    a.iter().zip(b).map(|(p, q)| p.conj() * q).sum()
}

/// Orthonormal vectors (in the Hermitian product) spanning `v`: modified Gram–Schmidt, twice;
/// `None` when they are dependent.
fn orthonormal(mut v: Vec<Vec<C>>) -> Option<Vec<Vec<C>>> {
    for i in 0..v.len() {
        let start = hdot(&v[i], &v[i]).re.sqrt();
        for _ in 0..2 {
            for j in 0..i {
                let d = hdot(&v[j], &v[i]);
                let (head, tail) = v.split_at_mut(i);
                tail[0].iter_mut().zip(&head[j]).for_each(|(a, b)| *a -= d * b);
            }
        }
        let len = hdot(&v[i], &v[i]).re.sqrt();
        if !(len > 1e-10 * start && len.is_finite()) {
            return None;
        }
        v[i].iter_mut().for_each(|c| *c /= len);
    }
    Some(v)
}

/// The eigenvectors of `op` (m × m) for its eigenpairs `ritz`, in their order: each degenerate
/// cluster's (θ within [`DEGENERATE`] of the largest |θ|) by block inverse iteration on the
/// cluster's mean, Hermitian-orthonormalised, so a cluster's vectors span its invariant
/// subspace (inverse iteration one vector at a time would return the same vector for each).
fn cluster_vectors(op: &[C], m: usize, ritz: &[(C, Vec<C>)]) -> Option<Vec<Vec<C>>> {
    let top = ritz.iter().map(|(t, _)| t.norm()).fold(0.0, f64::max).max(f64::MIN_POSITIVE);
    let mut out: Vec<Vec<C>> = Vec::with_capacity(ritz.len());
    let mut i = 0;
    while i < ritz.len() {
        let mut end = i + 1;
        while end < ritz.len() && (ritz[end].0 - ritz[i].0).norm() <= DEGENERATE * top {
            end += 1;
        }
        if end - i == 1 {
            out.push(ritz[i].1.clone());
        } else {
            let k = end - i;
            let mean = ritz[i..end].iter().map(|(t, _)| t).sum::<C>() / k as f64;
            let shift = mean + C::new(1e-10 * top, 1e-10 * top);
            let a: Vec<Vec<C>> = (0..m).map(|r| (0..m).map(|c| op[r * m + c] - if r == c { shift } else { C::new(0.0, 0.0) }).collect()).collect();
            let mut block: Vec<Vec<C>> = (0..k)
                .map(|j| (0..m).map(|r| C::new(((r + 1) as f64 * (j + 1) as f64 * 0.7).cos(), 0.1 * (r as f64 - j as f64))).collect())
                .collect();
            for _ in 0..3 {
                block = block.into_iter().map(|v| crate::zdense::solve(a.clone(), v)).collect::<Option<Vec<_>>>()?;
                block = orthonormal(block)?;
            }
            out.extend(block);
        }
        i = end;
    }
    Some(out)
}

impl CrossSection {
    /// The `count` modes with the largest β² − α² at free-space wavenumber `k0` (rad/m), in
    /// triangles of complex relative permittivity `eps` and permeability `mu` (one each per
    /// triangle, as at the frequency: εr(ω)(1 − j tan δ), μr(1 − j tan δ_m)). See the module
    /// docs.
    pub fn modes_lossy(&self, k0: f64, eps: &[C], mu: &[C], count: usize) -> Result<Vec<LossyPortMode>> {
        if !(k0.is_finite() && k0 > 0.0) {
            return Err(invalid("port modes", format!("the wavenumber must be finite and positive, got {k0}")));
        }
        let nt = self.tris.len();
        if eps.len() != nt || mu.len() != nt {
            return Err(invalid("port modes", "one permittivity and one permeability per triangle"));
        }
        if !eps.iter().chain(mu).all(|c| c.is_finite() && c.norm() > 0.0) {
            return Err(invalid("port modes", "materials must be finite and not zero"));
        }
        let inv_mu: Vec<C> = mu.iter().map(|m| 1.0 / m).collect();
        let [curl, mass, b, t] = self.assemble_with(&|t| eps[t], &|t| inv_mu[t])?;
        let n = self.unknowns;
        let count = count.clamp(1, n);
        let k2 = k0 * k0;
        let index2 = eps.iter().zip(mu).map(|(e, m)| (e * m).norm()).fold(1.0, f64::max);
        let sigma = -1.05 * k2 * index2;
        let a_val: Vec<C> = curl.iter().zip(&mass).map(|(c, m)| c - k2 * m).collect();
        let b_val: Vec<C> = b.iter().zip(&t).map(|(b, t)| b - k2 * t).collect();
        let mut shifted = self.pattern.clone();
        for (v, (a, b)) in shifted.val.iter_mut().zip(a_val.iter().zip(&b_val)) {
            *v = a - sigma * b;
        }
        let factor = Ldlt::factor(&shifted, &self.points)?;
        let mul = |val: &[C], x: &[C]| -> Vec<C> {
            let p = &self.pattern;
            (0..p.n).map(|i| (p.ptr[i]..p.ptr[i + 1]).map(|k| val[k] * x[p.col[k]]).sum()).collect()
        };
        let project = self.constraint_lossy(&b_val)?;
        let q = (count + count.max(6)).min(n);
        let mut rand = crate::subspace::uniform(0x9e37_79b9_7f4a_7c15);
        let mut x: Vec<Vec<C>> = (0..q).map(|_| (0..n).map(|_| C::new(rand(), 0.0)).collect()).collect();
        for v in &mut x {
            project(v)?;
        }
        let mut previous: Vec<C> = Vec::new();
        let mut result: Option<(Vec<C>, Vec<Vec<C>>)> = None;
        for _ in 0..MAX_ITERATIONS {
            // W = (A − σB)⁻¹ B X, orthonormalised.
            let mut w: Vec<Vec<C>> = x.iter().map(|v| mul(&b_val, v)).collect();
            factor.solve_many(&mut w)?;
            for v in &mut w {
                project(v)?;
            }
            let qb = orthonormal(w).ok_or_else(|| invalid("port modes", "the subspace lost its independence"))?;
            // Ritz values θ of G⁻¹ Bq, G = Qᴴ(A − σB)Q, Bq = QᴴBQ; γ² = σ + 1/θ.
            let kq: Vec<Vec<C>> = qb.iter().map(|v| mul(&shifted.val, v)).collect();
            let bq: Vec<Vec<C>> = qb.iter().map(|v| mul(&b_val, v)).collect();
            let m = qb.len();
            let g: Vec<Vec<C>> = (0..m).map(|i| (0..m).map(|j| hdot(&qb[i], &kq[j])).collect()).collect();
            let cols = (0..m)
                .map(|j| crate::zdense::solve(g.clone(), (0..m).map(|i| hdot(&qb[i], &bq[j])).collect()))
                .collect::<Option<Vec<Vec<C>>>>()
                .ok_or_else(|| invalid("port modes", "a singular projection"))?;
            let op: Vec<C> = (0..m * m).map(|ij| cols[ij % m][ij / m]).collect();
            let mut ritz = crate::zdense::eigen(&op, m).ok_or_else(|| invalid("port modes", "the projected eigenproblem did not converge"))?;
            // Nearest the shift first.
            ritz.sort_by(|p, r| r.0.norm().total_cmp(&p.0.norm()));
            let wanted: Vec<C> =
                ritz.iter().take(count).map(|(th, _)| if th.norm() > 0.0 { sigma + 1.0 / th } else { C::new(f64::INFINITY, 0.0) }).collect();
            let done = previous.len() == count && wanted.iter().zip(&previous).all(|(a, b)| (a - b).norm() <= 1e-11 * k2 * index2);
            previous = wanted;
            if done {
                let small = cluster_vectors(&op, m, &ritz).ok_or_else(|| invalid("port modes", "the projected eigenvectors did not converge"))?;
                let vectors = small.iter().take(count).map(|y| (0..n).map(|r| (0..m).map(|k| qb[k][r] * y[k]).sum()).collect()).collect();
                result = Some((previous.clone(), vectors));
                break;
            }
            x = qb;
        }
        let (lambdas, mut vectors) = result.ok_or_else(|| invalid("port modes", format!("no convergence in {MAX_ITERATIONS} iterations")))?;
        if !lambdas.iter().all(|l| l.is_finite()) {
            return Err(invalid("port modes", "a mode at the shift"));
        }
        self.separate_clusters(&lambdas, &mut vectors, &b_val, &mul)?;
        let mut modes = lambdas.into_iter().zip(vectors).map(|(l, v)| self.finish_lossy(l, v, &inv_mu)).collect::<Result<Vec<_>>>()?;
        modes.sort_by(|p, q| q.beta2().total_cmp(&p.beta2()));
        Ok(modes)
    }

    /// Makes the vectors of each degenerate cluster of `lambdas` orthogonal in the unconjugated
    /// form xᵀB y (Gram–Schmidt); refuses a cluster whose vectors cannot be told apart.
    fn separate_clusters(&self, lambdas: &[C], vectors: &mut [Vec<C>], b_val: &[C], mul: &dyn Fn(&[C], &[C]) -> Vec<C>) -> Result<()> {
        let top = lambdas.iter().map(|l| l.norm()).fold(0.0, f64::max);
        for i in 0..vectors.len() {
            for j in 0..i {
                if (lambdas[i] - lambdas[j]).norm() > DEGENERATE * top {
                    continue;
                }
                let bj = mul(b_val, &vectors[j]);
                let jj: C = vectors[j].iter().zip(&bj).map(|(p, q)| p * q).sum();
                let ij: C = vectors[i].iter().zip(&bj).map(|(p, q)| p * q).sum();
                let size = hdot(&vectors[j], &bj).norm();
                if !(jj.norm() > 1e-8 * size) {
                    return Err(invalid(
                        "port modes",
                        "not supported yet: degenerate lossy modes without power (refine the mesh, or ask for fewer modes)",
                    ));
                }
                let r = ij / jj;
                let (head, tail) = vectors.split_at_mut(i);
                tail[0].iter_mut().zip(&head[j]).for_each(|(a, b)| *a -= r * b);
                let len = hdot(&tail[0], &tail[0]).re.sqrt();
                let was = hdot(&head[j], &head[j]).re.sqrt();
                if !(len > 1e-6 * was && len.is_finite()) {
                    return Err(invalid(
                        "port modes",
                        "not supported yet: degenerate lossy modes the solver could not tell apart (refine the mesh, or ask for fewer modes)",
                    ));
                }
            }
        }
        Ok(())
    }

    /// The projection onto the scalar rows' constraint B_zt e + B_zz φ = 0 with the complex B
    /// (see [`CrossSection::constraint`]).
    fn constraint_lossy<'a>(&'a self, b_val: &'a [C]) -> Result<impl Fn(&mut Vec<C>) -> Result<()> + 'a> {
        let (nt, n) = (self.transverse, self.unknowns);
        let p = &self.pattern;
        let nz = n - nt;
        let groups: Vec<Vec<usize>> =
            (nt..n).map(|i| (p.ptr[i]..p.ptr[i + 1]).map(|k| p.col[k]).filter(|&j| j >= nt).map(|j| j - nt).chain([i - nt]).collect()).collect();
        let mut zz = Csr::from_groups(nz, groups.iter().map(|g| g.as_slice()));
        for i in nt..n {
            for k in (p.ptr[i]..p.ptr[i + 1]).filter(|&k| p.col[k] >= nt) {
                zz.add(i - nt, p.col[k] - nt, b_val[k]);
            }
        }
        let factor = if nz > 0 { Some(Ldlt::factor(&zz, &self.points[nt..])?) } else { None };
        Ok(move |v: &mut Vec<C>| -> Result<()> {
            let Some(f) = &factor else { return Ok(()) };
            let mut r: Vec<C> =
                (nt..n).map(|i| -(p.ptr[i]..p.ptr[i + 1]).filter(|&k| p.col[k] < nt).map(|k| b_val[k] * v[p.col[k]]).sum::<C>()).collect();
            f.solve(&mut r)?;
            v[nt..].copy_from_slice(&r);
            Ok(())
        })
    }

    /// Scales a mode to ∫ |e_t|² = 1 with ∫ e_t · e_t real and positive, signs it as
    /// [`CrossSection::finish`] does, and computes D and γ = √λ (Re γ ≥ 0).
    fn finish_lossy(&self, lambda: C, mut x: Vec<C>, inv_mu: &[C]) -> Result<LossyPortMode> {
        let zero = C::new(0.0, 0.0);
        let (mut e2, mut ee, mut d, mut sy, mut sx, mut sr, mut area) = (0.0, zero, zero, zero, zero, zero, 0.0);
        let centre = Point3::centroid(self.nodes.iter().copied()).unwrap_or(Point3::ORIGIN);
        for t in 0..self.tris.len() {
            let tri = self.triangle(t)?;
            area += tri.area;
            let im = inv_mu.get(t).copied().unwrap_or(C::new(1.0, 0.0));
            for (l, w) in tri.rule() {
                let (e, g) = self.at_lossy(&tri, t, &x, l);
                e2 += w * e.norm_sqr();
                ee += w * e.dot(e);
                d += w * im * e.dot(g);
                sy += w * e.y;
                sx += w * e.x;
                sr += w * e.dot_real(tri.point(l) - centre);
            }
        }
        if !(e2 > 0.0 && e2.is_finite()) {
            return Err(invalid("port modes", "a mode without a transverse field"));
        }
        // c² ee = |ee|/e2: real and positive.
        let phase = if ee.norm() > 1e-9 * e2 { (ee / ee.norm()).sqrt().conj() } else { C::new(1.0, 0.0) };
        let len = area.sqrt();
        let unit = e2.sqrt();
        let tol = 1e-6 * len;
        let sign = [sy, sx, sr / len].into_iter().map(|s| (s * phase).re).find(|s| s.abs() > tol * unit).map_or(1.0, f64::signum);
        let scale = phase * (sign / unit);
        x.iter_mut().for_each(|c| *c *= scale);
        let g = lambda.sqrt();
        let gamma = if g.re < 0.0 || (g.re == 0.0 && g.im < 0.0) { -g } else { g };
        Ok(LossyPortMode { gamma, x, d: d * scale * scale })
    }

    /// e_t and g = e_t + ∇φ at barycentric `l` of triangle `t` for complex coefficients `x`.
    fn at_lossy(&self, tri: &Triangle, t: usize, x: &[C], l: [f64; 3]) -> (CVec3, CVec3) {
        let (vt, vz) = self.unknowns_of(t);
        let coef = |u: Slot| u.get().map_or(C::new(0.0, 0.0), |u| x.get(u).copied().unwrap_or_default());
        let times = |v: Vec3, c: C| CVec3::new(c * v.x, c * v.y, c * v.z);
        let e = tri.values(l).into_iter().zip(&vt).fold(CVec3::ZERO, |s, (v, u)| s + times(v, coef(*u)));
        let grad = tri.scalar_gradients(l).into_iter().zip(&vz).fold(CVec3::ZERO, |s, (g, u)| s + times(g, coef(*u)));
        (e, e + grad)
    }

    /// e_t and g = e_t + ∇φ of a lossy `mode` at a point of triangle `t` (its z ignored).
    pub fn fields_lossy(&self, mode: &LossyPortMode, t: usize, p: Point3) -> Result<(CVec3, CVec3)> {
        if t >= self.tris.len() {
            return Err(invalid("port modes", format!("no triangle {t}")));
        }
        let tri = self.triangle(t)?;
        Ok(self.at_lossy(&tri, t, &mode.x, tri.barycentric(p)))
    }

    /// εr and μr of each triangle as the section was built with (real).
    pub fn materials(&self) -> (&[f64], &[f64]) {
        (&self.eps, &self.mu)
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::rectangle;
    use super::*;
    use emcraft_fem::Order;

    #[test]
    fn lossless_materials_give_the_real_solver_modes() {
        // WR-90 half filled at 10 GHz: with real εr the complex solver finds the real one's β².
        let (a, b, er) = (0.02286, 0.01016, 2.25);
        let k0 = 2.0 * std::f64::consts::PI * 10e9 / emcraft_model::units::C0;
        let cs = rectangle(Order::Two, a, b, 12, 6, |_, y| if y < b / 2.0 { er } else { 1.0 });
        let real = cs.modes(k0, 4).unwrap();
        let (eps, mu) = cs.materials();
        let eps: Vec<C> = eps.iter().map(|e| C::new(*e, 0.0)).collect();
        let mu: Vec<C> = mu.iter().map(|e| C::new(*e, 0.0)).collect();
        let lossy = cs.modes_lossy(k0, &eps, &mu, 4).unwrap();
        for (r, l) in real.iter().zip(&lossy) {
            assert!((r.beta2 - l.beta2()).abs() < 1e-9 * k0 * k0, "{} {}", r.beta2, l.beta2());
            assert!((r.d - l.d.re).abs() < 1e-8 && l.d.im.abs() < 1e-8, "{} {}", r.d, l.d);
            // The same field, signed alike.
            let worst = r.x.iter().zip(&l.x).map(|(p, q)| (q - p).norm()).fold(0.0, f64::max);
            let size = r.x.iter().map(|p| p.abs()).fold(0.0, f64::max);
            assert!(worst < 1e-6 * size, "{worst} {size}");
        }
    }

    #[test]
    fn a_lossy_slab_gives_collins_complex_modes() {
        // WR-90 with εr = 2.25(1 − 0.02j) in its lower half at 10 GHz, against the LSE/LSM
        // equations solved in complex arithmetic (emcraft_theory::slab).
        let (a, b) = (0.02286, 0.01016);
        let eps_slab = C::new(2.25, -0.045);
        let k0 = 2.0 * std::f64::consts::PI * 10e9 / emcraft_model::units::C0;
        let exact = emcraft_theory::slab::slab_loaded_modes_lossy(a, b, b / 2.0, eps_slab, k0, -3.0 * k0 * k0, 4);
        let cs = rectangle(Order::Two, a, b, 16, 8, |_, y| if y < b / 2.0 { 2.25 } else { 1.0 });
        let eps: Vec<C> = cs.materials().0.iter().map(|e| if *e > 1.0 { eps_slab } else { C::new(1.0, 0.0) }).collect();
        let mu = vec![C::new(1.0, 0.0); eps.len()];
        let modes = cs.modes_lossy(k0, &eps, &mu, 4).unwrap();
        for (m, e) in modes.iter().zip(&exact) {
            let got = -(m.gamma * m.gamma);
            assert!((got - e).norm() < 1e-3 * (e.norm() + k0 * k0), "{got} {e}");
        }
        // The dominant mode's attenuation within 0.1 %.
        let g = emcraft_theory::slab::gamma_of(exact[0]);
        assert!((modes[0].gamma - g).norm() < 1e-3 * g.re.max(1e-3 * g.im) + 1e-5 * g.im, "{} {g}", modes[0].gamma);
        assert!((modes[0].gamma.re / g.re - 1.0).abs() < 1e-3, "{} {g}", modes[0].gamma);
    }

    #[test]
    fn a_degenerate_lossy_pair_is_made_orthogonal() {
        // A square guide filled with a lossy dielectric: TE10 and TE01 share γ; the solver's two
        // vectors are made orthogonal in xᵀBy, so their cross power vanishes.
        let a = 0.02;
        let k0 = 2.0 * std::f64::consts::PI * 12e9 / emcraft_model::units::C0;
        let cs = rectangle(Order::Two, a, a, 8, 8, |_, _| 1.0);
        let n = cs.tris.len();
        let eps = vec![C::new(2.0, -0.04); n];
        let mu = vec![C::new(1.0, 0.0); n];
        let modes = cs.modes_lossy(k0, &eps, &mu, 2).unwrap();
        let k2 = C::new(2.0, -0.04) * k0 * k0;
        let exact = (C::new((std::f64::consts::PI / a).powi(2), 0.0) - k2).sqrt();
        for m in &modes {
            assert!((m.gamma - exact).norm() < 1e-3 * exact.norm(), "{} {exact}", m.gamma);
        }
        let mut cross = C::new(0.0, 0.0);
        for t in 0..n {
            let tri = cs.triangle(t).unwrap();
            for (l, w) in tri.rule() {
                let (e0, _) = cs.at_lossy(&tri, t, &modes[0].x, l);
                let (_, g1) = cs.at_lossy(&tri, t, &modes[1].x, l);
                cross += w * e0.dot(g1);
            }
        }
        assert!(cross.norm() < 1e-8 * modes[0].d.norm(), "{cross}");
    }
}
