//! The workflow (docs/ux/workflow.md §2–3): a step bar under the ribbon, steps 0 Intent to
//! 8 Results each with its status (done, to do, waiting, warning, needs attention, out of date)
//! from `project.status` — and from the window: a refused Run marks the steps that blocked it, a
//! failed run marks Run, a running one shows on Run — and the
//! active step's panel at the right: its settings (the step's commands, their dialogs docked in
//! the panel), its checks with a button to the fix, and the 3D view in the step's overlay.
//! Excitation previews the port modes, Mesh shows the mesh's statistics and the solve's
//! estimate (`project.estimate`), Run checks the model first (blocking problems refuse to start,
//! warnings can be accepted), Results marks results of a changed model stale and exports and
//! checks them.

use egui::{Color32, FontId, Rect, Sense, Stroke, pos2, vec2};
use serde_json::{Value, json};

use crate::handle::{Fixed, Handle};
use crate::ribbon::RibbonTab;
use crate::scene3d::Overlay;
use crate::theme::Tokens;
use crate::{Action, Actions, EmcraftApp, UiError, UiExt, View, dialogs, scenario, ui_kit};

pub use emcraft_engine::workflow::{Check, Status, Step, StepState};

mod bar;
mod panel;
mod run;

pub use bar::bar;
pub(crate) use panel::fmt;
pub use panel::{KindCard, apply_band, export_touchstone, kind_card, not_needed, panel};
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

/// A status's colour in the step bar and panels: green done, neutral to do and waiting, amber
/// warning, red needs attention, blue out of date.
fn status_color(status: Status, t: &Tokens) -> Color32 {
    match status {
        Status::Done => t.success,
        Status::Todo | Status::Waiting => t.text_dim,
        Status::Warning => t.warning,
        Status::Missing => t.error,
        Status::Stale => t.info,
    }
}

/// Whether command `id` is a run (a solve or a study, as step 7 starts one): what a failure
/// marks on Run and what Run shows running.
pub fn is_run(id: &str) -> bool {
    is_solve(id) || id.starts_with("study.")
}

/// Whether command `id` is a solve: a `solve.` command other than the port modes' and those that
/// pick or list the devices.
pub fn is_solve(id: &str) -> bool {
    id.starts_with("solve.") && !matches!(id, "solve.port_modes" | "solve.device" | "solve.devices")
}

/// Whether a run is going on.
pub fn running(app: &EmcraftApp) -> bool {
    app.job.as_ref().is_some_and(|j| is_run(&j.command))
}

/// The mark of results out of date (a turning arrow on blue), for the views and the browser.
pub fn stale_mark(p: &egui::Painter, c: egui::Pos2, t: &Tokens) {
    bar::mark(p, c, Status::Stale, t);
}

/// Whether the results are older than the model (the Results step out of date).
pub fn results_stale(app: &EmcraftApp) -> bool {
    statuses_now(app).get(Step::Results.index()).is_some_and(|s| s.status == Status::Stale)
}

/// The solver Run starts: a kind of analysis (`project.analysis`), its id the Setup step's
/// `setup:solver:<id>` and step 0's `intent:kind:<id>`.
pub use emcraft_engine::workflow::Analysis as Solver;

/// The command that runs `solver`.
pub const fn command(solver: Solver) -> &'static str {
    emcraft_engine::workflow::solve_command(solver)
}

/// The solver the Setup step starts on: the one picked there, else the job's kind of analysis
/// (chosen at step 0, else inferred from the model).
pub fn solver(app: &EmcraftApp) -> Solver {
    app.workflow.fields.solver.unwrap_or_else(|| app.session.model().analysis())
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
    /// What the Setup panel's band diagram last drew: the cut-offs' labels and the solve's
    /// frequencies.
    pub diagram: Option<Value>,
    /// The fields as the project's last run left them (restored on opening it): while they
    /// stay so, Run runs that run again (`solve.rerun`), its parameters whole.
    pub from_run: Option<Setup>,
}

/// The Setup step's fields that make a run.
#[derive(Clone, Debug, PartialEq)]
pub struct Setup {
    solver: Option<Solver>,
    points: String,
    order: String,
    count: String,
    frequency: String,
}

impl Fields {
    /// The fields that make a run, as they stand.
    pub fn setup(&self) -> Setup {
        Setup {
            solver: self.solver,
            points: self.points.clone(),
            order: self.order.clone(),
            count: self.count.clone(),
            frequency: self.frequency.clone(),
        }
    }

