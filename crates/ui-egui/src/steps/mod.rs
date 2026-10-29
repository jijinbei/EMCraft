//! The workflow (docs/ux/workflow.md §2–3): a step bar under the ribbon, steps 0 Intent to
//! 8 Results each with its status (done, warning, missing, stale) from `project.status`, and the
//! active step's panel at the right: its settings (the step's commands, their dialogs docked in
//! the panel), its checks with a button to the fix, and the 3D view in the step's overlay.
//! Excitation previews the port modes, Mesh shows the mesh's statistics and the solve's
//! estimate (`project.estimate`), Run checks the model first (blocking problems refuse to start,
//! warnings can be accepted), Results marks results of a changed model stale and exports and
//! checks them.

use egui::{Color32, FontId, Rect, Sense, Stroke, pos2, vec2};
use serde_json::{Value, json};

use crate::scene3d::Overlay;
use crate::theme::Tokens;
use crate::{EmcraftApp, UiError, View, dialogs, scenario, ui_kit};

pub use emcraft_engine::workflow::{Check, Status, Step, StepState};

mod bar;
mod panel;
mod run;

pub use bar::bar;
pub use panel::{apply_band, panel};
pub use run::{Preflight, preflight, run_params, start_run};

/// What the 3D view puts forward in a step.
pub fn overlay(step: Step) -> Overlay {
    match step {
        Step::Materials => Overlay::Materials,
        Step::Excitation => Overlay::Ports,
        Step::Boundaries => Overlay::Boundaries,
        Step::Mesh => Overlay::Mesh,
        _ => Overlay::Plain,
    }
}

/// A status's colour in the step bar and panels.
fn status_color(status: Status, t: &Tokens) -> Color32 {
    match status {
        Status::Done => t.success,
        Status::Warning => t.warning,
        Status::Missing => t.error,
        Status::Stale => Color32::from_rgb(70, 130, 210),
    }
}

/// The solver Run starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Solver {
    Frequency,
    Eigenmodes,
    Radiation,
    TimeDomain,
}

impl Solver {
    pub const ALL: [Solver; 4] = [Solver::Frequency, Solver::Eigenmodes, Solver::Radiation, Solver::TimeDomain];

    pub fn command(self) -> &'static str {
        match self {
            Solver::Frequency => "solve.frequency",
            Solver::Eigenmodes => "solve.eigenmodes",
            Solver::Radiation => "solve.radiation",
            Solver::TimeDomain => "solve.time_domain",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Solver::Frequency => "frequency",
            Solver::Eigenmodes => "eigenmodes",
            Solver::Radiation => "radiation",
            Solver::TimeDomain => "time_domain",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Solver::Frequency => "Frequency sweep (S-parameters)",
            Solver::Eigenmodes => "Eigenmodes (resonances)",
            Solver::Radiation => "Radiation (open problem)",
            Solver::TimeDomain => "Time domain",
        }
    }

    pub fn parse(s: &str) -> Option<Solver> {
        Solver::ALL.into_iter().find(|x| x.id() == s)
    }

    /// The solver a model calls for: resonances without excitation, radiation with sources or
    /// lumped ports, else a sweep.
    pub fn infer(m: &emcraft_model::Model) -> Solver {
        if m.ports.is_empty() && m.lumped.is_empty() && m.sources.is_empty() {
            Solver::Eigenmodes
        } else if !m.sources.is_empty() || !m.lumped.is_empty() || m.open.is_some() {
            Solver::Radiation
        } else {
            Solver::Frequency
        }
    }
}

/// The text fields of the step panels, with each one's error.
#[derive(Clone, Debug, Default)]
pub struct Fields {
    pub start: String,
    pub stop: String,
    /// The band the start and stop fields were last filled from.
    pub band_seen: Option<(f64, f64)>,
    pub solver: Option<Solver>,
    pub points: String,
    pub order: String,
    pub count: String,
    pub frequency: String,
    pub touchstone: String,
    pub format: String,
    pub errors: Vec<(&'static str, String)>,
    /// Run's last refusal.
    pub refused: Option<String>,
}

impl Fields {
    fn error(&self, name: &str) -> Option<&str> {
        self.errors.iter().find(|(n, _)| *n == name).map(|(_, e)| e.as_str())
    }

