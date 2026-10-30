//! The job as a whole (docs/ux/workflow.md): its frequency band, where each step of the workflow
//! stands, an estimate of the solve, the project file (the model and the commands that built
//! it), undo and redo, and the results' exports and checks.

use std::collections::VecDeque;

use emcraft_model::units::Display;
use emcraft_model::{Band, Model};
use emcraft_solver::Complex64 as C;
use serde_json::{Value, json};

use crate::runs::Run;
use crate::{EngineError, Params, Result, Session, Step};

/// A state to undo to: the model, the display units and the history that built it, and —
/// before a command that replaced the project (`file.open`, `file.new`, `file.sample`) — the
/// project the model belonged to.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub model: Model,
    pub display: Display,
    pub history: Vec<Step>,
    pub variables: crate::expr::Variables,
    /// The project to go back to with the model, when the command after this state replaced it.
    pub project: Option<Box<Identity>>,
}

/// What a project is besides its model: its file, the sample it came from, the state it was
/// last saved or opened in, its runs and their results. Undoing a command that replaced the
/// project brings it back whole, so a save after the undo writes the project's own file (never
/// the one opened over it) and the dirty mark is the project's own.
#[derive(Clone, Debug)]
pub struct Identity {
    pub path: Option<String>,
    pub sample: Option<String>,
    /// The history as last saved or opened.
    pub saved: Vec<Step>,
    /// The runs as last saved or opened.
    pub saved_runs: Vec<Run>,
    pub runs: Vec<Run>,
    pub results: crate::Results,
    /// The revision the results were solved at, and whether that was the model's revision then
    /// (they come back current; else stale).
    pub solved_revision: Option<u64>,
    pub current: bool,
}

/// The most states kept to undo to.
pub const UNDO_DEPTH: usize = 50;

/// Pushes a state to go back to, forgetting the oldest beyond [`UNDO_DEPTH`].
pub fn push_undo(stack: &mut VecDeque<Snapshot>, s: Snapshot) {
    stack.push_back(s);
    if stack.len() > UNDO_DEPTH {
        stack.pop_front();
    }
}

impl Session {
    /// The project the model belongs to, as it is now.
    pub(crate) fn identity(&self) -> Identity {
        Identity {
            path: self.path.clone(),
            sample: self.sample.clone(),
            saved: self.saved.clone(),
            saved_runs: self.saved_runs.clone(),
            runs: self.runs.clone(),
            results: self.results.clone(),
            solved_revision: self.solved_revision,
            current: self.solved_revision == Some(self.revision),
        }
    }

    /// The undoable state now, replaced by `with`; a new revision. The results stay, marked
    /// stale, unless `with` brings its project back: then that project's own come back.
    fn swap_snapshot(&mut self, with: Snapshot) -> Snapshot {
        let current = self.solved_revision == Some(self.revision);
        self.revision += 1;
        let project = with.project.map(|back| {
            let back = *back;
            let now = Identity {
                path: std::mem::replace(&mut self.path, back.path),
                sample: std::mem::replace(&mut self.sample, back.sample),
                saved: std::mem::replace(&mut self.saved, back.saved),
                saved_runs: std::mem::replace(&mut self.saved_runs, back.saved_runs),
                runs: std::mem::replace(&mut self.runs, back.runs),
                results: std::mem::replace(&mut self.results, back.results),
                solved_revision: self.solved_revision,
                current,
            };
            self.solved_revision = if back.current { Some(self.revision) } else { back.solved_revision };
            Box::new(now)
        });
        Snapshot {
            model: std::mem::replace(&mut self.model, with.model),
            display: std::mem::replace(&mut self.display, with.display),
            history: std::mem::replace(&mut self.history, with.history),
            variables: std::mem::replace(&mut self.variables, with.variables),
            project,
        }
    }
}

/// What a command does to the job.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Changes the model: joins the history, can be undone.
    Record,
    /// Replaces the project (a sample, a project file, a new one): can be undone, back to the
    /// project before it (its file, its saved state, its runs and results), and brings its own
    /// history.
    Replace,
    /// Rebuilds the model from a history of its own making (a variable changed, an
    /// optimization's best point): can be undone; the project stays the same.
    Rebuild,
    /// Solves: its results belong to the model's current revision.
    Solve,
    None,
}

