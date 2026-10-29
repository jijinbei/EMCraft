//! `mesh.adapt`: adaptive mesh refinement, the loop of passes until the S-parameters (or an open
//! problem's far field) settle.
//!
//! Each pass meshes, solves at one frequency and compares S with the previous pass's: the
//! largest change of any entry, ΔS = max_pq |S_pq − S′_pq|. The loop stops when ΔS is below the
//! tolerance (converged), when the next mesh would pass the tetrahedra or unknowns cap (the last
//! mesh that fitted stays), or after the passes asked for.
//!
//! A solid (an unstructured mesh) is refined where the error is: the residual indicator of each
//! tetrahedron (`emcraft_solver::adapt`), summed over the ports' excitations, marks the fewest
//! tetrahedra that hold the fraction θ of its total (Dörfler), each becomes a ball of half its
//! size, and the solid is meshed again with every pass's balls (`emcraft_mesh::adapt`). A
//! structured waveguide grid has one size: it is refined uniformly, h ← h/1.4 a pass (half the
//! cells' volume), its grading towards edges kept.
//!
//! An open problem (`model.open`) is solved as `solve.radiation` solves it, and its indicator
//! (`emcraft_solver::adapt::open_indicator`) leaves out the perfectly matched layer — never
//! marked, so the layer keeps the size and thickness it was meshed with — and the point and
//! line sources' own tetrahedra, and takes a plane wave's incident field as the scattered
//! field's data. What converges: with lumped ports or a waveguide feed, ΔS over their
//! reflections; with sources alone (current elements, a plane wave), the far field — the
//! relative change δ = ‖F − F′‖ / ‖F′‖ of r e^{jkr}(E_θ, E_φ) over 48 fixed directions
//! ([`directions`]), which moves with the radiated or scattered power and with the pattern.

use std::time::Instant;

use emcraft_fem::Order;
use emcraft_model::{Mesh, Model};
use emcraft_solver::radiation::OpenResult;
use emcraft_solver::{Complex64 as C, Driven, FrequencyResult, SMatrix, SolveError};
use serde_json::Value;

use crate::{EngineError, Params, Result, Session};

/// The most passes.
pub const MAX_PASSES: usize = 12;
/// A waveguide grid's size shrinks by this a pass.
const UNIFORM: f64 = 1.4;
/// Marked tetrahedra are refined to no less than the starting size over this.
const FLOOR: f64 = 16.0;

/// The largest change of any entry between two S-matrices.
fn delta(a: &SMatrix, b: &SMatrix) -> f64 {
    a.entries().map(|((p, q), x)| b.get(p, q).map_or(f64::INFINITY, |y| (x - y).norm())).fold(0.0, f64::max)
}

/// The largest change of any entry between two lists of reflections.
fn delta_list(a: &[C], b: &[C]) -> f64 {
    if a.len() != b.len() {
        return f64::INFINITY;
    }
    a.iter().zip(b).map(|(x, y)| (x - y).norm()).fold(0.0, f64::max)
}

/// The relative change ‖a − b‖ / ‖b‖ of two far fields sampled in the same directions.
fn delta_far(a: &[C], b: &[C]) -> f64 {
    if a.len() != b.len() {
        return f64::INFINITY;
    }
    let diff: f64 = a.iter().zip(b).map(|(x, y)| (x - y).norm_sqr()).sum();
    let norm: f64 = b.iter().map(|y| y.norm_sqr()).sum();
    if norm > 0.0 {
        (diff / norm).sqrt()
    } else if diff == 0.0 {
        0.0
    } else {
        f64::INFINITY
    }
}

/// The directions an open problem's far field is compared in: θ = 15°, 45°, …, 165° (off the
/// poles, where E_θ and E_φ swap names), φ = 0°, 45°, …, 315° — 48, spread over the sphere.
pub fn directions() -> Vec<(f64, f64)> {
    (0..6).flat_map(|i| (0..8).map(move |j| ((15.0 + 30.0 * f64::from(i)).to_radians(), (45.0 * f64::from(j)).to_radians()))).collect()
}

/// What a pass's convergence is judged on.
enum Measure {
    /// A closed problem's S-matrix.
    S(SMatrix),
    /// An open problem's reflections: each lumped port's, then a waveguide feed's.
    Reflections(Vec<C>),
    /// An open problem's far field r e^{jkr}(E_θ, E_φ) in [`directions`].
    FarField(Vec<C>),
}