    /// Empty fields take their defaults, as the Setup panel shows them.
    fn fill_defaults(&mut self) {
        for (field, default) in [(&mut self.points, "101"), (&mut self.order, "2"), (&mut self.count, "5")] {
            if field.is_empty() {
                *field = default.to_owned();
            }
        }
    }

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
    /// The estimate of the step's run (the Mesh step: one solve at an order; Setup and Run: the
    /// run as set up).
    estimate: Option<Estimate>,
    /// The estimate of the run the open dialog would start.
    pub dialog_estimate: Option<Estimate>,
    pub fields: Fields,
    /// `results.check`'s last reply, and the revision it was made at.
    pub check: Option<(u64, Value)>,
    /// The port mode chosen in the Excitation panel's table, which the Port modes view marks.
    pub mode_focus: Option<ModeFocus>,
    /// `solve.devices`' reply, asked once: what this machine can run on.
    pub devices: Option<Value>,
    /// Run was refused for blocking problems: the steps still in the way show as needing
    /// attention, until a run starts or another project opens.
    pub blocked: bool,
}

/// `project.estimate`'s reply, with the revision and the request it was asked for.
#[derive(Clone, Debug)]
pub struct Estimate {
    pub revision: u64,
    pub asked: Value,
    pub reply: Result<Value, String>,
}

impl Estimate {
    /// `slot` made to answer `asked` at `revision`: asked of the session again unless it does.
    fn refresh(slot: &mut Option<Estimate>, session: &mut emcraft_engine::Session, revision: u64, asked: Value) {
        if slot.as_ref().is_some_and(|e| e.revision == revision && e.asked == asked) {
            return;
        }
        let reply = session.execute("project.estimate", &asked).map_err(|e| e.to_string());
        *slot = Some(Estimate { revision, asked, reply });
    }
}

/// A port mode picked in the Excitation panel: the Port modes view marks its row and scrolls
/// to it once.
#[derive(Clone, Debug, PartialEq)]
pub struct ModeFocus {
    pub port: String,
    pub mode: usize,
    pub scroll: bool,
}

/// Brings the workflow's caches up to date before a frame is drawn: the steps' statuses (from
/// `project.status`, once per revision), the solve's estimate in the Mesh and Run steps, the
/// devices in the Setup step.
pub fn prepare(app: &mut EmcraftApp) {
    let key = app.revision;
    if app.workflow.status.as_ref().is_none_or(|(k, _)| *k != key) {
        app.workflow.status = Some((key, emcraft_engine::workflow::steps(&app.session).to_vec()));
    }
    // What the step's estimate is of: one solve at the Setup's order (Mesh), the run as set up
    // (Setup, Run).
    let asked = match app.workflow.step {
        Some(Step::Mesh) => {
            let order = app.workflow.fields.order.trim().parse::<u64>().ok().filter(|o| (1..=2).contains(o)).unwrap_or(2);
            Some(json!({"command": "solve.frequency", "params": {"points": 1}, "order": order}))
        }
        Some(Step::Setup | Step::Run) => {
            let session = &app.session;
            run::run_params(session, &mut app.workflow.fields.clone())
                .ok()
                .map(|(solver, params)| json!({"command": command(solver), "params": params}))
        }
        _ => None,
    };
    if let Some(asked) = asked {
        Estimate::refresh(&mut app.workflow.estimate, &mut app.session, key, asked);
    }
    if app.workflow.step == Some(Step::Setup) && app.workflow.devices.is_none() {
        let v = app.session.execute("solve.devices", &json!({})).unwrap_or_else(|e| json!({"error": e.to_string()}));
        app.workflow.devices = Some(v);
    }
    // The open dialog's run, when it starts one.
    let dialog = app
        .dialog
        .as_ref()
        .filter(|d| ESTIMATED.contains(&d.command.id))
        .and_then(|d| Some(json!({"command": d.command.id, "params": d.quiet_params()?})));
    match dialog {
        Some(asked) => Estimate::refresh(&mut app.workflow.dialog_estimate, &mut app.session, key, asked),
        None => app.workflow.dialog_estimate = None,
    }
}

/// The commands whose dialogs show how long their run will take (`project.estimate`).
pub const ESTIMATED: [&str; 9] = [
    "solve.frequency",
    "solve.refine",
    "solve.eigenmodes",
    "solve.radiation",
    "solve.dispersion",
    "solve.time_domain",
    "mesh.adapt",
    "study.sweep",
    "study.optimize",
];

