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
//! **The gradients are projected out** of the subspace at every step (P. Arbenz, R. Geus,
//! *Multilevel preconditioned iterative eigensolvers for Maxwell eigenvalue problems*, Appl.
//! Numer. Math. 54(2):107–121, 2005): with Z the discrete gradients — of the nodal functions
//! through the Whitney functions (coefficients φ_j − φ_i on edge i→j) and, at order 2, the edge
//! gradients ∇(λ_iλ_j), each a basis function of its own — every iterate becomes
//! x − Z (ZᵀMZ)⁻¹ ZᵀM x, M-orthogonal to them. The zero eigenvalue then cannot crowd the
//! subspace, so modes far above the shift come as readily as those near it.
//!
//! **Lossy dielectrics** make M complex, ε = ε′(1 − j tan δ), and **magnetic losses** K,
//! μ = μ′(1 − j tan δ_m): the eigenvalues k² are then complex, and the iteration runs in
//! complex arithmetic — (K − σM)⁻¹ is complex symmetric, the Rayleigh–Ritz projection uses the
//! unconjugated YᵀKY, YᵀMY, its small non-Hermitian problem goes to [`crate::zdense`]. Each
//! mode's complex frequency ω = c√(k²) gives the materials' Q exactly, Q = Re ω / (2 Im ω), not
//! by perturbation. With both kinds of loss it is shared between Q_d and Q_m in proportion to
//! the powers ½ωε₀∫ε″|E|² and ½ωμ₀∫μ″|H|² the mode's field puts into each, so that
//! 1/Q_d + 1/Q_m = 1/Q; with one kind the exact Q is that kind's.
//!
//! **Q factors** of the walls come by perturbation (D. M. Pozar, *Microwave Engineering*, 4th ed., §6.7):
//! with the lossless mode, Q_c = ωU/P_c from the walls' ½R_s∫|H_t|² and Q_d = ωU/P_d from the
//! dielectrics' ½ωε₀∫ε″|E|², 1/Q = 1/Q_c + 1/Q_d.

use std::f64::consts::PI;

use emcraft_fem::Order;
use emcraft_model::Model;
use emcraft_model::units::C0;
use num_complex::Complex64 as C;

use crate::dense::{Cholesky, JacobiStop, jacobi};
use crate::driven::{Driven, Result, SolveError, check_frequency, invalid, wavenumber};
use crate::ldlt::Ldlt;
use crate::sparse::Csr;
use crate::subspace::{Deflation, gradients, start_vectors};

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
    /// Q of the walls, of the dielectrics and of the magnetic losses (`None` when lossless).
    pub q_walls: Option<f64>,
    pub q_dielectric: Option<f64>,
    pub q_magnetic: Option<f64>,
}

