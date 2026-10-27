//! EMCraft's command engine. Everything a user can do is a command ([`CommandSpec`]: a dotted,
//! lower-case id, a label, documented parameters and a JSON `run`); the desktop UI, scripts and
//! the command line all reach the same commands through [`Session::execute`], which checks the
//! parameters, runs the command under a panic guard and restores the session when it fails.
//!
//! Parameters and replies are in the session's display units (millimetres and gigahertz by
//! default, [`emcraft_model::units`]); the model and the solvers are in SI.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod adapt;
pub mod bodies;
mod cmd;
mod geometry;
mod meshing;
pub mod open;
pub mod params;
pub mod progress;
pub mod project;
mod results;
pub mod sample;
mod solve;
pub use solve::Compute;
pub mod view;
pub mod workflow;

use std::fmt;
use std::sync::Arc;

use emcraft_fem::Order;
use emcraft_model::Model;
use emcraft_model::units::Display;
use emcraft_solver::{Driven, FrequencyResult};
use serde_json::{Value, json};

pub use cmd::COMMANDS;
pub use params::Params;
pub use project::Effect;
pub use results::field_view;

#[derive(Clone, Debug, thiserror::Error, PartialEq)]
pub enum EngineError {
    #[error("unknown command `{0}`")]
    UnknownCommand(String),
    #[error("{0}")]
    Params(String),
    #[error("{0}")]
    Failed(String),
    #[error("internal error in `{0}` (the session was restored): please report it")]
    Panicked(String),
}

pub type Result<T> = std::result::Result<T, EngineError>;

impl EngineError {
    /// A command that could not do its work, for `why`.
    pub fn failed(why: impl fmt::Display) -> EngineError {
        EngineError::Failed(why.to_string())
    }
}

/// The lower layers' errors: the command failed for the reason they give.
macro_rules! failed_from {
    ($($error:ty),* $(,)?) => {$(
        impl From<$error> for EngineError {
            fn from(e: $error) -> Self {
                EngineError::failed(e)
            }
        }
    )*};
}

failed_from!(
    emcraft_model::ModelError,
    emcraft_mesh::MeshError,
    emcraft_fdtd::FdtdError,
    emcraft_solver::SolveError,
    emcraft_theory::TheoryError,
    emcraft_io::Error,
);

/// The physical quantity of a numeric parameter, which decides its display unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quantity {
    /// A plain number.
    None,
    Length,
    Frequency,
}

impl Quantity {
    /// The unit's kind as the command reference names it ("" for a plain number).
    pub const fn name(self) -> &'static str {
        match self {
            Quantity::None => "",
            Quantity::Length => "length",
            Quantity::Frequency => "frequency",
        }
    }
}

/// The JSON type of a parameter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Number,
    Integer,
    String,
    Array,
    Object,
    Boolean,
}

impl Kind {
    pub const fn name(self) -> &'static str {
        match self {
            Kind::Number => "number",
            Kind::Integer => "integer",
            Kind::String => "string",
            Kind::Array => "array",
            Kind::Object => "object",
            Kind::Boolean => "boolean",
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A command parameter.
#[derive(Clone, Copy, Debug)]
pub struct ParamSpec {
    pub name: &'static str,
    pub kind: Kind,
    pub quantity: Quantity,
    pub required: bool,
    pub doc: &'static str,
    /// The values a string parameter may take, when it is a choice.
    pub choices: &'static [&'static str],
}

impl ParamSpec {
    /// A parameter every call must give.
    pub const fn required(name: &'static str, kind: Kind, doc: &'static str) -> ParamSpec {
        ParamSpec { name, kind, quantity: Quantity::None, required: true, doc, choices: &[] }
    }

    /// A parameter calls may leave out.
    pub const fn optional(name: &'static str, kind: Kind, doc: &'static str) -> ParamSpec {
        ParamSpec { required: false, ..ParamSpec::required(name, kind, doc) }
    }

    /// The parameter as a physical quantity, read in the session's display unit for it.
    pub const fn of(self, quantity: Quantity) -> ParamSpec {
        ParamSpec { quantity, ..self }
    }