impl Measure {
    /// The change from `prev`: ΔS, or the far field's relative change.
    fn change(&self, prev: &Measure) -> f64 {
        match (self, prev) {
            (Measure::S(a), Measure::S(b)) => delta(a, b),
            (Measure::Reflections(a), Measure::Reflections(b)) => delta_list(a, b),
            (Measure::FarField(a), Measure::FarField(b)) => delta_far(a, b),
            _ => f64::INFINITY,
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Measure::S(_) | Measure::Reflections(_) => "s_parameters",
            Measure::FarField(_) => "far_field",
        }
    }

    /// The first reflection, S11.
    fn s11(&self) -> Option<C> {
        match self {
            Measure::S(s) => s.get(0, 0),
            Measure::Reflections(r) => r.first().copied(),
            Measure::FarField(_) => None,
        }
    }
}

/// Why the loop stopped.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stop {
    Converged,
    Passes,
    Limit,
}

impl Stop {
    fn reason(self) -> &'static str {
        match self {
            Stop::Converged => "converged",
            Stop::Passes => "passes",
            Stop::Limit => "mesh or unknowns limit",
        }
    }
}

/// How each pass's mesh is made.
enum Refine {
    /// A solid: meshed again with the balls of every pass's marked tetrahedra.
    Adaptive { sizes: emcraft_mesh::solid::Sizes, curved: bool, theta: f64, floor: f64, balls: Vec<emcraft_mesh::solid::Refinement> },
    /// A waveguide grid: its size shrunk by [`UNIFORM`] a pass.
    Uniform { size: f64 },
}

impl Refine {
    fn mesh(&self, s: &Session, p: &Params, pass: usize) -> Result<std::result::Result<Mesh, emcraft_mesh::MeshError>> {
        Ok(match self {
            Refine::Adaptive { sizes, curved, balls, .. } => emcraft_mesh::solid::solid_refined(&s.model, sizes, *curved, balls),
            Refine::Uniform { size } => {
                let h = size / UNIFORM.powi(pass as i32);
                emcraft_mesh::waveguide_with(&s.model, &crate::meshing::grid_options(p, h)?)
            }
        })
    }

    /// Marks where the error `eta` (per tetrahedron of `mesh`) is and adds its balls for the
    /// next pass; how many tetrahedra it marked (none for a uniform grid).
    fn mark(&mut self, mesh: &Mesh, eta: impl FnOnce() -> Result<Vec<f64>>) -> Result<usize> {
        let Refine::Adaptive { theta, floor, balls, .. } = self else { return Ok(0) };
        crate::progress::check()?;
        let eta = eta()?;
        let marked = emcraft_mesh::adapt::dorfler(&eta, *theta)?;
        balls.extend(emcraft_mesh::adapt::refinements(mesh, &marked, *floor)?);
        Ok(marked.len())
    }

    fn name(&self) -> &'static str {
        match self {
            Refine::Adaptive { .. } => "adaptive",
            Refine::Uniform { .. } => "uniform",
        }
    }
}

/// One pass, as the reply lists it.
#[derive(serde::Serialize)]
struct Pass {
    pass: usize,
    tetrahedra: usize,
    unknowns: usize,
    /// ΔS (S-parameters).
    #[serde(skip_serializing_if = "Option::is_none")]
    delta_s: Option<f64>,
    /// The far field's relative change (sources without ports).
    #[serde(skip_serializing_if = "Option::is_none")]
    delta_far_field: Option<f64>,
    marked: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    s11: Option<Value>,
    /// Open problems: the first lumped port's input impedance (Ω).
    #[serde(skip_serializing_if = "Option::is_none")]
    impedance: Option<Value>,
    /// Open problems: the power (W) through the Huygens surface, radiated or scattered.
    #[serde(skip_serializing_if = "Option::is_none")]
    power: Option<f64>,
    seconds: f64,
}

/// A pass solved: its measure and its row's open-problem extras.
struct Solved {
    unknowns: usize,
    measure: Measure,
    impedance: Option<C>,
    power: Option<f64>,
}

/// What a pass solved, for its indicator.
enum Solution {
    /// A closed problem's sweep point.
    Closed(FrequencyResult),
    /// An open problem as `solve.radiation` solves it.
    Open(OpenResult),
}

