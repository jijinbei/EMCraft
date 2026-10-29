//! Dispersion diagrams: the eigenfrequencies of a periodic cell as the Floquet phase across it
//! is swept, the Bloch modes of an infinite periodic structure (a slow-wave guide, a photonic or
//! electromagnetic band-gap lattice).
//!
//! **The problem.** At a phase ψ per period ([`crate::driven::periodic`]), the cell's
//! perfectly conducting cavity problem K x = k² M x becomes Kψ x_r = k² Mψ x_r with
//! Kψ = Tᴴ K T and Mψ = Tᴴ M T: Hermitian, Mψ positive definite, so k² is real for a lossless
//! cell (J.-M. Jin, *The Finite Element Method in Electromagnetics*, 3rd ed., Wiley 2014, the chapter on
//! periodic structures; the band structures of photonic crystals, J. D. Joannopoulos et al., *Photonic Crystals*, 2nd ed., Princeton 2008,
//! ch. 3). Each ψ is solved on its own, the band index the order of the frequencies.
//!
//! **The iteration** is [`crate::eigen`]'s, in Hermitian arithmetic: shift-invert subspace
//! iteration on (Kψ − σMψ)⁻¹Mψ (factored by the unsymmetric LU, the shift's matrix being
//! Hermitian indefinite), Rayleigh–Ritz with the conjugate products YᴴKψY and YᴴMψY, whose
//! small Hermitian pencil is reduced by Cholesky and solved as the real symmetric problem
//! [[Re C, −Im C], [Im C, Re C]] of twice its order by Jacobi rotations (each eigenvalue twice;
//! one complex vector kept per pair). The curl's null space is projected out at every step:
//! the discrete gradients of Bloch-periodic nodal functions (φ = 1 at a node, e^{−jψ·s} at its
//! images s periods away), x ↦ x − Z (ZᴴMψZ)⁻¹ ZᴴMψ x (P. Arbenz, R. Geus, *Multilevel
//! preconditioned iterative eigensolvers for Maxwell eigenvalue problems*, Appl. Numer. Math.
//! 54(2):107–121, 2005).

use std::f64::consts::PI;

use emcraft_fem::Order;
use emcraft_model::Model;
use emcraft_model::units::C0;
use num_complex::Complex64 as C;

use crate::dense::{Cholesky, JacobiStop, jacobi};
use crate::driven::{Driven, Result, SolveError, Tie, check_frequency, invalid, wavenumber};
use crate::ldlt::Analysis;
use crate::lu::Lu;
use crate::subspace::{Deflation, gradients, start_vectors};

/// The most bands one diagram may ask for.
pub const MAX_BANDS: usize = 20;
/// The most phases one diagram may sweep.
pub const MAX_PHASES: usize = 201;
const MAX_ITERATIONS: usize = 200;

/// What to sweep: the phase per period along `axis` (0, 1, 2) from `from` to `to` (rad) in
/// `points` steps, the other periodic axes held at phase 0; `bands` frequencies at each, sought
/// below and about `near` (Hz).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sweep {
    pub axis: usize,
    pub from: f64,
    pub to: f64,
    pub points: usize,
    pub bands: usize,
    pub near: f64,
}

/// The bands at one phase.
#[derive(Clone, Debug, PartialEq)]
pub struct BlochPoint {
    /// The phase per period (rad).
    pub psi: f64,
    /// The Bloch wavenumber ψ/L (rad/m).
    pub beta: f64,
    /// The eigenfrequencies (Hz), ascending.
    pub frequencies: Vec<f64>,
    pub iterations: usize,
}

/// A dispersion diagram.
#[derive(Clone, Debug, PartialEq)]
pub struct Dispersion {
    pub axis: usize,
    /// The period along the axis swept (m).
    pub period: f64,
    pub points: Vec<BlochPoint>,
    /// Unknowns after the periodic reduction.
    pub unknowns: usize,
}

/// The conjugate product xᴴy.
fn dotc(x: &[C], y: &[C]) -> C {
    x.iter().zip(y).map(|(a, b)| a.conj() * b).sum()
}