/// A duration in brief: `0.4 s`, `12 s`, `3 min`, `1.5 h`.
pub fn duration(seconds: f64) -> String {
    match seconds {
        s if !s.is_finite() || s < 0.0 => "–".to_owned(),
        s if s < 10.0 => format!("{s:.1} s"),
        s if s < 90.0 => format!("{s:.0} s"),
        s if s < 5400.0 => format!("{:.0} min", s / 60.0),
        s => format!("{:.1} h", s / 3600.0),
    }
}

/// A run's estimate (`project.estimate`'s reply) in a line — its time, `ref.` when it is the
/// reference machine's — with what it counts on hover, published as `handle`.
pub fn estimate_line(ui: &mut egui::Ui, e: &Result<Value, String>, handle: crate::Fixed) {
    let t = &crate::theme::Tokens::get();
    let r = match e {
        Ok(e) => {
            let n = |k: &str| e.get(k).and_then(Value::as_f64);
            let measured = e.get("basis").and_then(Value::as_str) == Some("measured");
            let time = duration(n("seconds").unwrap_or(f64::NAN));
            let text = if measured { format!("⏱ ≈ {time}") } else { format!("⏱ ≈ {time} ({})", crate::lang::tr("ref.")) };
            let mut detail = vec![e.get("what").and_then(Value::as_str).unwrap_or("").to_owned()];
            if let (Some(u), Some(mb)) = (n("unknowns"), n("memory_mb")) {
                detail.push(crate::lang::trf("{n} unknowns, {mb} MB", crate::lang::args!(n = format!("{u:.0}"), mb = format!("{mb:.0}"))));
            }
            detail.push(
                crate::lang::tr(if measured {
                    "scaled from this machine's last sweep"
                } else {
                    "from a reference machine; a sweep here measures this one"
                })
                .to_owned(),
            );
            ui.label(egui::RichText::new(text).size(t.font_small).color(t.text)).on_hover_text(detail.join("\n"))
        }
        Err(e) => ui.label(egui::RichText::new(format!("⏱ {e}")).size(t.font_small).color(t.text_dim)),
    };
    crate::scenario::publish_target(handle, r.rect);
}

/// The step's estimate as `prepare` left it for this revision.
pub fn run_estimate(app: &EmcraftApp) -> Option<&Result<Value, String>> {
    app.workflow.estimate.as_ref().filter(|e| e.revision == app.revision).map(|e| &e.reply)
}

/// The statuses of the steps: the session's (the cache when it is current, else worked out on
/// a copy), with what the window knows on top: after a refused Run, the steps that block it
/// need attention; after a run failed, Run does.
pub fn statuses_now(app: &EmcraftApp) -> Vec<StepState> {
    let mut states = match &app.workflow.status {
        Some((k, v)) if *k == app.revision => v.clone(),
        _ => emcraft_engine::workflow::steps(&app.session).to_vec(),
    };
    if app.workflow.blocked {
        for s in states.iter_mut().filter(|s| run::required(s.step, None) && s.status == Status::Todo) {
            s.status = Status::Missing;
        }
    }
    if run_failure(app).is_some()
        && let Some(run) = states.get_mut(Step::Run.index())
    {
        run.status = run.status.min(Status::Missing);
    }
    states
}

/// Why the last run failed, while that is the last word (the Run panel shows it in full).
pub fn run_failure(app: &EmcraftApp) -> Option<&str> {
    app.failure.as_ref().filter(|f| is_run(&f.command) && app.job.is_none()).map(|f| f.error.as_str())
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
        "running": running(app),
        "stale": states.get(Step::Results.index()).is_some_and(|s| s.status == Status::Stale),
        "messages": messages,
        "errors": f.errors.iter().map(|(k, v)| (k.to_string(), json!(v))).collect::<serde_json::Map<_, _>>(),
        "refused": f.refused,
        "solver": solver(app).id(),
        "analysis": emcraft_engine::workflow::analysis_reply(app.session.model()),
        "not_needed": Step::ALL.into_iter().filter(|s| not_needed(app, *s).is_some()).map(Step::id).collect::<Vec<_>>(),
        "setup": {"points": f.points, "order": f.order, "count": f.count, "frequency": f.frequency},
        "rerun": reruns(app),
        "check": app.workflow.check.as_ref().map(|c| c.1.clone()),
        "diagram": f.diagram,
        "device": app.session.compute().device(),
        "precision": app.session.compute().precision.id(),
        "failure": app.failure.as_ref().map(|f| json!({"command": f.command, "error": f.error})),
        "mode_focus": app.workflow.mode_focus.as_ref().map(|m| json!({"port": m.port, "mode": m.mode})),
    })
}

