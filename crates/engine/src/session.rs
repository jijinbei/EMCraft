//! The session: the model, its results, the display units, the history that built the model and
//! undo; [`Session::execute`] runs every command.

use std::sync::Arc;

use emcraft_fem::Order;
use emcraft_model::Model;
use emcraft_model::units::Display;
use emcraft_solver::{Driven, FrequencyResult};
use serde_json::{Value, json};

use crate::cmd::{CommandSpec, check_keys, find_command};
use crate::project::{self, Effect};
use crate::{EngineError, Params, Result, expr, open, progress, solve, study};

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
    /// The last parameter study (`study.sweep`, `study.optimize`): solved on models rebuilt
    /// from the history, so no change to the model now makes it wrong.
    pub study: Option<Arc<study::Study>>,
    /// A periodic cell's dispersion diagram from `solve.dispersion`.
    pub dispersion: Option<Arc<emcraft_solver::bloch::Dispersion>>,
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
            Change::Geometry => *self = Results { study: self.study.take(), ..Results::default() },
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
        self.sweep.is_some()
            || self.eigen.is_some()
            || self.time_domain.is_some()
            || self.radiation.is_some()
            || self.td_radiation.is_some()
            || self.dispersion.is_some()
    }

    /// The sweep, or an error asking for one.
    pub fn sweep(&self) -> Result<&Sweep> {
        self.sweep.as_ref().ok_or_else(|| EngineError::failed("no results: solve first (solve.frequency)"))
    }
}

/// The session as it was before a command ran: to undo to when it succeeds, to go back to when
/// it fails.
struct Before {
    /// The model, units and variables, and the history (for commands that change the job).
    state: project::Snapshot,
    results: Results,
    revision: u64,
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
    /// The named variables expressions in parameters read (`project.variable`), defined by
    /// commands of the history.
    pub variables: expr::Variables,
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
        let before = self.before(spec.effect);
        let reader = Params::new(params, self.display).with_variables(&before.state.variables);
        let accelerator = self.compute.accelerator();
        let outcome =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| emcraft_solver::accel::with(accelerator, || (spec.run)(self, &reader))));
        let error = match outcome {
            Ok(Ok(reply)) => {
                self.commit(spec, params, before);
                return Ok(reply);
            }
            Ok(Err(e)) => e,
            Err(_) => EngineError::Panicked(spec.id.to_owned()),
        };
        self.restore(before);
        Err(error)
    }

    /// What a command with `effect` may change, as it is now.
    fn before(&self, effect: Effect) -> Before {
        let history = if effect == Effect::None { Vec::new() } else { self.history.clone() };
        let state = project::Snapshot { model: self.model.clone(), display: self.display, history, variables: self.variables.clone() };
        Before { state, results: self.results.clone(), revision: self.revision }
    }

    /// Records a command that succeeded by its effect: one that changed the model leaves the
    /// state before it to undo to and joins the history; a solve's results are current.
    fn commit(&mut self, spec: &CommandSpec, params: &Value, before: Before) {
        match spec.effect {
            Effect::Record | Effect::Replace => {
                // A replaced session solved to its last state (a sample opened with its solve)
                // keeps its results current.
                let current = spec.effect == Effect::Replace && self.solved_revision == Some(self.revision);
                project::push_undo(&mut self.undo, before.state);
                self.redo.clear();
                if spec.effect == Effect::Record {
                    self.history.push(json!({"command": spec.id, "params": params}));
                }
                // (A replaced session counts its own revisions from zero.)
                self.revision = before.revision.max(self.revision) + 1;
                if current {
                    self.solved_revision = Some(self.revision);
                }
            }
            Effect::Solve => self.solved_revision = Some(self.revision),
            Effect::None => {}
        }
    }

    /// Puts back the model, units, variables and results a failed command may have changed.
    fn restore(&mut self, before: Before) {
        self.model = before.state.model;
        self.display = before.state.display;
        self.variables = before.state.variables;
        self.results = before.results;
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
            "materials": self.model.materials.iter().map(|m| json!({"name": m.name, "eps_r": m.eps_r, "mu_r": m.mu_r, "loss_tangent": m.loss_tangent, "magnetic_loss_tangent": m.magnetic_loss_tangent})).collect::<Vec<_>>(),
            "waveguide": w,
            "solid": self.model.solid.as_ref().map(|s| json!({"shape": s.shape.kind(), "material": s.material})),
            "ports": self.model.ports.iter().map(|p| json!({"name": p.name, "end": p.end, "modes": p.modes})).collect::<Vec<_>>(),
            "mesh": self.model.mesh.as_ref().map(|m| json!({"nodes": m.nodes.len(), "tetrahedra": m.tets.len()})),
            "results": self.results.sweep.as_ref().map(|s| json!({"order": s.order.as_u8(), "unknowns": s.driven.dofs.unknowns, "frequencies": s.results.len()})),
            "eigenmodes": self.results.eigen.as_ref().map(|e| e.modes.len()),
        })
    }
}
