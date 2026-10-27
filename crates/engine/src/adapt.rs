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

/// Why the loop stopped.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stop {
    Converged,
    Passes,
    Limit,
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
    let frequency = p
        .frequency("frequency")
        .or_maybe(s.model.band.map(|b| b.stop))
        .required_or("the frequency to adapt at, or set the band: project.frequency")?;
    let passes = p.integer("passes", 1..=MAX_PASSES).or(6)?;
    let tolerance = p.positive("tolerance").check(|t| *t <= 1.0, "at most 1").or(0.02)?;
    let theta = p.positive("fraction").check(|t| *t <= 1.0, "at most 1").or(0.3)?;
    let order = p.order()?;
    let curved = p.flag("curved").or(true)?;
    let solid_sizes = emcraft_mesh::solid::Sizes { default: sizes.default, per_material: sizes.per_material.clone() };
    let grid_size = sizes.grid_size(s)?;
    let floor = solid_sizes.per_material.iter().flatten().copied().fold(solid_sizes.default, f64::min) / FLOOR;

    let mut refine: Vec<emcraft_mesh::solid::Refinement> = Vec::new();
    let mut last: Option<(Mesh, SMatrix)> = None;
    let mut rows = Vec::new();
    let mut stop = Stop::Passes;
    for pass in 0..passes {
        crate::progress::report(pass, passes, "adaptive pass")?;
        let t0 = Instant::now();
        let mesh = if solid {
            emcraft_mesh::solid::solid_refined(&s.model, &solid_sizes, curved, &refine)
        } else {
            let h = grid_size / UNIFORM.powi(pass as i32);
            emcraft_mesh::waveguide_with(&s.model, &crate::meshing::grid_options(p, h)?)
        };
        let mesh = match mesh {
            Ok(m) => m,
            Err(emcraft_mesh::MeshError::TooLarge { .. }) if last.is_some() => {
                stop = Stop::Limit;
                break;
            }
            Err(e) => return Err(e.into()),
        };
        let tets = mesh.tets.len();
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
        let change = last.as_ref().map(|(_, prev)| delta(&r.s, prev));
        let mut marked = 0;
        let converged = change.is_some_and(|d| d < tolerance);
        if solid && !converged && pass + 1 < passes {
            crate::progress::check()?;
            let eta = emcraft_solver::adapt::indicator(&driven, &r)?;
            let m = emcraft_mesh::adapt::dorfler(&eta, theta)?;
            marked = m.len();
            refine.extend(emcraft_mesh::adapt::refinements(&driven.mesh, &m, floor)?);
        }
        rows.push(json!({
            "pass": pass + 1,
            "tetrahedra": tets,
            "unknowns": driven.dofs.unknowns,
            "delta_s": change,
            "marked": marked,
            "s11": crate::results::complex(r.s.get(0, 0).unwrap_or_default()),
            "seconds": t0.elapsed().as_secs_f64(),
        }));
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
        "refinement": if solid { "adaptive" } else { "uniform" },
        "tolerance": tolerance,
        "converged": stop == Stop::Converged,
        "stopped": match stop {
            Stop::Converged => "converged",
            Stop::Passes => "passes",
            Stop::Limit => "mesh or unknowns limit",
        },
        "delta_s": rows.last().and_then(|r| r["delta_s"].as_f64()),
        "passes": rows,
        // The last pass's S-parameters.
        "s11": crate::results::complex(sm.get(0, 0).unwrap_or_default()),
        "s21": crate::results::complex(sm.get(1, 0).unwrap_or_default()),
        "nodes": mesh.nodes.len(),
        "tetrahedra": mesh.tets.len(),
    });
    s.model.mesh = Some(mesh);
    s.results.sweep = None;
    Ok(reply)
}