impl Mode {
    /// The total Q, 1/Q = 1/Q_c + 1/Q_d + 1/Q_m.
    pub fn q(&self) -> Option<f64> {
        let inv = [self.q_walls, self.q_dielectric, self.q_magnetic].iter().map(|q| q.map_or(0.0, |q| 1.0 / q)).sum::<f64>();
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

/// The cavity's matrices on their pattern: K (real) and M (complex permittivity).
#[derive(Clone, Copy)]
struct Matrices<'a> {
    pattern: &'a Csr,
    stiffness: &'a [C],
    mass: &'a [C],
}

/// The arithmetic of the subspace iteration: [`Real`] for a lossless cavity (M real, k²
/// real, the projected pencil symmetric definite), [`Complex`] for lossy dielectrics (M
/// complex symmetric, k² complex, the projected pencil general). Everything else — the start
/// vectors, the deflation, the convergence test, the modes — is shared ([`iterate`]).
trait Arithmetic {
    /// An eigenvalue k² (1/m²).
    type Value: Copy;
    /// K − σM, to factor.
    fn shifted(m: Matrices, sigma: f64) -> Csr;
    /// M's entry k (on the pattern) as the deflation sees it.
    fn mass_entry(m: Matrices, k: usize) -> C;
    fn kmul(m: Matrices, v: &[C]) -> Vec<C>;
    fn mmul(m: Matrices, v: &[C]) -> Vec<C>;
    /// The iterates after the solve with (K − σM)⁻¹.
    fn after_solve(_y: &mut [Vec<C>]) {}
    /// Rayleigh–Ritz on the span of Y (KY and MY given): each eigenpair of the projected pencil
    /// with its Ritz vector.
    fn ritz(m: Matrices, y: &[Vec<C>], ky: &[Vec<C>], my: &[Vec<C>]) -> Result<Vec<(Self::Value, Vec<C>)>>;
    /// |k² − σ|: Ritz pairs are kept nearest σ first.
    fn distance(value: Self::Value, sigma: f64) -> f64;
    /// What is compared with the gradients' null threshold: k² (real), |k²| (complex).
    fn level(value: Self::Value) -> f64;
    /// Whether an eigenvalue has settled since the previous iteration's.
    fn settled(now: Self::Value, before: Self::Value) -> bool;
    /// The resonant frequency (Hz).
    fn frequency(value: Self::Value) -> f64;
    /// The materials' Q (dielectric, magnetic): exactly from the complex frequency, or by
    /// perturbation, ω U / P_d, `omega_u` being ω U.
    fn q_materials(driven: &Driven, vector: &[C], value: Self::Value, frequency: f64, omega_u: f64) -> Result<(Option<f64>, Option<f64>)>;
}

/// A lossless cavity: everything real.
struct Real;

/// Lossy dielectrics: complex arithmetic, (K − σM)⁻¹ complex symmetric, the Rayleigh–Ritz
/// projection by the unconjugated YᵀKY, YᵀMY, its small non-Hermitian problem by
/// [`crate::zdense`].
struct Complex;

/// y = A x for a matrix with values `val` on `pattern` (real parts only).
fn mul_real(pattern: &Csr, val: impl Fn(usize) -> f64, x: &[C]) -> Vec<C> {
    (0..pattern.n)
        .map(|i| {
            let s: f64 = pattern.row(i).map(|k| val(k) * x[pattern.col[k]].re).sum();
            C::new(s, 0.0)
        })
        .collect()
}

fn dot_real(a: &[C], b: &[C]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x.re * y.re).sum()
}

