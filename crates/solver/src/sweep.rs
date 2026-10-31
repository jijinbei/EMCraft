//! A fast frequency sweep by reduced-basis (multi-point Galerkin) projection: a few
//! frequencies are solved in full, their fields span a basis V, and every other frequency is
//! solved in that basis — a system as small as the basis — with the residual of the full
//! equations as its error estimate; while some frequency's residual exceeds the tolerance, the
//! worst one is solved in full and its field joins the basis (the greedy reduced-basis method:
//! Y. Maday, A. T. Patera and others; for driven electromagnetics, M. W. Fernández, D. Pardo et
//! al., and the "adaptive frequency sweep" of commercial solvers; P. Benner, M. Hinze, E. J. W.
//! ter Maten (eds.), *Model Reduction for Circuit Simulation*, Springer 2011, ch. 1).
//!
//! **Moments.** Each frequency solved in full adds, beside its fields x, the next few vectors of
//! their Krylov sequences A⁻¹ M x, A⁻¹ M A⁻¹ M x, … — the fields' derivatives in k₀² with the
//! ports' terms held — from the same factorization, a triangular solve each (multi-point moment
//! matching: R. D. Slone, R. Lee, J.-F. Lee, IEEE Trans. Antennas Propag. 49(10):1504–1513,
//! 2001). They make each factorization span a wider band: on the junction templates' 101 points,
//! 2 to 3 frequencies solved in full rather than 6 to 9, S within 3e-9 of the sweep without them.
//!
//! **Projection.** V holds the real and imaginary parts of the solved fields, orthonormalised,
//! so it is real and Vᵀ A V keeps A's complex symmetry. The parts of the system that do not
//! depend on the frequency — the curl–curl and mass matrices, the walls' and lumped ports'
//! matrices up to a scalar — are projected once, a row and column as each vector joins the
//! basis, so a frequency's reduced matrix costs O(m²) and its field and residual O(m n); the
//! layer (its stretches depend on k₀) and the ports' modes (they can) are formed at each
//! frequency. Port modes enter through their rank-one terms, factor · (Vᵀv)(vᵀV), not the
//! bordered matrix of the full solve. The frequencies in the basis are solved in parallel, each
//! on its own, and the basis' products in parallel, each entry summed in one fixed order, so
//! the sweep is the same whatever the threads. The fields of the frequencies in the basis are
//! dropped before each new factorization (the next pass solves them again), so they never sit
//! in memory beside a factor.
//!
//! **Dispersive materials.** Their permittivities εr,d(ω) are not affine in ω², but the system
//! stays a sum of fixed matrices with scalar coefficients, K − k₀² (M + Σ_d εr,d(ω) M_d): each
//! M_d V is projected once per basis and scaled by εr,d(ω) at each frequency, so the projection
//! is as exact as without dispersion, and the residual of the full equations still decides which
//! frequencies are solved in full.

use num_complex::Complex64 as C;
use rayon::prelude::*;

use crate::driven::{Driven, FrequencyResult, PortTerm, Result, Timings, check_frequency, invalid, wall_factor, wavenumber};
use crate::sparse::Csr;
use crate::zdense;

/// How a fast sweep decides it is done.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SweepOptions {
    /// The largest relative residual ‖b − A x‖/‖b‖ accepted at a frequency solved in the basis.
    /// S's error goes about as its square (Galerkin's outputs converge twice as fast): 1e-4
    /// leaves it near 1e-10, far below the discretisation's.
    pub tolerance: f64,
    /// The most frequencies solved in full.
    pub max_solves: usize,
    /// The derivative vectors each full solve adds to the basis per excitation, beside its
    /// field ([`Driven::solve_spanning`]): each a solve with the frequency's factorization, so
    /// fewer frequencies need one of their own.
    pub moments: usize,
}

impl Default for SweepOptions {
    fn default() -> Self {
        SweepOptions { tolerance: 1e-4, max_solves: 64, moments: 3 }
    }
}

/// A fast sweep's results and what it took.
#[derive(Clone, Debug)]
pub struct FastSweep {
    /// One result per frequency asked for, in that order.
    pub results: Vec<FrequencyResult>,
    /// Which of them were solved in full.
    pub full: Vec<bool>,
    /// The basis' size at the end.
    pub basis: usize,
    /// The largest relative residual of the frequencies solved in the basis.
    pub worst_residual: f64,
}