    fn set_error(&mut self, name: &'static str, e: Option<String>) {
        self.errors.retain(|(n, _)| *n != name);
        if let Some(e) = e {
            self.errors.push((name, e));
        }
    }
}

/// The workflow's state in the app.
#[derive(Default)]
pub struct Workflow {
    pub step: Option<Step>,
    status: Option<(u64, Vec<StepState>)>,
    estimate: Option<(u64, Result<Value, String>)>,
    pub fields: Fields,
    /// `results.check`'s last reply, and the revision it was made at.
    pub check: Option<(u64, Value)>,
    /// The port mode chosen in the Excitation panel's table, which the Port modes view marks.
    pub mode_focus: Option<ModeFocus>,
    /// What the Setup panel's band diagram last drew: the cut-offs' labels and the solve's
    /// frequencies.
    pub diagram: Option<Value>,
    /// `solve.devices`' reply, asked once: what this machine can run on.
    pub devices: Option<Value>,
}

/// A port mode picked in the Excitation panel: the Port modes view marks its row and scrolls
/// to it once.
#[derive(Clone, Debug, PartialEq)]
pub struct ModeFocus {
    pub port: String,
    pub mode: usize,
    pub scroll: bool,
}

/// The statuses of the steps (from `project.status`, once per revision).
pub fn statuses(app: &mut EmcraftApp) -> Vec<StepState> {
    let key = app.revision;
    if let Some((k, v)) = &app.workflow.status
        && *k == key
    {
        return v.clone();
    }
    let v = emcraft_engine::workflow::steps(&app.session).to_vec();
    app.workflow.status = Some((key, v.clone()));
    v
}

/// The statuses for `ui.inspect`: the cache when it is current, else worked out on a copy.
pub fn statuses_now(app: &EmcraftApp) -> Vec<StepState> {
    if let Some((k, v)) = &app.workflow.status
        && *k == app.revision
    {
        return v.clone();
    }
    emcraft_engine::workflow::steps(&app.session).to_vec()
}

/// The workflow's state for `ui.inspect`.
pub fn inspect(app: &EmcraftApp) -> Value {
    let states = statuses_now(app);
    let steps: serde_json::Map<String, Value> = states.iter().map(|s| (s.step.id().to_owned(), json!(s.status.id()))).collect();
    let messages: serde_json::Map<String, Value> = states
        .iter()
        .map(|s| (s.step.id().to_owned(), json!(s.messages.iter().map(|m| json!({"text": m.text, "command": m.command})).collect::<Vec<_>>())))
        .collect();
    let f = &app.workflow.fields;
    json!({
        "step": app.workflow.step.map(Step::id),
        "steps": steps,
        "messages": messages,
        "errors": f.errors.iter().map(|(k, v)| (k.to_string(), json!(v))).collect::<serde_json::Map<_, _>>(),
        "refused": f.refused,
        "solver": f.solver.unwrap_or_else(|| Solver::infer(app.session.model())).id(),
        "check": app.workflow.check.as_ref().map(|c| c.1.clone()),
        "diagram": app.workflow.diagram,
        "device": app.session.compute().device(),
        "precision": app.session.compute().precision.name(),
        "failure": app.failure.as_ref().map(|f| json!({"command": f.0, "error": f.1})),
        "mode_focus": app.workflow.mode_focus.as_ref().map(|m| json!({"port": m.port, "mode": m.mode})),
    })
}

/// The solve's estimate (`project.estimate`, once per revision).
pub fn estimate(app: &mut EmcraftApp) -> Result<Value, String> {
    let key = app.revision;
    if let Some((k, v)) = &app.workflow.estimate
        && *k == key
    {
        return v.clone();
    }
    let order = app.workflow.fields.order.trim().parse::<u64>().ok().filter(|o| (1..=2).contains(o)).unwrap_or(2);
    let v = app.session.execute("project.estimate", &json!({"order": order})).map_err(|e| e.to_string());
    app.workflow.estimate = Some((key, v.clone()));
    v
}

/// Opens a step: its panel and the view it calls for.
pub fn open_step(app: &mut EmcraftApp, step: Option<Step>) {
    app.workflow.step = step;
    let Some(step) = step else { return };
    // The ribbon shows the step's tab.
    app.tab = match step {
        Step::Intent | Step::Geometry => "MODEL",
        Step::Materials | Step::Boundaries => "MATERIALS",
        Step::Excitation => "PORTS & SOURCES",
        Step::Mesh => "MESH",
        Step::Setup | Step::Run => "SIMULATE",
        Step::Results => "RESULTS",
    }
    .into();
    app.view = match step {
        Step::Results => {
            if app.session.results().sweep.is_some() || app.session.results().time_domain.is_some() {
                View::SParameters
            } else if app.session.results().eigen.is_some() {
                View::Eigenmodes
            } else if app.session.results().radiation.is_some() || app.session.results().td_radiation.is_some() {
                View::FarField
            } else {
                View::Model
            }
        }
        _ => View::Model,
    };
    // A dialog of another step closes.
    if app.dialog.as_ref().is_some_and(|d| Step::of_command(d.command.id) != Some(step)) {
        app.dialog = None;
    }
}

/// What a check's Fix does: the step that owns its command, with that command's dialog.
pub fn fix(app: &mut EmcraftApp, command: &str) {
    let step = Step::of_command(command);
    if step.is_some() {
        open_step(app, step);
    }
    let runs = command.starts_with("solve.") && command != "solve.port_modes";
    if command != "project.frequency"
        && !runs
        && let Err(e) = app.open_dialog(command)
    {
        app.message(e, true);
    }
}

/// A positive finite number from a field, or why not.
fn positive(text: &str) -> Result<f64, String> {
    let v = dialogs::parse_number(text)?;
    if !(v > 0.0) {
        return Err("must be greater than 0".into());
    }
    Ok(v)
}

/// An integer within `lo..=hi`.
fn integer(text: &str, lo: u64, hi: u64) -> Result<u64, String> {
    let s = text.trim();
    let v: u64 = s.parse().map_err(|_| format!("must be a whole number from {lo} to {hi}"))?;
    if !(lo..=hi).contains(&v) {
        return Err(format!("must be from {lo} to {hi}"));
    }
    Ok(v)
}

/// The band (display units) the session holds.
fn band(app: &EmcraftApp) -> Option<(f64, f64)> {
    let f = app.session.display().frequency;
    app.session.model().band.map(|b| (f.from_si(b.start), f.from_si(b.stop)))
}