    /// The parameter as a choice among `choices`.
    pub const fn one_of(self, choices: &'static [&'static str]) -> ParamSpec {
        ParamSpec { choices, ..self }
    }
}

/// A command.
pub struct CommandSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub doc: &'static str,
    pub params: &'static [ParamSpec],
    /// What it does to the job: whether it joins the history, can be undone, or solves.
    pub effect: Effect,
    pub run: fn(&mut Session, &Params) -> Result<Value>,
}

impl fmt::Debug for CommandSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandSpec").field("id", &self.id).field("effect", &self.effect).finish_non_exhaustive()
    }
}

/// The most eigenmodes one command may ask for.
pub(crate) const EIGEN_MAX: usize = emcraft_solver::eigen::MAX_MODES;

pub fn find_command(id: &str) -> Option<&'static CommandSpec> {
    COMMANDS.iter().find(|c| c.id == id)
}

/// A frequency sweep's results.
#[derive(Clone, Debug)]
pub struct Sweep {
    pub order: Order,
    pub driven: Arc<Driven>,
    /// Ascending frequency.
    pub results: Vec<FrequencyResult>,
}

impl Sweep {
    /// The result at a frequency (Hz) solved within 10⁻⁹ of it, or the nearest when `nearest`.
    pub fn at(&self, frequency: f64, nearest: bool) -> Option<&FrequencyResult> {
        let best = self.results.iter().min_by(|a, b| (a.frequency - frequency).abs().total_cmp(&(b.frequency - frequency).abs()))?;
        if nearest || (best.frequency - frequency).abs() <= 1e-9 * frequency.abs() { Some(best) } else { None }
    }
}

/// What the solvers made of the model, each from its own command.
#[derive(Clone, Debug, Default)]
pub struct Results {
    /// The driven frequency sweep from `solve.frequency`.
    pub sweep: Option<Sweep>,
    /// The cavity's eigenmodes from `solve.eigenmodes`.
    pub eigen: Option<Arc<emcraft_solver::eigen::Eigen>>,
    /// The time-domain solver's S-parameters from `solve.time_domain`.
    pub time_domain: Option<emcraft_fdtd::TdResult>,
    /// The open problem from `solve.radiation`.
    pub radiation: Option<Arc<open::Radiated>>,
    /// The open problem from `solve.time_domain` on an open model.
    pub td_radiation: Option<Arc<emcraft_fdtd::open::OpenRun>>,
}

/// What changed in the model, which decides the results that no longer hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    /// The geometry, the mesh or a symmetry plane: every result was solved on another model.
    Geometry,
    /// The walls' conductivity: the sweep and the eigenmodes' Q.
    Walls,
    /// A waveguide port: the sweep and the time domain's S-parameters.
    Ports,
    /// The open boundary, a source or a lumped port: the open problem's results.
    Open,
}

impl Results {
    /// Drops the results a change to the model leaves wrong. Changes that leave them right
    /// but old (the band, the units) keep them, marked stale by the workflow.
    pub fn invalidate(&mut self, change: Change) {
        match change {
            Change::Geometry => *self = Results::default(),
            Change::Walls => {
                self.sweep = None;
                self.eigen = None;
            }
            Change::Ports => {
                self.sweep = None;
                self.time_domain = None;
            }
            Change::Open => {
                self.radiation = None;
                self.td_radiation = None;
            }
        }
    }

    /// Whether any solver has run.
    pub fn any(&self) -> bool {
        self.sweep.is_some() || self.eigen.is_some() || self.time_domain.is_some() || self.radiation.is_some() || self.td_radiation.is_some()
    }

    /// The sweep, or an error asking for one.
    pub fn sweep(&self) -> Result<&Sweep> {
        self.sweep.as_ref().ok_or_else(|| EngineError::failed("no results: solve first (solve.frequency)"))
    }
}

/// Limits on the work one command may ask for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
    /// Frequencies in one sweep.
    pub frequencies: usize,
    /// Pixels on a picture's side.
    pub picture_side: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { frequencies: emcraft_solver::driven::MAX_FREQUENCIES, picture_side: 4096 }
    }
}

