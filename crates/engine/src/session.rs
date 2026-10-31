//! The session: the model, its results, the display units, the history that built the model and
//! undo; [`Session::execute`] runs every command.

use std::collections::VecDeque;
use std::sync::Arc;

use emcraft_fem::Order;
use emcraft_model::Model;
use emcraft_model::units::Display;
use emcraft_solver::{Driven, FrequencyResult, SPort};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::cmd::{CommandSpec, check_keys, find_command};
use crate::project::{self, Effect};
use crate::runs::Run;
use crate::{EngineError, Params, Result, Step, expr, open, progress, solve, study};

/// A frequency sweep's results.
#[derive(Clone, Debug)]
pub struct Sweep {
    pub order: Order,
    pub driven: Arc<Driven>,
    /// Ascending frequency.
    pub results: Vec<FrequencyResult>,
    /// Each port's de-embedding (m) the S-parameters are referred to: the ports' as solved, or as
    /// `model.port.deembed` moved them since.
    pub deembed: Vec<f64>,
}

impl Sweep {
    /// A sweep's results as `driven` solved them (its ports' de-embedding).
    pub fn new(order: Order, driven: Arc<Driven>, results: Vec<FrequencyResult>) -> Sweep {
        let deembed = driven.ports.iter().map(|p| p.deembed).collect();
        Sweep { order, driven, results, deembed }
    }

    /// `results` (solved by this sweep's system) with their S referred to this sweep's
    /// reference planes: each port's waves moved by e^{γΔd} from the plane its system solved
    /// at (D. M. Pozar, *Microwave Engineering*, 4th ed., §4.3).
    pub(crate) fn referred(&self, results: Vec<FrequencyResult>) -> crate::Result<Vec<FrequencyResult>> {
        let solved: Vec<f64> = self.driven.ports.iter().map(|p| p.deembed).collect();
        rereference(&self.driven, results, &solved, &self.deembed)
    }
}

/// `results` (solved by `driven`) with each port's reference plane moved from de-embedding
/// `from` to `to` (m): S_pq e^{γ_p Δd_p + γ_q Δd_q}, γ the port's first mode's.
pub(crate) fn rereference(driven: &Driven, mut results: Vec<FrequencyResult>, from: &[f64], to: &[f64]) -> crate::Result<Vec<FrequencyResult>> {
    let moved: Vec<(usize, f64)> = from.iter().zip(to).enumerate().filter(|(_, (a, b))| a != b).map(|(k, (a, b))| (k, b - a)).collect();
    if moved.is_empty() {
        return Ok(results);
    }
    for r in &mut results {
        let mut shift = vec![emcraft_solver::Complex64::new(1.0, 0.0); r.s.ports()];
        for (k, d) in &moved {
            let gamma = driven.port_terms(*k, r.frequency)?.first().map_or(emcraft_solver::Complex64::new(0.0, 0.0), |t| t.gamma);
            if let Some(slot) = shift.get_mut(*k) {
                *slot = (gamma * *d).exp();
            }
        }
        let s = &r.s;
        r.s = emcraft_solver::SMatrix::from_fn(s.ports(), |p, q| s[(p, q)] * shift[p] * shift[q]);
    }
    Ok(results)
}

impl Sweep {
    /// S's entries: each port, or each conductor of a terminal port.
    pub fn entries(&self) -> Vec<SPort> {
        self.driven.entries()
    }

    /// The result at a frequency (Hz) solved within 10⁻⁹ of it, or the nearest when `nearest`.
    pub fn at(&self, frequency: f64, nearest: bool) -> Option<&FrequencyResult> {
        let best = self.results.iter().min_by(|a, b| (a.frequency - frequency).abs().total_cmp(&(b.frequency - frequency).abs()))?;
        if nearest || (best.frequency - frequency).abs() <= 1e-9 * frequency.abs() { Some(best) } else { None }
    }
}

