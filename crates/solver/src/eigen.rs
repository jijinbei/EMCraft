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
//! **Lossy dielectrics** make M complex, ε = ε′(1 − j tan δ): the eigenvalues k² are then
//! complex, and the iteration runs in complex arithmetic — (K − σM)⁻¹ is complex symmetric,
//! the Rayleigh–Ritz projection uses the unconjugated YᵀKY, YᵀMY, its small non-Hermitian
//! problem goes to [`crate::zdense`]. Each mode's complex frequency ω = c√(k²) gives the
//! dielectric's Q exactly, Q_d = Re ω / (2 Im ω), not by perturbation.
//!
//! **Q factors** of the walls come by perturbation (D. M. Pozar, *Microwave Engineering*, 4th ed., §6.7):
//! with the lossless mode, Q_c = ωU/P_c from the walls' ½R_s∫|H_t|² and Q_d = ωU/P_d from the
//! dielectrics' ½ωε₀∫ε″|E|², 1/Q = 1/Q_c + 1/Q_d.

use std::f64::consts::PI;

use emcraft_fem::Order;
use emcraft_model::Model;
use emcraft_model::units::C0;
use num_complex::Complex64 as C;

use crate::driven::{Driven, Result, SolveError, check_frequency, invalid, wavenumber};
use crate::ldlt::Ldlt;
use crate::sparse::Csr;

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
    /// Q of the walls and of the dielectrics (`None` when lossless).
    pub q_walls: Option<f64>,
    pub q_dielectric: Option<f64>,
}

impl Mode {
    /// The total Q, 1/Q = 1/Q_c + 1/Q_d.
    pub fn q(&self) -> Option<f64> {
        let inv = self.q_walls.map_or(0.0, |q| 1.0 / q) + self.q_dielectric.map_or(0.0, |q| 1.0 / q);
        (inv > 0.0).then(|| 1.0 / inv)
    }
}

/// The discrete gradients as sparse columns over the unknowns, and a point per column (for the
/// ordering of ZᵀMZ). Only gradients that lie wholly among the unknowns: a node on a conducting
/// wall has its gradient's tangential part eliminated.
fn gradients(driven: &Driven) -> (Vec<Vec<(usize, f64)>>, Vec<emcraft_model::Point3>) {
    let (topo, dofs, mesh) = (&driven.topo, &driven.dofs, &driven.mesh);
    let pe = dofs.order.per_edge();
    let nodes = mesh.nodes.len();
    let mut incident: Vec<Vec<(usize, f64)>> = vec![Vec::new(); nodes];
    let mut blocked = vec![false; nodes];
    for (e, &[a, b]) in topo.edges.iter().enumerate() {
        let (a, b) = (a as usize, b as usize);
        match dofs.unknown_of(e * pe) {
            Some(u) => {
                // ∇φ_n = Σ_edges (φ(b) − φ(a)) w_ab: +1 where n is the edge's second node.
                if let Some(l) = incident.get_mut(a) {
                    l.push((u, -1.0));
                }
                if let Some(l) = incident.get_mut(b) {
                    l.push((u, 1.0));
                }
            }
            None => {
                for n in [a, b] {
                    if let Some(x) = blocked.get_mut(n) {
                        *x = true;
                    }
                }
            }
        }
    }
    let mut columns = Vec::new();
    let mut points = Vec::new();
    for (n, list) in incident.into_iter().enumerate() {
        if !blocked[n] && !list.is_empty() {
            columns.push(list);
            points.push(mesh.nodes[n]);
        }
    }
    if pe > 1 {
        for (e, &[a, b]) in topo.edges.iter().enumerate() {
            if let Some(u) = dofs.unknown_of(e * pe + 1) {
                columns.push(vec![(u, 1.0)]);
                points.push(mesh.nodes[a as usize].midpoint(mesh.nodes[b as usize]));
            }
        }
    }
    (columns, points)
}

/// x ↦ x − Z (ZᵀMZ)⁻¹ ZᵀM x: the M-orthogonal projection off the discrete gradients.
struct Deflation {
    columns: Vec<Vec<(usize, f64)>>,
    factor: Option<Ldlt>,
}