/// The Setup step's fields from the project's latest run of a solver it starts
/// (`project.runs`, in display units now), as after opening a project; none: left as they are.
pub fn restore_setup(app: &mut EmcraftApp) {
    let chosen = app.session.model().analysis;
    let f = &mut app.workflow.fields;
    f.from_run = None;
    let Ok(v) = app.session.execute("project.runs", &json!({})) else { return };
    let runs = v.get("runs").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
    let found = runs.iter().rev().find_map(|r| {
        let command = r.get("command").and_then(Value::as_str)?;
        let solver = Solver::ALL.into_iter().find(|s| self::command(*s) == command)?;
        // A kind chosen at step 0 restores only its own solver's run.
        if chosen.is_some_and(|c| c != solver) {
            return None;
        }
        Some((solver, r.get("params")?))
    });
    let Some((solver, p)) = found else { return };
    let text = |k: &str| p.get(k).map(dialogs::value_text).unwrap_or_default();
    f.solver = Some(solver);
    f.order = text("order");
    match solver {
        Solver::Frequency | Solver::TimeDomain => {
            let listed = p.get("frequencies").and_then(Value::as_array).map(Vec::len);
            f.points = listed.map_or_else(|| text("points"), |n| n.to_string());
        }
        Solver::Eigenmodes => {
            f.count = text("count");
            f.frequency = text("near");
        }
        Solver::Radiation => f.frequency = text("frequency"),
    }
    f.fill_defaults();
    f.errors.clear();
    f.from_run = Some(f.setup());
}

/// Whether Run runs the project's last run again (`solve.rerun`): the Setup step's fields are
/// as that run left them.
pub fn reruns(app: &EmcraftApp) -> Option<&'static str> {
    let f = &app.workflow.fields;
    let command = self::command(f.solver?);
    (f.from_run.as_ref() == Some(&f.setup()) && app.session.runs().iter().any(|r| r.command == command)).then_some(command)
}

/// The solve's estimate (`project.estimate`), as `prepare` left it for this revision.
pub fn estimate(app: &EmcraftApp) -> Result<&Value, &str> {
    run_estimate(app).ok_or("no estimate yet")?.as_ref().map_err(String::as_str)
}

/// Opens a step: its panel and the view it calls for.
pub fn open_step(app: &mut EmcraftApp, step: Option<Step>) {
    app.workflow.step = step;
    let Some(step) = step else { return };
    // The ribbon shows the step's tab.
    app.tab = match step {
        Step::Intent | Step::Geometry => RibbonTab::Model,
        Step::Materials => RibbonTab::Materials,
        Step::Boundaries => RibbonTab::Boundaries,
        Step::Excitation => RibbonTab::PortsSources,
        Step::Mesh => RibbonTab::Mesh,
        Step::Setup | Step::Run => RibbonTab::Simulate,
        Step::Results => RibbonTab::Results,
    };
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
pub fn fix(app: &mut EmcraftApp, command: &str) -> Result<(), UiError> {
    let step = Step::of_command(command);
    if step.is_some() {
        open_step(app, step);
    }
    if !matches!(command, "project.frequency" | "project.analysis") && !is_run(command) {
        app.open_dialog(command)?;
    }
    Ok(())
}

/// A positive finite number from a field, or why not.
fn positive(text: &str) -> Result<f64, String> {
    let v = dialogs::parse_number(text)?;
    if !(v > 0.0) {
        return Err(crate::lang::tr("must be greater than 0").into());
    }
    Ok(v)
}

/// An integer within `lo..=hi`.
fn integer(text: &str, lo: u64, hi: u64) -> Result<u64, String> {
    let s = text.trim();
    let range = crate::lang::args!(lo = lo, hi = hi);
    let v: u64 = s.parse().map_err(|_| crate::lang::trf("must be a whole number from {lo} to {hi}", range))?;
    if !(lo..=hi).contains(&v) {
        return Err(crate::lang::trf("must be from {lo} to {hi}", range));
    }
    Ok(v)
}

/// The band (display units) the session holds.
fn band(s: &emcraft_engine::Session) -> Option<(f64, f64)> {
    let f = s.display().frequency;
    s.model().band.map(|b| (f.from_si(b.start.get()), f.from_si(b.stop.get())))
}
