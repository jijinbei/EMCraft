//! The workflow (docs/ux/workflow.md §2–3): a step bar under the ribbon, steps 0 Home to
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
use emcraft_engine::{Device, Precision};
use serde_json::{Value, json};

use crate::handle::{Fixed, Handle};
use crate::ribbon::RibbonTab;
use crate::scene3d::Overlay;
use crate::theme::Tokens;
use crate::{Action, Actions, EmcraftApp, UiError, UiExt, View, dialogs, scenario, ui_kit};

pub use emcraft_engine::workflow::{Check, Status, Step, StepState};

/// A step's check as the window shows it: its template translated, its values filled in (the
/// steps it names by their shown names); the English text when it has no template.
pub fn check_text(c: &Check) -> String {
    if c.template.is_empty() {
        return crate::lang::lookup(&c.text).to_owned();
    }
    let steps = |ids: &str| {
        ids.split(", ").map(|id| Step::ALL.iter().find(|s| s.id() == id).map_or(id, |s| crate::lang::shown(*s))).collect::<Vec<_>>().join("、")
    };
    let values: Vec<String> = c.args.iter().map(|(k, v)| if k == "steps" { steps(v) } else { v.clone() }).collect();
    let args: Vec<(&str, &dyn std::fmt::Display)> = c.args.iter().zip(&values).map(|((k, _), v)| (k.as_str(), v as &dyn std::fmt::Display)).collect();
    crate::lang::fill(crate::lang::lookup(&c.template), &args)
}

mod bar;
mod panel;
mod run;

pub use bar::bar;
pub(crate) use bar::{Look, Pill, pills};
pub(crate) use panel::fmt;
pub use panel::{export_touchstone, not_needed, panel};
pub use run::{Preflight, confirm_setup, preflight, setup_params, start_run};

/// What the 3D view puts forward in a step.
pub fn overlay(step: Step) -> Overlay {
    match step {
        // The bodies drawn in their materials' colours, the step listing the materials.
        Step::Geometry => Overlay::Materials,
        Step::Excitation => Overlay::Ports,
        Step::Boundaries => Overlay::Boundaries,
        Step::Mesh => Overlay::Mesh,
        _ => Overlay::Plain,
    }
}

/// A status's colour in the step bar and panels: green done, neutral to do and waiting, amber
/// warning, red needs attention, blue out of date.
fn status_color(status: Status) -> Color32 {
    let t = &Tokens::get();
    match status {
        Status::Done => t.success,
        Status::Todo | Status::Waiting => t.text_dim,
        Status::Warning => t.warning,
        Status::Missing => t.error,
        Status::Stale => t.info,
    }
}

/// Whether command `id` is a run (a solve or a study, as step 6 starts one): what a failure
/// marks on Run and what Run shows running.
pub fn is_run(id: &str) -> bool {
    is_solve(id) || id.starts_with("study.")
}

/// Whether command `id` is a solve ([`emcraft_engine::CommandSpec::solves`]).
pub fn is_solve(id: &str) -> bool {
    emcraft_engine::find_command(id).is_some_and(emcraft_engine::CommandSpec::solves)
}

/// Whether a run is going on.
pub fn running(app: &EmcraftApp) -> bool {
    app.job.as_ref().is_some_and(|j| is_run(&j.command))
}

/// The mark of results out of date (a turning arrow on blue), for the views and the browser.
pub fn stale_mark(p: &egui::Painter, c: egui::Pos2) {
    bar::mark(p, c, Status::Stale);
}

/// Whether the results are older than the model (the Results step out of date).
pub fn results_stale(app: &EmcraftApp) -> bool {
    statuses_now(app).get(Step::Results.index()).is_some_and(|s| s.status == Status::Stale)
}

/// The analysis Run starts (`project.analysis`), chosen by Home's `intent:kind:<id>`.
pub use emcraft_engine::workflow::Analysis as Solver;

/// The command that runs `solver`.
pub const fn command(solver: Solver) -> &'static str {
    emcraft_engine::workflow::solve_command(solver)
}

/// The analysis chosen on Home, or Home's inferred choice. Setup never overrides it.
pub fn solver(app: &EmcraftApp) -> Solver {
    app.session.model().analysis()
}

