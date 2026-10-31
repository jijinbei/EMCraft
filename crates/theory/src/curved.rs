//! The continuously curved rectangular waveguide and the circular bend: the dominant mode's
//! angular wavenumber ν₀ and the S-parameters of a bend of any angle between straight guides.
//!
//! **The curved guide's modes** (C. W. Cochran, R. A. Pecina, *Mode propagation in continuously
//! curved waveguides*, Radio Science 1(6):679–696, 1966; L. Lewin, D. C. Chang, E. F. Kuester,
//! *Electromagnetic Waves and Curved Structures*, Peter Peregrinus 1977, ch. 3). A guide bent
//! about the axis ẑ, its walls on the cylinders ρ = r and ρ = r + w, keeps the separation in
//! (ρ, φ): every field goes as f = u(ρ) e^{−jνφ}, with
//!
//! ρ² u″ + ρ u′ + (κ² ρ² − ν²) u = 0, r < ρ < r + w,
//!
//! the Bessel equation of order ν. In the H-plane bend (the curvature in the plane of the broad
//! dimension, w = a) f = E_z, κ = k and u = 0 on the walls (the TE_m0 modes bent). In the E-plane
//! bend (the curvature in the plane of the narrow dimension, w = b) f is the magnetic Hertz
//! potential ψ, the fields going as sin(πz/a) across the broad dimension, κ² = k² − (π/a)² and
//! u′ = 0 on the walls (the LSE modes bent, E radial and along φ). For a given frequency the
//! eigenvalues ν² are real and ordered; the dominant one ν₀² is the largest, and ν₀ → β·R as the
//! radius grows (R the mean radius, β the straight guide's wavenumber).
//!
//! **ν₀ by Chebyshev collocation** ([`dominant`]; L. N. Trefethen, *Spectral Methods in MATLAB*,
//! SIAM 2000, ch. 6 and 7). The equation is written as L u = ν² u with
//! L = ρ² d²/dρ² + ρ d/dρ + κ²ρ² on the N + 1 Chebyshev points mapped to [r, r + w]; Dirichlet
//! ends drop the boundary rows, Neumann ends eliminate the two boundary values through the
//! differentiation matrix's first and last rows. The eigenvalue is found by inverse iteration
//! from the spectral Galerkin estimate below (which orders the spectrum, so the root found is
//! the dominant one), and the order is raised until two orders agree to 1e-11. It converges
//! spectrally (the coefficients are smooth): for WR-90 at r = 28.6 mm, 1e-7 at order 8 and
//! 1e-13 by order 20, after which rounding, growing as the order to the fourth, holds it.
//!
//! **The modes in the straight guide's modes** ([`CurvedGuide::modes`]): the weak form
//! −∫ρ u′v′ + κ²∫ρ uv = ν² ∫uv/ρ is symmetric, its mass matrix positive definite, so expanding
//! u in the straight guide's own mode functions φ_m (sin(mπt/w), m ≥ 1, Dirichlet; cos(mπt/w),
//! m ≥ 0, Neumann; t = ρ − r) gives a symmetric-definite eigenproblem, solved by Cholesky and
//! the cyclic Jacobi method (G. H. Golub, C. F. Van Loan, *Matrix Computations*, 4th ed., Johns
//! Hopkins 2013, §8.5). The eigenvectors' coefficients are the curved modes in the straight
//! ones, as in S. O. Rice, *Reflections from circular bends in rectangular wave guides — matrix
//! theory*, Bell System Technical Journal 27(2):305–349, 1948; they converge algebraically
//! (≈ N^{−3} for ν², the sine series of u being only C²-continuous when extended).
//!
//! **The bend** ([`bend`], [`junction`]): straight guide, a curved section of angle θ, straight
//! guide. At each junction plane φ = const the tangential fields are f and (1/ρ)∂_φ f in the
//! curved guide, f and ∂_s f in the straight one (E_z and H_ρ in the H-plane bend; H_z and E_ρ in
//! the E-plane one), and since the curved modes are expanded in the straight ones' functions,
//! f's continuity holds coefficient by coefficient while ∂f's is tested with the curved modes
//! (orthonormal in ∫uv/ρ): a + b = C(c + d), Cᵀ D B (a − b) = V (c − d), with C the
//! coefficients, D the straight modes' norms ∫φ_m², B their β_m and V the curved ν_n (−j|ν| when
//! evanescent). Rice's matrix theory gives a single junction's reflection of the dominant mode
//! as (w/R)² times a slowly varying factor for gentle bends; [`junction`] computes it, and a bend
//! reflects at most about twice that (two junctions in phase). S is that of the transverse
//! electric field: E_z = f in the H-plane bend; in the E-plane bend E is radial and ∝ ∂_s ψ
//! across the guide, so S11 = −b₀/a₀ and, the radial direction followed round the bend,
//! S21 = e₀/a₀. Ports that keep one direction instead (E along +x into a 90° bend drawn as in
//! the templates, along +y out of it, where the radial direction is −x in and +y out) see
//! −e₀/a₀, as in [`crate::bend`].
//!
//! ```
//! use emcraft_theory::curved::{CurvedGuide, Plane, bend, dominant};
//! use std::f64::consts::FRAC_PI_2;
//! // WR-90 bent in the H-plane, inner radius 28.6 mm, at 10 GHz.
//! let guide = CurvedGuide { plane: Plane::H, a: 22.86e-3, b: 10.16e-3, radius: 28.6e-3 };
//! let nu = dominant(&guide, 10e9)?;
//! // Close to β times the mean radius (158.25 rad/m × 40.03 mm = 6.33), a little less.
//! assert!(nu.nu > 5.5 && nu.nu < 6.33 && nu.change < 1e-12);
//! // A 90° bend: lossless, and its phase the curved section's to within the small reflection.
//! let (s11, s21) = bend(&guide, FRAC_PI_2, 10e9, 24)?;
//! assert!((s11.norm_sqr() + s21.norm_sqr() - 1.0).abs() < 1e-9);
//! assert!(s11.norm() < 0.1);
//! // Hostile input is refused.
//! assert!(dominant(&CurvedGuide { radius: -1.0, ..guide }, 10e9).is_err());
//! assert!(dominant(&guide, f64::NAN).is_err());
//! # Ok::<(), emcraft_theory::TheoryError>(())
//! ```