impl Deflation {
    /// `mass_entry(k)`: M's entry k on `pattern` (its real part alone for a real ZᵀMZ).
    fn new(driven: &Driven, pattern: &Csr, mass_entry: impl Fn(usize) -> C) -> Result<Deflation> {
        let (columns, points) = gradients(driven);
        let m = columns.len();
        if m == 0 {
            return Ok(Deflation { columns, factor: None });
        }
        // Which columns hold each unknown.
        let mut owners: Vec<Vec<(usize, f64)>> = vec![Vec::new(); pattern.n];
        for (c, col) in columns.iter().enumerate() {
            for &(u, z) in col {
                if let Some(o) = owners.get_mut(u) {
                    o.push((c, z));
                }
            }
        }
        // ZᵀMZ row by row: w = M z_a, then (ZᵀMZ)_ab = z_b · w; summed in a fixed order.
        let mut ptr = Vec::with_capacity(m + 1);
        ptr.push(0);
        let (mut col, mut val) = (Vec::new(), Vec::new());
        for za in &columns {
            let mut w: std::collections::BTreeMap<usize, C> = std::collections::BTreeMap::new();
            for &(i, z) in za {
                for k in pattern.ptr[i]..pattern.ptr[i + 1] {
                    *w.entry(pattern.col[k]).or_insert(C::new(0.0, 0.0)) += mass_entry(k) * z;
                }
            }
            let mut row: std::collections::BTreeMap<usize, C> = std::collections::BTreeMap::new();
            for (j, wj) in w {
                for &(b, zb) in owners.get(j).map_or(&[][..], Vec::as_slice) {
                    *row.entry(b).or_insert(C::new(0.0, 0.0)) += wj * zb;
                }
            }
            for (b, v) in row {
                col.push(b);
                val.push(v);
            }
            ptr.push(col.len());
        }
        let a = Csr { n: m, ptr, col, val };
        Ok(Deflation { columns, factor: Some(Ldlt::factor(&a, &points)?) })
    }

    /// Projects each vector in place; `my` holds M times each (as computed by the caller).
    fn project(&self, xs: &mut [Vec<C>], mys: &[Vec<C>]) -> Result<()> {
        let Some(f) = &self.factor else { return Ok(()) };
        let mut rhs: Vec<Vec<C>> = mys.iter().map(|my| self.columns.iter().map(|z| z.iter().map(|&(u, c)| my[u] * c).sum()).collect()).collect();
        f.solve_many(&mut rhs)?;
        for (x, s) in xs.iter_mut().zip(&rhs) {
            for (z, coef) in self.columns.iter().zip(s) {
                for &(u, c) in z {
                    x[u] -= *coef * c;
                }
            }
        }
        Ok(())
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
    stiffness: &'a [f64],
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
    /// The dielectric's Q: exactly from the complex frequency, or by perturbation, ω U / P_d,
    /// `omega_u` being ω U.
    fn q_dielectric(driven: &Driven, vector: &[C], value: Self::Value, frequency: f64, omega_u: f64) -> Result<Option<f64>>;
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
            let s: f64 = (pattern.ptr[i]..pattern.ptr[i + 1]).map(|k| val(k) * x[pattern.col[k]].re).sum();
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
            *x = C::new(m.stiffness[k] - sigma * m.mass[k].re, 0.0);
        }
        a
    }

    fn mass_entry(m: Matrices, k: usize) -> C {
        C::new(m.mass[k].re, 0.0)
    }

    fn kmul(m: Matrices, v: &[C]) -> Vec<C> {
        mul_real(m.pattern, |k| m.stiffness[k], v)
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

    fn q_dielectric(driven: &Driven, vector: &[C], _: f64, frequency: f64, omega_u: f64) -> Result<Option<f64>> {
        let pd = driven.dielectric_loss(vector, frequency)?;
        Ok((pd > 0.0).then(|| omega_u / pd))
    }
}

impl Arithmetic for Complex {
    type Value = C;

    fn shifted(m: Matrices, sigma: f64) -> Csr {
        let mut a = m.pattern.clone();
        for (k, x) in a.val.iter_mut().enumerate() {
            *x = C::new(m.stiffness[k], 0.0) - m.mass[k] * sigma;
        }
        a
    }

    fn mass_entry(m: Matrices, k: usize) -> C {
        m.mass[k]
    }

    fn kmul(m: Matrices, v: &[C]) -> Vec<C> {
        let p = m.pattern;
        (0..p.n).map(|i| (p.ptr[i]..p.ptr[i + 1]).map(|k| v[p.col[k]] * m.stiffness[k]).sum()).collect()
    }

    fn mmul(m: Matrices, v: &[C]) -> Vec<C> {
        let p = m.pattern;
        (0..p.n).map(|i| (p.ptr[i]..p.ptr[i + 1]).map(|k| m.mass[k] * v[p.col[k]]).sum()).collect()
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

    fn q_dielectric(_: &Driven, _: &[C], value: C, _: f64, _: f64) -> Result<Option<f64>> {
        let omega = complex_omega(value);
        Ok((omega.im.abs() > 0.0).then(|| omega.re / (2.0 * omega.im.abs())))
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
    let mut l: Vec<f64> = vec![0.0; q * q];
    for j in 0..q {
        let d = mr[j * q + j] - (0..j).map(|k| l[j * q + k].powi(2)).sum::<f64>();
        if !(d > 0.0) {
            return Err(lost_independence());
        }
        l[j * q + j] = d.sqrt();
        for i in j + 1..q {
            l[i * q + j] = (mr[i * q + j] - (0..j).map(|k| l[i * q + k] * l[j * q + k]).sum::<f64>()) / l[j * q + j];
        }
    }
    // Z = L⁻¹ Kr, then C = L⁻¹ Zᵀ (C symmetric).
    let lsolve = |b: &[f64]| -> Vec<f64> {
        let mut z = vec![0.0; q];
        for i in 0..q {
            z[i] = (b[i] - (0..i).map(|k| l[i * q + k] * z[k]).sum::<f64>()) / l[i * q + i];
        }
        z
    };
    let mut z = vec![0.0; q * q];
    for j in 0..q {
        let col: Vec<f64> = (0..q).map(|i| kr[i * q + j]).collect();
        let s = lsolve(&col);
        for i in 0..q {
            z[i * q + j] = s[i];
        }
    }
    let mut cm = vec![0.0; q * q];
    for i in 0..q {
        let row: Vec<f64> = (0..q).map(|j| z[i * q + j]).collect();
        let s = lsolve(&row);
        for j in 0..q {
            cm[j * q + i] = s[j];
        }
    }
    let (lam, v) = jacobi(cm, q);
    // Φ = L⁻ᵀ V.
    let mut phi = vec![0.0; q * q];
    for c in 0..q {
        for i in (0..q).rev() {
            let s: f64 = (i + 1..q).map(|k| l[k * q + i] * phi[k * q + c]).sum();
            phi[i * q + c] = (v[i * q + c] - s) / l[i * q + i];
        }
    }
    Ok((lam, phi))
}

/// Deterministic starting vectors: `q` of length `n`, entries uniform in [−½, ½) from a
/// xorshift generator with a fixed seed.
fn start_vectors(q: usize, n: usize) -> Vec<Vec<C>> {
    let mut state: u64 = 0x2545_f491_4f6c_dd1d;
    let mut rand = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 11) as f64 / (1u64 << 53) as f64 - 0.5
    };
    (0..q).map(|_| (0..n).map(|_| C::new(rand(), 0.0)).collect()).collect()
}