/// The text fields of the step panels, with each one's error. The Setup step's are a form of
/// the run's settings: loaded from the confirmed ones (`project.setup`) whenever those change,
/// confirmed by its button.
#[derive(Clone, Debug, Default)]
pub struct Fields {
    pub start: String,
    pub stop: String,
    pub points: String,
    pub order: String,
    pub count: String,
    pub frequency: String,
    pub cell: String,
    /// The time domain's ports to excite, as confirmed (no field: kept through the form).
    pub excite: Vec<String>,
    /// The confirmed settings the form was last loaded from (`project.values`).
    pub seen: Option<Value>,
    pub touchstone: String,
    pub format: String,
    pub errors: Vec<(&'static str, String)>,
    /// Confirm was pressed: an empty field's error shows too (before, only a field typed in).
    pub tried: bool,
    /// What the Setup panel's band diagram last drew: the cut-offs' labels and the solve's
    /// frequencies.
    pub diagram: Option<Value>,
}

impl Fields {
    /// Empty fields take their defaults, as the Setup panel shows them.
    fn fill_defaults(&mut self) {
        for (field, default) in [(&mut self.points, "101"), (&mut self.order, "2"), (&mut self.count, "5"), (&mut self.cell, "1")] {
            if field.is_empty() {
                default.clone_into(field);
            }
        }
    }

    pub(crate) fn error(&self, name: &str) -> Option<&str> {
        self.errors.iter().find(|(n, _)| *n == name).map(|(_, e)| e.as_str())
    }

    /// [`Fields::error`] as the form shows it: an empty field's only once Confirm was pressed.
    pub(crate) fn shown_error(&self, name: &str) -> Option<&str> {
        let text = match name {
            "start" => &self.start,
            "stop" => &self.stop,
            "points" => &self.points,
            "order" => &self.order,
            "count" => &self.count,
            "frequency" => &self.frequency,
            "cell" => &self.cell,
            _ => return self.error(name),
        };
        self.error(name).filter(|_| self.tried || !text.trim().is_empty())
    }

    fn set_error(&mut self, name: &'static str, e: Option<String>) {
        self.errors.retain(|(n, _)| *n != name);
        if let Some(e) = e {
            self.errors.push((name, e));
        }
    }

    /// The form filled from settings as `project.setup` takes them (a confirmed run's, or a
    /// proposal's: the band a PCB route's model is made for).
    pub fn load(&mut self, v: &Value) {
        let text = |k: &str| v.get(k).map(dialogs::value_text);
        for (field, key) in [
            (&mut self.start, "start"),
            (&mut self.stop, "stop"),
            (&mut self.points, "points"),
            (&mut self.order, "order"),
            (&mut self.count, "count"),
            (&mut self.frequency, "frequency"),
            (&mut self.cell, "cell"),
        ] {
            if let Some(t) = text(key) {
                *field = t;
            }
        }
        if let Some(list) = v.get("excite").and_then(Value::as_array) {
            self.excite = list.iter().filter_map(Value::as_str).map(str::to_owned).collect();
        }
        self.fill_defaults();
        self.errors.clear();
    }

    /// The fields as typed, for a recording (crate::record).
    pub fn drafts(&self) -> Value {
        serde_json::json!({
            "start": self.start, "stop": self.stop, "points": self.points, "order": self.order, "count": self.count,
            "frequency": self.frequency, "cell": self.cell, "touchstone": self.touchstone, "format": self.format, "excite": self.excite,
        })
    }

    /// The fields typed as [`Fields::drafts`] wrote them (a text missing or not one left as it is).
    pub fn set_drafts(&mut self, v: &Value) {
        let text = |k: &str| v.get(k).and_then(Value::as_str).map(dialogs::bounded);
        for (field, key) in [
            (&mut self.start, "start"),
            (&mut self.stop, "stop"),
            (&mut self.points, "points"),
            (&mut self.order, "order"),
            (&mut self.count, "count"),
            (&mut self.frequency, "frequency"),
            (&mut self.cell, "cell"),
            (&mut self.touchstone, "touchstone"),
            (&mut self.format, "format"),
        ] {
            if let Some(t) = text(key) {
                *field = t;
            }
        }
        if let Some(list) = v.get("excite").and_then(Value::as_array) {
            self.excite = list.iter().filter_map(Value::as_str).take(1024).map(dialogs::bounded).collect();
        }
    }
}

emcraft_macros::named_enum! {
    /// Device choice progresses once from Pending to Automatic; any user command makes it
    /// Explicit. Opening another project preserves the choice for this machine.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum DeviceSelection {
        #[default]
        Pending = "pending",
        Automatic = "automatic",
        Explicit = "explicit",
    }
}

