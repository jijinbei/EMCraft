//! `mesh.adapt`: adaptive mesh refinement, the loop of passes until the S-parameters settle.
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

use std::time::Instant;

use emcraft_model::Mesh;
use emcraft_solver::{Driven, SMatrix, SolveError};
use serde_json::{Value, json};

use crate::{Change, EngineError, Params, Result, Session};

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

    /// Marks where the error is and adds its balls for the next pass; how many tetrahedra it
    /// marked (none for a uniform grid).
    fn mark(&mut self, driven: &Driven, r: &emcraft_solver::driven::FrequencyResult) -> Result<usize> {
        let Refine::Adaptive { theta, floor, balls, .. } = self else { return Ok(0) };
        crate::progress::check()?;
        let eta = emcraft_solver::adapt::indicator(driven, r)?;
        let marked = emcraft_mesh::adapt::dorfler(&eta, *theta)?;
        balls.extend(emcraft_mesh::adapt::refinements(&driven.mesh, &marked, *floor)?);
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
    delta_s: Option<f64>,
    marked: usize,
    s11: Value,
    seconds: f64,
}

pub fn adapt(s: &mut Session, p: &Params) -> Result<Value> {
    if s.model.open.is_some() {
        return Err(EngineError::failed(
            "not supported yet: adaptive refinement of an open problem (its layer and sources); mesh.generate with per_wavelength",
        ));
    }
    if s.model.ports.is_empty() {
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
    let mut refine = if solid {
        let sizes = emcraft_mesh::solid::Sizes { default: sizes.default, per_material: sizes.per_material.clone() };
        let floor = sizes.per_material.iter().flatten().copied().fold(sizes.default, f64::min) / FLOOR;
        Refine::Adaptive { sizes, curved, theta, floor, balls: Vec::new() }
    } else {
        Refine::Uniform { size: sizes.grid_size(s)? }
    };

    let mut last: Option<(Mesh, SMatrix)> = None;
    let mut rows: Vec<Pass> = Vec::new();
    let mut stop = Stop::Passes;
    for pass in 0..passes {
        crate::progress::report(pass, passes, "adaptive pass")?;
        let t0 = Instant::now();
        let mesh = match refine.mesh(s, p, pass)? {
            Ok(m) => m,
            Err(emcraft_mesh::MeshError::TooLarge { .. }) if last.is_some() => {
                stop = Stop::Limit;
                break;
            }
            Err(e) => return Err(e.into()),
        };
        let tetrahedra = mesh.tets.len();
        s.model.mesh = Some(mesh);
        let driven = match Driven::new(&s.model, order) {
            Ok(d) => d,
            Err(SolveError::TooLarge { .. }) if last.is_some() => {
                stop = Stop::Limit;
                break;
            }
            Err(e) => return Err(e.into()),
        };
        crate::progress::check()?;
        let r = driven.solve(frequency)?;
        let delta_s = last.as_ref().map(|(_, prev)| delta(&r.s, prev));
        let converged = delta_s.is_some_and(|d| d < tolerance);
        let marked = if !converged && pass + 1 < passes { refine.mark(&driven, &r)? } else { 0 };
        rows.push(Pass {
            pass: pass + 1,
            tetrahedra,
            unknowns: driven.dofs.unknowns,
            delta_s,
            marked,
            s11: crate::reply::complex(r.s.get(0, 0).unwrap_or_default()),
            seconds: t0.elapsed().as_secs_f64(),
        });
        last = Some((driven.mesh, r.s));
        if converged {
            stop = Stop::Converged;
            break;
        }
    }
    let (mesh, sm) = last.ok_or_else(|| EngineError::failed("no pass finished"))?;
    let reply = json!({
        "frequency": s.show_frequency(frequency),
        "order": order.as_u8(),
        "refinement": refine.name(),
        "tolerance": tolerance,
        "converged": stop == Stop::Converged,
        "stopped": stop.reason(),
        "delta_s": rows.last().and_then(|r| r.delta_s),
        "passes": rows,
        // The last pass's S-parameters.
        "s11": crate::reply::complex(sm.get(0, 0).unwrap_or_default()),
        "s21": crate::reply::complex(sm.get(1, 0).unwrap_or_default()),
        "nodes": mesh.nodes.len(),
        "tetrahedra": mesh.tets.len(),
    });
    s.model.mesh = Some(mesh);
    s.results.invalidate(Change::Geometry);
    Ok(reply)
}