/// The `count` modes nearest σ = k_near² by shift-invert subspace iteration in arithmetic `A`,
/// with the number of iterations it took.
fn iterate<A: Arithmetic>(model: &Model, driven: &Driven, count: usize, sigma: f64) -> Result<(Vec<Mode>, usize)> {
    let (pattern, stiffness, mass) = driven.matrices();
    let m = Matrices { pattern, stiffness, mass };
    let n = pattern.n;
    let factor = Ldlt::factor(&A::shifted(m, sigma), &driven.dofs.points)?;
    let q = (count + count.max(8)).min(n);
    let mut x = start_vectors(q, n);
    let deflation = Deflation::new(driven, pattern, |k| A::mass_entry(m, k))?;
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
            Some(s) => Some(omega * u / driven.wall_loss_perturbation(&vector, frequency, s)?),
            None => None,
        };
        let q_dielectric = A::q_dielectric(driven, &vector, lambda, frequency, omega * u)?;
        modes.push(Mode { frequency, vector, q_walls, q_dielectric });
    }
    modes.sort_by(|a, b| a.frequency.total_cmp(&b.frequency));
    Ok((modes, iterations))
}

/// Eigenvalues and vectors of a dense symmetric matrix (row-major, n × n) by cyclic Jacobi
/// rotations (Golub & Van Loan, *Matrix Computations*, 4th ed., §8.5).
fn jacobi(mut a: Vec<f64>, n: usize) -> (Vec<f64>, Vec<f64>) {
    let mut v = vec![0.0; n * n];
    for i in 0..n {
        v[i * n + i] = 1.0;
    }
    let norm: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    for _ in 0..100 {
        let off: f64 =
            (0..n).flat_map(|i| (0..n).filter(move |&j| j != i).map(move |j| (i, j))).map(|(i, j)| a[i * n + j].powi(2)).sum::<f64>().sqrt();
        if off <= 1e-15 * norm {
            break;
        }
        for p in 0..n {
            for q in p + 1..n {
                let apq = a[p * n + q];
                if apq == 0.0 {
                    continue;
                }
                let theta = (a[q * n + q] - a[p * n + p]) / (2.0 * apq);
                let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
                let t = if theta == 0.0 { 1.0 } else { t };
                let c = 1.0 / (t * t + 1.0).sqrt();
                let s = t * c;
                for k in 0..n {
                    let (akp, akq) = (a[k * n + p], a[k * n + q]);
                    a[k * n + p] = c * akp - s * akq;
                    a[k * n + q] = s * akp + c * akq;
                }
                for k in 0..n {
                    let (apk, aqk) = (a[p * n + k], a[q * n + k]);
                    a[p * n + k] = c * apk - s * aqk;
                    a[q * n + k] = s * apk + c * aqk;
                }
                for k in 0..n {
                    let (vkp, vkq) = (v[k * n + p], v[k * n + q]);
                    v[k * n + p] = c * vkp - s * vkq;
                    v[k * n + q] = s * vkp + c * vkq;
                }
            }
        }
    }
    ((0..n).map(|i| a[i * n + i]).collect(), v)
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
    let (pattern, _, mass) = driven.matrices();
    if pattern.n == 0 {
        return Err(SolveError::NotReady("the cavity has no unknowns".into()));
    }
    let k_near = wavenumber(near);
    let sigma = k_near * k_near;
    let lossy = mass.iter().any(|m| m.im != 0.0);
    let (modes, iterations) = if lossy { iterate::<Complex>(model, &driven, count, sigma)? } else { iterate::<Real>(model, &driven, count, sigma)? };
    Ok(Eigen { driven, modes, iterations })
}
