//! How long a run will take, before it starts (`project.estimate`): the run as set up — a solve
//! command with its parameters, an adaptive mesh, a parameter study — costed from the model
//! alone, cheaply and deterministically (the same model and parameters give the same numbers).
//!
//! A frequency-domain solve's cost is one factorization and its solves: its unknowns counted
//! from the mesh's edges and faces, the factor's entries scaled as n^(4/3) and its time as n²
//! (nested dissection on a 3D mesh, A. George, *Nested dissection of a regular finite element
//! mesh*, SIAM J. Numer. Anal. 10(2), 1973) from the session's own last sweep when it has one —
//! measured on this machine — else from a reference machine's (the coaxial line's 45 000
//! unknowns, 24 million entries, 1.6 s). The run multiplies it: the frequencies (a fast sweep
//! solves a few in full and the rest in its reduced basis: after each full solve, a pass over
//! the others, each forming its m × m system from the basis' m vectors of n unknowns, about
//! m² n multiply-adds, so a sweep of many frequencies over a large model is dominated by them;
//! measured on the cross-guide coupler's 112 000 unknowns, see [`BASIS_MAC`]), the
//! eigenmodes' subspace iterations, the adaptive passes (each refined mesh about 1.6 times the
//! unknowns of the last), the phases of a dispersion diagram, a study's points. The time
//! domain's cost is its cells times its steps (the Courant limit over the run's time, the band's
//! lowest frequency's periods and the structure's transit) at the reference machine's 20 ns per
//! cell and step.

use emcraft_model::units::C0;
use serde_json::{Value, json};

use crate::{EngineError, Params, Result, Session};

/// A solve the others are scaled from: its unknowns, factor entries and seconds of
/// factorization and solves.
#[derive(Clone, Copy, Debug)]
struct Basis {
    unknowns: f64,
    entries: f64,
    seconds: f64,
}

/// The reference machine's solve.
const REFERENCE: Basis = Basis { unknowns: 44_910.0, entries: 24.0e6, seconds: 1.6 };
/// The reference machine's time-domain cost per cell and step (s).
const CELL_STEP: f64 = 2.0e-8;
/// The reference machine's seconds per multiply-add of a fast sweep's work in its basis (each
/// frequency's Vᵀ A V and residuals, memory-bound on one thread). Measured on the cross-guide
/// coupler (112 306 unknowns at order 2, four ports, six full solves): 13 s in the basis at 41
/// frequencies, 26 s at 101 — 0.4 and 0.3 ns by [`basis_work`].
const BASIS_MAC: f64 = 0.35e-9;
/// The most full solves a fast sweep makes (the solver's `SweepOptions::max_solves`).
const MAX_FULL: f64 = 64.0;
/// How much each adaptive pass grows the unknowns.
const PASS_GROWTH: f64 = 1.6;
/// The most nested studies costed (a study's solve is not a study).
const MAX_DEPTH: usize = 2;

/// One factorization and its solves of the meshed model at an order.
#[derive(Clone, Copy, Debug)]
struct Solve {
    unknowns: f64,
    memory_mb: f64,
    seconds: f64,
    /// Scaled from this session's own sweep (this machine), not the reference's.
    measured: bool,
}

/// The meshed model's solve at `order`.
fn solve_cost(s: &Session, order: usize) -> Result<Solve> {
    let mesh = s.model.mesh.as_ref().ok_or_else(|| EngineError::failed("no mesh: mesh the model first (mesh.generate)"))?;
    let topo = emcraft_mesh::Topology::new(mesh);
    let (edges, faces) = (topo.edges.len() as f64, topo.faces.len() as f64);
    let unknowns = if order == 1 { edges } else { 2.0 * edges + 2.0 * faces };
    let measured = s.results.sweep.as_ref().and_then(|sw| {
        let r = sw.results.iter().find(|r| r.timings.factor > 0.0)?;
        Some(Basis { unknowns: sw.driven.dofs.unknowns as f64, entries: r.factor_entries as f64, seconds: r.timings.factor + r.timings.solve })
    });
    let basis = measured.unwrap_or(REFERENCE);
    let ratio = unknowns / basis.unknowns.max(1.0);
    Ok(Solve {
        unknowns,
        memory_mb: basis.entries * ratio.powf(4.0 / 3.0) * 16.0 / 1.0e6,
        seconds: basis.seconds * ratio * ratio,
        measured: measured.is_some(),
    })
}

