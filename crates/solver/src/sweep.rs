//! A fast frequency sweep by reduced-basis (multi-point Galerkin) projection: a few
//! frequencies are solved in full, their fields span a basis V, and every other frequency is
//! solved in that basis — a system as small as the basis — with the residual of the full
//! equations as its error estimate; while some frequency's residual exceeds the tolerance, the
//! worst one is solved in full and its field joins the basis (the greedy reduced-basis method:
//! Y. Maday, A. T. Patera and others; for driven electromagnetics, M. W. Fernández, D. Pardo et
//! al., and the "adaptive frequency sweep" of commercial solvers; P. Benner, M. Hinze, E. J. W.
//! ter Maten (eds.), *Model Reduction for Circuit Simulation*, Springer 2011, ch. 1).
//!
//! **Projection.** V holds the real and imaginary parts of the solved fields, orthonormalised,
//! so it is real and Vᵀ A V keeps A's complex symmetry. The parts of the system that do not
//! depend on the frequency — the curl–curl and mass matrices, the walls' and lumped ports'
//! matrices up to a scalar — are projected once per basis; the layer (its stretches depend on
//! k₀) and the ports' modes (they can) are formed at each frequency. Port modes enter through
//! their rank-one terms, factor · (Vᵀv)(vᵀV), not the bordered matrix of the full solve.
//!
//! **Dispersive materials.** Their permittivities εr,d(ω) are not affine in ω², but the system
//! stays a sum of fixed matrices with scalar coefficients, K − k₀² (M + Σ_d εr,d(ω) M_d): each
//! M_d V is projected once per basis and scaled by εr,d(ω) at each frequency, so the projection
//! is as exact as without dispersion, and the residual of the full equations still decides which
//! frequencies are solved in full.

use num_complex::Complex64 as C;

use crate::driven::{Driven, FrequencyResult, PortTerm, Result, Timings, invalid, wall_factor, wavenumber};
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
}

impl Default for SweepOptions {
    fn default() -> Self {
        SweepOptions { tolerance: 1e-4, max_solves: 64 }
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
}

fn dot_rr(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn dot_rc(a: &[f64], b: &[C]) -> C {
    a.iter().zip(b).map(|(x, y)| y * *x).sum()
}

/// A u for a real vector u, A's values `val` on the pattern `p`.
fn product<T>(p: &Csr, val: &[T], u: &[f64]) -> Vec<T>
where
    T: Copy + std::ops::Mul<f64, Output = T> + std::iter::Sum,
{
    (0..p.n).map(|i| p.row(i).map(|k| val[k] * u[p.col[k]]).sum()).collect()
}

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
            let mut u = part;
            for _ in 0..2 {
                for b in &self.v {
                    let c = dot_rr(&u, b);
                    u.iter_mut().zip(b).for_each(|(p, q)| *p -= c * q);
                }
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
        }
    }
}