/// The workflow's state in the app.
#[derive(Default)]
pub struct Workflow {
    status: Option<(u64, [StepState; 8])>,
    /// The planes the model is symmetric about ([`emcraft_model::Model::find_symmetry`]), found
    /// once per revision for the Boundaries step.
    pub symmetry: Option<(u64, Vec<emcraft_model::Found>)>,
    /// The rotational symmetry it allows ([`emcraft_model::Model::find_rotation`]), alike.
    pub rotation: Option<(u64, Option<emcraft_model::symmetry::rotation::FoundRotation>)>,
    /// [`statuses_now`] worked out once for the frame, with what it was worked out from.
    shown: Option<(Shown, [StepState; 8])>,
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
    pub device_selection: DeviceSelection,
    /// Why Run was last refused for blocking problems: the steps still in the way show as
    /// needing attention, until a run starts, another project opens or nothing blocks it.
    pub refusal: Option<String>,
    /// The Setup form differs from the confirmed settings, or none are ([`unconfirmed`]).
    pub unconfirmed: bool,
}

/// What [`statuses_now`] depends on besides the session's statuses.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Shown {
    revision: u64,
    unconfirmed: bool,
    refused: bool,
    failed: bool,
}

impl Shown {
    fn of(app: &EmcraftApp) -> Shown {
        Shown {
            revision: app.revision,
            unconfirmed: app.session.model().job.setup().is_some() && unconfirmed(app),
            refused: app.workflow.refusal.is_some(),
            failed: run_failure(app).is_some(),
        }
    }
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

pub use emcraft_ui_core::steps::ModeFocus;

/// Brings the workflow's caches up to date before a frame is drawn: the steps' statuses (from
/// `project.status`, once per revision), the solve's estimate in the Mesh and Run steps, the
/// devices in the Setup step.
pub fn prepare(app: &mut EmcraftApp) {
    let key = app.revision;
    sync_setup(app);
    check_form(app);
    if app.workflow.status.as_ref().is_none_or(|(k, _)| *k != key) {
        app.workflow.status = Some((key, emcraft_engine::workflow::steps(&app.session)));
    }
    if app.place.step() == Some(Step::Boundaries) && app.workflow.symmetry.as_ref().is_none_or(|(k, _)| *k != key) {
        app.workflow.symmetry = Some((key, app.session.model().find_symmetry()));
    }
    if app.place.step() == Some(Step::Boundaries) && app.workflow.rotation.as_ref().is_none_or(|(k, _)| *k != key) {
        app.workflow.rotation = Some((key, app.session.model().find_rotation()));
    }
    // A refusal lasts while something blocks Run.
    if let Some(r) = app.workflow.refusal.take()
        && !run::preflight(&app.session, &statuses_now(app)[..]).blocking.is_empty()
    {
        app.workflow.refusal = Some(r);
    }
    let shown = Shown::of(app);
    if app.workflow.shown.as_ref().is_none_or(|(k, _)| *k != shown) {
        app.workflow.shown = Some((shown, statuses_now(app).into_owned()));
    }
    // What the step's estimate is of: one solve at the Setup's order (Mesh), the run as the
    // form sets it up (Setup, Run).
    let asked = match app.place.step() {
        Some(Step::Mesh) => {
            let order = app.workflow.fields.order.trim().parse::<u64>().ok().filter(|o| (1..=2).contains(o)).unwrap_or(2);
            Some(json!({"command": "solve.frequency", "params": {"points": 1}, "order": order}))
        }
        Some(Step::Setup | Step::Run) => {
            run::setup_params(&app.session, &mut app.workflow.fields.clone()).ok().map(|params| json!({"command": "solve.run", "params": params}))
        }
        _ => None,
    };
    if let Some(asked) = asked {
        Estimate::refresh(&mut app.workflow.estimate, &mut app.session, key, asked);
    }
    if app.place.step() == Some(Step::Setup) && app.workflow.devices.is_none() {
        let v = app.session.execute("solve.devices", &json!({})).unwrap_or_else(|e| json!({"error": e.to_string()}));
        match app.workflow.device_selection {
            DeviceSelection::Pending => {
                let (device, precision) = recommended_device(&v, app.session.model().analysis());
                let _ = app.session.execute("solve.device", &json!({"device": device.id(), "precision": precision.id()}));
                app.workflow.device_selection = DeviceSelection::Automatic;
            }
            DeviceSelection::Automatic | DeviceSelection::Explicit => {}
        }
        app.workflow.devices = Some(v);
    }
    // The open dialog's run, when it starts one.
    let dialog = app
        .ui_state
        .dialog()
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
    let t = &Tokens::get();
    let r = match e {
        Ok(e) => {
            let n = |k: &str| e.get(k).and_then(Value::as_f64);
            let measured = e.get("basis").and_then(Value::as_str) == Some("measured");
            let time = duration(n("seconds").unwrap_or(f64::NAN));
            let text = if measured { format!("⏱ ≈ {time}") } else { format!("⏱ ≈ {time} ({})", crate::lang::tr(t!("ref."))) };
            let mut detail = vec![e.get("what").and_then(Value::as_str).unwrap_or("").to_owned()];
            if let (Some(u), Some(mb)) = (n("unknowns"), n("memory_mb")) {
                detail.push(crate::lang::trf(t!("{n} unknowns, {mb} MB"), crate::lang::args!(n = format!("{u:.0}"), mb = format!("{mb:.0}"))));
            }
            detail.push(
                crate::lang::tr(if measured {
                    t!("scaled from this machine's last sweep")
                } else {
                    t!("from a reference machine; a sweep here measures this one")
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
/// need attention; after a run failed, Run does. Worked out once a frame (`prepare`), borrowed
/// from there while nothing it depends on changed.
pub fn statuses_now(app: &EmcraftApp) -> std::borrow::Cow<'_, [StepState; 8]> {
    let now = Shown::of(app);
    if let Some((k, states)) = &app.workflow.shown
        && *k == now
    {
        return std::borrow::Cow::Borrowed(states);
    }
    let mut states = match &app.workflow.status {
        Some((k, v)) if *k == app.revision => v.clone(),
        _ => emcraft_engine::workflow::steps(&app.session),
    };
    // Settings changed in the form but not confirmed: Setup is to do again, and Run waits.
    if app.session.model().job.setup().is_some() && unconfirmed(app) {
        let text = t!("the settings changed: confirm them again").en();
        if let Some(setup) = states.get_mut(Step::Setup.index()) {
            setup.status = setup.status.min(Status::Todo);
            setup.messages.push(Check::new(text, &[], Some("project.setup")));
        }
        if let Some(run) = states.get_mut(Step::Run.index()).filter(|r| r.status > Status::Waiting) {
            run.status = Status::Waiting;
            run.messages = vec![Check::new(t!("waits for {steps}").en(), &[("steps", Step::Setup.id().to_owned())], None)];
        }
    }
    if app.workflow.refusal.is_some() {
        let solver = app.session.model().analysis();
        for s in states.iter_mut().filter(|s| run::required(s.step, solver) && s.status == Status::Todo) {
            s.status = Status::Missing;
        }
    }
    if run_failure(app).is_some()
        && let Some(run) = states.get_mut(Step::Run.index())
    {
        run.status = run.status.min(Status::Missing);
    }
    std::borrow::Cow::Owned(states)
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
        "step": app.place.step().map(Step::id),
        "steps": steps,
        "running": running(app),
        "stale": states.get(Step::Results.index()).is_some_and(|s| s.status == Status::Stale),
        "messages": messages,
        "errors": f.errors.iter().map(|(k, v)| (k.to_string(), json!(v))).collect::<serde_json::Map<_, _>>(),
        "refused": app.workflow.refusal,
        "solver": solver(app).id(),
        "analysis": emcraft_engine::workflow::analysis_reply(app.session.model()),
        "not_needed": Step::ALL.into_iter().filter(|s| not_needed(app, *s).is_some()).map(Step::id).collect::<Vec<_>>(),
        "setup": {"start": f.start, "stop": f.stop, "points": f.points, "order": f.order, "count": f.count, "frequency": f.frequency, "cell": f.cell},
        "confirmed": !unconfirmed(app),
        "check": app.workflow.check.as_ref().map(|c| c.1.clone()),
        "diagram": f.diagram,
        "device": app.session.compute().device().id(),
        "device_selection": app.workflow.device_selection.id(),
        "precision": app.session.compute().precision.id(),
        "failure": app.failure.as_ref().map(|f| json!({"command": f.command, "error": f.error})),
        "mode_focus": app.workflow.mode_focus.as_ref().map(|m| json!({"port": m.port, "mode": m.mode})),
    })
}

/// The form follows the confirmed settings when they change (a confirmation, undo, a project
/// opened); settings taken back (another kind chosen) leave the form as typed.
pub fn sync_setup(app: &mut EmcraftApp) {
    let confirmed = app.session.execute("project.values", &json!({"command": "project.setup"})).unwrap_or_else(|_| json!({}));
    let f = &mut app.workflow.fields;
    if f.seen.as_ref() == Some(&confirmed) {
        return;
    }
    if confirmed.as_object().is_some_and(|o| !o.is_empty()) {
        f.load(&confirmed);
    } else {
        f.fill_defaults();
    }
    f.seen = Some(confirmed);
}

/// Whether the form differs from the confirmed settings (or none are confirmed), as of the
/// frame's start ([`prepare`]; the panel borrows the form while it draws).
pub fn unconfirmed(app: &EmcraftApp) -> bool {
    app.workflow.unconfirmed
}

/// Works out [`unconfirmed`] from the form as it stands.
pub fn check_form(app: &mut EmcraftApp) {
    let f = &app.workflow.fields;
    let confirmed = f.seen.as_ref().filter(|v| v.as_object().is_some_and(|o| !o.is_empty()));
    app.workflow.unconfirmed = match (confirmed, run::setup_params(&app.session, &mut f.clone())) {
        (Some(c), Ok(p)) => !same(c, &p),
        _ => true,
    };
}

/// Two settings alike, numbers to a part in 10⁹ (a unit's round trip).
pub(crate) fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w))),
        (Value::Number(x), Value::Number(y)) => match (x.as_f64(), y.as_f64()) {
            (Some(x), Some(y)) => (x - y).abs() <= 1e-9 * x.abs().max(y.abs()),
            _ => x == y,
        },
        _ => a == b,
    }
}