/// The Hermitian Rayleigh–Ritz step on the span of Y: the eigenvalues of (YᴴKY, YᴴMY) and the
/// Ritz vectors Y Φ (of unit M-norm), ascending.
fn ritz(y: &[Vec<C>], ky: &[Vec<C>], my: &[Vec<C>]) -> Result<Vec<(f64, Vec<C>)>> {
    let q = y.len();
    let lost = || invalid("dispersion iteration", "the subspace lost its independence");
    let mut kr = vec![C::new(0.0, 0.0); q * q];
    let mut mr = kr.clone();
    for i in 0..q {
        for j in 0..q {
            kr[i * q + j] = dotc(&y[i], &ky[j]);
            mr[i * q + j] = dotc(&y[i], &my[j]);
        }
    }
    // Hermitian parts (rounding).
    for i in 0..q {
        for j in i..q {
            let (k, m) = (0.5 * (kr[i * q + j] + kr[j * q + i].conj()), 0.5 * (mr[i * q + j] + mr[j * q + i].conj()));
            kr[i * q + j] = k;
            kr[j * q + i] = k.conj();
            mr[i * q + j] = m;
            mr[j * q + i] = m.conj();
        }
    }
    // Mr = L Lᴴ.
    let l = Cholesky::new(&mr, q).ok_or_else(lost)?;
    // C = L⁻¹ Kr L⁻ᴴ: Z = L⁻¹ Kr, then C = (L⁻¹ Zᴴ)ᴴ.
    let mut z = vec![C::new(0.0, 0.0); q * q];
    for j in 0..q {
        let col: Vec<C> = (0..q).map(|i| kr[i * q + j]).collect();
        for (i, v) in l.lower_solve(&col).into_iter().enumerate() {
            z[i * q + j] = v;
        }
    }
    let mut cm = vec![C::new(0.0, 0.0); q * q];
    for i in 0..q {
        let row: Vec<C> = (0..q).map(|j| z[i * q + j].conj()).collect();
        for (j, v) in l.lower_solve(&row).into_iter().enumerate() {
            cm[i * q + j] = v.conj();
        }
    }
    // The real symmetric embedding [[A, −B], [B, A]] of C = A + jB.
    let n2 = 2 * q;
    let mut big = vec![0.0; n2 * n2];
    for i in 0..q {
        for j in 0..q {
            let c = 0.5 * (cm[i * q + j] + cm[j * q + i].conj());
            big[i * n2 + j] = c.re;
            big[(i + q) * n2 + j + q] = c.re;
            big[i * n2 + j + q] = -c.im;
            big[(i + q) * n2 + j] = c.im;
        }
    }
    let (lam, v) = jacobi(big, n2, JacobiStop::Relative);
    let mut order: Vec<usize> = (0..n2).collect();
    order.sort_by(|a, b| lam[*a].total_cmp(&lam[*b]));
    // One complex vector per pair: u + jv, kept when independent of those kept.
    let mut kept: Vec<(f64, Vec<C>)> = Vec::with_capacity(q);
    for &c in &order {
        if kept.len() == q {
            break;
        }
        let mut w: Vec<C> = (0..q).map(|i| C::new(v[i * n2 + c], v[(i + q) * n2 + c])).collect();
        for (_, k) in &kept {
            let p = dotc(k, &w);
            w.iter_mut().zip(k).for_each(|(a, b)| *a -= p * b);
        }
        let norm = dotc(&w, &w).re.sqrt();
        if norm > 0.5 {
            w.iter_mut().for_each(|a| *a /= norm);
            kept.push((lam[c], w));
        }
    }
    if kept.len() < q {
        return Err(lost());
    }
    // Φ = L⁻ᴴ W; the Ritz vectors Y Φ.
    let n = y.first().map_or(0, Vec::len);
    Ok(kept
        .into_iter()
        .map(|(lambda, w)| {
            let phi = l.upper_solve(&w);
            let x: Vec<C> = (0..n).map(|r| (0..q).map(|k| y[k][r] * phi[k]).sum()).collect();
            (lambda, x)
        })
        .collect())
}