/// The basis and its projections.
struct Basis {
    v: Vec<Vec<f64>>,
    /// K v and M v (and W v for walls of finite conductivity) per basis vector.
    kv: Vec<Vec<C>>,
    mv: Vec<Vec<C>>,
    wv: Vec<Vec<f64>>,
    /// M_d v per dispersive material d, per basis vector.
    dv: Vec<Vec<Vec<f64>>>,
    /// The frequency's fixed parts projected: Vᵀ K V, Vᵀ M V, Vᵀ W V and Vᵀ M_d V (by row),
    /// grown with the basis so each frequency's reduced matrix costs O(m²), not O(m² n).
    kk: Vec<Vec<C>>,
    mm: Vec<Vec<C>>,
    ww: Vec<Vec<f64>>,
    dd: Vec<Vec<Vec<f64>>>,
}

fn dot_rr(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn dot_rc(a: &[f64], b: &[C]) -> C {
    a.iter().zip(b).map(|(x, y)| y * *x).sum()
}

/// A u for a real vector u, A's values `val` on the pattern `p`: the rows in parallel, each
/// summed in its own order.
fn product<T>(p: &Csr, val: &[T], u: &[f64]) -> Vec<T>
where
    T: Copy + Send + Sync + std::ops::Mul<f64, Output = T> + std::iter::Sum,
{
    (0..p.n).into_par_iter().map(|i| p.row(i).map(|k| val[k] * u[p.col[k]]).sum()).collect()
}

/// Entries per task of the basis' vector updates.
const CHUNK: usize = 4096;

/// A rank-one term factor · v vᵀ of the system, v sparse.
struct RankOne {
    factor: C,
    v: Vec<(usize, C)>,
}

impl Basis {
    /// Adds the real and imaginary parts of `x`, orthogonalised against the basis (twice), unless
    /// nothing new is left of them.
    fn add(&mut self, d: &Driven, x: &[C]) {
        for part in [x.iter().map(|c| c.re).collect::<Vec<f64>>(), x.iter().map(|c| c.im).collect()] {
            let start = dot_rr(&part, &part).sqrt();
            if !(start > 0.0 && start.is_finite()) {
                continue;
            }
            // Classical Gram–Schmidt, twice (as stable as the modified one, and each pass's
            // products independent: in parallel, each summed in its own order).
            let mut u = part;
            for _ in 0..2 {
                let c: Vec<f64> = self.v.par_iter().map(|b| dot_rr(&u, b)).collect();
                let v = &self.v;
                u.par_chunks_mut(CHUNK).enumerate().for_each(|(chunk, part)| {
                    for (k, p) in part.iter_mut().enumerate() {
                        let i = chunk * CHUNK + k;
                        *p -= c.iter().zip(v).map(|(c, b)| c * b[i]).sum::<f64>();
                    }
                });
            }
            let len = dot_rr(&u, &u).sqrt();
            if !(len > 1e-10 * start) {
                continue;
            }
            u.iter_mut().for_each(|p| *p /= len);
            let p = &d.pattern;
            self.kv.push(product(p, &d.stiffness, &u));
            self.mv.push(product(p, &d.mass, &u));
            if let Some(w) = &d.wall {
                self.wv.push(product(p, &w.mass, &u));
            }
            self.dv.push(d.dispersive_mass.iter().map(|md| product(p, md, &u)).collect());
            self.v.push(u);
            self.project_last();
        }
    }

    fn empty(dispersive: usize) -> Basis {
        Basis {
            v: Vec::new(),
            kv: Vec::new(),
            mv: Vec::new(),
            wv: Vec::new(),
            dv: Vec::new(),
            kk: Vec::new(),
            mm: Vec::new(),
            ww: Vec::new(),
            dd: vec![Vec::new(); dispersive],
        }
    }

    /// The projections' new row and column for the last basis vector: vᵢᵀ A v_j for each fixed
    /// matrix A, symmetric as A is.
    fn project_last(&mut self) {
        let Some(j) = self.v.len().checked_sub(1) else {
            return;
        };
        let grow = |g: &mut Vec<Vec<C>>, av: &[C], v: &[Vec<f64>]| {
            let col: Vec<C> = v.par_iter().map(|vi| dot_rc(vi, av)).collect();
            for (row, c) in g.iter_mut().zip(&col) {
                row.push(*c);
            }
            g.push(col);
        };
        let grow_real = |g: &mut Vec<Vec<f64>>, av: &[f64], v: &[Vec<f64>]| {
            let col: Vec<f64> = v.par_iter().map(|vi| dot_rr(vi, av)).collect();
            for (row, c) in g.iter_mut().zip(&col) {
                row.push(*c);
            }
            g.push(col);
        };
        grow(&mut self.kk, &self.kv[j], &self.v);
        grow(&mut self.mm, &self.mv[j], &self.v);
        if let Some(wv) = self.wv.get(j) {
            grow_real(&mut self.ww, wv, &self.v);
        }
        for (dd, dv) in self.dd.iter_mut().zip(&self.dv[j]) {
            grow_real(dd, dv, &self.v);
        }
    }
}

/// How far a fast sweep is, as [`fast_sweep_each`] reports it. Its full solves are not known in
/// advance (they are added until every residual is small): it says how many are done, the most
/// it may make, how far the pass in the basis is, and the worst residual of the last pass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SweepStep {
    /// Frequencies solved in full so far.
    pub full: usize,
    /// The most that may be solved in full ([`SweepOptions::max_solves`], at most the frequencies).
    pub max_full: usize,
    /// Frequencies of the current pass solved in the basis so far, of `pass_total` (0 between
    /// passes).
    pub pass_done: usize,
    pub pass_total: usize,
    /// The worst relative residual of the last finished pass (`None` before the first).
    pub residual: Option<f64>,
    /// The residual every frequency must reach.
    pub tolerance: f64,
}

