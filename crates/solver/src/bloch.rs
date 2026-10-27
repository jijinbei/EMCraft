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
use emcraft_model::units::C0;
use emcraft_model::{Model, Point3};
use num_complex::Complex64 as C;

use crate::driven::{Driven, Result, SolveError, Tie, check_frequency, invalid, wavenumber};
use crate::eigen::{galerkin, uniform};
use crate::ldlt::Analysis;
use crate::lu::Lu;
use crate::sparse::Csr;

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

/// The Bloch-periodic discrete gradients as columns over the kept unknowns (with a point
/// each): Whitney coefficients φ(b) − φ(a) on the master edges, and at order 2 each edge's
/// gradient function ∇(λ_iλ_j) alone. Nodes whose group touches an eliminated edge are left out.
fn gradients(d: &Driven, tie: &Tie, psi: [f64; 3]) -> (Vec<Vec<(usize, C)>>, Vec<Point3>) {
    let (topo, dofs, mesh) = (&d.topo, &d.dofs, &d.mesh);
    let pe = dofs.order.per_edge();
    let nn = mesh.nodes.len();
    // φ of master node n at node v: e^{−jψ·s} if v's master is n.
    let mut columns: Vec<std::collections::BTreeMap<usize, C>> = vec![Default::default(); nn];
    let mut blocked = vec![false; nn];
    for (e, &[a, b]) in topo.edges.iter().enumerate() {
        let (Some((ma, sa)), Some((mb, sb))) = (tie.node_master(a as usize), tie.node_master(b as usize)) else { continue };
        let (ma, mb) = (ma as usize, mb as usize);
        match dofs.unknown_of(e * pe).and_then(|u| tie.master_of(u)) {
            Some(r) => {
                if let Some(c) = columns.get_mut(ma) {
                    *c.entry(r).or_default() -= Tie::phase(psi, sa);
                }
                if let Some(c) = columns.get_mut(mb) {
                    *c.entry(r).or_default() += Tie::phase(psi, sb);
                }
            }
            None => {
                // A slave edge repeats its master's coefficients; an eliminated or dropped one
                // blocks its nodes' groups.
                let kept = dofs.unknown_of(e * pe).and_then(|u| tie.coefficient(u, psi)).is_some();
                if !kept {
                    for m in [ma, mb] {
                        if let Some(x) = blocked.get_mut(m) {
                            *x = true;
                        }
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    let mut points = Vec::new();
    for (n, col) in columns.into_iter().enumerate() {
        if !blocked[n] && !col.is_empty() && col.values().any(|v| v.norm() > 1e-12) {
            out.push(col.into_iter().filter(|(_, v)| v.norm() > 1e-14).collect());
            points.push(mesh.nodes[n]);
        }
    }
    if pe > 1 {
        for (e, &[a, b]) in topo.edges.iter().enumerate() {
            if let Some(r) = dofs.unknown_of(e * pe + 1).and_then(|u| tie.master_of(u)) {
                out.push(vec![(r, C::new(1.0, 0.0))]);
                points.push(mesh.nodes[a as usize].midpoint(mesh.nodes[b as usize]));
            }
        }
    }
    (out, points)
}

/// x ↦ x − Z (ZᴴMZ)⁻¹ ZᴴM x.
struct Deflation {
    columns: Vec<Vec<(usize, C)>>,
    factor: Option<(Lu, Csr)>,
}

impl Deflation {
    fn new(columns: Vec<Vec<(usize, C)>>, points: &[Point3], m: &Csr) -> Result<Deflation> {
        let nz = columns.len();
        if nz == 0 {
            return Ok(Deflation { columns, factor: None });
        }
        // (ZᴴMZ)_ab = Σ conj(z_a,i) M_ij z_b,j: row i of z_aᴴM is Σ_i z̄_i M_ij.
        let mut a = galerkin(&columns, m, |k, z: C| z.conj() * m.val[k], |wj, zb| wj * zb);
        // A symmetric pattern for the analysis: the transpose's entries too (zero).
        symmetrize_pattern(&mut a);
        let an = Analysis::new(&a, points, 0)?;
        let f = Lu::factor_analysed(&a, &an, crate::ldlt::memory_budget())?;
        Ok(Deflation { columns, factor: Some((f, a)) })
    }

    fn project(&self, xs: &mut [Vec<C>], m: &Csr) -> Result<()> {
        let Some((f, _)) = &self.factor else { return Ok(()) };
        for x in xs.iter_mut() {
            let mx = m.mul(x);
            let mut rhs: Vec<C> = self.columns.iter().map(|z| z.iter().map(|&(u, c)| c.conj() * mx[u]).sum()).collect();
            f.solve(&mut rhs)?;
            for (z, coef) in self.columns.iter().zip(&rhs) {
                for &(u, c) in z {
                    x[u] -= *coef * c;
                }
            }
        }
        Ok(())
    }
}

/// Adds the structural zeros that make `a`'s pattern symmetric.
fn symmetrize_pattern(a: &mut Csr) {
    let n = a.n;
    let mut rows: Vec<Vec<(usize, C)>> = (0..n).map(|i| (a.ptr[i]..a.ptr[i + 1]).map(|k| (a.col[k], a.val[k])).collect()).collect();
    let mut add: Vec<Vec<usize>> = vec![Vec::new(); n];
    for i in 0..n {
        for &(j, _) in &rows[i] {
            if j != i && j < n {
                add[j].push(i);
            }
        }
    }
    let zero = C::new(0.0, 0.0);
    for (i, row) in rows.iter_mut().enumerate() {
        row.extend(add[i].iter().map(|&j| (j, zero)));
        row.push((i, zero));
        row.sort_by_key(|e| e.0);
        // Merge duplicates, keeping the sum.
        let mut merged: Vec<(usize, C)> = Vec::with_capacity(row.len());
        for &(j, v) in row.iter() {
            match merged.last_mut() {
                Some(last) if last.0 == j => last.1 += v,
                _ => merged.push((j, v)),
            }
        }
        *row = merged;
    }
    a.ptr = vec![0];
    a.col.clear();
    a.val.clear();
    for row in rows {
        a.col.extend(row.iter().map(|e| e.0));
        a.val.extend(row.iter().map(|e| e.1));
        a.ptr.push(a.col.len());
    }
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
    let mut l = vec![C::new(0.0, 0.0); q * q];
    for j in 0..q {
        let d = mr[j * q + j].re - (0..j).map(|k| l[j * q + k].norm_sqr()).sum::<f64>();
        if !(d > 0.0) {
            return Err(lost());
        }
        l[j * q + j] = C::new(d.sqrt(), 0.0);
        for i in j + 1..q {
            let s: C = (0..j).map(|k| l[i * q + k] * l[j * q + k].conj()).sum();
            l[i * q + j] = (mr[i * q + j] - s) / l[j * q + j];
        }
    }
    let lsolve = |b: &[C]| -> Vec<C> {
        let mut z = vec![C::new(0.0, 0.0); q];
        for i in 0..q {
            let s: C = (0..i).map(|k| l[i * q + k] * z[k]).sum();
            z[i] = (b[i] - s) / l[i * q + i];
        }
        z
    };
    // C = L⁻¹ Kr L⁻ᴴ: Z = L⁻¹ Kr, then C = (L⁻¹ Zᴴ)ᴴ.
    let mut z = vec![C::new(0.0, 0.0); q * q];
    for j in 0..q {
        let col: Vec<C> = (0..q).map(|i| kr[i * q + j]).collect();
        for (i, v) in lsolve(&col).into_iter().enumerate() {
            z[i * q + j] = v;
        }
    }
    let mut cm = vec![C::new(0.0, 0.0); q * q];
    for i in 0..q {
        let row: Vec<C> = (0..q).map(|j| z[i * q + j].conj()).collect();
        for (j, v) in lsolve(&row).into_iter().enumerate() {
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
    let (lam, v) = crate::eigen::jacobi(big, n2);
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
            let mut phi = vec![C::new(0.0, 0.0); q];
            for i in (0..q).rev() {
                let s: C = (i + 1..q).map(|k| l[k * q + i].conj() * phi[k]).sum();
                phi[i] = (w[i] - s) / l[i * q + i].conj();
            }
            let x: Vec<C> = (0..n).map(|r| (0..q).map(|k| y[k][r] * phi[k]).sum()).collect();
            (lambda, x)
        })
        .collect())
}

/// Deterministic complex starting vectors.
fn start_vectors(q: usize, n: usize) -> Vec<Vec<C>> {
    let mut rand = uniform(0x9e37_79b9_7f4a_7c15);
    (0..q).map(|_| (0..n).map(|_| C::new(rand(), rand())).collect()).collect()
}

/// The `bands` eigenvalues k² nearest σ above the null space, at phases `psi`.
fn bands_at(d: &Driven, tie: &Tie, an: &Analysis, psi: [f64; 3], bands: usize, sigma: f64) -> Result<(Vec<f64>, usize)> {
    let (pattern, stiffness, mass) = d.matrices();
    let kval: Vec<C> = stiffness.iter().map(|s| C::new(*s, 0.0)).collect();
    let k = tie.reduce(&kval, psi);
    let m = tie.reduce(&mass.iter().map(|m| C::new(m.re, 0.0)).collect::<Vec<_>>(), psi);
    let _ = pattern;
    let mut s = k.clone();
    for (x, mv) in s.val.iter_mut().zip(&m.val) {
        *x -= mv * sigma;
    }
    let f = Lu::factor_analysed(&s, an, crate::ldlt::memory_budget())?;
    let n = k.n;
    let (columns, points) = gradients(d, tie, psi);
    let deflation = Deflation::new(columns, &points, &m)?;
    let q = (bands + bands.max(8)).min(n);
    let mut x = start_vectors(q, n);
    deflation.project(&mut x, &m)?;
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
        deflation.project(&mut y, &m)?;
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
    let (_, _, mass) = d.matrices();
    if mass.iter().any(|m| m.im != 0.0) {
        return Err(invalid("dispersion", "not supported yet: lossy dielectrics in a dispersion diagram (complex k²)").into());
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
        m.mesh = Some(emcraft_mesh::waveguide(&m, 0.003).unwrap());
        m
    }

    #[test]
    fn an_empty_guide_periodic_along_z_has_te10s_dispersion() {
        let m = guide(vec![Section::new(0.01, "vacuum")]);
        let sweep = Sweep { axis: 2, from: 0.0, to: PI, points: 3, bands: 2, near: 15e9 };
        let d = dispersion(&m, Order::Two, &sweep).unwrap();
        for p in &d.points {
            let exact = emcraft_theory::floquet::empty_guide_bands(0.02286, 0.01016, 0.01, 1.0, p.psi, 2);
            for (f, e) in p.frequencies.iter().zip(&exact) {
                assert!((f - e).abs() < 2e-3 * e, "ψ = {}: {f} vs {e} ({:?})", p.psi, p.frequencies);
            }
        }
    }
}