/// The solve's estimate (`project.estimate`), as `prepare` left it for this revision.
pub fn estimate(app: &EmcraftApp) -> Result<&Value, &str> {
    run_estimate(app).ok_or("no estimate yet")?.as_ref().map_err(String::as_str)
}

/// Opens a step: its panel and the view it calls for.
/// Whether the job is a system's (its chain, no model to solve a field in).
pub(crate) fn is_system(app: &EmcraftApp) -> bool {
    app.session.model().analysis() == Solver::System
}

/// Whether the bar shows `step`: every step but the kind; a system's none (its canvas holds
/// its chain, its band and its budget, read again at every change).
pub(crate) fn on_bar(app: &EmcraftApp, step: Step) -> bool {
    step != Step::Intent && !is_system(app)
}

/// A step's number and name as the bar and its panel show them.
pub(crate) fn numbered(_app: &EmcraftApp, step: Step) -> (usize, &'static str) {
    (step.index(), crate::lang::shown(step))
}

pub fn open_step(app: &mut EmcraftApp, step: Option<Step>) {
    if app.session.model().analysis() == Solver::Pcb && step.is_some() {
        app.place = app.place.show(View::Pcb);
        return;
    }
    // Step 0, the kind of analysis, is the step bar's switch: no panel, the model in view.
    if step == Some(Step::Intent) {
        app.place = View::Model.place();
        return;
    }

    app.place = app.place.with_step(step);
    let Some(step) = step else { return };
    // A system has no steps: its canvas holds its chain, band and budget.
    if is_system(app) {
        app.tab = RibbonTab::System;
        app.place = View::System.place();
        return;
    }
    // The ribbon shows the step's tab.
    app.tab = match step {
        Step::Intent | Step::Geometry => RibbonTab::Model,
        Step::Boundaries => RibbonTab::Boundaries,
        Step::Excitation => RibbonTab::PortsSources,
        Step::Mesh => RibbonTab::Mesh,
        Step::Setup | Step::Run => RibbonTab::Simulate,
        Step::Results => RibbonTab::Results,
    };
    app.place = app.place.show(match step {
        Step::Intent => View::Model,
        Step::Results => crate::after::results_view(&app.session),
        _ => View::Model,
    });
    // A dialog of another step closes.
    if app.ui_state.dialog().is_some_and(|d| Step::of_command(d.command.id) != Some(step)) {
        app.ui_state.take_dialog();
    }
}