use std::f64::consts::PI;

use emcraft_fem::jacobi::{JacobiStop, jacobi};
use emcraft_model::units::C0;
use num_complex::Complex64 as C;

use crate::TheoryError;
use crate::post::dense_solve;

/// The plane the guide is bent in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plane {
    /// The bend in the plane of the broad dimension a (E across the bend's plane).
    H,
    /// The bend in the plane of the narrow dimension b (E in the bend's plane).
    E,
}

/// A rectangular guide a × b bent at inner radius `radius` (m) in the H- or E-plane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CurvedGuide {
    pub plane: Plane,
    pub a: f64,
    pub b: f64,
    pub radius: f64,
}

/// The highest Chebyshev collocation order.
pub const MAX_ORDER: usize = 256;
/// The most modes of a Galerkin expansion.
pub const MAX_MODES: usize = 200;

/// ν₀ and the change from the previous collocation order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dominant {
    /// The dominant mode's angular wavenumber ν₀ (radians of phase per radian of bend).
    pub nu: f64,
    /// The collocation order it settled at.
    pub order: usize,
    /// |ν₀(order) − ν₀(previous order)| / ν₀.
    pub change: f64,
}

/// The curved modes of a Galerkin expansion: ν² (largest first) and their coefficients in the
/// straight modes, column n of `coefficients` (row-major, `modes` × `modes`) the mode n.
#[derive(Clone, Debug, PartialEq)]
pub struct Modes {
    pub nu2: Vec<f64>,
    pub coefficients: Vec<f64>,
}

impl CurvedGuide {
    fn check(&self) -> Result<(), TheoryError> {
        if ![self.a, self.b, self.radius].iter().all(|v| v.is_finite() && *v > 0.0) {
            return Err(TheoryError::Invalid(format!("curved guide {self:?}")));
        }
        if self.radius > 1e4 * self.width() {
            return Err(TheoryError::Invalid(format!("curved guide: radius {} m is effectively straight", self.radius)));
        }
        Ok(())
    }

    /// The width in the bend's plane.
    pub fn width(&self) -> f64 {
        match self.plane {
            Plane::H => self.a,
            Plane::E => self.b,
        }
    }