/// Solves `d` at `frequencies` (Hz) by the reduced-basis sweep: the lowest and highest in full,
/// then the worst-resolved one in full until every residual is within `o.tolerance`.
pub fn fast_sweep(d: &Driven, frequencies: &[f64], o: &SweepOptions) -> Result<FastSweep> {
    fast_sweep_each(d, frequencies, o, |_| Ok(()))
}

/// [`fast_sweep`], calling `each` before every full solve and every frequency solved in the
/// basis with how far the sweep is; an error from `each` (a cancel) stops it there.
pub fn fast_sweep_each<E>(
    d: &Driven,
    frequencies: &[f64],
    o: &SweepOptions,
    mut each: impl FnMut(SweepStep) -> std::result::Result<(), E>,
) -> std::result::Result<FastSweep, E>
where
    E: From<crate::driven::SolveError>,
{
    for &f in frequencies {
        check_frequency(f)?;
    }
    if !(o.tolerance.is_finite() && o.tolerance > 0.0) || o.max_solves < 2 {
        return Err(invalid("sweep options", "the tolerance must be > 0 and at least two frequencies solved in full").into());
    }
    let nf = frequencies.len();
    let max_full = o.max_solves.min(nf);
    let step = |full: usize, pass_done: usize, pass_total: usize, residual: Option<f64>| SweepStep {
        full,
        max_full,
        pass_done,
        pass_total,
        residual,
        tolerance: o.tolerance,
    };
    // A periodic cell's phases change with the frequency (and its system is not symmetric):
    // not supported yet in the reduced basis, so each frequency is solved in full.
    if d.tie.is_some() {
        let mut results = Vec::with_capacity(nf);
        for (k, f) in frequencies.iter().enumerate() {
            each(SweepStep { max_full: nf, ..step(k, 0, 0, None) })?;
            results.push(d.solve(*f)?);
        }
        return Ok(FastSweep { results, full: vec![true; nf], basis: 0, worst_residual: 0.0 });
    }
    let mut results: Vec<Option<FrequencyResult>> = vec![None; nf];
    let mut full = vec![false; nf];
    let mut solved = 0;
    let mut last: Option<f64> = None;
    let mut basis = Basis::empty(d.dispersive_mass.len());
    let solve_full = |i: usize, basis: &mut Basis, results: &mut Vec<Option<FrequencyResult>>, full: &mut Vec<bool>| -> Result<()> {
        let (r, extra) = d.solve_spanning(frequencies[i], o.moments)?;
        for x in r.fields.iter().chain(&extra) {
            basis.add(d, x);
        }
        results[i] = Some(r);
        full[i] = true;
        Ok(())
    };
    // The ends of the band first (each excitation's field adds two vectors).
    let by = |a: &usize, b: &usize| frequencies[*a].total_cmp(&frequencies[*b]);
    let (Some(lo), Some(hi)) = ((0..nf).min_by(by), (0..nf).max_by(by)) else {
        return Ok(FastSweep { results: Vec::new(), full, basis: 0, worst_residual: 0.0 });
    };
    for i in if hi != lo { vec![lo, hi] } else { vec![lo] } {
        each(step(solved, 0, 0, None))?;
        solve_full(i, &mut basis, &mut results, &mut full)?;
        solved += 1;
    }
    // The ports' terms at each frequency do not depend on the basis: made once.
    let mut terms: Vec<Option<Vec<Vec<PortTerm<'_>>>>> = (0..nf).map(|_| None).collect();
    let worst_residual = loop {
        // Every frequency not solved in full, in the basis: each on its own, in parallel in
        // chunks (`each` hears between them), the worst then found in the frequencies' order, so
        // the result is the serial one's.
        let pending: Vec<usize> = (0..nf).filter(|&i| !full[i]).collect();
        for &i in &pending {
            if terms[i].is_none() {
                terms[i] = Some(d.checked_terms(frequencies[i])?);
            }
        }
        let mut worst: Option<(usize, f64)> = None;
        let chunk = 4 * rayon::current_num_threads().max(1);
        for (c, part) in pending.chunks(chunk).enumerate() {
            each(step(solved, c * chunk, pending.len(), last))?;
            let pass: Vec<Result<(FrequencyResult, f64)>> = part
                .par_iter()
                .map(|&i| {
                    let t = terms[i].as_ref().ok_or_else(|| invalid("sweep", "a frequency's port terms are missing"))?;
                    reduced(d, &basis, frequencies[i], t)
                })
                .collect();
            for (&i, r) in part.iter().zip(pass) {
                let (r, residual) = r?;
                if worst.is_none_or(|(_, w)| residual > w) {
                    worst = Some((i, residual));
                }
                results[i] = Some(r);
            }
        }
        last = worst.map(|(_, w)| w);
        match worst {
            Some((i, w)) if w > o.tolerance && solved < o.max_solves => {
                each(step(solved, 0, 0, last))?;
                // The next pass solves them again in the larger basis: their fields are not
                // kept beside the factorization.
                for (r, _) in results.iter_mut().zip(&full).filter(|(_, f)| !**f) {
                    *r = None;
                }
                solve_full(i, &mut basis, &mut results, &mut full)?;
                solved += 1;
            }
            _ => break worst.map_or(0.0, |(_, w)| w),
        }
    };
    let results = results.into_iter().map(|r| r.ok_or_else(|| invalid("sweep", "a frequency was left unsolved"))).collect::<Result<Vec<_>>>()?;
    Ok(FastSweep { results, full, basis: basis.v.len(), worst_residual })
}