/// What the solvers made of the model, each from its own command. Each result is shared: the
/// copy a command keeps to go back to when it fails costs no copy of the fields.
#[derive(Clone, Debug, Default)]
pub struct Results {
    /// The driven frequency sweep from `solve.frequency`.
    pub sweep: Option<Arc<Sweep>>,
    /// The cavity's eigenmodes from `solve.eigenmodes`.
    pub eigen: Option<Arc<emcraft_solver::eigen::Eigen>>,
    /// The time-domain solver's S-parameters from `solve.time_domain`.
    pub time_domain: Option<Arc<emcraft_fdtd::TdResult>>,
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
        self.sweep.as_deref().ok_or(EngineError::NotSolved { what: "results", run: "solve.frequency" })
    }
}

/// The session as it was before a command ran: to undo to when it succeeds, to go back to when
/// it fails.
struct Before {
    /// The model, units and variables, and the history (for commands that change the job).
    state: project::Snapshot,
    results: Results,
    revision: u64,
    /// How the results are looked at (small: kept for every command).
    views: crate::views::Views,
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

/// A session: the model, its results and the display units. Only commands change it
/// ([`Session::execute`]); everyone else reads it through its accessors.
#[derive(Clone, Debug, Default)]
pub struct Session {
    pub(crate) model: Model,
    pub(crate) results: Results,
    pub(crate) display: Display,
    pub(crate) limits: Limits,
    /// The commands that built the model, in order (a script that rebuilds it).
    pub(crate) history: Vec<Step>,
    /// States to go back to (`edit.undo`) and forward again (`edit.redo`), the latest last.
    pub(crate) undo: VecDeque<project::Snapshot>,
    pub(crate) redo: VecDeque<project::Snapshot>,
    /// The model's revision, raised by every command that changes it, and the revision the
    /// latest results were solved at (older results are stale).
    pub(crate) revision: u64,
    pub(crate) solved_revision: Option<u64>,
    /// The project file the session was opened from or last saved to (`.emcraft`).
    pub(crate) path: Option<String>,
    /// The sample the session was opened from, which names it until it is saved.
    pub(crate) sample: Option<String>,
    /// The history as last saved or opened: the session has unsaved changes when its history
    /// differs (an undo back to it is clean again).
    pub(crate) saved: Vec<Step>,
    /// The project's runs: the last solve of each solve command ([`crate::runs`]), the latest
    /// last, and the runs as last saved or opened.
    pub(crate) runs: Vec<Run>,
    pub(crate) saved_runs: Vec<Run>,
    /// The analysis history: the solves kept with their models and results
    /// ([`crate::archive`]), the last id given, and the kept solve the session was restored to.
    pub(crate) archive: Vec<crate::archive::Archived>,
    pub(crate) archive_next: u32,
    pub(crate) archive_current: Option<u32>,
    /// Where the solvers run (`solve.device`).
    pub(crate) compute: solve::Compute,
    /// The named variables expressions in parameters read (`project.variable`), defined by
    /// commands of the history.
    pub(crate) variables: expr::Variables,
    /// How the project's results are looked at (charts' styles, probes: [`crate::views`]), and
    /// as last saved or opened.
    pub(crate) views: crate::views::Views,
    pub(crate) saved_views: crate::views::Views,
    /// The run a solve command records in place of its own (`solve.refine` records the merged
    /// sweep as `solve.frequency`); taken when it succeeds.
    pub(crate) next_run: Option<Run>,
}

/// Reading the session.
impl Session {
    pub fn model(&self) -> &Model {
        &self.model
    }

    pub fn results(&self) -> &Results {
        &self.results
    }

    /// The display units parameters are read in and replies written in.
    pub fn display(&self) -> Display {
        self.display
    }

    /// The limits on the work one command may ask for.
    pub fn limits(&self) -> Limits {
        self.limits
    }

    /// The session with other limits.
    pub fn with_limits(self, limits: Limits) -> Session {
        Session { limits, ..self }
    }

    /// The commands that built the model, in order (a script that rebuilds it).
    pub fn history(&self) -> &[Step] {
        &self.history
    }

    /// The states `edit.undo` goes back to, the latest last.
    pub fn undo(&self) -> &VecDeque<project::Snapshot> {
        &self.undo
    }