/// What a check's Fix does: the step that owns its command, with that command's dialog.
pub fn fix(app: &mut EmcraftApp, command: &str) -> Result<(), UiError> {
    let step = Step::of_command(command);
    if step.is_some() {
        open_step(app, step);
    }
    if !matches!(command, "project.setup" | "project.analysis") && !is_run(command) {
        app.open_dialog(command)?;
    }
    Ok(())
}

/// A positive finite number from a field, or why not.
fn positive(text: &str) -> Result<f64, String> {
    let v = dialogs::parse_number(text)?;
    if !(v > 0.0) {
        return Err(crate::lang::tr(t!("must be greater than 0")).into());
    }
    Ok(v)
}

/// An integer within `lo..=hi`.
fn integer(text: &str, lo: u64, hi: u64) -> Result<u64, String> {
    let s = text.trim();
    let range = crate::lang::args!(lo = lo, hi = hi);
    let v: u64 = s.parse().map_err(|_| crate::lang::trf(t!("must be a whole number from {lo} to {hi}"), range))?;
    if !(lo..=hi).contains(&v) {
        return Err(crate::lang::trf(t!("must be from {lo} to {hi}"), range));
    }
    Ok(v)
}

/// The band (display units) the session holds.
fn band(s: &emcraft_engine::Session) -> Option<(f64, f64)> {
    let f = s.display().frequency;
    s.model().job.range().map(|b| (f.from_si(b.start.get()), f.from_si(b.stop.get())))
}

