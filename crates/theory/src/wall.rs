//! A rectangular guide of width a whose narrow wall's height b(z) changes smoothly along it (an
//! E-plane corrugation, a taper in height), fed by TE₁₀: solved as the two-dimensional problem it
//! is, the smooth wall as it is, with no steps.
//!
//! Between the side walls x = 0 and x = a, a structure uniform in x carries fields of the form
//! sin(πx/a) for TE₁₀'s harmonic, and about perfect conductors uniform in x the fields TE and TM
//! to x do not couple (every wall condition involves one of the two potentials only). TE₁₀ is
//! TE to x (E_x = 0), from the potential H_x = ψ(y, z) sin(πx/a), and
//!
//! ∂²ψ/∂y² + ∂²ψ/∂z² + k_t² ψ = 0, k_t² = k₀² εr μr − (π/a)²,
//!
//! with ∂ψ/∂n = 0 on the floor y = 0 and on the wall y = b(z) (its tangential E is ψ's normal
//! derivative): the LSE^x_1n family the mode matching of E-plane steps uses
//! ([`crate::modematch::EPlaneLine`]), here without cutting the wall into steps, whose edges each
//! carry a singular field and make the steps' limit converge slowly (about N^(−0.7) in their
//! number N).
//!
//! The method: the guide mapped onto a rectangle, η = y/b(z) ∈ [0, 1] (the wall exact, through
//! b(z) and b′(z) at every quadrature point), the weak form ∫ (∇ψ·∇v − k_t² ψ v) dA with the
//! floor's and the wall's condition natural, discretised with tensor-product Lagrange elements
//! of degree `p` on Chebyshev–Gauss–Lobatto nodes (spectral elements; the wall's corners — where
//! its slope jumps — on element boundaries), Gauss–Legendre quadrature of p + 3 points a
//! direction. The ends are uniform leads of height b₀ closed by the exact modal condition
//! (Dirichlet-to-Neumann) in the modes cos(mπy/b₀): ∂ψ/∂n = Σ jβ_m (2A_m − ψ_m) φ_m at the
//! input, A the incident TE₁₀, and −jβ_m ψ_m at the output (A. Bayliss, M. Gunzburger, E. Turkel,
//! SIAM J. Appl. Math. 42(2), 1982; J. Jin, *The Finite Element Method in Electromagnetics*,
//! 3rd ed., Wiley 2014, §9.3). The banded system is solved by Gaussian elimination without
//! pivoting (complex symmetric), checked by its residual. S is referred to the end faces.

use num_complex::Complex64 as C;

use crate::TheoryError;

/// The most elements along the guide, across it, and the highest degree.
pub const MAX_ALONG: usize = 4000;
pub const MAX_ACROSS: usize = 64;
pub const MAX_DEGREE: usize = 10;

/// How finely [`SmoothGuide::solve`] discretises: elements along the guide (between each pair
/// of the wall's corners, in proportion to their distance), across it, their degree and the
/// modes each end's condition carries.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Discretisation {
    pub along: usize,
    pub across: usize,
    pub degree: usize,
    pub modes: usize,
}

/// A guide of width `a` and `length`, its narrow wall's height `wall(z) = (b, db/dz)` (m),
/// uniform (b₀, no slope) at both ends; `corners`: where along it the wall's slope jumps (m),
/// kept on element boundaries. Filled with one lossless material.
///
/// ```
/// use emcraft_theory::wall::{Discretisation, Plane, SmoothGuide};
/// // A straight WR-90 30 mm long: S21 = e^{−jβL}, nothing reflected.
/// fn wr90(_z: f64) -> (f64, f64) {
///     (10.16e-3, 0.0) // m, and no slope
/// }
/// let a = 22.86e-3;
/// let g = SmoothGuide { plane: Plane::E { width: a }, length: 0.03, eps_r: 1.0, wall: wr90 as fn(f64) -> (f64, f64), corners: vec![] };
/// let d = Discretisation { along: 20, across: 2, degree: 4, modes: 4 };
/// let (s11, s21) = g.solve(10e9, d)?;
/// let k0 = 2.0 * std::f64::consts::PI * 10e9 / 299_792_458.0;
/// let beta = (k0 * k0 - (std::f64::consts::PI / a).powi(2)).sqrt();
/// assert!(s11.norm() < 1e-9);
/// assert!((s21 - num_complex::Complex64::from_polar(1.0, -beta * 0.03)).norm() < 1e-8);
/// // A wall that touches the floor is refused.
/// fn shut(_z: f64) -> (f64, f64) {
///     (0.0, 0.0)
/// }
/// let shut = SmoothGuide { wall: shut as fn(f64) -> (f64, f64), ..g };
/// assert!(shut.solve(10e9, d).is_err());
/// # Ok::<(), emcraft_theory::TheoryError>(())
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct SmoothGuide<F: Fn(f64) -> (f64, f64)> {
    pub plane: Plane,
    pub length: f64,
    pub eps_r: f64,
    pub wall: F,
    pub corners: Vec<f64>,
}