    /// The mean radius r + w/2.
    pub fn mean_radius(&self) -> f64 {
        self.radius + self.width() / 2.0
    }

    /// κ² of the bend's 2-D problem (rad²/m²).
    fn kappa2(&self, frequency: f64) -> Result<f64, TheoryError> {
        if !(frequency.is_finite() && frequency > 0.0 && frequency < 1e15) {
            return Err(TheoryError::Invalid(format!("frequency {frequency}")));
        }
        let k = 2.0 * PI * frequency / C0;
        Ok(match self.plane {
            Plane::H => k * k,
            Plane::E => k * k - (PI / self.a).powi(2),
        })
    }

    fn neumann(&self) -> bool {
        self.plane == Plane::E
    }

    /// The straight guide's mode m (m = 0, 1, … in order): its index in sin/cos(index·πt/w).
    fn index(&self, m: usize) -> usize {
        if self.neumann() { m } else { m + 1 }
    }

    /// The straight guide's β_m (−j|β| when evanescent).
    fn straight_beta(&self, kappa2: f64, m: usize) -> C {
        crate::decaying_sqrt(C::new(kappa2 - (self.index(m) as f64 * PI / self.width()).powi(2), 0.0))
    }

    /// ∫₀^w φ_m² dt.
    fn norm(&self, m: usize) -> f64 {
        if self.index(m) == 0 { self.width() } else { self.width() / 2.0 }
    }

    /// The curved modes in `modes` straight mode functions at `frequency` (Hz).
    pub fn modes(&self, frequency: f64, modes: usize) -> Result<Modes, TheoryError> {
        self.check()?;
        let kappa2 = self.kappa2(frequency)?;
        if modes == 0 || modes > MAX_MODES {
            return Err(TheoryError::Invalid(format!("mode count {modes} (1 to {MAX_MODES})")));
        }
        let n = modes;
        let (r, w) = (self.radius, self.width());
        // Composite Gauss–Legendre: 32 points on each of `panels` panels resolve φ_m φ_n.
        let panels = n / 4 + 4;
        let (gx, gw) = emcraft_fem::quadrature::gauss_legendre(32);
        let mut a = vec![0.0; n * n];
        let mut m = vec![0.0; n * n];
        let mut phi = vec![0.0; n];
        let mut dphi = vec![0.0; n];
        for p in 0..panels {
            for (x, wq) in gx.iter().zip(&gw) {
                let t = (p as f64 + x) * w / panels as f64;
                let weight = wq * w / panels as f64;
                let rho = r + t;
                for (i, (f, d)) in phi.iter_mut().zip(dphi.iter_mut()).enumerate() {
                    let q = self.index(i) as f64 * PI / w;
                    if self.neumann() {
                        *f = (q * t).cos();
                        *d = -q * (q * t).sin();
                    } else {
                        *f = (q * t).sin();
                        *d = q * (q * t).cos();
                    }
                }
                for i in 0..n {
                    for j in 0..n {
                        a[i * n + j] += weight * (kappa2 * rho * phi[i] * phi[j] - rho * dphi[i] * dphi[j]);
                        m[i * n + j] += weight * phi[i] * phi[j] / rho;
                    }
                }
            }
        }
        let l = cholesky(&m, n).ok_or_else(|| TheoryError::Invalid("curved modes: the mass matrix is not positive definite".into()))?;
        // S = L⁻¹ A L⁻ᵀ.
        let mut s = a;
        for col in 0..n {
            forward(&l, n, &mut s, col, n);
        }
        let mut st = transpose(&s, n);
        for col in 0..n {
            forward(&l, n, &mut st, col, n);
        }
        // Symmetric against rounding.
        for i in 0..n {
            for j in 0..i {
                let s = 0.5 * (st[i * n + j] + st[j * n + i]);
                st[i * n + j] = s;
                st[j * n + i] = s;
            }
        }
        let (values, vectors) = jacobi(st, n, JacobiStop::Relative);
        if !values.iter().chain(&vectors).all(|v| v.is_finite()) {
            return Err(TheoryError::Invalid("curved modes: the eigenproblem is not finite".into()));
        }
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by(|&i, &j| values[j].total_cmp(&values[i]));
        // c = L⁻ᵀ y: M-orthonormal.
        let mut coefficients = vec![0.0; n * n];
        let mut nu2 = Vec::with_capacity(n);
        for (k, &e) in order.iter().enumerate() {
            nu2.push(values[e]);
            let mut y: Vec<f64> = (0..n).map(|i| vectors[i * n + e]).collect();
            for i in (0..n).rev() {
                let s: f64 = (i + 1..n).map(|j| l[j * n + i] * y[j]).sum();
                y[i] = (y[i] - s) / l[i * n + i];
            }
            // The sign: the mode's largest coefficient positive (deterministic).
            let big = y.iter().copied().fold(0.0f64, |b, v| if v.abs() > b.abs() { v } else { b });
            let sign = if big < 0.0 { -1.0 } else { 1.0 };
            for i in 0..n {
                coefficients[i * n + k] = sign * y[i];
            }
        }
        Ok(Modes { nu2, coefficients })
    }