/// A run's cost: seconds in all, solves (factorizations, or a time domain's runs), the one
/// solve it repeats (none for the time domain), what it does in words.
struct Cost {
    seconds: f64,
    solves: f64,
    solve: Option<Solve>,
    what: String,
}

/// Full solves a fast sweep of `n` frequencies makes (the rest in its basis). They follow what
/// the band holds, not the count: the coupler made six at 11, 41 and 101 frequencies, the
/// high-pass filter six at 21; a little more with more frequencies.
fn full_solves(n: f64) -> f64 {
    if n < 3.0 { n } else { (4.0 + (n.log2() / 2.0).ceil()).min(n) }
}

/// The multiply-adds of a fast sweep's work in its basis: `n` frequencies, `full` of them solved
/// in full (the band's ends, then one per pass), each full solve adding two vectors (its real
/// and imaginary parts) per excitation; the pass after the k-th full solve solves the other
/// n − k in a basis of m = 2 · excitations · k vectors of `unknowns`, each forming Vᵀ A V
/// (m² n) and each excitation's field and residual (2 m n).
fn basis_work(n: f64, full: f64, excitations: f64, unknowns: f64) -> f64 {
    let per_solve = 2.0 * excitations.max(1.0);
    (2..=full.clamp(0.0, MAX_FULL) as usize)
        .map(|k| {
            let m = per_solve * k as f64;
            (n - k as f64).max(0.0) * unknowns * (m * m + 2.0 * excitations.max(1.0) * m)
        })
        .sum()
}

/// The frequencies a sweep's parameters give: `frequencies` listed, else `points` (11).
fn frequency_count(p: &Params, max: usize) -> Result<f64> {
    if let Some(list) = p.list("frequencies", max).optional()? {
        return Ok(list.len().max(1) as f64);
    }
    Ok(p.integer("points", 1..=max).or(crate::solve::DEFAULT_POINTS)? as f64)
}