    /// The states `edit.redo` goes forward to, the latest last.
    pub fn redo(&self) -> &VecDeque<project::Snapshot> {
        &self.redo
    }

    /// The model's revision, raised by every command that changes it.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The revision the latest results were solved at (older results are stale).
    pub fn solved_revision(&self) -> Option<u64> {
        self.solved_revision
    }

    /// The project file the session was opened from or last saved to.
    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    /// The sample the session was opened from.
    pub fn sample(&self) -> Option<&str> {
        self.sample.as_deref()
    }

    /// Where the solvers run (`solve.device`).
    pub fn compute(&self) -> &solve::Compute {
        &self.compute
    }

    /// The named variables (`project.variable`).
    pub fn variables(&self) -> &expr::Variables {
        &self.variables
    }

    /// How the project's results are looked at: the charts' styles and the probes.
    pub fn views(&self) -> &crate::views::Views {
        &self.views
    }
}

impl Session {
    /// S's entries: the sweep's (each conductor of a terminal port its own), or before a sweep
    /// one per waveguide port.
    pub fn sparameter_entries(&self) -> Vec<SPort> {
        match &self.results.sweep {
            Some(sw) => sw.entries(),
            None => self
                .model
                .ports
                .iter()
                .enumerate()
                .map(|(i, p)| SPort { name: p.name.clone(), port: i, terminal: None, reference: p.reference })
                .collect(),
        }
    }

    /// The entry of S named `name` (a terminal `<port>:<k>`, or a port: its first entry) and its
    /// index; `None` when nothing has that name.
    pub fn sparameter_entry(&self, name: &str) -> Option<(usize, SPort)> {
        let entries = self.sparameter_entries();
        let exact = entries.iter().position(|e| e.name == name);
        let by_port = || entries.iter().position(|e| self.model.ports.get(e.port).is_some_and(|p| p.name == name));
        exact.or_else(by_port).and_then(|k| entries.get(k).cloned().map(|e| (k, e)))
    }

    pub fn new() -> Session {
        Session::default()
    }

    /// The analysis history ([`crate::archive`]), the oldest first.
    pub fn archive(&self) -> &[crate::archive::Archived] {
        &self.archive
    }

    /// A fresh session with this one's limits.
    pub(crate) fn fresh(&self) -> Session {
        Session { limits: self.limits, compute: self.compute.clone(), ..Session::default() }
    }

    /// This session replaced by `fresh` (a sample, a project file, a new one), its undo kept:
    /// it goes back to before the replacement.
    pub(crate) fn replace_with(&mut self, fresh: Session) {
        *self = Session { undo: std::mem::take(&mut self.undo), ..fresh };
    }

    /// The history and the runs as they are now count as saved.
    pub(crate) fn mark_saved(&mut self) {
        self.saved = self.history.clone();
        self.saved_runs = self.runs.clone();
        self.saved_views = self.views.clone();
    }

    /// Whether the project changed since it was last saved or opened: its model's history or
    /// its runs (a solve with other parameters than the saved run's) or how its results are
    /// looked at (a chart's style, a probe).
    pub fn is_dirty(&self) -> bool {
        self.history != self.saved || self.runs != self.saved_runs || self.views != self.saved_views
    }

    /// The project's runs, the latest last ([`crate::runs`]).
    pub fn runs(&self) -> &[Run] {
        &self.runs
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
        let spec = find_command(id).ok_or_else(|| EngineError::UnknownCommand(crate::text::clip(id, 100)))?;
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
        let history = if matches!(effect, Effect::None | Effect::View) { Vec::new() } else { self.history.clone() };
        // A command that replaces the project keeps the project to undo back to.
        let project = (effect == Effect::Replace).then(|| Box::new(self.identity()));
        Before { state: self.snapshot(history, project), results: self.results.clone(), revision: self.revision, views: self.views.clone() }
    }