/// A session: the model, its results and the display units.
#[derive(Clone, Debug, Default)]
pub struct Session {
    pub model: Model,
    pub results: Results,
    pub display: Display,
    pub limits: Limits,
    /// The commands that built the model, in order (a script that rebuilds it).
    pub history: Vec<Value>,
    /// States to go back to (`edit.undo`) and forward again (`edit.redo`).
    pub undo: Vec<project::Snapshot>,
    pub redo: Vec<project::Snapshot>,
    /// The model's revision, raised by every command that changes it, and the revision the
    /// latest results were solved at (older results are stale).
    pub revision: u64,
    pub solved_revision: Option<u64>,
    /// The project file the session was opened from or last saved to (`.emcraft`).
    pub path: Option<String>,
    /// The sample the session was opened from, which names it until it is saved.
    pub sample: Option<String>,
    /// The history as last saved or opened: the session has unsaved changes when its history
    /// differs (an undo back to it is clean again).
    pub saved: Vec<Value>,
    /// Where the solvers run (`solve.device`).
    pub compute: solve::Compute,
}

impl Session {
    pub fn new() -> Session {
        Session::default()
    }

    /// A fresh session with this one's limits.
    pub(crate) fn fresh(&self) -> Session {
        Session { limits: self.limits, compute: self.compute.clone(), ..Session::default() }
    }

    /// Whether the model changed since it was last saved or opened.
    pub fn is_dirty(&self) -> bool {
        self.history != self.saved
    }

    /// The project's name: its file's stem, the sample's name, or "Untitled".
    pub fn name(&self) -> String {
        match self.path.as_deref().and_then(|p| std::path::Path::new(p).file_stem()) {
            Some(stem) => stem.to_string_lossy().into_owned(),
            None => self.sample.clone().unwrap_or_else(|| "Untitled".to_owned()),
        }
    }

    /// Runs a command. Unknown parameters are refused; a failed or panicking command leaves the
    /// session as it was.
    pub fn execute(&mut self, id: &str, params: &Value) -> Result<Value> {
        // An id from hostile input is not echoed in full.
        let spec = find_command(id).ok_or_else(|| EngineError::UnknownCommand(id.chars().take(100).collect()))?;
        check_keys(spec, params)?;
        let before = project::Snapshot { model: self.model.clone(), display: self.display, history: Vec::new() };
        let results_before = self.results.clone();
        let (history_before, revision_before) = (if spec.effect == Effect::None { Vec::new() } else { self.history.clone() }, self.revision);
        let reader = Params::new(params, self.display);
        let accelerator = self.compute.accelerator();
        let outcome =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| emcraft_solver::accel::with(accelerator, || (spec.run)(self, &reader))));
        let error = match outcome {
            Ok(Ok(reply)) => {
                match spec.effect {
                    // The state before it, to undo to; the command joins the history.
                    Effect::Record | Effect::Replace => {
                        project::push_undo(&mut self.undo, project::Snapshot { history: history_before, ..before });
                        self.redo.clear();
                        if spec.effect == Effect::Record {
                            self.history.push(json!({"command": spec.id, "params": params}));
                        }
                        // (A replaced session counts its own revisions from zero.)
                        self.revision = revision_before.max(self.revision) + 1;
                    }
                    Effect::Solve => self.solved_revision = Some(self.revision),
                    Effect::None => {}
                }
                return Ok(reply);
            }
            Ok(Err(e)) => e,
            Err(_) => EngineError::Panicked(spec.id.to_owned()),
        };
        self.model = before.model;
        self.display = before.display;
        self.results = results_before;
        Err(error)
    }

    /// Runs a script: a JSON array of `{"command", "params"}` steps; stops at the first error.
    /// Returns each step's reply.
    pub fn run_script(&mut self, script: &Value) -> Result<Vec<Value>> {
        let steps = script.as_array().ok_or_else(|| EngineError::Params("a script is a JSON array of {\"command\", \"params\"} steps".into()))?;
        steps
            .iter()
            .enumerate()
            .map(|(i, step)| {
                let id = step.get("command").and_then(Value::as_str).ok_or_else(|| EngineError::Params(format!("step {}: no `command`", i + 1)))?;
                let params = step.get("params").cloned().unwrap_or_else(|| json!({}));
                progress::report(i, steps.len(), id)?;
                self.execute(id, &params).map_err(|e| EngineError::Failed(format!("step {} ({id}): {e}", i + 1)))
            })
            .collect()
    }

    /// A frequency (Hz) as a value in display units.
    pub fn show_frequency(&self, hz: f64) -> f64 {
        self.display.frequency.from_si(hz)
    }

    pub fn show_length(&self, m: f64) -> f64 {
        self.display.length.from_si(m)
    }

    /// A short summary of the model and results.
    pub fn summary(&self) -> Value {
        let l = self.display.length;
        let w = self.model.waveguide.as_ref().map(|w| {
            json!({
                "width": l.from_si(w.width), "height": l.from_si(w.height), "length": l.from_si(w.length()),
                "sections": w.sections.iter().enumerate().map(|(i, s)| {
                    let a = w.aperture(i).unwrap_or(w.across_x());
                    json!({"length": l.from_si(s.length), "material": s.material, "aperture": [l.from_si(a.start), l.from_si(a.width)]})
                }).collect::<Vec<_>>(),
            })
        });
        json!({
            "units": {"length": l.symbol(), "frequency": self.display.frequency.symbol()},
            "materials": self.model.materials.iter().map(|m| json!({"name": m.name, "eps_r": m.eps_r, "mu_r": m.mu_r})).collect::<Vec<_>>(),
            "waveguide": w,
            "solid": self.model.solid.as_ref().map(|s| json!({"shape": s.shape.kind(), "material": s.material})),
            "ports": self.model.ports.iter().map(|p| json!({"name": p.name, "end": p.end, "modes": p.modes})).collect::<Vec<_>>(),
            "mesh": self.model.mesh.as_ref().map(|m| json!({"nodes": m.nodes.len(), "tetrahedra": m.tets.len()})),
            "results": self.results.sweep.as_ref().map(|s| json!({"order": s.order.as_u8(), "unknowns": s.driven.dofs.unknowns, "frequencies": s.results.len()})),
            "eigenmodes": self.results.eigen.as_ref().map(|e| e.modes.len()),
        })
    }
}