pub fn frequency(s: &mut Session, p: &Params) -> Result<Value> {
    let (start, stop) = (p.frequency("start").required()?, p.frequency("stop").required()?);
    s.model.set_band(Some(Band::new(start, stop)?));
    Ok(json!({"start": s.show_frequency(start), "stop": s.show_frequency(stop), "unit": s.display.frequency.symbol()}))
}

pub fn units(s: &mut Session, p: &Params) -> Result<Value> {
    if let Some(length) = p.choice("length").optional()? {
        s.display.length = length;
    }
    if let Some(frequency) = p.choice("frequency").optional()? {
        s.display.frequency = frequency;
    }
    Ok(crate::reply::units(s.display))
}

/// Replaces the session with a sample's model (and its results, when `solve`).
pub fn sample(s: &mut Session, p: &Params) -> Result<Value> {
    let name = p.string("name").required()?;
    let sample = emcraft_templates::find(name).ok_or_else(|| {
        p.error("name", crate::text::unknown(format_args!("names no sample: `{}`", crate::text::clip(name, 40)), "known: ", emcraft_templates::names()))
    })?;
    let solve = p.flag("solve").or(false)?;
    let script: Vec<Step> = serde_json::from_str(sample.script).map_err(|e| EngineError::failed(format!("sample {name}: {e}")))?;
    let mut fresh = s.fresh();
    // Without `solve`, the sample's solves are recorded as its runs, not run.
    for (i, step) in script.iter().enumerate() {
        crate::progress::report(i, script.len(), &step.command)?;
        let spec =
            crate::find_command(&step.command).ok_or_else(|| EngineError::failed(format!("sample {name}: unknown command `{}`", step.command)))?;
        if spec.effect == Effect::Solve && !solve {
            let run = Run { command: step.command.clone(), params: step.params.clone(), units: fresh.display };
            crate::runs::record(&mut fresh.runs, run);
        } else {
            fresh.execute(&step.command, &step.params).map_err(|e| e.in_step(i + 1, &step.command))?;
        }
    }
    fresh.mark_saved();
    fresh.sample = Some(name.to_owned());
    // Undo goes back to before the sample, not through the steps that built it.
    fresh.undo.clear();
    s.replace_with(fresh);
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
    let mesh = s.model.mesh.as_ref().ok_or_else(|| EngineError::failed("no mesh: mesh the model first (mesh.generate)"))?;
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

/// A project file (docs/project-file.md): the history that builds the model, the runs solved
/// on it (their parameters, not their results), and the model for other tools to read (opening
/// replays the history and does not read it).
#[derive(serde::Serialize, serde::Deserialize)]
struct ProjectFile<'m> {
    format: String,
    #[serde(default = "first_version")]
    version: u32,
    #[serde(default)]
    history: Vec<Step>,
    /// Since version 2.
    #[serde(default)]
    runs: Vec<Run>,
    #[serde(skip_deserializing)]
    model: Option<&'m Model>,
}

/// What a project file's `format` says.
const FORMAT: &str = "emcraft-project";

/// The version of project files written, the newest read (2: the runs).
const VERSION: u32 = 2;

fn first_version() -> u32 {
    1
}

/// Most bytes of a project file read.
const MAX_PROJECT_BYTES: u64 = 64 << 20;

/// The extension of project files.
pub const EXTENSION: &str = "emcraft";

/// `file.save`: the project to `path` (or its own file); with `copy`, a copy that leaves the
/// session's file, name and saved state alone (an autosave).
pub fn save(s: &mut Session, p: &Params) -> Result<Value> {
    let copy = p.flag("copy").or(false)?;
    let path = match p.string("path").optional()? {
        Some(path) if path.trim().is_empty() => return Err(p.error("path", "must not be empty")),
        Some(path) if std::path::Path::new(path).extension().is_none() => format!("{path}.{EXTENSION}"),
        Some(path) => path.to_owned(),
        None if copy => return Err(p.error("path", "is required for a copy")),
        None => s.path.clone().ok_or_else(|| p.error("path", "is required: the project has no file yet"))?,
    };
    let file = ProjectFile { format: FORMAT.to_owned(), version: VERSION, history: s.history.clone(), runs: s.runs.clone(), model: Some(&s.model) };
    // Through a JSON value, whose objects keep their keys sorted: the file's text does not
    // depend on the order the fields are declared in.
    let doc = serde_json::to_value(&file).map_err(EngineError::failed)?;
    let text = serde_json::to_string_pretty(&doc).map_err(EngineError::failed)?;
    write_atomically(&path, text.as_bytes())?;
    if copy {
        return Ok(json!({"path": path, "copy": true, "commands": s.history.len(), "runs": s.runs.len()}));
    }
    s.path = Some(path.clone());
    s.mark_saved();
    Ok(json!({"path": path, "name": s.name(), "commands": s.history.len(), "runs": s.runs.len()}))
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
        fresh.execute("units.set", &crate::reply::units(s.display))?;
        fresh.mark_saved();
    }
    s.replace_with(fresh);
    Ok(s.summary())
}