    /// ν₀ by Chebyshev collocation of order `order` (4 to [`MAX_ORDER`]).
    pub fn dominant_at(&self, frequency: f64, order: usize) -> Result<f64, TheoryError> {
        let estimate = self.modes(frequency, 24)?.nu2[0];
        self.collocated(frequency, order, estimate)
    }

    fn collocated(&self, frequency: f64, order: usize, estimate: f64) -> Result<f64, TheoryError> {
        self.check()?;
        let kappa2 = self.kappa2(frequency)?;
        if !(4..=MAX_ORDER).contains(&order) {
            return Err(TheoryError::Invalid(format!("collocation order {order} (4 to {MAX_ORDER})")));
        }
        let n = order;
        let (r, w) = (self.radius, self.width());
        let (x, d) = chebyshev(n);
        // d/dρ = (2/w) d/dx, ρ = r + w(1 + x)/2.
        let d: Vec<f64> = d.iter().map(|v| v * 2.0 / w).collect();
        let rho: Vec<f64> = x.iter().map(|x| r + w * (1.0 + x) / 2.0).collect();
        let size = n + 1;
        let mut d2 = vec![0.0; size * size];
        for i in 0..size {
            for j in 0..size {
                d2[i * size + j] = (0..size).map(|k| d[i * size + k] * d[k * size + j]).sum();
            }
        }
        let op = |i: usize, j: usize| {
            let diag = if i == j { kappa2 * rho[i] * rho[i] } else { 0.0 };
            rho[i] * rho[i] * d2[i * size + j] + rho[i] * d[i * size + j] + diag
        };
        let m = n - 1;
        let mut l = vec![0.0; m * m];
        for i in 0..m {
            for j in 0..m {
                l[i * m + j] = op(i + 1, j + 1);
            }
        }
        if self.neumann() {
            // [d00 d0N; dN0 dNN] (u0, uN) = −(row 0, row N of D on the interior) u.
            let (a00, a0n, an0, ann) = (d[0], d[n], d[n * size], d[n * size + n]);
            let det = a00 * ann - a0n * an0;
            if !(det.abs() > 0.0) {
                return Err(TheoryError::Invalid("collocation: singular boundary rows".into()));
            }
            for j in 0..m {
                let (e0, en) = (d[j + 1], d[n * size + j + 1]);
                let u0 = -(ann * e0 - a0n * en) / det;
                let un = -(-an0 * e0 + a00 * en) / det;
                for i in 0..m {
                    l[i * m + j] += op(i + 1, 0) * u0 + op(i + 1, n) * un;
                }
            }
        }
        // Inverse iteration, shifted just off the estimate.
        let sigma = estimate + 1e-6 * estimate.abs().max(1.0);
        let mut shifted = l.clone();
        for i in 0..m {
            shifted[i * m + i] -= sigma;
        }
        let mut v = vec![1.0; m];
        let mut lambda = estimate;
        for _ in 0..60 {
            let y = emcraft_fem::dense_solve(shifted.clone(), v.clone())
                .ok_or_else(|| TheoryError::Invalid("collocation: singular shifted operator".into()))?;
            let scale = y.iter().copied().fold(0.0f64, |b, v| if v.abs() > b.abs() { v } else { b });
            if !(scale.is_finite() && scale != 0.0) {
                return Err(TheoryError::Invalid("collocation: inverse iteration broke down".into()));
            }
            v = y.iter().map(|y| y / scale).collect();
            let lv: Vec<f64> = (0..m).map(|i| (0..m).map(|j| l[i * m + j] * v[j]).sum()).collect();
            let next = v.iter().zip(&lv).map(|(a, b)| a * b).sum::<f64>() / v.iter().map(|a| a * a).sum::<f64>();
            let settled = (next - lambda).abs() <= 1e-15 * next.abs().max(1.0);
            lambda = next;
            if settled {
                break;
            }
        }
        if !(lambda.is_finite()) {
            return Err(TheoryError::Invalid("collocation: no eigenvalue".into()));
        }
        if lambda <= 0.0 {
            return Err(TheoryError::Invalid(format!("frequency {frequency} Hz: the curved guide's dominant mode does not propagate")));
        }
        Ok(lambda.sqrt())
    }
}

