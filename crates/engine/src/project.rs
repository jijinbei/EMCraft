//! The job as a whole (docs/ux/workflow.md): its frequency band, where each step of the workflow
//! stands, an estimate of the solve, the project file (the model and the commands that built
//! it), undo and redo, and the results' exports and checks.

use emcraft_model::units::Display;
use emcraft_model::{Band, Model};
use emcraft_solver::Complex64 as C;
use serde_json::{Value, json};

use crate::{EngineError, Params, Result, Session};

/// A state to undo to: the model, the display units and the history that built it.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub model: Model,
    pub display: Display,
    pub history: Vec<Value>,
    pub variables: crate::expr::Variables,
}

/// The most states kept to undo to.
pub const UNDO_DEPTH: usize = 50;

pub fn push_undo(stack: &mut Vec<Snapshot>, s: Snapshot) {
    stack.push(s);
    if stack.len() > UNDO_DEPTH {
        stack.remove(0);
    }
}

impl Session {
    /// The undoable state now, replaced by `with`.
    fn swap_snapshot(&mut self, with: Snapshot) -> Snapshot {
        Snapshot {
            model: std::mem::replace(&mut self.model, with.model),
            display: std::mem::replace(&mut self.display, with.display),
            history: std::mem::replace(&mut self.history, with.history),
            variables: std::mem::replace(&mut self.variables, with.variables),
        }
    }
}

/// What a command does to the job.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Changes the model: joins the history, can be undone.
    Record,
    /// Replaces the session (a sample, a project file): can be undone, brings its own history.
    Replace,
    /// Solves: its results belong to the model's current revision.
    Solve,
    None,
}

pub fn frequency(s: &mut Session, p: &Params) -> Result<Value> {
    let (start, stop) = (p.frequency("start").required()?, p.frequency("stop").required()?);
    s.model.band = Some(Band::new(start, stop)?);
    Ok(json!({"start": s.show_frequency(start), "stop": s.show_frequency(stop), "unit": s.display.frequency.symbol()}))
}

pub fn units(s: &mut Session, p: &Params) -> Result<Value> {
    if let Some(length) = p.choice("length").optional()? {
        s.display.length = length;
    }
    if let Some(frequency) = p.choice("frequency").optional()? {
        s.display.frequency = frequency;
    }
    Ok(json!({"length": s.display.length.symbol(), "frequency": s.display.frequency.symbol()}))
}

/// Replaces the session with a sample's model (and its results, when `solve`).
pub fn sample(s: &mut Session, p: &Params) -> Result<Value> {
    let name = p.string("name").required()?;
    let sample = crate::sample::find(name).ok_or_else(|| {
        p.error("name", crate::text::unknown(format_args!("names no sample: `{}`", crate::text::clip(name, 40)), "known: ", crate::sample::names()))
    })?;
    let solve = p.flag("solve").or(false)?;
    let script: Vec<Value> = serde_json::from_str(sample.script).map_err(|e| EngineError::Failed(format!("sample {name}: {e}")))?;
    let is_solve = |step: &Value| step.get("command").and_then(Value::as_str).is_some_and(|c| c.starts_with("solve."));
    let steps: Vec<Value> = script.into_iter().filter(|step| solve || !is_solve(step)).collect();
    let mut fresh = s.fresh();
    fresh.run_script(&Value::Array(steps))?;
    fresh.saved = fresh.history.clone();
    fresh.sample = Some(name.to_owned());
    *s = fresh;
    Ok(s.summary())
}

pub fn summary(s: &mut Session, _: &Params) -> Result<Value> {
    Ok(s.summary())
}

/// Where each step of the workflow stands ([`crate::workflow`]); no solve.
pub fn status(s: &mut Session, _: &Params) -> Result<Value> {
    let mut reply = json!({"steps": crate::workflow::steps(s), "revision": s.revision});
    if let Some(b) = s.model.band {
        reply["frequency"] = json!({"start": s.show_frequency(b.start), "stop": s.show_frequency(b.stop), "unit": s.display.frequency.symbol()});
    }
    Ok(reply)
}