/// The `bands` eigenvalues k² nearest σ above the null space, at phases `psi`.
fn bands_at(d: &Driven, tie: &Tie, an: &Analysis, psi: [f64; 3], bands: usize, sigma: f64) -> Result<(Vec<f64>, usize)> {
    let (pattern, stiffness, mass) = d.matrices();
    let kval: Vec<C> = stiffness.iter().map(|s| C::new(s.re, 0.0)).collect();
    let k = tie.reduce(&kval, psi);
    let m = tie.reduce(&mass.iter().map(|m| C::new(m.re, 0.0)).collect::<Vec<_>>(), psi);
    let _ = pattern;
    let mut s = k.clone();
    for (x, mv) in s.val.iter_mut().zip(&m.val) {
        *x -= mv * sigma;
    }
    let f = Lu::factor_analysed(&s, an, crate::ldlt::memory_budget())?;
    let n = k.n;
    // The Bloch-periodic gradients: of master node n's function, 1 at n and e^{−jψ·s} at its
    // images s periods away, on the master edges; a node whose group touches an eliminated or
    // dropped edge is left out (a slave edge repeats its master's coefficients).
    let (columns, points) = gradients::<C>(
        d,
        |n| tie.node_master(n).map(|(m, s)| (m as usize, Tie::phase(psi, s))),
        |slot| d.dofs.unknown_of(slot).and_then(|u| tie.master_of(u)),
        |slot| d.dofs.unknown_of(slot).and_then(|u| tie.coefficient(u, psi)).is_none(),
    );
    let deflation = Deflation::new(columns, &points, &m, |k| m.val[k])?;
    let q = (bands + bands.max(8)).min(n);
    let mut x = start_vectors::<C>(q, n, 0x9e37_79b9_7f4a_7c15);
    let mx: Vec<Vec<C>> = x.iter().map(|v| m.mul(v)).collect();
    deflation.project(&mut x, &mx)?;
    let null = 1e-6 * sigma;
    let mut previous: Vec<f64> = Vec::new();
    let mut lambdas: Vec<f64> = Vec::new();
    let mut iterations = 0;
    for it in 0..MAX_ITERATIONS {
        iterations = it + 1;
        let mut y: Vec<Vec<C>> = Vec::with_capacity(q);
        for v in &x {
            let mut r = m.mul(v);
            f.solve(&mut r)?;
            y.push(r);
        }
        let my_raw: Vec<Vec<C>> = y.iter().map(|v| m.mul(v)).collect();
        deflation.project(&mut y, &my_raw)?;
        let ky: Vec<Vec<C>> = y.iter().map(|v| k.mul(v)).collect();
        let my: Vec<Vec<C>> = y.iter().map(|v| m.mul(v)).collect();
        let mut pairs = ritz(&y, &ky, &my)?;
        pairs.sort_by(|a, b| (a.0 - sigma).abs().total_cmp(&(b.0 - sigma).abs()));
        (lambdas, x) = pairs.into_iter().unzip();
        let wanted = lowest(&lambdas, null, bands);
        let converged =
            wanted.len() == bands && previous.len() == bands && wanted.iter().zip(&previous).all(|(a, b)| (a - b).abs() <= 1e-10 * a.abs());
        previous = wanted;
        if converged {
            break;
        }
    }
    Ok((lowest(&lambdas, null, bands), iterations))
}

/// The lowest `count` eigenvalues above the null threshold, ascending.
fn lowest(lambdas: &[f64], null: f64, count: usize) -> Vec<f64> {
    let mut v: Vec<f64> = lambdas.iter().copied().filter(|l| *l > null).collect();
    v.sort_by(f64::total_cmp);
    v.truncate(count);
    v
}

/// The dispersion diagram of the model's periodic cell (its walls perfectly conducting).
pub fn dispersion(model: &Model, order: Order, sweep: &Sweep) -> Result<Dispersion> {
    dispersion_each(model, order, sweep, |_, _| Ok(()))
}