/// The relative change between two collocation orders that [`dominant`] accepts.
pub const SETTLED: f64 = 1e-11;

/// ν₀ of the curved guide at `frequency` (Hz), the collocation order raised (12, 16, 20, … up to
/// [`MAX_ORDER`]) until two orders agree to [`SETTLED`].
pub fn dominant(guide: &CurvedGuide, frequency: f64) -> Result<Dominant, TheoryError> {
    let estimate = guide.modes(frequency, 24)?.nu2[0];
    let mut previous = guide.collocated(frequency, 8, estimate)?;
    let mut order = 12;
    while order <= MAX_ORDER {
        let nu = guide.collocated(frequency, order, estimate)?;
        let change = (nu - previous).abs() / nu;
        if change < SETTLED {
            return Ok(Dominant { nu, order, change });
        }
        previous = nu;
        order += 4;
    }
    Err(TheoryError::Invalid(format!("curved guide: ν₀ did not settle by collocation order {MAX_ORDER}")))
}

/// The straight guide's and the curved guide's matrices for the junctions: C (coefficients),
/// G = Cᵀ D B, and the ν and β of each mode.
struct Junction {
    n: usize,
    c: Vec<f64>,
    g: Vec<C>,
    nu: Vec<C>,
}

fn junction_matrices(guide: &CurvedGuide, frequency: f64, modes: usize) -> Result<Junction, TheoryError> {
    let kappa2 = guide.kappa2(frequency)?;
    let n = modes;
    let set = guide.modes(frequency, n)?;
    if !(guide.straight_beta(kappa2, 0).re > 0.0 && set.nu2[0] > 0.0) {
        return Err(TheoryError::Invalid(format!("frequency {frequency} Hz: the dominant mode does not propagate")));
    }
    let mut g = vec![C::new(0.0, 0.0); n * n];
    for k in 0..n {
        for m in 0..n {
            g[k * n + m] = set.coefficients[m * n + k] * guide.norm(m) * guide.straight_beta(kappa2, m);
        }
    }
    let nu = set.nu2.iter().map(|v| crate::decaying_sqrt(C::new(*v, 0.0))).collect();
    Ok(Junction { n, c: set.coefficients, g, nu })
}

/// The dominant mode's reflection at a single junction of the straight guide into the curved
/// one (matched beyond), with `modes` modes: Rice's junction reflection, ∝ (w/R)² when gentle.
pub fn junction(guide: &CurvedGuide, frequency: f64, modes: usize) -> Result<C, TheoryError> {
    let j = junction_matrices(guide, frequency, modes)?;
    let n = j.n;
    let dim = 2 * n;
    // Unknowns (b, c): b − C c = −a; −G b − V c = −G a.
    let mut mat = vec![C::new(0.0, 0.0); dim * dim];
    let mut rhs = vec![C::new(0.0, 0.0); dim];
    for i in 0..n {
        mat[i * dim + i] = C::new(1.0, 0.0);
        for k in 0..n {
            mat[i * dim + n + k] = C::new(-j.c[i * n + k], 0.0);
            mat[(n + i) * dim + k] = -j.g[i * n + k];
        }
        mat[(n + i) * dim + n + i] = -j.nu[i];
        rhs[n + i] = -j.g[i * n];
    }
    rhs[0] = C::new(-1.0, 0.0);
    let x = dense_solve(mat, rhs, dim).ok_or_else(|| TheoryError::Invalid("junction: singular system".into()))?;
    Ok(if guide.neumann() { -x[0] } else { x[0] })
}