/// Solves the pass's problem on the session's mesh: a closed one at `frequency`, an open one as
/// `solve.radiation` does.
fn solve_pass(s: &Session, driven: &Driven, frequency: f64, open: bool) -> Result<(Solved, Solution)> {
    crate::progress::check()?;
    let unknowns = driven.dofs.unknowns;
    if !open {
        let r = driven.solve(frequency)?;
        let solved = Solved { unknowns, measure: Measure::S(r.s.clone()), impedance: None, power: None };
        return Ok((solved, Solution::Closed(r)));
    }
    let r = driven.solve_open(&s.model, frequency)?;
    let huygens = driven.huygens(&s.model, &r.field, frequency)?;
    let reflections: Vec<C> = r.ports.iter().map(|p| p.s11).chain(r.feed.map(|(s11, _)| s11)).collect();
    let measure = if reflections.is_empty() {
        Measure::FarField(emcraft_solver::radiation::far_fields(&huygens, &directions()).iter().flat_map(|f| [f.e_theta, f.e_phi]).collect())
    } else {
        Measure::Reflections(reflections)
    };
    let solved = Solved { unknowns, measure, impedance: r.ports.first().map(|p| p.impedance), power: Some(huygens.power()) };
    Ok((solved, Solution::Open(r)))
}

/// The error indicator of each tetrahedron of a pass's mesh.
fn indicator(driven: &Driven, model: &Model, solution: &Solution) -> Result<Vec<f64>> {
    Ok(match solution {
        Solution::Closed(r) => emcraft_solver::adapt::indicator(driven, r)?,
        Solution::Open(r) => emcraft_solver::adapt::open_indicator(driven, model, r)?,
    })
}

/// What `mesh.adapt` is asked for.
struct Options {
    /// Whether the model is an open problem.
    open: bool,
    /// Hz.
    frequency: f64,
    passes: usize,
    tolerance: f64,
    order: Order,
}

/// `mesh.adapt`'s options and how its passes refine, checked against the model.
fn options(s: &Session, p: &Params) -> Result<(Options, Refine)> {
    let open = s.model.open.is_some();
    if open {
        if s.model.sources.is_empty() && s.model.lumped.is_empty() && s.model.ports.is_empty() {
            return Err(EngineError::failed(
                "adaptive refinement of an open problem compares its solutions between passes: add a source (source.dipole, source.plane_wave), a lumped port (model.port.lumped) or a waveguide port",
            ));
        }
    } else if s.model.ports.is_empty() {
        return Err(EngineError::failed("adaptive refinement compares S-parameters between passes: the model needs ports (model.port.create)"));
    }
    let solid = s.model.waveguide.is_none() && s.model.solid.is_some();
    if !solid && s.model.waveguide.is_none() {
        return Err(EngineError::failed("nothing to mesh: create a waveguide or a solid first"));
    }
    let sizes = crate::meshing::Sizes::read(s, p)?;
    let frequency = crate::args::frequency_or_band_top(s, p, "frequency", "the frequency to adapt at, or set the band: project.frequency")?;
    let passes = p.integer("passes", 1..=MAX_PASSES).or(6)?;
    let tolerance = p.positive("tolerance").check(|t| *t <= 1.0, "at most 1").or(0.02)?;
    let theta = p.positive("fraction").check(|t| *t <= 1.0, "at most 1").or(0.3)?;
    let order = p.order()?;
    let curved = p.flag("curved").or(true)?;
    let refine = if solid {
        let sizes = emcraft_mesh::solid::Sizes { default: sizes.default, per_material: sizes.per_material.clone() };
        let floor = sizes.per_material.iter().flatten().copied().fold(sizes.default, f64::min) / FLOOR;
        Refine::Adaptive { sizes, curved, theta, floor, balls: Vec::new() }
    } else {
        Refine::Uniform { size: sizes.grid_size(s)? }
    };
    Ok((Options { open, frequency, passes, tolerance, order }, refine))
}