/// [`dispersion`], calling `each(k, points)` before phase k (its error stops the sweep: progress
/// and cancellation).
pub fn dispersion_each<E>(
    model: &Model,
    order: Order,
    sweep: &Sweep,
    mut each: impl FnMut(usize, usize) -> std::result::Result<(), E>,
) -> std::result::Result<Dispersion, E>
where
    E: From<SolveError>,
{
    if sweep.bands == 0 || sweep.bands > MAX_BANDS {
        return Err(invalid("bands", format!("must be 1 to {MAX_BANDS}")).into());
    }
    if sweep.points == 0 || sweep.points > MAX_PHASES {
        return Err(invalid("phases", format!("must be 1 to {MAX_PHASES}")).into());
    }
    if !(sweep.from.is_finite() && sweep.to.is_finite() && sweep.from.abs() <= 2.0 * PI && sweep.to.abs() <= 2.0 * PI) {
        return Err(invalid("phase", "must be finite, within ±360°").into());
    }
    check_frequency(sweep.near)?;
    let periodic = model.periodic.ok_or_else(|| SolveError::NotReady("no periodic boundaries: set them with model.periodic".into()))?;
    if !periodic.axes().contains(&sweep.axis) {
        return Err(invalid("axis", "the axis swept must be periodic").into());
    }
    let d = Driven::cavity(model, order)?;
    if d.is_dispersive() {
        return Err(invalid("dispersion", "not supported yet: dispersive materials (the eigenproblem is nonlinear in the frequency)").into());
    }
    let (_, stiffness, mass) = d.matrices();
    if mass.iter().any(|m| m.im != 0.0) {
        return Err(invalid("dispersion", "not supported yet: lossy dielectrics in a dispersion diagram (complex k²)").into());
    }
    if stiffness.iter().any(|k| k.im != 0.0) {
        return Err(invalid("dispersion", "not supported yet: magnetically lossy materials in a dispersion diagram (complex k²)").into());
    }
    let tie = d.tie.as_ref().ok_or_else(|| invalid("dispersion", "the cell has no periodic reduction"))?;
    if tie.reduced == 0 {
        return Err(SolveError::NotReady("the cell has no unknowns".into()).into());
    }
    let an = Analysis::new(tie.pattern(), &tie.points, 0).map_err(SolveError::from)?;
    let k_near = wavenumber(sweep.near);
    let sigma = k_near * k_near;
    let period = tie.periods[sweep.axis];
    let mut points = Vec::with_capacity(sweep.points);
    for i in 0..sweep.points {
        each(i, sweep.points)?;
        let t = if sweep.points == 1 { 0.0 } else { i as f64 / (sweep.points - 1) as f64 };
        let psi_axis = sweep.from + (sweep.to - sweep.from) * t;
        let mut psi = [0.0; 3];
        psi[sweep.axis] = psi_axis;
        let (k2, iterations) = bands_at(&d, tie, &an, psi, sweep.bands, sigma)?;
        let frequencies = k2.iter().map(|l| l.sqrt() * C0 / (2.0 * PI)).collect();
        points.push(BlochPoint { psi: psi_axis, beta: psi_axis / period, frequencies, iterations });
    }
    Ok(Dispersion { axis: sweep.axis, period, points, unknowns: tie.reduced })
}

#[cfg(test)]
mod tests {
    use super::*;
    use emcraft_model::{Periodic, Polarization, Section, Waveguide};

    fn guide(sections: Vec<Section>) -> Model {
        let mut m = Model::new();
        m.add_material(emcraft_model::Material::new("fill", 2.56, 1.0).unwrap()).unwrap();
        m.set_waveguide(Waveguide { width: 0.02286, height: 0.01016, sections, layers: Vec::new(), strips: Vec::new(), vias: Vec::new() }).unwrap();
        m.set_periodic(Some(Periodic { x: false, y: false, z: true, theta: 0.0, phi: 0.0, polarization: Polarization::Te })).unwrap();
        m.mesh = Some(emcraft_mesh::waveguide(&m, 0.003).unwrap().into());
        m
    }

    #[test]
    fn an_empty_guide_periodic_along_z_has_te10s_dispersion() {
        let m = guide(vec![Section::new(0.01, "vacuum")]);
        let sweep = Sweep { axis: 2, from: 0.0, to: PI, points: 3, bands: 2, near: 15e9 };
        let d = dispersion(&m, Order::Two, &sweep).unwrap();
        for p in &d.points {
            let exact = emcraft_theory::floquet::empty_guide_bands(
                &emcraft_theory::floquet::EmptyGuide { a: 0.02286, b: 0.01016, period: 0.01, eps_r: 1.0 },
                p.psi,
                2,
            );
            for (f, e) in p.frequencies.iter().zip(&exact) {
                assert!((f - e).abs() < 2e-3 * e, "ψ = {}: {f} vs {e} ({:?})", p.psi, p.frequencies);
            }
        }
    }
}