/// (S11, S21) of the dominant mode of a bend of `angle` (radians, 0 to 2π) between two straight
/// guides, at the junction planes, with `modes` modes (1 to [`MAX_MODES`]).
pub fn bend(guide: &CurvedGuide, angle: f64, frequency: f64, modes: usize) -> Result<(C, C), TheoryError> {
    if !(angle.is_finite() && angle > 0.0 && angle <= 2.0 * PI) {
        return Err(TheoryError::Invalid(format!("bend angle {angle}")));
    }
    let j = junction_matrices(guide, frequency, modes)?;
    let n = j.n;
    let dim = 4 * n;
    let e: Vec<C> = j.nu.iter().map(|nu| (-C::new(0.0, 1.0) * nu * angle).exp()).collect();
    // Unknowns (b, c, d′, e): d = E d′ at the first junction, d′ the backward amplitude at the
    // second; e the wave out of the second junction.
    let (bo, co, dpo, eo) = (0, n, 2 * n, 3 * n);
    let mut mat = vec![C::new(0.0, 0.0); dim * dim];
    let mut rhs = vec![C::new(0.0, 0.0); dim];
    let one = C::new(1.0, 0.0);
    for i in 0..n {
        let (r1, r2, r3, r4) = (i, n + i, 2 * n + i, 3 * n + i);
        // a + b = C(c + E d′).
        mat[r1 * dim + bo + i] = one;
        // G(a − b) = V(c − E d′).
        mat[r2 * dim + co + i] = -j.nu[i];
        mat[r2 * dim + dpo + i] = j.nu[i] * e[i];
        // C(E c + d′) = e.
        mat[r3 * dim + eo + i] = -one;
        // V(E c − d′) = G e.
        mat[r4 * dim + co + i] = j.nu[i] * e[i];
        mat[r4 * dim + dpo + i] = -j.nu[i];
        for k in 0..n {
            let cik = C::new(j.c[i * n + k], 0.0);
            mat[r1 * dim + co + k] = -cik;
            mat[r1 * dim + dpo + k] = -cik * e[k];
            mat[r2 * dim + bo + k] = -j.g[i * n + k];
            mat[r3 * dim + co + k] = cik * e[k];
            mat[r3 * dim + dpo + k] = cik;
            mat[r4 * dim + eo + k] = -j.g[i * n + k];
        }
        rhs[r2] = -j.g[i * n];
    }
    rhs[0] = -one;
    let x = dense_solve(mat, rhs, dim).ok_or_else(|| TheoryError::Invalid("bend: singular system".into()))?;
    let sign = if guide.neumann() { -1.0 } else { 1.0 };
    Ok((sign * x[bo], x[eo]))
}

/// Chebyshev points x_j = cos(jπ/N) and the differentiation matrix (Trefethen 2000, `cheb`).
fn chebyshev(n: usize) -> (Vec<f64>, Vec<f64>) {
    let size = n + 1;
    let x: Vec<f64> = (0..size).map(|j| (PI * j as f64 / n as f64).cos()).collect();
    let c = |i: usize| (if i == 0 || i == n { 2.0 } else { 1.0 }) * if i.is_multiple_of(2) { 1.0 } else { -1.0 };
    let mut d = vec![0.0; size * size];
    for i in 0..size {
        let mut sum = 0.0;
        for j in 0..size {
            if i != j {
                let v = c(i) / c(j) / (x[i] - x[j]);
                d[i * size + j] = v;
                sum += v;
            }
        }
        d[i * size + i] = -sum;
    }
    (x, d)
}

/// The lower Cholesky factor of a symmetric positive definite matrix (row-major).
fn cholesky(a: &[f64], n: usize) -> Option<Vec<f64>> {
    let mut l = vec![0.0; n * n];
    for i in 0..n {
        for j in 0..=i {
            let s: f64 = (0..j).map(|k| l[i * n + k] * l[j * n + k]).sum();
            if i == j {
                let d = a[i * n + i] - s;
                if !(d > 0.0) {
                    return None;
                }
                l[i * n + i] = d.sqrt();
            } else {
                l[i * n + j] = (a[i * n + j] - s) / l[j * n + j];
            }
        }
    }
    Some(l)
}