fn check_keys(spec: &CommandSpec, params: &Value) -> Result<()> {
    let obj = match params {
        Value::Object(o) => o,
        Value::Null => return Ok(()),
        _ => return Err(EngineError::Params(format!("{}: parameters must be a JSON object", spec.id))),
    };
    if let Some(k) = obj.keys().find(|k| !spec.params.iter().any(|p| p.name == k.as_str())) {
        let known: Vec<&str> = spec.params.iter().map(|p| p.name).collect();
        return Err(EngineError::Params(format!(
            "{}: unknown parameter `{}` (known: {})",
            spec.id,
            k.chars().take(64).collect::<String>(),
            known.join(", ")
        )));
    }
    match spec.params.iter().find(|p| p.required && !obj.contains_key(p.name)) {
        Some(p) => Err(EngineError::Params(format!("{}: missing parameter `{}`", spec.id, p.name))),
        None => Ok(()),
    }
}

/// Every command as a Markdown page (docs/features.md).
pub fn features_markdown() -> String {
    let mut s = String::from(
        "# Commands\n\nGenerated by `cargo xtask features` (`emcraft-cli features`). Lengths are in millimetres and frequencies in gigahertz unless the session's display units say otherwise.\n",
    );
    for c in COMMANDS {
        s.push_str(&format!("\n## `{}` — {}\n\n{}\n\n", c.id, c.label, c.doc));
        if c.params.is_empty() {
            s.push_str("No parameters.\n");
            continue;
        }
        s.push_str("| Parameter | Type | Unit | Required | Description |\n|---|---|---|---|---|\n");
        for p in c.params {
            let required = if p.required { "yes" } else { "no" };
            s.push_str(&format!("| `{}` | {} | {} | {} | {} |\n", p.name, p.kind, p.quantity.name(), required, p.doc));
        }
    }
    s
}

#[cfg(test)]
mod tests;