/// Unknowns, the factor's memory and the time per frequency of a solve of the meshed model at
/// `order`: counted from the mesh's edges and faces, the factor's size scaled as n^(4/3) and
/// its time as n² from the session's own last sweep when it has one (else from a reference
/// machine's: the coaxial line's 45 000 unknowns, 24 million entries, 1.2 s).
pub fn estimate(s: &mut Session, p: &Params) -> Result<Value> {
    let order = p.integer("order", 1..=2).or(2)?;
    let mesh = s.model.mesh.as_ref().ok_or_else(|| EngineError::Failed("no mesh: mesh the model first (mesh.generate)".into()))?;
    let topo = emcraft_mesh::Topology::new(mesh);
    let (edges, faces) = (topo.edges.len() as f64, topo.faces.len() as f64);
    let unknowns = if order == 1 { edges } else { 2.0 * edges + 2.0 * faces };
    let (n0, entries0, seconds0, measured) =
        match s.results.sweep.as_ref().and_then(|sw| sw.results.iter().find(|r| r.timings.factor > 0.0).map(|r| (sw, r))) {
            Some((sw, r)) => (sw.driven.dofs.unknowns as f64, r.factor_entries as f64, r.timings.factor + r.timings.solve, true),
            None => (44_910.0, 24.0e6, 1.6, false),
        };
    let ratio = unknowns / n0.max(1.0);
    let entries = entries0 * ratio.powf(4.0 / 3.0);
    Ok(json!({
        "order": order,
        "unknowns": unknowns.round(),
        "memory_mb": (entries * 16.0 / 1.0e6).round(),
        "seconds_per_frequency": seconds0 * ratio * ratio,
        "from_this_session": measured,
    }))
}

/// Most bytes of a project file read.
const MAX_PROJECT_BYTES: u64 = 64 << 20;

/// The extension of project files.
pub const EXTENSION: &str = "emcraft";

pub fn save(s: &mut Session, p: &Params) -> Result<Value> {
    let path = match p.string("path").optional()? {
        Some(path) if path.trim().is_empty() => return Err(p.error("path", "must not be empty")),
        Some(path) if std::path::Path::new(path).extension().is_none() => format!("{path}.{EXTENSION}"),
        Some(path) => path.to_owned(),
        None => s.path.clone().ok_or_else(|| p.error("path", "is required: the project has no file yet"))?,
    };
    let doc = json!({"format": "emcraft-project", "version": 1, "history": s.history, "model": s.model});
    let text = serde_json::to_string_pretty(&doc).map_err(EngineError::failed)?;
    write_atomically(&path, text.as_bytes())?;
    s.path = Some(path.clone());
    s.saved = s.history.clone();
    Ok(json!({"path": path, "name": s.name(), "commands": s.history.len()}))
}

/// Writes `bytes` to a temporary file beside `path` and renames it over: a crash or a full
/// disk leaves the old file whole.
fn write_atomically(path: &str, bytes: &[u8]) -> Result<()> {
    let target = std::path::Path::new(path);
    let temp = target.with_extension(format!("{}.tmp", target.extension().and_then(|e| e.to_str()).unwrap_or(EXTENSION)));
    let fail = |e: std::io::Error| EngineError::cannot_write(path, e);
    std::fs::write(&temp, bytes).map_err(fail)?;
    std::fs::rename(&temp, target).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        fail(e)
    })
}

/// An empty project with the session's display units.
pub fn new(s: &mut Session, _: &Params) -> Result<Value> {
    let mut fresh = s.fresh();
    if s.display != fresh.display {
        let units = json!({"length": s.display.length.symbol(), "frequency": s.display.frequency.symbol()});
        fresh.execute("units.set", &units)?;
        fresh.saved = fresh.history.clone();
    }
    *s = Session { undo: std::mem::take(&mut s.undo), ..fresh };
    Ok(s.summary())
}

/// Opens a project: its history replayed in a fresh session (so the mesh comes back the same).
pub fn open(s: &mut Session, p: &Params) -> Result<Value> {
    let path = p.string("path").required()?;
    let bytes = crate::geometry::read_capped(path, MAX_PROJECT_BYTES)?;
    let doc: Value = serde_json::from_slice(&bytes).map_err(|e| EngineError::Failed(format!("`{path}`: not a project file ({e})")))?;
    if doc.get("format").and_then(Value::as_str) != Some("emcraft-project") {
        return Err(EngineError::Failed(format!("`{path}`: not an EMCraft project")));
    }
    let history = doc.get("history").cloned().unwrap_or(Value::Array(Vec::new()));
    let mut fresh = s.fresh();
    fresh.run_script(&history)?;
    // Undo inside the opened project, then back to before it.
    fresh.undo.clear();
    fresh.path = Some(path.to_owned());
    fresh.saved = fresh.history.clone();
    *s = Session { undo: std::mem::take(&mut s.undo), ..fresh };
    Ok(json!({"path": path, "name": s.name(), "commands": s.history.len(), "summary": s.summary()}))
}