/// The cost of `command` run with parameters `p`, its order `order` unless they give one.
fn cost(s: &Session, command: &str, p: &Params, order: Option<usize>, depth: usize) -> Result<Cost> {
    let order_of = || -> Result<usize> { Ok(order.unwrap_or(p.integer("order", 1..=2).or(2)?)) };
    let max = s.limits.frequencies;
    match command {
        "solve.frequency" | "solve.refine" => {
            let n = frequency_count(p, max)?;
            let solve = solve_cost(s, order_of()?)?;
            let full = if p.string("sweep").optional()? == Some("full") { n } else { full_solves(n) };
            let what = if full < n {
                format!("{n} frequencies, fast sweep: about {full} solved in full")
            } else {
                format!("{n} frequencies, each solved in full")
            };
            let excitations = (s.model.ports.len() + s.model.lumped.len()) as f64;
            let basis = if full < n { basis_work(n, full, excitations, solve.unknowns) * BASIS_MAC } else { 0.0 };
            Ok(Cost { seconds: solve.seconds * full + basis, solves: full, solve: Some(solve), what })
        }
        "solve.eigenmodes" => {
            let count = p.integer("count", 1..=crate::EIGEN_MAX).or(5)? as f64;
            let solve = solve_cost(s, order_of()?)?;
            // One factorization, then subspace iterations of solves: about a quarter of one per mode.
            let factor = 1.0 + 0.25 * count;
            Ok(Cost { seconds: solve.seconds * factor, solves: 1.0, solve: Some(solve), what: format!("{count} eigenmodes") })
        }
        "solve.radiation" => {
            let solve = solve_cost(s, order_of()?)?;
            Ok(Cost { seconds: solve.seconds, solves: 1.0, solve: Some(solve), what: "one frequency with its layer".into() })
        }
        "solve.dispersion" => {
            let points = p.integer("points", 1..=201).or(11)? as f64;
            let bands = p.integer("bands", 1..=20).or(4)? as f64;
            let solve = solve_cost(s, order_of()?)?;
            // Each phase: an unsymmetric LU (about twice a symmetric factorization) and its iterations.
            let each = 2.0 * (1.0 + 0.25 * bands);
            Ok(Cost {
                seconds: solve.seconds * each * points,
                solves: points,
                solve: Some(solve),
                what: format!("{points} phases, {bands} bands each"),
            })
        }
        "mesh.adapt" => {
            let passes = p.integer("passes", 1..=12).or(6)? as f64;
            let solve = solve_cost(s, order_of()?)?;
            // The passes' meshes grow: the k-th solve costs PASS_GROWTH^(2k) of the first.
            let sum: f64 = (0..passes as usize).map(|k| PASS_GROWTH.powi(2 * k as i32)).sum();
            Ok(Cost { seconds: solve.seconds * sum, solves: passes, solve: Some(solve), what: format!("at most {passes} adaptive passes") })
        }
        "solve.time_domain" => time_domain(s, p),
        "study.sweep" | "study.optimize" if depth < MAX_DEPTH => {
            let points = if command == "study.optimize" {
                p.integer("max_evaluations", 1..=500).or(50)? as f64
            } else if let Some(vars) = p.list("variables", 4).optional()? {
                let mut n = 1.0f64;
                for (i, v) in vars.iter().enumerate() {
                    n *= variable_points(&p.nested(v, format!("variable {}", i + 1)))?;
                }
                n.min(400.0)
            } else {
                variable_points(p)?
            };
            // A surrogate sweep solves its budget only.
            let points = match p.raw("surrogate").filter(|_| command == "study.sweep") {
                Some(g) => points.min(p.nested(g, "surrogate").integer("budget", 2..=400).required()? as f64),
                None => points,
            };
            let solve = p.raw("solve").ok_or_else(|| p.error("solve", "is needed: {\"command\", \"params\"}"))?;
            let inner_command = solve.get("command").and_then(Value::as_str).ok_or_else(|| p.error("solve", "needs a `command`"))?;
            let empty = json!({});
            let inner = p.nested(solve.get("params").unwrap_or(&empty), "solve");
            let one = cost(s, inner_command, &inner, None, depth + 1)?;
            let what = format!("{points} points, each {}", one.what);
            Ok(Cost { seconds: one.seconds * points, solves: one.solves * points, solve: one.solve, what })
        }
        other => Err(EngineError::Params(format!("`command`: cannot estimate `{}`", crate::text::clip(other, 40)))),
    }
}

/// The values a study's variable takes: `values` listed, else `points` (5).
fn variable_points(p: &Params) -> Result<f64> {
    Ok(match p.list("values", 101).optional()? {
        Some(list) => list.len() as f64,
        None => p.integer("points", 1..=101).or(5)? as f64,
    })
}

/// The time domain's run: cells of `cell` (1 mm) over the model's box (and its layers), steps at
/// the Courant limit over 20 periods of the lowest frequency and four transits of the box.
fn time_domain(s: &Session, p: &Params) -> Result<Cost> {
    let cell = p.positive_length("cell").or(1e-3)?;
    let b = s.model.bounds().ok_or_else(|| EngineError::failed("no geometry"))?;
    let size = (b.max - b.min).to_array();
    let open = s.model.open.is_some();
    // Ten cells of layer on each open side; a guide's feeds along z.
    let pad = if open { [20.0; 3] } else { [0.0, 0.0, 40.0] };
    let cells: f64 = (0..3).map(|k| (size[k] / cell).ceil().max(1.0) + pad[k]).product();
    let low = match p.list("frequencies", s.limits.frequencies).optional()? {
        Some(list) => list.iter().filter_map(|v| p.number_of("frequencies", v).ok()).fold(f64::INFINITY, f64::min) * s.display.frequency.to_si(1.0),
        None => p.frequency("start").or_maybe(s.model.band.map(|b| b.start.get())).required_or("set the band (project.frequency) or give `start`")?,
    };
    if !(low.is_finite() && low > 0.0) {
        return Err(p.error("frequencies", "must be above 0"));
    }
    let diagonal = size.iter().map(|v| v * v).sum::<f64>().sqrt();
    let dt = cell / (C0 * 3f64.sqrt());
    let steps = ((20.0 / low + 4.0 * diagonal / C0) / dt).ceil().min(1.0e6);
    // A guide runs twice: the device and a uniform reference.
    let runs = if open { 1.0 } else { 2.0 };
    let what = format!("{cells:.0} cells, about {steps:.0} steps{}", if open { "" } else { ", twice (device and reference)" });
    Ok(Cost { seconds: cells * steps * runs * CELL_STEP, solves: runs, solve: None, what })
}