/// Column `col` of `x` (row-major, `width` columns) replaced by L⁻¹ times it.
fn forward(l: &[f64], n: usize, x: &mut [f64], col: usize, width: usize) {
    for i in 0..n {
        let s: f64 = (0..i).map(|k| l[i * n + k] * x[k * width + col]).sum();
        x[i * width + col] = (x[i * width + col] - s) / l[i * n + i];
    }
}

fn transpose(a: &[f64], n: usize) -> Vec<f64> {
    let mut t = vec![0.0; n * n];
    for i in 0..n {
        for j in 0..n {
            t[j * n + i] = a[i * n + j];
        }
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::FRAC_PI_2;

    const A: f64 = 22.86e-3;
    const B: f64 = 10.16e-3;

    fn h(r: f64) -> CurvedGuide {
        CurvedGuide { plane: Plane::H, a: A, b: B, radius: r }
    }
    fn e(r: f64) -> CurvedGuide {
        CurvedGuide { plane: Plane::E, a: A, b: B, radius: r }
    }
    fn beta(f: f64) -> f64 {
        let k = 2.0 * PI * f / C0;
        (k * k - (PI / A).powi(2)).sqrt()
    }

    #[test]
    fn the_straight_limit_gives_beta() {
        // ν₀/R → β with the error O((w/R)²).
        for g in [h(1.0), e(1.0), h(10.0), e(10.0)] {
            let nu = dominant(&g, 10e9).unwrap().nu;
            let rel = (nu / g.mean_radius() - beta(10e9)).abs() / beta(10e9);
            let w_r = g.width() / g.mean_radius();
            assert!(rel < 0.5 * w_r * w_r, "{g:?} {rel}");
        }
        let err = |r: f64| (dominant(&h(r), 10e9).unwrap().nu / h(r).mean_radius() - beta(10e9)).abs();
        let rate = (err(1.0) / err(2.0)).log2();
        assert!((rate - 2.0).abs() < 0.05, "{rate}");
    }

    #[test]
    fn collocation_converges_spectrally_and_matches_galerkin() {
        for g in [h(28.6e-3), e(35e-3)] {
            let reference = dominant(&g, 10e9).unwrap();
            assert!(reference.order <= 48 && reference.change < SETTLED, "{reference:?}");
            let err = |n: usize| (g.dominant_at(10e9, n).unwrap() - reference.nu).abs() / reference.nu;
            assert!(err(8) < 1e-4 && err(16) < 1e-10 && err(32) < 1e-11, "{} {} {}", err(8), err(16), err(32));
            // The Galerkin ν² converges algebraically to the same value.
            let gal = |n: usize| (g.modes(10e9, n).unwrap().nu2[0].sqrt() - reference.nu).abs() / reference.nu;
            assert!(gal(64) < gal(16) && gal(64) < 1e-6, "{} {}", gal(16), gal(64));
        }
    }

    #[test]
    fn the_eigenvalue_solves_the_bessel_equation() {
        // Integrate ρ²u″ + ρu′ + (κ²ρ² − ν₀²)u = 0 from the inner wall by RK4: it meets the
        // outer wall's condition, with no zero inside (the dominant mode).
        for g in [h(28.6e-3), e(35e-3)] {
            let f = 9e9;
            let nu = dominant(&g, f).unwrap().nu;
            let kappa2 = g.kappa2(f).unwrap();
            let rhs = |rho: f64, y: [f64; 2]| [y[1], -y[1] / rho - (kappa2 - nu * nu / (rho * rho)) * y[0]];
            let steps = 4000;
            let hstep = g.width() / steps as f64;
            let mut y = if g.neumann() { [1.0, 0.0] } else { [0.0, 1.0] };
            let mut rho = g.radius;
            let mut sign_changes = 0;
            for _ in 0..steps {
                let k1 = rhs(rho, y);
                let k2 = rhs(rho + hstep / 2.0, [y[0] + hstep / 2.0 * k1[0], y[1] + hstep / 2.0 * k1[1]]);
                let k3 = rhs(rho + hstep / 2.0, [y[0] + hstep / 2.0 * k2[0], y[1] + hstep / 2.0 * k2[1]]);
                let k4 = rhs(rho + hstep, [y[0] + hstep * k3[0], y[1] + hstep * k3[1]]);
                let prev = y[0];
                for i in 0..2 {
                    y[i] += hstep / 6.0 * (k1[i] + 2.0 * k2[i] + 2.0 * k3[i] + k4[i]);
                }
                rho += hstep;
                if prev * y[0] < 0.0 {
                    sign_changes += 1;
                }
            }
            if g.neumann() {
                assert!(y[1].abs() < 1e-8 * kappa2.sqrt(), "{y:?}");
                assert_eq!(sign_changes, 0);
            } else {
                assert!(y[0].abs() < 1e-9, "{y:?}");
            }
        }
    }

    #[test]
    fn bends_are_lossless_and_reduce_to_the_straight_guide() {
        for g in [h(28.6e-3), e(35e-3)] {
            for f in [8.2e9, 10e9, 12.4e9] {
                let (s11, s21) = bend(&g, FRAC_PI_2, f, 24).unwrap();
                assert!((s11.norm_sqr() + s21.norm_sqr() - 1.0).abs() < 1e-9, "{g:?} {f} {s11} {s21}");
            }
        }
        // A large radius and a short arc of length 30 mm: S21 → e^{−jβL}, S11 → 0.
        for g in [h(5.0), e(5.0)] {
            let theta = 0.03 / g.mean_radius();
            let (s11, s21) = bend(&g, theta, 10e9, 16).unwrap();
            let want = C::from_polar(1.0, -beta(10e9) * 0.03);
            assert!(s11.norm() < 1e-4 && (s21 - want).norm() < 1e-4, "{s11} {s21} {want}");
        }
    }

    #[test]
    fn bends_converge_in_the_modes_and_follow_nu() {
        for g in [h(28.6e-3), e(35e-3)] {
            let f = 10e9;
            let (r, t) = bend(&g, FRAC_PI_2, f, 40).unwrap();
            let (r2, t2) = bend(&g, FRAC_PI_2, f, 20).unwrap();
            assert!((r - r2).norm() < 1e-4 && (t - t2).norm() < 1e-4, "{} {}", (r - r2).norm(), (t - t2).norm());
            // The phase is the curved section's ν₀π/2 to within a few times |S11|.
            let nu = dominant(&g, f).unwrap().nu;
            let lead = C::from_polar(1.0, -nu * FRAC_PI_2);
            let diff = (t / lead).arg().abs();
            assert!(diff < 3.0 * r.norm() + 1e-3, "{diff} {}", r.norm());
        }
    }

    #[test]
    fn a_junction_reflects_as_the_square_of_the_curvature() {
        // Rice 1948: |Γ| ∝ (w/R)² for gentle bends.
        for g in [h(0.2), e(0.2)] {
            let g2 = CurvedGuide { radius: 2.0 * g.mean_radius() - g.width() / 2.0, ..g };
            let (r1, r2) = (junction(&g, 10e9, 24).unwrap().norm(), junction(&g2, 10e9, 24).unwrap().norm());
            let ratio = r1 / r2;
            assert!((ratio - 4.0).abs() < 0.2, "{g:?} {ratio}");
        }
    }

    #[test]
    fn hostile_inputs_are_refused() {
        let g = h(28.6e-3);
        for f in [f64::NAN, -1.0, 0.0, f64::INFINITY, 1e20] {
            assert!(dominant(&g, f).is_err(), "{f}");
            assert!(bend(&g, FRAC_PI_2, f, 10).is_err(), "{f}");
        }
        // Below cut-off.
        assert!(dominant(&g, 5e9).is_err());
        assert!(dominant(&e(35e-3), 5e9).is_err());
        assert!(bend(&g, FRAC_PI_2, 5e9, 10).is_err());
        for r in [0.0, -1.0, f64::NAN, f64::INFINITY, 1e9] {
            assert!(dominant(&h(r), 10e9).is_err(), "{r}");
        }
        for angle in [0.0, -1.0, f64::NAN, 7.0] {
            assert!(bend(&g, angle, 10e9, 10).is_err(), "{angle}");
        }
        assert!(bend(&g, FRAC_PI_2, 10e9, 0).is_err());
        assert!(bend(&g, FRAC_PI_2, 10e9, MAX_MODES + 1).is_err());
        assert!(g.dominant_at(10e9, 3).is_err() && g.dominant_at(10e9, MAX_ORDER + 1).is_err());
    }
}