pub fn undo(s: &mut Session, _: &Params) -> Result<Value> {
    let back = s.undo.pop().ok_or_else(|| EngineError::failed("nothing to undo"))?;
    let now = s.swap_snapshot(back);
    push_undo(&mut s.redo, now);
    // Results stay, marked stale.
    s.revision += 1;
    Ok(json!({"undo": s.undo.len(), "redo": s.redo.len()}))
}

pub fn redo(s: &mut Session, _: &Params) -> Result<Value> {
    let next = s.redo.pop().ok_or_else(|| EngineError::failed("nothing to redo"))?;
    let now = s.swap_snapshot(next);
    push_undo(&mut s.undo, now);
    s.revision += 1;
    Ok(json!({"undo": s.undo.len(), "redo": s.redo.len()}))
}

/// How a Touchstone file writes each complex value (`format`).
#[derive(Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Format {
    /// Real and imaginary parts.
    Ri,
    /// Magnitude and angle in degrees.
    Ma,
}

/// The sweep's S-parameters as a Touchstone (version 1) file: frequencies in GHz, `ri` (real,
/// imaginary) or `ma` (magnitude, angle in degrees), the reference impedance on the option line.
pub fn touchstone(s: &mut Session, p: &Params) -> Result<Value> {
    let path = p.string("path").required()?;
    let ma = p.choice("format").or(Format::Ri)? == Format::Ma;
    let sw = s.results.sweep()?;
    let entries = sw.entries();
    let n = entries.len();
    let refs: Vec<Option<f64>> = entries.iter().map(|e| e.reference).collect();
    let one = refs.first().copied().flatten().filter(|r| refs.iter().all(|x| *x == Some(*r)));
    let mut out = String::new();
    out.push_str("! EMCraft S-parameters\n");
    out.push_str(&format!("! ports: {}\n", entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>().join(", ")));
    if one.is_none() {
        out.push_str("! power waves of the ports' modes (no common reference impedance): R 50 is nominal\n");
    }
    out.push_str(&format!("# GHz S {} R {}\n", if ma { "MA" } else { "RI" }, one.unwrap_or(50.0)));
    let pair = |c: C| if ma { format!("{:.12e} {:.12e}", c.norm(), c.arg().to_degrees()) } else { format!("{:.12e} {:.12e}", c.re, c.im) };
    for r in &sw.results {
        let at = |i: usize, j: usize| r.s.get(i, j).unwrap_or_default();
        let f = format!("{:.12e}", r.frequency / 1e9);
        match n {
            1 => out.push_str(&format!("{f} {}\n", pair(at(0, 0)))),
            // Version 1's two-port order: S11 S21 S12 S22.
            2 => out.push_str(&format!("{f} {} {} {} {}\n", pair(at(0, 0)), pair(at(1, 0)), pair(at(0, 1)), pair(at(1, 1)))),
            _ => {
                // A row of the matrix per line, at most four values to a line (continued).
                for i in 0..n {
                    for (k, chunk) in (0..n).map(|j| pair(at(i, j))).collect::<Vec<_>>().chunks(4).enumerate() {
                        let lead = if i == 0 && k == 0 { format!("{f} ") } else { String::new() };
                        out.push_str(&format!("{lead}{}\n", chunk.join(" ")));
                    }
                }
            }
        }
    }
    std::fs::write(path, out).map_err(|e| EngineError::cannot_write(path, e))?;
    Ok(json!({"path": path, "ports": n, "frequencies": sw.results.len()}))
}

/// The sweep's physics checks: passivity (the largest singular value of S, ≤ 1 for a passive
/// structure), reciprocity (max |S − Sᵀ|) and unitarity (max |SᴴS − 1|, 0 for a lossless
/// structure), the worst over the frequencies.
pub fn check(s: &mut Session, _: &Params) -> Result<Value> {
    let sw = s.results.sweep()?;
    let worst = |f: fn(&emcraft_solver::SMatrix) -> f64| sw.results.iter().map(|r| f(&r.s)).fold(0.0, f64::max);
    let gain = worst(emcraft_solver::SMatrix::largest_singular_value);
    let asym = worst(emcraft_solver::SMatrix::reciprocity_error);
    let lossless = worst(emcraft_solver::SMatrix::unitarity_error);
    Ok(
        json!({"largest_singular_value": gain, "passive": gain <= 1.0 + 1e-9, "reciprocity_error": asym, "unitarity_error": lossless, "frequencies": sw.results.len()}),
    )
}