/// The unconjugated aᵀb.
fn dot(a: &[C], b: &[C]) -> C {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn lost_independence() -> SolveError {
    invalid("eigen iteration", "the subspace lost its independence")
}

impl Arithmetic for Real {
    type Value = f64;

    fn shifted(m: Matrices, sigma: f64) -> Csr {
        let mut a = m.pattern.clone();
        for (k, x) in a.val.iter_mut().enumerate() {
            *x = C::new(m.stiffness[k].re - sigma * m.mass[k].re, 0.0);
        }
        a
    }

    fn mass_entry(m: Matrices, k: usize) -> C {
        C::new(m.mass[k].re, 0.0)
    }

    fn kmul(m: Matrices, v: &[C]) -> Vec<C> {
        mul_real(m.pattern, |k| m.stiffness[k].re, v)
    }

    fn mmul(m: Matrices, v: &[C]) -> Vec<C> {
        mul_real(m.pattern, |k| m.mass[k].re, v)
    }

    fn after_solve(y: &mut [Vec<C>]) {
        for r in y {
            r.iter_mut().for_each(|c| *c = C::new(c.re, 0.0));
        }
    }

    fn ritz(m: Matrices, y: &[Vec<C>], ky: &[Vec<C>], my: &[Vec<C>]) -> Result<Vec<(f64, Vec<C>)>> {
        let (n, q) = (m.pattern.n, y.len());
        let (lam, phi) = symmetric_ritz(y, ky, my)?;
        Ok((0..q).map(|c| (lam[c], (0..n).map(|r| C::new((0..q).map(|k| y[k][r].re * phi[k * q + c]).sum(), 0.0)).collect())).collect())
    }

    fn distance(value: f64, sigma: f64) -> f64 {
        (value - sigma).abs()
    }

    fn level(value: f64) -> f64 {
        value
    }

    fn settled(now: f64, before: f64) -> bool {
        (now - before).abs() <= 1e-11 * now.abs()
    }

    fn frequency(value: f64) -> f64 {
        value.sqrt() * C0 / (2.0 * PI)
    }

    fn q_materials(driven: &Driven, vector: &[C], _: f64, frequency: f64, omega_u: f64) -> Result<(Option<f64>, Option<f64>)> {
        let pd = driven.dielectric_loss(vector, frequency)?;
        Ok(((pd > 0.0).then(|| omega_u / pd), None))
    }
}

impl Arithmetic for Complex {
    type Value = C;

    fn shifted(m: Matrices, sigma: f64) -> Csr {
        let mut a = m.pattern.clone();
        for (k, x) in a.val.iter_mut().enumerate() {
            *x = m.stiffness[k] - m.mass[k] * sigma;
        }
        a
    }

    fn mass_entry(m: Matrices, k: usize) -> C {
        m.mass[k]
    }

    fn kmul(m: Matrices, v: &[C]) -> Vec<C> {
        let p = m.pattern;
        (0..p.n).map(|i| p.row(i).map(|k| v[p.col[k]] * m.stiffness[k]).sum()).collect()
    }

    fn mmul(m: Matrices, v: &[C]) -> Vec<C> {
        let p = m.pattern;
        (0..p.n).map(|i| p.row(i).map(|k| m.mass[k] * v[p.col[k]]).sum()).collect()
    }

    fn ritz(m: Matrices, y: &[Vec<C>], ky: &[Vec<C>], my: &[Vec<C>]) -> Result<Vec<(C, Vec<C>)>> {
        let (n, q) = (m.pattern.n, y.len());
        // Kr z = λ Mr z ⇔ (Mr⁻¹ Kr) z = λ z.
        let mr: Vec<Vec<C>> = (0..q).map(|i| (0..q).map(|j| dot(&y[i], &my[j])).collect()).collect();
        let mut b = vec![C::new(0.0, 0.0); q * q];
        for j in 0..q {
            let col: Vec<C> = (0..q).map(|i| dot(&y[i], &ky[j])).collect();
            let s = crate::zdense::solve(mr.clone(), col).ok_or_else(lost_independence)?;
            for i in 0..q {
                b[i * q + j] = s[i];
            }
        }
        let pairs = crate::zdense::eigen(&b, q).ok_or_else(|| invalid("eigen iteration", "the projected problem did not converge"))?;
        // Each Ritz vector of unit M-norm (xᵀMx = 1).
        Ok(pairs
            .into_iter()
            .map(|(lambda, z)| {
                let v: Vec<C> = (0..n).map(|r| (0..q).map(|k| y[k][r] * z[k]).sum()).collect();
                let norm = dot(&v, &Self::mmul(m, &v)).sqrt();
                (lambda, if norm.norm() > 0.0 { v.into_iter().map(|e| e / norm).collect() } else { v })
            })
            .collect())
    }

    fn distance(value: C, sigma: f64) -> f64 {
        (value - sigma).norm()
    }

    fn level(value: C) -> f64 {
        value.norm()
    }

    fn settled(now: C, before: C) -> bool {
        (now - before).norm() <= 1e-11 * now.norm()
    }

    /// ω = c k, the root with Re > 0; under e^{jωt} a decaying mode has Im ω > 0.
    fn frequency(value: C) -> f64 {
        complex_omega(value).re / (2.0 * PI)
    }

    fn q_materials(driven: &Driven, vector: &[C], value: C, frequency: f64, _: f64) -> Result<(Option<f64>, Option<f64>)> {
        let omega = complex_omega(value);
        if !(omega.im.abs() > 0.0) {
            return Ok((None, None));
        }
        let q = omega.re / (2.0 * omega.im.abs());
        let (pd, pm) = (driven.dielectric_loss(vector, frequency)?, driven.magnetic_loss(vector, frequency)?);
        Ok(match (pd > 0.0, pm > 0.0) {
            (true, false) => (Some(q), None),
            (false, true) => (None, Some(q)),
            (true, true) => (Some(q * (pd + pm) / pd), Some(q * (pd + pm) / pm)),
            (false, false) => (Some(q), None),
        })
    }
}

/// ω = c√(k²), the root with Re > 0.
fn complex_omega(lambda: C) -> C {
    let k = lambda.sqrt();
    let k = if k.re < 0.0 { -k } else { k };
    k * C0
}

/// The real symmetric Rayleigh–Ritz step: with Kr = YᵀKY and Mr = YᵀMY = L Lᵀ, the
/// eigenvalues λ and vectors V of C = L⁻¹ Kr L⁻ᵀ give Kr Φ = Mr Φ Λ with Φ = L⁻ᵀ V (row-major
/// q × q).
fn symmetric_ritz(y: &[Vec<C>], ky: &[Vec<C>], my: &[Vec<C>]) -> Result<(Vec<f64>, Vec<f64>)> {
    let q = y.len();
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
    let l = Cholesky::new(&mr, q).ok_or_else(lost_independence)?;
    // Z = L⁻¹ Kr, then C = L⁻¹ Zᵀ (C symmetric).
    let mut z = vec![0.0; q * q];
    for j in 0..q {
        let col: Vec<f64> = (0..q).map(|i| kr[i * q + j]).collect();
        for (i, s) in l.lower_solve(&col).into_iter().enumerate() {
            z[i * q + j] = s;
        }
    }
    let mut cm = vec![0.0; q * q];
    for i in 0..q {
        let row: Vec<f64> = (0..q).map(|j| z[i * q + j]).collect();
        for (j, s) in l.lower_solve(&row).into_iter().enumerate() {
            cm[j * q + i] = s;
        }
    }
    let (lam, v) = jacobi(cm, q, JacobiStop::Relative);
    // Φ = L⁻ᵀ V.
    let mut phi = vec![0.0; q * q];
    for c in 0..q {
        let col: Vec<f64> = (0..q).map(|i| v[i * q + c]).collect();
        for (i, s) in l.upper_solve(&col).into_iter().enumerate() {
            phi[i * q + c] = s;
        }
    }
    Ok((lam, phi))
}

/// The `count` modes nearest σ = k_near² by shift-invert subspace iteration in arithmetic `A`,
/// with the number of iterations it took.
fn iterate<A: Arithmetic>(model: &Model, driven: &Driven, count: usize, sigma: f64) -> Result<(Vec<Mode>, usize)> {
    let (pattern, stiffness, mass) = driven.matrices();
    let m = Matrices { pattern, stiffness, mass };
    let n = pattern.n;
    let factor = Ldlt::factor(&A::shifted(m, sigma), &driven.dofs.points)?;
    let q = (count + count.max(8)).min(n);
    let mut x = start_vectors::<f64>(q, n, 0x2545_f491_4f6c_dd1d);
    // The gradients of the nodal functions through the Whitney functions, those of nodes on a
    // conducting wall left out (their tangential part is eliminated).
    let (columns, points) = gradients::<f64>(driven, |n| Some((n, 1.0)), |slot| driven.dofs.unknown_of(slot), |_| true);
    let deflation = Deflation::new(columns, &points, pattern, |k| A::mass_entry(m, k))?;
    let mx: Vec<Vec<C>> = x.iter().map(|v| A::mmul(m, v)).collect();
    deflation.project(&mut x, &mx)?;
    let null = 1e-6 * sigma;
    let mut lambdas: Vec<A::Value> = Vec::new();
    let mut previous: Vec<A::Value> = Vec::new();
    let mut iterations = 0;
    for it in 0..MAX_ITERATIONS {
        iterations = it + 1;
        // Y = (K − σM)⁻¹ M X, M-orthogonal to the gradients.
        let mut y: Vec<Vec<C>> = x.iter().map(|v| A::mmul(m, v)).collect();
        factor.solve_many(&mut y)?;
        A::after_solve(&mut y);
        let my_raw: Vec<Vec<C>> = y.iter().map(|v| A::mmul(m, v)).collect();
        deflation.project(&mut y, &my_raw)?;
        let ky: Vec<Vec<C>> = y.iter().map(|v| A::kmul(m, v)).collect();
        let my: Vec<Vec<C>> = y.iter().map(|v| A::mmul(m, v)).collect();
        let mut pairs = A::ritz(m, &y, &ky, &my)?;
        // Nearest σ first (a stable sort: ties keep their order).
        pairs.sort_by(|a, b| A::distance(a.0, sigma).total_cmp(&A::distance(b.0, sigma)));
        (lambdas, x) = pairs.into_iter().unzip();
        let wanted: Vec<A::Value> = lambdas.iter().copied().filter(|l| A::level(*l) > null).take(count).collect();
        let converged = wanted.len() == count && previous.len() == count && wanted.iter().zip(&previous).all(|(a, b)| A::settled(*a, *b));
        previous = wanted;
        if converged {
            break;
        }
    }
    let mut modes = Vec::new();
    for (vector, lambda) in x.into_iter().zip(lambdas) {
        if A::level(lambda) <= null || modes.len() == count {
            continue;
        }
        let frequency = A::frequency(lambda);
        let omega = 2.0 * PI * frequency;
        let u = driven.stored_energy(&vector, frequency)?;
        let q_walls = match model.wall_conductivity {
            Some(s) => Some(omega * u / driven.wall_loss_perturbation(&vector, frequency, s.get(), model.wall_thickness.map(|t| t.get()))?),
            None => None,
        };
        let (q_dielectric, q_magnetic) = A::q_materials(driven, &vector, lambda, frequency, omega * u)?;
        modes.push(Mode { frequency, vector, q_walls, q_dielectric, q_magnetic });
    }
    modes.sort_by(|a, b| a.frequency.total_cmp(&b.frequency));
    Ok((modes, iterations))
}

/// The `count` modes of the model's guide closed into a cavity nearest above and below
/// `near` (Hz): the lowest ones when `near` is a little above them.
pub fn eigenmodes(model: &Model, order: Order, count: usize, near: f64) -> Result<Eigen> {
    if count == 0 || count > MAX_MODES {
        return Err(invalid("mode count", format!("must be 1 to {MAX_MODES}")));
    }
    check_frequency(near)?;
    if model.periodic.is_some() {
        return Err(invalid("eigenmodes", "a periodic cell's eigenmodes depend on the phase across it: use solve.dispersion"));
    }
    let driven = Driven::cavity(model, order)?;
    if driven.is_dispersive() {
        // ε(ω) makes the eigenproblem nonlinear in ω.
        return Err(invalid(
            "eigenmodes",
            "not supported yet: eigenmodes of a cavity with dispersive materials (its eigenproblem is nonlinear in the frequency)",
        ));
    }
    let (pattern, stiffness, mass) = driven.matrices();
    if pattern.n == 0 {
        return Err(SolveError::NotReady("the cavity has no unknowns".into()));
    }
    let k_near = wavenumber(near);
    let sigma = k_near * k_near;
    let lossy = mass.iter().any(|m| m.im != 0.0) || stiffness.iter().any(|k| k.im != 0.0);
    let (modes, iterations) = if lossy { iterate::<Complex>(model, &driven, count, sigma)? } else { iterate::<Real>(model, &driven, count, sigma)? };
    Ok(Eigen { driven, modes, iterations })
}