/// `project.estimate`: the run as set up — `command` with `params`, else the project's latest
/// run, else a frequency sweep of the band — its total seconds, and for a frequency-domain solve
/// its unknowns, the factor's memory and the seconds of one solve; `basis` says whether the
/// numbers are scaled from this machine's own last sweep (`measured`) or the reference
/// machine's (`reference`).
///
/// ```
/// let mut s = emcraft_engine::Session::new();
/// s.execute("file.sample", &serde_json::json!({"name": "highpass"})).unwrap();
/// // No mesh: an error, not a panic.
/// assert!(emcraft_engine::Session::new().execute("project.estimate", &serde_json::json!({})).is_err());
/// s.execute("mesh.generate", &serde_json::json!({"size": 4})).unwrap();
/// let sweep = serde_json::json!({"command": "solve.frequency", "params": {"points": 41, "order": 1}});
/// let e = s.execute("project.estimate", &sweep).unwrap();
/// assert_eq!(e["basis"], "reference");
/// assert!(e["seconds"].as_f64().unwrap() > 0.0);
/// // More frequencies cost more; the same set-up costs the same.
/// let more = serde_json::json!({"command": "solve.frequency", "params": {"points": 201, "order": 1}});
/// assert!(s.execute("project.estimate", &more).unwrap()["seconds"].as_f64() > e["seconds"].as_f64());
/// assert_eq!(s.execute("project.estimate", &sweep).unwrap(), e);
/// assert!(s.execute("project.estimate", &serde_json::json!({"command": "file.open"})).is_err());
/// ```
pub fn estimate(s: &mut Session, p: &Params) -> Result<Value> {
    let order = p.integer("order", 1..=2).optional()?;
    let empty = json!({});
    let (command, given, units) = match p.string("command").optional()? {
        Some(c) => (c, p.raw("params").unwrap_or(&empty), s.display),
        None => match s.runs.last() {
            Some(run) => (run.command.as_str(), &run.params, run.units),
            None => ("solve.frequency", &empty, s.display),
        },
    };
    if !given.is_object() {
        return Err(p.error("params", "must be an object"));
    }
    let inner = Params::new(given, units).with_variables(&s.variables);
    let c = cost(s, command, &inner, order, 0)?;
    let mut reply = json!({
        "command": command,
        "seconds": c.seconds,
        "solves": c.solves,
        "what": c.what,
        "basis": if c.solve.is_some_and(|x| x.measured) { "measured" } else { "reference" },
    });
    if let Some(one) = c.solve {
        reply["order"] = json!(order.unwrap_or(inner.integer("order", 1..=2).or(2)?));
        reply["unknowns"] = json!(one.unknowns.round());
        reply["memory_mb"] = json!(one.memory_mb.round());
        reply["seconds_per_frequency"] = json!(one.seconds);
        reply["from_this_session"] = json!(one.measured);
    }
    Ok(reply)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_fast_sweeps_basis_work_grows_with_its_frequencies_and_solves() {
        // The coupler at 41 frequencies: six full solves, four ports (measured 13 s).
        let w = super::basis_work(41.0, 6.0, 4.0, 112_306.0);
        assert!((w * super::BASIS_MAC - 13.0).abs() < 3.0, "{w:e}");
        assert!(super::basis_work(101.0, 6.0, 4.0, 112_306.0) > 2.0 * w);
        assert!(super::basis_work(41.0, 9.0, 4.0, 112_306.0) > 2.0 * w);
        // No pass without two full solves; hostile counts give nothing or the cap, not a panic.
        assert_eq!(super::basis_work(41.0, 1.0, 4.0, 1e5), 0.0);
        assert_eq!(super::basis_work(f64::NAN, f64::NAN, 4.0, 1e5), 0.0);
        assert!(super::basis_work(1e4, f64::INFINITY, 4.0, 1e5).is_finite());
        assert_eq!(super::full_solves(2.0), 2.0);
        assert_eq!((super::full_solves(11.0), super::full_solves(101.0)), (6.0, 8.0));
    }
}