/// Pass `pass`'s mesh, set on the session, and its problem; `None` when it would pass the
/// tetrahedra or unknowns cap and an earlier pass fitted (`fitted`).
fn build_pass(s: &mut Session, p: &Params, refine: &Refine, opts: &Options, pass: usize, fitted: bool) -> Result<Option<Driven>> {
    let mesh = match refine.mesh(s, p, pass)? {
        Ok(m) => m,
        Err(emcraft_mesh::MeshError::TooLarge { .. }) if fitted => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    s.model.set_mesh(mesh);
    let built = if opts.open { Driven::open(&s.model, opts.order) } else { Driven::new(&s.model, opts.order) };
    match built {
        Ok(d) => Ok(Some(d)),
        Err(SolveError::TooLarge { .. }) if fitted => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// The reply's measure: the last ΔS (null after one pass) and S-parameters, or the far
/// field's last change.
#[derive(serde::Serialize)]
#[serde(untagged)]
enum Measured {
    S { delta_s: Option<f64>, s11: Value, s21: Value },
    Reflections { delta_s: Option<f64>, s11: Value },
    FarField { delta_far_field: Option<f64> },
}

/// `mesh.adapt`'s reply.
#[derive(serde::Serialize)]
struct Reply {
    frequency: f64,
    order: u8,
    refinement: &'static str,
    quantity: &'static str,
    tolerance: f64,
    converged: bool,
    stopped: &'static str,
    passes: Vec<Pass>,
    nodes: usize,
    tetrahedra: usize,
    #[serde(flatten)]
    measured: Measured,
    /// The last pass's first lumped port's input impedance (Ω), for an open problem.
    #[serde(skip_serializing_if = "Option::is_none")]
    impedance: Option<Value>,
    /// The last pass's power through the Huygens surface (W), for an open problem.
    #[serde(skip_serializing_if = "Option::is_none")]
    power: Option<f64>,
}

pub fn adapt(s: &mut Session, p: &Params) -> Result<Value> {
    let (opts, mut refine) = options(s, p)?;
    let mut last: Option<(Mesh, Measure)> = None;
    let mut rows: Vec<Pass> = Vec::new();
    let mut stop = Stop::Passes;
    for pass in 0..opts.passes {
        crate::progress::report(pass, opts.passes, "adaptive pass")?;
        let t0 = Instant::now();
        let Some(driven) = build_pass(s, p, &refine, &opts, pass, last.is_some())? else {
            stop = Stop::Limit;
            break;
        };
        let (solved, solution) = solve_pass(s, &driven, opts.frequency, opts.open)?;
        let change = last.as_ref().map(|(_, prev)| solved.measure.change(prev));
        let converged = change.is_some_and(|d| d < opts.tolerance);
        // A pass marks for the next unless it is the last or converged.
        let marked = if pass + 1 < opts.passes && !converged {
            let model = &s.model;
            refine.mark(&driven.mesh, || indicator(&driven, model, &solution))?
        } else {
            0
        };
        let far = matches!(solved.measure, Measure::FarField(_));
        rows.push(Pass {
            pass: pass + 1,
            tetrahedra: driven.mesh.tets.len(),
            unknowns: solved.unknowns,
            delta_s: change.filter(|_| !far),
            delta_far_field: change.filter(|_| far),
            marked,
            s11: solved.measure.s11().map(crate::reply::complex),
            impedance: solved.impedance.map(crate::reply::parts),
            power: solved.power,
            seconds: t0.elapsed().as_secs_f64(),
        });
        last = Some((driven.mesh, solved.measure));
        if converged {
            stop = Stop::Converged;
            break;
        }
    }
    let (mesh, measure) = last.ok_or_else(|| EngineError::failed("no pass finished"))?;
    let change = rows.last().and_then(|r| r.delta_s.or(r.delta_far_field));
    let s11 = crate::reply::complex(measure.s11().unwrap_or_default());
    let measured = match &measure {
        Measure::S(sm) => Measured::S { delta_s: change, s11, s21: crate::reply::complex(sm.get(1, 0).unwrap_or_default()) },
        Measure::Reflections(_) => Measured::Reflections { delta_s: change, s11 },
        Measure::FarField(_) => Measured::FarField { delta_far_field: change },
    };
    let reply = Reply {
        frequency: s.show_frequency(opts.frequency),
        order: opts.order.as_u8(),
        refinement: refine.name(),
        quantity: measure.name(),
        tolerance: opts.tolerance,
        converged: stop == Stop::Converged,
        stopped: stop.reason(),
        nodes: mesh.nodes.len(),
        tetrahedra: mesh.tets.len(),
        impedance: rows.last().and_then(|r| r.impedance.clone()),
        power: rows.last().and_then(|r| r.power),
        passes: rows,
        measured,
    };
    s.model.set_mesh(mesh);
    serde_json::to_value(reply).map_err(EngineError::failed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    #[test]
    fn far_field_changes_are_relative() {
        let a = [C::new(1.0, 0.0), C::new(0.0, 1.0)];
        assert_eq!(delta_far(&a, &a), 0.0);
        let b = [C::new(1.1, 0.0), C::new(0.0, 1.0)];
        assert!((delta_far(&b, &a) - 0.1 / 2f64.sqrt()).abs() < 1e-12);
        assert_eq!(delta_far(&a, &[C::new(0.0, 0.0); 2]), f64::INFINITY);
        assert_eq!(delta_far(&a, &a[..1]), f64::INFINITY);
        assert_eq!(directions().len(), 48);
        assert!(directions().iter().all(|(t, _)| *t > 0.2 && *t < PI - 0.2));
    }
}