/// Solves `d` at `frequencies` (Hz) by the reduced-basis sweep: the lowest and highest in full,
/// then the worst-resolved one in full until every residual is within `o.tolerance`.
pub fn fast_sweep(d: &Driven, frequencies: &[f64], o: &SweepOptions) -> Result<FastSweep> {
    if frequencies.iter().any(|f| !(f.is_finite() && *f > 0.0)) {
        return Err(invalid("frequency", "must be finite numbers > 0"));
    }
    if !(o.tolerance.is_finite() && o.tolerance > 0.0) || o.max_solves < 2 {
        return Err(invalid("sweep options", "the tolerance must be > 0 and at least two frequencies solved in full"));
    }
    let nf = frequencies.len();
    // A periodic cell's phases change with the frequency (and its system is not symmetric):
    // not supported yet in the reduced basis, so each frequency is solved in full.
    if d.tie.is_some() {
        let results = frequencies.iter().map(|f| d.solve(*f)).collect::<Result<Vec<_>>>()?;
        return Ok(FastSweep { results, full: vec![true; nf], basis: 0, worst_residual: 0.0 });
    }
    let mut results: Vec<Option<FrequencyResult>> = vec![None; nf];
    let mut full = vec![false; nf];
    let mut basis = Basis { v: Vec::new(), kv: Vec::new(), mv: Vec::new(), wv: Vec::new(), dv: Vec::new() };
    let solve_full = |i: usize, basis: &mut Basis, results: &mut Vec<Option<FrequencyResult>>, full: &mut Vec<bool>| -> Result<()> {
        let r = d.solve(frequencies[i])?;
        for x in &r.fields {
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
    solve_full(lo, &mut basis, &mut results, &mut full)?;
    if hi != lo {
        solve_full(hi, &mut basis, &mut results, &mut full)?;
    }
    // The ports' terms at each frequency do not depend on the basis: made once.
    let mut terms: Vec<Option<Vec<Vec<PortTerm<'_>>>>> = (0..nf).map(|_| None).collect();
    let worst_residual = loop {
        // Every frequency not solved in full, in the basis.
        let mut worst: Option<(usize, f64)> = None;
        for i in (0..nf).filter(|&i| !full[i]) {
            if terms[i].is_none() {
                terms[i] = Some(d.checked_terms(frequencies[i])?);
            }
            let t = terms[i].as_ref().ok_or_else(|| invalid("sweep", "a frequency's port terms are missing"))?;
            let (r, residual) = reduced(d, &basis, frequencies[i], t)?;
            if worst.is_none_or(|(_, w)| residual > w) {
                worst = Some((i, residual));
            }
            results[i] = Some(r);
        }
        match worst {
            Some((i, w)) if w > o.tolerance && full.iter().filter(|f| **f).count() < o.max_solves => {
                solve_full(i, &mut basis, &mut results, &mut full)?
            }
            _ => break worst.map_or(0.0, |(_, w)| w),
        }
    };
    let results = results.into_iter().map(|r| r.ok_or_else(|| invalid("sweep", "a frequency was left unsolved"))).collect::<Result<Vec<_>>>()?;
    Ok(FastSweep { results, full, basis: basis.v.len(), worst_residual })
}

/// The system at one frequency in the basis V: A V by columns (all but the rank-one terms),
/// the rank-one terms, and Vᵀ A V with them.
struct Reduced {
    /// The unknowns of the full equations.
    n: usize,
    av: Vec<Vec<C>>,
    rank_one: Vec<RankOne>,
    ar: Vec<Vec<C>>,
}

impl Reduced {
    fn new(d: &Driven, basis: &Basis, frequency: f64, terms: &[Vec<PortTerm<'_>>]) -> Result<Reduced> {
        let k0 = wavenumber(frequency);
        let (n, m) = (d.dofs.unknowns, basis.v.len());
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
        // A V, column by column: K v − k₀² (M v + Σ εr,d M_d v) + c W v + P v (the rank-one terms
        // come after).
        let av: Vec<Vec<C>> = (0..m)
            .map(|j| {
                (0..n)
                    .map(|i| {
                        let mut x = basis.kv[j][i] - basis.mv[j][i] * (k0 * k0);
                        for (c, dv) in dispersive.iter().zip(&basis.dv[j]) {
                            x -= c * dv[i];
                        }
                        if let Some(c) = wall {
                            x += c * basis.wv[j][i];
                        }
                        if let Some(p) = &pv {
                            x += p[j][i];
                        }
                        x
                    })
                    .collect()
            })
            .collect();
        // Rank-one terms: lumped ports' (jωμ₀/R) c cᵀ and the port modes' factor v vᵀ.
        let mut rank_one: Vec<RankOne> =
            d.lumped.iter().map(|l| RankOne { factor: l.factor(frequency), v: l.c.iter().map(|&(i, x)| (i, C::new(x, 0.0))).collect() }).collect();
        rank_one.extend(terms.iter().flatten().map(|t: &PortTerm<'_>| RankOne { factor: t.factor, v: t.v.to_vec() }));
        let project = |v: &[(usize, C)]| -> Vec<C> { basis.v.iter().map(|b| v.iter().map(|&(i, x)| x * b[i]).sum()).collect() };
        let projected: Vec<(C, Vec<C>)> = rank_one.iter().map(|r| (r.factor, project(&r.v))).collect();
        let mut ar: Vec<Vec<C>> = (0..m).map(|i| (0..m).map(|j| dot_rc(&basis.v[i], &av[j])).collect()).collect();
        for (f, u) in &projected {
            for i in 0..m {
                for j in 0..m {
                    ar[i][j] += f * (u[i] * u[j]);
                }
            }
        }
        Ok(Reduced { n, av, rank_one, ar })
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
        // r = b − A x.
        let mut r: Vec<C> = (0..n).map(|i| -(0..m).map(|j| z[j] * self.av[j][i]).sum::<C>()).collect();
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
        },
        worst,
    ))
}