/// Prefer the available CUDA accelerator with double precision, preserving an explicit user
/// choice elsewhere. The CubeCL CPU runtime is a verification backend, not an upgrade.
fn recommended_device(list: &Value, solver: Solver) -> (Device, Precision) {
    use {Device::*, Precision::*};
    let candidates: &[(Device, Precision)] = match solver {
        Solver::TimeDomain => &[(Cuda, Double), (Cuda, Single), (Cpu, Double)],
        _ => &[(Cuda, Double), (Cpu, Double)],
    };
    let rows = device_rows(list);
    candidates.iter().copied().find(|&(device, p)| rows.iter().any(|r| r.device == device && r.available && r.runs(p))).unwrap_or((Cpu, Double))
}

/// A device as `solve.devices` lists it.
pub(crate) struct DeviceRow {
    pub device: Device,
    /// Whether this machine runs it.
    pub available: bool,
    /// The precisions it runs, in [`Precision::ALL`]'s order.
    pub precisions: Vec<Precision>,
    /// Why it cannot run here.
    pub problem: Option<String>,
}

impl DeviceRow {
    pub fn runs(&self, p: Precision) -> bool {
        self.precisions.contains(&p)
    }
}

/// The devices of a `solve.devices` reply (names this build does not know are left out).
pub(crate) fn device_rows(list: &Value) -> Vec<DeviceRow> {
    let rows = list.get("devices").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    rows.iter()
        .filter_map(|d| {
            let device = d.get("name").and_then(Value::as_str).and_then(Device::parse)?;
            let listed = d.get("precisions").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
            let precisions = Precision::ALL.into_iter().filter(|p| listed.iter().any(|v| v == p.id())).collect();
            let problem = d.get("problem").and_then(Value::as_str).map(str::to_owned);
            Some(DeviceRow { device, available: d.get("available").and_then(Value::as_bool) == Some(true), precisions, problem })
        })
        .collect()
}
#[cfg(test)]
mod device_preference_tests {
    use super::*;
    #[test]
    fn advanced_available_device_wins_and_unsupported_double_falls_back() {
        let mut list = json!({"devices":[{"name":"cpu","available":true,"precisions":["double"]},{"name":"cuda","available":true,"precisions":["single","double"]}]});
        assert_eq!(recommended_device(&list, Solver::Frequency), (Device::Cuda, Precision::Double));
        list["devices"][1]["precisions"] = json!(["single"]);
        assert_eq!(recommended_device(&list, Solver::Frequency), (Device::Cpu, Precision::Double));
        assert_eq!(recommended_device(&list, Solver::TimeDomain), (Device::Cuda, Precision::Single));
        list["devices"][1]["available"] = json!(false);
        assert_eq!(recommended_device(&list, Solver::TimeDomain), (Device::Cpu, Precision::Double));
    }
}
