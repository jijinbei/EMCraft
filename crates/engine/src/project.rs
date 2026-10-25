//! The job as a whole (docs/ux/workflow.md): its frequency band, where each step of the workflow
//! stands, an estimate of the solve, the project file (the model and the commands that built
//! it), undo and redo, and the results' exports and checks.

use emcraft_model::units::{C0, Display};
use emcraft_model::{Band, Model};
use emcraft_solver::Complex64 as C;
use serde_json::{Value, json};

use crate::params::{integer, required_string, string};
use crate::{EngineError, Result, Session};

/// A state to undo to: the model, the display units and the history that built it.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub model: Model,
    pub display: Display,
    pub history: Vec<Value>,
}

/// The most states kept to undo to.
pub const UNDO_DEPTH: usize = 50;

pub fn push_undo(stack: &mut Vec<Snapshot>, s: Snapshot) {
    stack.push(s);
    if stack.len() > UNDO_DEPTH {
        stack.remove(0);
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

pub fn effect(id: &str) -> Effect {
    match id {
        "file.sample" | "file.open" => Effect::Replace,
        "model.summary" => Effect::None,
        "project.frequency" => Effect::Record,
        _ if ["units.", "model.", "geometry.", "mesh.", "source."].iter().any(|p| id.starts_with(p)) => Effect::Record,
        _ if id.starts_with("solve.") && id != "solve.port_modes" => Effect::Solve,
        _ => Effect::None,
    }
}

pub fn frequency(s: &mut Session, p: &Value) -> Result<Value> {
    let start = s.frequency(p, "start")?.ok_or_else(|| EngineError::Params("`start` is required".into()))?;
    let stop = s.frequency(p, "stop")?.ok_or_else(|| EngineError::Params("`stop` is required".into()))?;
    s.model.band = Some(Band::new(start, stop)?);
    Ok(json!({"start": s.show_frequency(start), "stop": s.show_frequency(stop), "unit": s.display.frequency.symbol()}))
}

/// One step's standing and what to do about it.
struct Step {
    name: &'static str,
    status: &'static str,
    messages: Vec<Value>,
}

impl Step {
    fn new(name: &'static str) -> Step {
        Step { name, status: "done", messages: Vec::new() }
    }

    /// Lowers the status to `status` (missing < stale < warning < done) with a message.
    fn note(&mut self, status: &'static str, text: impl Into<String>, command: Option<&str>) {
        let rank = |s: &str| match s {
            "missing" => 0,
            "stale" => 1,
            "warning" => 2,
            _ => 3,
        };
        if rank(status) < rank(self.status) {
            self.status = status;
        }
        let mut m = json!({"text": text.into()});
        if let Some(c) = command {
            m["command"] = json!(c);
        }
        self.messages.push(m);
    }
}

/// Where each step of the workflow stands, from the model, the mesh and the results; no solve.
pub fn status(s: &mut Session, _: &Value) -> Result<Value> {
    let m = &s.model;
    let band = m.band;
    let mut intent = Step::new("intent");
    if band.is_none() {
        intent.note(
            "warning",
            "no frequency band: set the job's band first, it sets the mesh, the sweep and the port previews",
            Some("project.frequency"),
        );
    }
    let mut geometry = Step::new("geometry");
    let has_geometry = m.waveguide.is_some() || m.solid.is_some();
    if !has_geometry {
        geometry.note("missing", "no geometry: a waveguide, a solid or an imported surface", Some("geometry.waveguide"));
    }
    let mut materials = Step::new("materials");
    if !has_geometry {
        materials.note("missing", "no geometry to give materials to", Some("geometry.solid"));
    }
    if m.ports.iter().any(|p| m.port_material(p.end).is_ok_and(|x| x.loss_tangent != 0.0)) {
        materials.note(
            "warning",
            "a port lies in a lossy material, which ports do not support: end the guide with a lossless section",
            Some("geometry.waveguide"),
        );
    }
    let mut excitation = Step::new("excitation");
    let open = m.open.is_some();
    if m.ports.is_empty() && m.lumped.is_empty() && m.sources.is_empty() {
        excitation.note("missing", "nothing drives the model: a waveguide port, a lumped port or a source", Some("model.port.create"));
    }
    if !open && m.ports.len() == 1 && m.sources.is_empty() && m.lumped.is_empty() {
        excitation.note("warning", "one port: S11 only (a closed guide usually has two)", Some("model.port.create"));
    }
    // A closed-form port's first mode against the band's start.
    if let Some(b) = band {
        for p in &m.ports {
            if let (Ok(emcraft_model::PortShape::Rect { a, .. }), Ok(mat)) = (m.port_shape_of(p), m.port_material(p.end)) {
                let cutoff = C0 / (2.0 * a * (mat.eps_r * mat.mu_r).sqrt());
                if b.start <= cutoff {
                    excitation.note(
                        "warning",
                        format!(
                            "port `{}`: the band starts at or below its TE10 cut-off ({:.4} {}): no power enters there",
                            p.name,
                            s.show_frequency(cutoff),
                            s.display.frequency.symbol()
                        ),
                        Some("project.frequency"),
                    );
                }
            }
        }
    }
    let mut boundaries = Step::new("boundaries");
    if !open && (!m.sources.is_empty() || !m.lumped.is_empty()) {
        boundaries.note(
            "warning",
            "sources in a closed box: they radiate into its walls; line it with the open boundary for an antenna",
            Some("model.open"),
        );
    }
    if open && m.ports.iter().any(|p| p.face.is_none()) {
        boundaries.note(
            "warning",
            "a port on an end face of an open box: ports inside an open problem go on faces (`face`)",
            Some("model.port.create"),
        );
    }
    let mut mesh = Step::new("mesh");
    match &m.mesh {
        None => mesh.note("missing", "no mesh", Some("mesh.generate")),
        Some(me) => {
            // Elements per wavelength at the band's top in the densest material.
            if let Some(b) = band {
                let index = m.materials.iter().map(|x| (x.eps_r * x.mu_r).sqrt()).fold(1.0, f64::max);
                let lambda = C0 / (b.stop * index);
                let volume: f64 = (0..me.tets.len()).filter_map(|t| me.volume(t)).sum();
                // The edge of a regular tetrahedron of the mean volume.
                let h = (6.0 * std::f64::consts::SQRT_2 * volume / me.tets.len().max(1) as f64).cbrt();
                if h > lambda / 4.0 {
                    mesh.note(
                        "warning",
                        format!(
                            "elements of about {:.3} {} are coarse for the band's top (λ/4 = {:.3} {} in the densest material; second-order elements want about λ/6)",
                            s.show_length(h),
                            s.display.length.symbol(),
                            s.show_length(lambda / 4.0),
                            s.display.length.symbol()
                        ),
                        Some("mesh.generate"),
                    );
                }
            }
        }
    }
    let mut setup = Step::new("setup");
    if band.is_none() {
        setup.note("warning", "no band to sweep: give the sweep's frequencies when running, or set the band", Some("project.frequency"));
    }
    let solved = s.sweep.is_some() || s.eigen.is_some() || s.time_domain.is_some() || s.radiation.is_some() || s.td_radiation.is_some();
    let mut run = Step::new("run");
    let mut results = Step::new("results");
    let blocking = [&geometry, &excitation, &mesh].iter().any(|x| x.status == "missing");
    if blocking {
        run.note("missing", "the steps above marked missing must be done first", None);
    }
    if !solved {
        run.note("missing", "not run yet", Some("solve.frequency"));
        results.note("missing", "no results yet", Some("solve.frequency"));
    } else if s.solved_revision.is_some_and(|r| r < s.revision) {
        results.note("stale", "the model changed since these results were solved: run again", Some("solve.frequency"));
        run.note("stale", "the model changed since the last run", Some("solve.frequency"));
    }
    let steps: Vec<Value> = [intent, geometry, materials, excitation, boundaries, mesh, setup, run, results]
        .into_iter()
        .map(|x| json!({"step": x.name, "status": x.status, "messages": x.messages}))
        .collect();
    let mut reply = json!({"steps": steps, "revision": s.revision});
    if let Some(b) = band {
        reply["frequency"] = json!({"start": s.show_frequency(b.start), "stop": s.show_frequency(b.stop), "unit": s.display.frequency.symbol()});
    }
    Ok(reply)
}

/// Unknowns, the factor's memory and the time per frequency of a solve of the meshed model at
/// `order`: counted from the mesh's edges and faces, the factor's size scaled as n^(4/3) and
/// its time as n² from the session's own last sweep when it has one (else from a reference
/// machine's: the coaxial line's 45 000 unknowns, 24 million entries, 1.2 s).
pub fn estimate(s: &mut Session, p: &Value) -> Result<Value> {
    let order = integer(p, "order", 1..=2)?.unwrap_or(2);
    let mesh = s.model.mesh.as_ref().ok_or_else(|| EngineError::Failed("no mesh: mesh the model first (mesh.generate)".into()))?;
    let topo = emcraft_mesh::Topology::new(mesh);
    let (edges, faces) = (topo.edges.len() as f64, topo.faces.len() as f64);
    let unknowns = if order == 1 { edges } else { 2.0 * edges + 2.0 * faces };
    let (n0, entries0, seconds0, measured) = match s.sweep.as_ref().and_then(|sw| sw.results.iter().find(|r| r.timings.factor > 0.0).map(|r| (sw, r)))
    {
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

pub fn save(s: &mut Session, p: &Value) -> Result<Value> {
    let path = required_string(p, "path")?;
    let doc = json!({"format": "emcraft-project", "version": 1, "history": s.history, "model": s.model});
    let text = serde_json::to_string_pretty(&doc).map_err(|e| EngineError::Failed(e.to_string()))?;
    std::fs::write(path, text).map_err(|e| EngineError::Failed(format!("cannot write {path}: {e}")))?;
    Ok(json!({"path": path, "commands": s.history.len()}))
}

/// Opens a project: its history replayed in a fresh session (so the mesh comes back the same).
pub fn open(s: &mut Session, p: &Value) -> Result<Value> {
    let path = required_string(p, "path")?;
    let len = std::fs::metadata(path).map_err(|e| EngineError::Failed(format!("`{path}`: {e}")))?.len();
    if len > MAX_PROJECT_BYTES {
        return Err(EngineError::Failed(format!("`{path}` is {} MB; at most {} MB are read", len >> 20, MAX_PROJECT_BYTES >> 20)));
    }
    let text = std::fs::read_to_string(path).map_err(|e| EngineError::Failed(format!("`{path}`: {e}")))?;
    let doc: Value = serde_json::from_str(&text).map_err(|e| EngineError::Failed(format!("`{path}`: not a project file ({e})")))?;
    if doc.get("format").and_then(Value::as_str) != Some("emcraft-project") {
        return Err(EngineError::Failed(format!("`{path}`: not an EMCraft project")));
    }
    let history = doc.get("history").cloned().unwrap_or(Value::Array(Vec::new()));
    let mut fresh = Session { limits: s.limits, ..Session::new() };
    fresh.run_script(&history)?;
    // Undo inside the opened project, then back to before it.
    fresh.undo.clear();
    *s = Session { undo: std::mem::take(&mut s.undo), ..fresh };
    Ok(json!({"path": path, "commands": s.history.len(), "summary": s.summary()}))
}

pub fn undo(s: &mut Session, _: &Value) -> Result<Value> {
    let back = s.undo.pop().ok_or_else(|| EngineError::Failed("nothing to undo".into()))?;
    let now = Snapshot {
        model: std::mem::replace(&mut s.model, back.model),
        display: std::mem::replace(&mut s.display, back.display),
        history: std::mem::replace(&mut s.history, back.history),
    };
    push_undo(&mut s.redo, now);
    // Results stay, marked stale.
    s.revision += 1;
    Ok(json!({"undo": s.undo.len(), "redo": s.redo.len()}))
}

pub fn redo(s: &mut Session, _: &Value) -> Result<Value> {
    let next = s.redo.pop().ok_or_else(|| EngineError::Failed("nothing to redo".into()))?;
    let now = Snapshot {
        model: std::mem::replace(&mut s.model, next.model),
        display: std::mem::replace(&mut s.display, next.display),
        history: std::mem::replace(&mut s.history, next.history),
    };
    push_undo(&mut s.undo, now);
    s.revision += 1;
    Ok(json!({"undo": s.undo.len(), "redo": s.redo.len()}))
}

/// The sweep's S-parameters as a Touchstone (version 1) file: frequencies in GHz, `ri` (real,
/// imaginary) or `ma` (magnitude, angle in degrees), the reference impedance on the option line.
pub fn touchstone(s: &mut Session, p: &Value) -> Result<Value> {
    let path = required_string(p, "path")?;
    let ma = match string(p, "format")?.unwrap_or("ri") {
        "ri" => false,
        "ma" => true,
        _ => return Err(EngineError::Params("`format` must be ri or ma".into())),
    };
    let sw = s.sweep.as_ref().ok_or_else(|| EngineError::Failed("no sweep: run solve.frequency first".into()))?;
    let n = s.model.ports.len();
    let refs: Vec<Option<f64>> = s.model.ports.iter().map(|p| p.reference).collect();
    let one = refs.first().copied().flatten().filter(|r| refs.iter().all(|x| *x == Some(*r)));
    let mut out = String::new();
    out.push_str("! EMCraft S-parameters\n");
    out.push_str(&format!("! ports: {}\n", s.model.ports.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", ")));
    if one.is_none() {
        out.push_str("! power waves of the ports' modes (no common reference impedance): R 50 is nominal\n");
    }
    out.push_str(&format!("# GHz S {} R {}\n", if ma { "MA" } else { "RI" }, one.unwrap_or(50.0)));
    let pair = |c: C| if ma { format!("{:.12e} {:.12e}", c.norm(), c.arg().to_degrees()) } else { format!("{:.12e} {:.12e}", c.re, c.im) };
    for r in &sw.results {
        let at = |i: usize, j: usize| r.s.get(i).and_then(|row| row.get(j)).copied().unwrap_or_default();
        let f = format!("{:.12e}", r.frequency / 1e9);
        match n {
            1 => out.push_str(&format!("{f} {}\n", pair(at(0, 0)))),
            // Version 1's two-port order: S11 S21 S12 S22.
            2 => out.push_str(&format!("{f} {} {} {} {}\n", pair(at(0, 0)), pair(at(1, 0)), pair(at(0, 1)), pair(at(1, 1)))),
            _ => {
                for i in 0..n {
                    let row: Vec<String> = (0..n).map(|j| pair(at(i, j))).collect();
                    out.push_str(&format!("{}{}\n", if i == 0 { format!("{f} ") } else { String::new() }, row.join(" ")));
                }
            }
        }
    }
    std::fs::write(path, out).map_err(|e| EngineError::Failed(format!("cannot write {path}: {e}")))?;
    Ok(json!({"path": path, "ports": n, "frequencies": sw.results.len()}))
}

/// The sweep's physics checks: passivity (the largest singular value of S, ≤ 1 for a passive
/// structure) and reciprocity (max |S − Sᵀ|), the worst over the frequencies.
pub fn check(s: &mut Session, _: &Value) -> Result<Value> {
    let sw = s.sweep.as_ref().ok_or_else(|| EngineError::Failed("no sweep: run solve.frequency first".into()))?;
    let (mut gain, mut asym) = (0.0f64, 0.0f64);
    for r in &sw.results {
        let n = r.s.len();
        // σ_max² by power iteration on SᴴS (n is the port count).
        let mut v = vec![C::new(1.0, 0.0); n];
        let mut sigma2 = 0.0;
        for _ in 0..200 {
            let sv: Vec<C> = (0..n).map(|i| (0..n).map(|j| r.s[i][j] * v[j]).sum()).collect();
            let w: Vec<C> = (0..n).map(|j| (0..n).map(|i| r.s[i][j].conj() * sv[i]).sum()).collect();
            let norm = w.iter().map(|c| c.norm_sqr()).sum::<f64>().sqrt();
            if !(norm > 0.0) {
                break;
            }
            let vn = v.iter().map(|c| c.norm_sqr()).sum::<f64>().sqrt();
            sigma2 = norm / vn.max(f64::MIN_POSITIVE);
            v = w.into_iter().map(|c| c / norm).collect();
        }
        gain = gain.max(sigma2.sqrt());
        for i in 0..n {
            for j in 0..n {
                asym = asym.max((r.s[i][j] - r.s[j][i]).norm());
            }
        }
    }
    Ok(json!({"largest_singular_value": gain, "passive": gain <= 1.0 + 1e-9, "reciprocity_error": asym, "frequencies": sw.results.len()}))
}