    /// Records a command that succeeded by its effect: one that changed the model leaves the
    /// state before it to undo to and joins the history; a solve's results are current.
    fn commit(&mut self, spec: &CommandSpec, params: &Value, before: Before) {
        match spec.effect {
            Effect::Record | Effect::Adjust | Effect::Replace | Effect::Rebuild => {
                if let Some(change) = spec.invalidates {
                    self.results.invalidate(change);
                }
                // A replaced session solved to its last state (a sample opened with its solve)
                // keeps its results current, as does a change the command brought its results
                // along with.
                let current = match spec.effect {
                    Effect::Record => false,
                    Effect::Adjust => self.solved_revision == Some(before.revision),
                    _ => self.solved_revision == Some(self.revision),
                };
                project::push_undo(&mut self.undo, before.state);
                self.redo.clear();
                if matches!(spec.effect, Effect::Record | Effect::Adjust) {
                    self.history.push(Step::new(spec.id, params.clone()));
                }
                // (A replaced session counts its own revisions from zero.)
                self.revision = before.revision.max(self.revision) + 1;
                if current {
                    self.solved_revision = Some(self.revision);
                }
            }
            Effect::Solve => {
                self.solved_revision = Some(self.revision);
                let params = if params.is_null() { json!({}) } else { params.clone() };
                let run = self.next_run.take().unwrap_or(Run { command: spec.id.to_owned(), params, units: self.display });
                crate::archive::keep(self, &run.command, &run.params);
                self.archive_current = Some(self.archive_next);
                crate::runs::record(&mut self.runs, run);
            }
            Effect::View | Effect::None => {}
        }
    }

    /// Puts back the model, units, variables and results a failed command may have changed.
    fn restore(&mut self, before: Before) {
        self.model = before.state.model;
        self.display = before.state.display;
        self.variables = before.state.variables;
        self.results = before.results;
        self.views = before.views;
        self.next_run = None;
    }

    /// Runs a script: a JSON array of `{"command", "params"}` steps; stops at the first error.
    /// Returns each step's reply.
    pub fn run_script(&mut self, script: &Value) -> Result<Vec<Value>> {
        let steps = script.as_array().ok_or_else(|| EngineError::Params("a script is a JSON array of {\"command\", \"params\"} steps".into()))?;
        let steps = steps
            .iter()
            .enumerate()
            .map(|(i, step)| Step::deserialize(step).map_err(|_| EngineError::Params(format!("step {}: no `command`", i + 1))))
            .collect::<Result<Vec<Step>>>()?;
        self.run_steps(&steps)
    }

    /// Runs the steps of a script in order; stops at the first error. Returns each step's
    /// reply.
    pub fn run_steps(&mut self, steps: &[Step]) -> Result<Vec<Value>> {
        steps
            .iter()
            .enumerate()
            .map(|(i, step)| {
                progress::report(i, steps.len(), &step.command)?;
                self.execute(&step.command, &step.params).map_err(|e| e.in_step(i + 1, &step.command))
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
            "units": crate::reply::units(self.display),
            "analysis": crate::workflow::analysis_reply(&self.model),
            "materials": self.model.materials.iter().map(|m| json!({"name": m.name, "eps_r": m.eps_r, "mu_r": m.mu_r, "loss_tangent": m.loss_tangent, "magnetic_loss_tangent": m.magnetic_loss_tangent})).collect::<Vec<_>>(),
            "waveguide": w,
            "solid": self.model.solid.as_ref().map(|s| json!({"shape": s.shape.kind(), "material": s.material})),
            "ports": self.model.ports.iter().map(|p| {
                let mut v = json!({"name": p.name, "end": p.end, "modes": p.modes});
                if let Some(f) = p.face {
                    v["axis"] = json!(f.axis.id());
                }
                v
            }).collect::<Vec<_>>(),
            "mesh": self.model.mesh.as_ref().map(|m| json!({"nodes": m.nodes.len(), "tetrahedra": m.tets.len()})),
            "results": self.results.sweep.as_ref().map(|s| json!({"order": s.order.as_u8(), "unknowns": s.driven.dofs.unknowns, "frequencies": s.results.len()})),
            "eigenmodes": self.results.eigen.as_ref().map(|e| e.modes.len()),
        })
    }
}