/// Which wall moves: the narrow one of a guide `width` wide (E-plane: TE₁₀'s potential H_x, ∂ψ/∂n = 0
/// on the walls, modes cos(mπy/b₀) from m = 0, k_t² = k² − (π/a)²), or the broad one (H-plane:
/// the field E_y itself, uniform in y between the broad walls, ψ = 0 on the side walls, modes
/// sin(mπx/a₀) from m = 1, k_t² = k²). In both the wall's height (or width) is `wall`, from the
/// fixed wall at 0.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Plane {
    E { width: f64 },
    H,
}

fn invalid(what: impl Into<String>) -> TheoryError {
    TheoryError::Invalid(what.into())
}

/// Chebyshev–Gauss–Lobatto nodes of degree p on [0, 1].
fn nodes(p: usize) -> Vec<f64> {
    (0..=p).map(|i| 0.5 * (1.0 - (std::f64::consts::PI * i as f64 / p as f64).cos())).collect()
}

/// The Lagrange basis on `x` at `t`: values and derivatives.
fn lagrange(x: &[f64], t: f64) -> (Vec<f64>, Vec<f64>) {
    let n = x.len();
    let mut v = vec![0.0; n];
    let mut d = vec![0.0; n];
    for i in 0..n {
        let den: f64 = (0..n).filter(|&j| j != i).map(|j| x[i] - x[j]).product();
        v[i] = (0..n).filter(|&j| j != i).map(|j| t - x[j]).product::<f64>() / den;
        d[i] = (0..n).filter(|&k| k != i).map(|k| (0..n).filter(|&j| j != i && j != k).map(|j| t - x[j]).product::<f64>()).sum::<f64>() / den;
    }
    (v, d)
}

/// A banded matrix (`kl` below and above the diagonal), row-major in its band.
struct Band {
    n: usize,
    k: usize,
    a: Vec<C>,
}

impl Band {
    fn new(n: usize, k: usize) -> Band {
        Band { n, k, a: vec![C::new(0.0, 0.0); n * (2 * k + 1)] }
    }

    fn at(&mut self, i: usize, j: usize) -> Option<&mut C> {
        let off = (j + self.k).checked_sub(i).filter(|o| *o <= 2 * self.k)?;
        self.a.get_mut(i * (2 * self.k + 1) + off)
    }

    fn get(&self, i: usize, j: usize) -> C {
        let Some(off) = (j + self.k).checked_sub(i).filter(|o| *o <= 2 * self.k) else { return C::new(0.0, 0.0) };
        self.a.get(i * (2 * self.k + 1) + off).copied().unwrap_or_default()
    }

    /// A x.
    fn apply(&self, x: &[C]) -> Vec<C> {
        (0..self.n).map(|i| (i.saturating_sub(self.k)..(i + self.k + 1).min(self.n)).map(|j| self.get(i, j) * x[j]).sum()).collect()
    }

    /// Solves A x = b in place by elimination without pivoting; `None` on a zero pivot.
    fn solve(mut self, mut b: Vec<C>) -> Option<Vec<C>> {
        let (n, k) = (self.n, self.k);
        for c in 0..n {
            let piv = self.get(c, c);
            if piv.norm() == 0.0 || !piv.is_finite() {
                return None;
            }
            for r in c + 1..(c + k + 1).min(n) {
                let f = self.get(r, c) / piv;
                if f == C::new(0.0, 0.0) {
                    continue;
                }
                for j in c..(c + k + 1).min(n) {
                    let v = self.get(c, j);
                    if let Some(e) = self.at(r, j) {
                        *e -= f * v;
                    }
                }
                let bc = b[c];
                b[r] -= f * bc;
            }
        }
        for c in (0..n).rev() {
            let s: C = (c + 1..(c + k + 1).min(n)).map(|j| self.get(c, j) * b[j]).sum();
            b[c] = (b[c] - s) / self.get(c, c);
        }
        Some(b)
    }
}