/// A project file read and checked: its history and runs.
fn read_project(path: &str) -> Result<(Vec<Step>, Vec<Run>)> {
    let bytes = crate::geometry::read_capped(path, MAX_PROJECT_BYTES)?;
    let doc: Value = serde_json::from_slice(&bytes).map_err(|e| EngineError::failed(format!("`{path}`: not a project file ({e})")))?;
    if doc.get("format").and_then(Value::as_str) != Some(FORMAT) {
        return Err(EngineError::failed(format!("`{path}`: not an EMCraft project")));
    }
    let file: ProjectFile<'_> = serde_json::from_value(doc).map_err(|e| EngineError::failed(format!("`{path}`: not a project file ({e})")))?;
    if file.version > VERSION {
        return Err(EngineError::failed(format!(
            "`{path}`: a project of version {}, newer than this EMCraft reads ({VERSION}): open it with a newer EMCraft",
            file.version
        )));
    }
    let runs = crate::runs::checked(file.runs).map_err(|e| EngineError::failed(format!("`{path}`: {e}")))?;
    Ok((file.history, runs))
}

/// Opens a project: its history replayed in a fresh session (so the mesh comes back the same),
/// its runs restored without running them. With `original`, `path` is a copy of that project
/// (an autosave being recovered): the session takes the original's file, and is clean only
/// where it matches what that file holds.
pub fn open(s: &mut Session, p: &Params) -> Result<Value> {
    let path = p.string("path").required()?;
    let original = match p.string("original").optional()? {
        Some(o) if o.trim().is_empty() => return Err(p.error("original", "must not be empty")),
        o => o,
    };
    let (history, runs) = read_project(path)?;
    let mut fresh = s.fresh();
    fresh.run_steps(&history)?;
    // Undo inside the opened project, then back to before it.
    fresh.undo.clear();
    fresh.runs = runs;
    match original {
        None => {
            fresh.path = Some(path.to_owned());
            fresh.mark_saved();
        }
        Some(o) => {
            // The original as it was last saved: the copy may be ahead of it. One that cannot be
            // read is unlike anything recovered (a baseline no history equals).
            let (saved, saved_runs) = read_project(o).unwrap_or_else(|_| (vec![Step::new(UNREADABLE, json!({}))], Vec::new()));
            fresh.path = Some(o.to_owned());
            fresh.saved = saved;
            fresh.saved_runs = saved_runs;
        }
    }
    s.replace_with(fresh);
    Ok(json!({"path": s.path, "name": s.name(), "commands": s.history.len(), "runs": s.runs.len(), "dirty": s.is_dirty(), "summary": s.summary()}))
}

/// The saved baseline of a recovered project whose original cannot be read: no command has this
/// id, so no history equals it.
const UNREADABLE: &str = "(the original could not be read)";

pub fn undo(s: &mut Session, _: &Params) -> Result<Value> {
    let back = s.undo.pop_back().ok_or_else(|| EngineError::failed("nothing to undo"))?;
    let now = s.swap_snapshot(back);
    push_undo(&mut s.redo, now);
    Ok(stacks(s))
}

pub fn redo(s: &mut Session, _: &Params) -> Result<Value> {
    let next = s.redo.pop_back().ok_or_else(|| EngineError::failed("nothing to redo"))?;
    let now = s.swap_snapshot(next);
    push_undo(&mut s.undo, now);
    Ok(stacks(s))
}

/// Undo's and redo's reply: the states left on each side and the project now.
fn stacks(s: &Session) -> Value {
    json!({"undo": s.undo.len(), "redo": s.redo.len(), "path": s.path, "name": s.name(), "dirty": s.is_dirty()})
}

emcraft_macros::named_enum! {
    /// How a Touchstone file writes each complex value (`format`).
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum Format {
        /// Real and imaginary parts.
        Ri = "ri",
        /// Magnitude and angle in degrees.
        Ma = "ma",
    }
}

emcraft_macros::named_serde!(Format);

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