/// The system at one frequency in the basis V: the coefficients of its fixed parts, the layer's
/// P V (when there is a layer), the rank-one terms, and Vᵀ A V with them.
struct Reduced {
    /// The unknowns of the full equations.
    n: usize,
    /// k₀², each dispersive material's k₀² εr,d(ω), the walls' factor.
    k2: f64,
    dispersive: Vec<C>,
    wall: Option<C>,
    /// P v per basis vector, the layer's matrix at this k₀.
    pv: Option<Vec<Vec<C>>>,
    rank_one: Vec<RankOne>,
    ar: Vec<Vec<C>>,
}

impl Reduced {
    fn new(d: &Driven, basis: &Basis, frequency: f64, terms: &[Vec<PortTerm<'_>>]) -> Result<Reduced> {
        let k0 = wavenumber(frequency);
        let (n, m) = (d.dofs.unknowns, basis.v.len());
        let k2 = k0 * k0;
        let wall = d.wall.as_ref().map(|w| wall_factor(frequency, w.sigma, w.thickness));
        // k₀² εr,d(ω) per dispersive material.
        let dispersive = d.dispersive_scales(frequency);
        // The layer at this k₀: its own matrix on the pattern, times the basis.
        let pv: Option<Vec<Vec<C>>> = if d.pml.is_some() {
            let mut p = d.pattern.zeroed();
            d.add_pml(&mut p, k0)?;
            Some(basis.v.iter().map(|v| p.mul(&v.iter().map(|x| C::new(*x, 0.0)).collect::<Vec<_>>())).collect())
        } else {
            None
        };
        // Rank-one terms: lumped ports' (jωμ₀/R) c cᵀ and the port modes' factor v vᵀ.
        let mut rank_one: Vec<RankOne> =
            d.lumped.iter().map(|l| RankOne { factor: l.factor(frequency), v: l.c.iter().map(|&(i, x)| (i, C::new(x, 0.0))).collect() }).collect();
        rank_one.extend(terms.iter().flatten().map(|t: &PortTerm<'_>| RankOne { factor: t.factor, v: t.v.to_vec() }));
        let project = |v: &[(usize, C)]| -> Vec<C> { basis.v.iter().map(|b| v.iter().map(|&(i, x)| x * b[i]).sum()).collect() };
        let projected: Vec<(C, Vec<C>)> = rank_one.iter().map(|r| (r.factor, project(&r.v))).collect();
        // Vᵀ A V from the projections the basis keeps: Vᵀ K V − k₀² (Vᵀ M V + Σ εr,d Vᵀ M_d V)
        // + c Vᵀ W V, then the layer's and the rank-one terms'.
        let mut ar: Vec<Vec<C>> = (0..m)
            .map(|i| {
                (0..m)
                    .map(|j| {
                        let mut x = basis.kk[i][j] - basis.mm[i][j] * k2;
                        for (c, dd) in dispersive.iter().zip(&basis.dd) {
                            x -= c * dd[i][j];
                        }
                        if let Some(c) = wall {
                            x += c * basis.ww[i][j];
                        }
                        if let Some(p) = &pv {
                            x += dot_rc(&basis.v[i], &p[j]);
                        }
                        x
                    })
                    .collect()
            })
            .collect();
        for (f, u) in &projected {
            for i in 0..m {
                for j in 0..m {
                    ar[i][j] += f * (u[i] * u[j]);
                }
            }
        }
        Ok(Reduced { n, k2, dispersive, wall, pv, rank_one, ar })
    }

    /// The field of a unit incident wave of the mode `t`, solved in the basis, and its relative
    /// residual ‖b − A x‖/‖b‖ in the full equations.
    fn solve(&self, basis: &Basis, t: &PortTerm<'_>) -> Result<(Vec<C>, f64)> {
        let (n, m) = (self.n, basis.v.len());
        // b = rhs · v of the excited port's first mode.
        let b: Vec<(usize, C)> = t.excitation().collect();
        let br: Vec<C> = basis.v.iter().map(|v| b.iter().map(|&(i, x)| x * v[i]).sum()).collect();
        let z = zdense::solve(self.ar.clone(), br).ok_or_else(|| invalid("sweep", "a singular reduced system"))?;
        let x: Vec<C> = (0..n).map(|i| (0..m).map(|j| z[j] * basis.v[j][i]).sum()).collect();
        // A V z from the fixed parts' products, each basis vector's coefficients folded in once:
        // K V z − k₀² M V z − Σ k₀² εr,d M_d V z + c W V z + P V z.
        let mz: Vec<C> = z.iter().map(|zj| zj * self.k2).collect();
        let dz: Vec<Vec<C>> = self.dispersive.iter().map(|c| z.iter().map(|zj| zj * c).collect()).collect();
        let wz: Option<Vec<C>> = self.wall.map(|c| z.iter().map(|zj| zj * c).collect());
        // r = b − A x.
        let mut r: Vec<C> = (0..n)
            .map(|i| {
                let mut ax = C::new(0.0, 0.0);
                for j in 0..m {
                    ax += z[j] * basis.kv[j][i] - mz[j] * basis.mv[j][i];
                    for (dzd, dvj) in dz.iter().zip(&basis.dv[j]) {
                        ax -= dzd[j] * dvj[i];
                    }
                    if let Some(wz) = &wz {
                        ax += wz[j] * basis.wv[j][i];
                    }
                    if let Some(p) = &self.pv {
                        ax += z[j] * p[j][i];
                    }
                }
                -ax
            })
            .collect();
        for &(i, bi) in &b {
            r[i] += bi;
        }
        for RankOne { factor, v } in &self.rank_one {
            let vx: C = v.iter().map(|&(i, c)| x[i] * c).sum();
            for &(i, c) in v {
                r[i] -= factor * vx * c;
            }
        }
        let bn = b.iter().map(|(_, x)| x.norm_sqr()).sum::<f64>().sqrt();
        let rn = r.iter().map(|x| x.norm_sqr()).sum::<f64>().sqrt();
        Ok((x, if bn > 0.0 { rn / bn } else { rn }))
    }
}

/// One frequency solved in the basis: its result and the largest relative residual of its
/// excitations.
fn reduced(d: &Driven, basis: &Basis, frequency: f64, terms: &[Vec<PortTerm<'_>>]) -> Result<(FrequencyResult, f64)> {
    let t0 = std::time::Instant::now();
    let system = Reduced::new(d, basis, frequency, terms)?;
    let mut fields = Vec::with_capacity(terms.len());
    let mut worst: f64 = 0.0;
    for t in d.excitations().into_iter().filter_map(|(p, m)| terms.get(p)?.get(m)) {
        let (x, residual) = system.solve(basis, t)?;
        worst = worst.max(residual);
        fields.push(x);
    }
    // S as the full solve reads it.
    let (s, impedances) = d.s_parameters(terms, &mut fields, frequency)?;
    let seconds = t0.elapsed().as_secs_f64();
    Ok((
        FrequencyResult {
            frequency,
            s,
            impedances,
            fields,
            backward_error: worst,
            perturbed_pivots: 0,
            factor_entries: 0,
            largest_front: 0,
            timings: Timings { assembly: seconds, analysis: 0.0, factor: 0.0, solve: 0.0 },
            full: false,
        },
        worst,
    ))
}