impl<F: Fn(f64) -> (f64, f64)> SmoothGuide<F> {
    /// S₁₁ and S₂₁ of TE₁₀ at `frequency` (Hz).
    pub fn solve(&self, frequency: f64, d: Discretisation) -> Result<(C, C), TheoryError> {
        let Discretisation { along, across, degree: p, modes } = d;
        if !(1..=MAX_ALONG).contains(&along) || !(1..=MAX_ACROSS).contains(&across) || !(1..=MAX_DEGREE).contains(&p) || !(1..=64).contains(&modes) {
            return Err(invalid("discretisation"));
        }
        if let Plane::E { width } = self.plane
            && !(width > 0.0 && width.is_finite())
        {
            return Err(invalid("width"));
        }
        if !(self.length > 0.0 && self.length.is_finite() && self.eps_r > 0.0 && self.eps_r.is_finite()) {
            return Err(invalid("guide"));
        }
        if !(frequency > 0.0 && frequency.is_finite()) {
            return Err(invalid("frequency"));
        }
        let (b0, s0) = (self.wall)(0.0);
        let (b1, s1) = (self.wall)(self.length);
        if !(b0 > 0.0 && b0.is_finite()) || (b1 - b0).abs() > 1e-9 * b0 || s0 != 0.0 || s1 != 0.0 {
            return Err(invalid("wall: its ends must be of one height, level"));
        }
        let k0 = 2.0 * std::f64::consts::PI * frequency / 299_792_458.0;
        let (kt2, dirichlet) = match self.plane {
            Plane::E { width } => (k0 * k0 * self.eps_r - (std::f64::consts::PI / width).powi(2), false),
            Plane::H => (k0 * k0 * self.eps_r, true),
        };
        // The ends' modes: cos(mπy/b₀) from m = 0, or sin from m = 1.
        let order = |mi: usize| if dirichlet { mi + 1 } else { mi };
        // The element boundaries along z: the ends and the corners, each stretch cut in
        // proportion to its length.
        let mut cuts: Vec<f64> =
            std::iter::once(0.0).chain(self.corners.iter().copied().filter(|z| *z > 0.0 && *z < self.length)).chain([self.length]).collect();
        cuts.sort_by(f64::total_cmp);
        cuts.dedup();
        let mut zs = vec![0.0];
        for w in cuts.windows(2) {
            let n = ((along as f64 * (w[1] - w[0]) / self.length).round() as usize).max(1);
            zs.extend((1..=n).map(|i| w[0] + (w[1] - w[0]) * i as f64 / n as f64));
        }
        let ez = zs.len() - 1;
        let ny = across * p + 1;
        let nz = ez * p + 1;
        let n = ny * nz;
        if n > 4_000_000 {
            return Err(invalid("discretisation: too many unknowns"));
        }
        let idx = |iz: usize, iy: usize| iz * ny + iy;
        let mut m = Band::new(n, p * ny + p);
        let ref_nodes = nodes(p);
        let (gx, gw) = emcraft_fem::quadrature::gauss_legendre(p + 3);
        let tables: Vec<(Vec<f64>, Vec<f64>)> = gx.iter().map(|&t| lagrange(&ref_nodes, t)).collect();
        let he = 1.0 / across as f64;
        for e in 0..ez {
            let (za, zb) = (zs[e], zs[e + 1]);
            let hz = zb - za;
            for (qz, &wz) in gw.iter().enumerate() {
                let z = za + hz * gx[qz];
                let (b, db) = (self.wall)(z);
                if !(b > 0.0 && b.is_finite() && db.is_finite()) {
                    return Err(invalid(format!("wall: height {b} at z = {z}")));
                }
                let (lz, dlz) = &tables[qz];
                for c in 0..across {
                    for (qy, &wy) in gw.iter().enumerate() {
                        let eta = (c as f64 + gx[qy]) * he;
                        let (ly, dly) = &tables[qy];
                        let w = wz * hz * wy * he * b;
                        // Each basis function's physical gradient and value at this point.
                        let mut f = Vec::with_capacity((p + 1) * (p + 1));
                        for i in 0..=p {
                            for j in 0..=p {
                                let u = lz[i] * ly[j];
                                let u_eta = lz[i] * dly[j] / he;
                                let u_z = dlz[i] * ly[j] / hz;
                                let gy = u_eta / b;
                                let gz = u_z - eta * db / b * u_eta;
                                f.push((idx(e * p + i, c * p + j), u, gy, gz));
                            }
                        }
                        for &(r, u, gy, gz) in &f {
                            for &(s, v, hy, hzz) in &f {
                                if let Some(x) = m.at(r, s) {
                                    *x += w * (gy * hy + gz * hzz - kt2 * u * v);
                                }
                            }
                        }
                    }
                }
            }
        }
        // The ends: each mode's projection P_m(ψ) = ∫ ψ φ_m dy, φ_m = √(ε_m/b₀) cos(mπy/b₀).
        let mut rhs = vec![C::new(0.0, 0.0); n];
        let mut proj: Vec<Vec<f64>> = Vec::with_capacity(modes);
        for mi in 0..modes {
            let norm = if order(mi) == 0 { (1.0 / b0).sqrt() } else { (2.0 / b0).sqrt() };
            let mut v = vec![0.0; ny];
            for c in 0..across {
                for (qy, &wy) in gw.iter().enumerate() {
                    let y = (c as f64 + gx[qy]) * he * b0;
                    let arg = order(mi) as f64 * std::f64::consts::PI * y / b0;
                    let phi = norm * if dirichlet { arg.sin() } else { arg.cos() };
                    for j in 0..=p {
                        v[c * p + j] += wy * he * b0 * phi * tables[qy].0[j];
                    }
                }
            }
            proj.push(v);
        }
        let beta = |mi: usize| {
            let x = kt2 - (order(mi) as f64 * std::f64::consts::PI / b0).powi(2);
            if x >= 0.0 { C::new(x.sqrt(), 0.0) } else { C::new(0.0, -(-x).sqrt()) }
        };
        for (mi, v) in proj.iter().enumerate() {
            let jb = C::new(0.0, 1.0) * beta(mi);
            for iz in [0, nz - 1] {
                for (r, &pr) in v.iter().enumerate() {
                    for (s, &ps) in v.iter().enumerate() {
                        if let Some(x) = m.at(idx(iz, r), idx(iz, s)) {
                            *x += jb * pr * ps;
                        }
                    }
                }
            }
            if mi == 0 {
                for (r, &pr) in v.iter().enumerate() {
                    rhs[idx(0, r)] += 2.0 * jb * pr;
                }
            }
        }
        // The side walls of an H-plane guide: ψ = 0 there, their rows and columns the identity's.
        if dirichlet {
            for iz in 0..nz {
                for iy in [0, ny - 1] {
                    let r = idx(iz, iy);
                    for c in r.saturating_sub(m.k)..(r + m.k + 1).min(n) {
                        let v = if c == r { C::new(1.0, 0.0) } else { C::new(0.0, 0.0) };
                        if let Some(x) = m.at(r, c) {
                            *x = v;
                        }
                        if let Some(x) = m.at(c, r) {
                            *x = v;
                        }
                    }
                    rhs[r] = C::new(0.0, 0.0);
                }
            }
        }
        let scale: f64 = rhs.iter().map(|x| x.norm()).fold(0.0, f64::max);
        let kept = Band { n: m.n, k: m.k, a: m.a.clone() };
        let x = m.solve(rhs.clone()).ok_or_else(|| invalid("system: a zero pivot"))?;
        let r = kept.apply(&x);
        let res = r.iter().zip(&rhs).map(|(a, b)| (a - b).norm()).fold(0.0, f64::max);
        if !(res <= 1e-8 * scale) {
            return Err(invalid(format!("system: residual {res:.2e} of {scale:.2e}")));
        }
        let at = |iz: usize| -> C { proj[0].iter().enumerate().map(|(j, &w)| x[idx(iz, j)] * w).sum() };
        // S is the transverse E's: an E-plane ψ is H_x, whose reflected wave carries E the other
        // way round (E_y ∝ ∂ψ/∂z); an H-plane ψ is E_y itself.
        let reflected = at(0) - 1.0;
        Ok((if dirichlet { reflected } else { -reflected }, at(nz - 1)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    const A: f64 = 22.86e-3;
    const B: f64 = 10.16e-3;

    /// A wall waving `h` (m) about `base` for four 10 mm periods between 10 mm leads.
    fn corrugated(base: f64, h: f64) -> impl Fn(f64) -> (f64, f64) + Copy {
        let k = 2.0 * PI / 10e-3;
        move |z: f64| if z > 10e-3 && z < 50e-3 { (base + h * (k * (z - 10e-3)).sin(), h * k * (k * (z - 10e-3)).cos()) } else { (base, 0.0) }
    }

    /// A straight H-plane guide carries TE10 as e^{−jβL}, reflecting nothing, as the E-plane one
    /// does (the doctest).
    #[test]
    fn a_straight_guide_is_a_line() {
        let g = SmoothGuide { plane: Plane::H, length: 0.03, eps_r: 1.0, wall: |_z: f64| (A, 0.0), corners: vec![] };
        let (s11, s21) = g.solve(10e9, Discretisation { along: 20, across: 3, degree: 5, modes: 4 }).unwrap_or_default();
        let k0 = 2.0 * PI * 10e9 / 299_792_458.0;
        let beta = (k0 * k0 - (PI / A).powi(2)).sqrt();
        assert!(s11.norm() < 1e-9 && (s21 - C::from_polar(1.0, -beta * 0.03)).norm() < 1e-8, "{s11} {s21}");
    }

    /// The E-plane corrugation of the `corrugated-eplane` oracle: lossless, and the same to a
    /// thousandth of a degree when the elements are finer, higher and the ends carry more modes.
    #[test]
    fn the_corrugation_converges_and_loses_nothing() {
        let g = SmoothGuide { plane: Plane::E { width: A }, length: 60e-3, eps_r: 1.0, wall: corrugated(B, 2e-3), corners: vec![10e-3, 50e-3] };
        let coarse = g.solve(10e9, Discretisation { along: 60, across: 2, degree: 5, modes: 8 }).unwrap_or_default();
        let fine = g.solve(10e9, Discretisation { along: 120, across: 3, degree: 6, modes: 12 }).unwrap_or_default();
        let deg = |s: C| s.arg().to_degrees();
        assert!((deg(coarse.1) - deg(fine.1)).abs() < 0.03, "{} {}", deg(coarse.1), deg(fine.1));
        assert!((fine.0.norm_sqr() + fine.1.norm_sqr() - 1.0).abs() < 1e-6);
        // The wave delays it 23° behind a straight guide's −βL.
        assert!((deg(fine.1) - 152.44).abs() < 0.02, "{}", deg(fine.1));
    }

    /// Where steps are a fair model — a broad wall, the field zero on it — the smooth wall's
    /// answer is the limit of mode matching's steps (`HPlaneLine`, 1280 steps: within 0.1°).
    #[test]
    #[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
    fn a_broad_wall_is_the_limit_of_its_steps() {
        use crate::modematch::{HPlaneLine, HSection};
        let wall = corrugated(A, 2e-3);
        let g = SmoothGuide { plane: Plane::H, length: 60e-3, eps_r: 1.0, wall, corners: vec![10e-3, 50e-3] };
        let (_, smooth) = g.solve(10e9, Discretisation { along: 240, across: 4, degree: 8, modes: 24 }).unwrap_or_default();
        let steps = 1280;
        let mut sections = vec![HSection { length: 10e-3, eps_r: 1.0, mu_r: 1.0, loss_tangent: 0.0, x0: 0.0, width: A }];
        for k in 0..steps {
            let z = 10e-3 + (k as f64 + 0.5) * 40e-3 / steps as f64;
            sections.push(HSection { length: 40e-3 / steps as f64, eps_r: 1.0, mu_r: 1.0, loss_tangent: 0.0, x0: 0.0, width: wall(z).0 });
        }
        sections.push(HSection { length: 10e-3, eps_r: 1.0, mu_r: 1.0, loss_tangent: 0.0, x0: 0.0, width: A });
        let (_, stepped) = HPlaneLine { sections }.solve(10e9, 160).unwrap_or_default();
        assert!((smooth.arg() - stepped.arg()).to_degrees().abs() < 0.1, "{} {}", smooth.arg().to_degrees(), stepped.arg().to_degrees());
    }

    #[test]
    fn hostile_input_is_refused() {
        let ok = Discretisation { along: 10, across: 2, degree: 3, modes: 4 };
        let g = SmoothGuide { plane: Plane::E { width: A }, length: 0.03, eps_r: 1.0, wall: |_z: f64| (B, 0.0), corners: vec![] };
        assert!(g.solve(f64::NAN, ok).is_err());
        assert!(g.solve(10e9, Discretisation { degree: 0, ..ok }).is_err());
        assert!(g.solve(10e9, Discretisation { along: MAX_ALONG + 1, ..ok }).is_err());
        assert!(SmoothGuide { plane: Plane::E { width: -1.0 }, ..g.clone() }.solve(10e9, ok).is_err());
        // Ends of different heights, or a wall through the floor.
        let tapered = SmoothGuide { plane: Plane::E { width: A }, length: 0.03, eps_r: 1.0, wall: |z: f64| (B + z, 1.0), corners: vec![] };
        assert!(tapered.solve(10e9, ok).is_err());
        let through = SmoothGuide {
            plane: Plane::E { width: A },
            length: 0.03,
            eps_r: 1.0,
            wall: |z: f64| (B * (1.0 - 8.0 * (z / 0.03) * (1.0 - z / 0.03)), 0.0),
            corners: vec![],
        };
        assert!(through.solve(10e9, ok).is_err());
    }
}
