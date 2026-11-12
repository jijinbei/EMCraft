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
use serde_json::{Value, json};

use crate::handle::{Fixed, Handle};
use crate::ribbon::RibbonTab;
use crate::theme::Tokens;
use crate::{Action, Actions, EmcraftApp, UiError, UiExt, View, dialogs, scenario, ui_kit};

pub use emcraft_engine::workflow::{Check, Status, Step, StepState};
pub use emcraft_ui_core::steps::devices::{DeviceRow, DeviceSelection, recommended as recommended_device, rows as device_rows};
pub use emcraft_ui_core::steps::form::{Fields, integer, positive};
pub use emcraft_ui_core::steps::{ESTIMATED, check_text, is_run, overlay};

mod bar;
mod panel;
mod run;

pub use bar::bar;
pub(crate) use bar::{Look, Pill, pills};
pub use panel::{export_touchstone, not_needed, panel};
pub use run::{Preflight, confirm_setup, preflight, setup_params, start_run};

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

/// Whether command `id` is a solve ([`emcraft_engine::CommandSpec::solves`]).
pub use emcraft_ui_core::job::is_solve;

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

pub use emcraft_ui_core::job::duration;

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
    let states = match &app.workflow.status {
        Some((k, v)) if *k == app.revision => v.clone(),
        _ => emcraft_engine::workflow::steps(&app.session),
    };
    let window = emcraft_ui_core::steps::run::Window { unconfirmed: now.unconfirmed, refused: now.refused, failed: now.failed };
    let states = emcraft_ui_core::steps::run::shown(states, window, app.session.model().analysis());
    std::borrow::Cow::Owned(states)
}

/// Why the last run failed, while that is the last word (the Run panel shows it in full).
pub fn run_failure(app: &EmcraftApp) -> Option<&str> {
    app.failure.as_ref().filter(|f| is_run(&f.command) && app.job.is_none()).map(|f| f.error.as_str())
}

/// What the Setup panel's band diagram draws, for `ui.inspect`: the cut-offs' labels and the
/// solve's frequencies.
fn diagram(app: &EmcraftApp) -> Option<Value> {
    use emcraft_ui_core::steps::band;
    let f = band::with_proposal(app.workflow.fields.clone().with_defaults(), &app.session);
    let solves = band::solves(solver(app), &f, app.session.limits().frequencies as u64);
    (app.place.step() == Some(Step::Setup)).then(|| band::diagram(&app.session, &f, solves).map(|d| d.inspect())).flatten()
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
        "diagram": diagram(app),
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
    app.workflow.fields = std::mem::take(&mut app.workflow.fields).synced(confirmed);
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

pub(crate) use emcraft_ui_core::steps::same;

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

/// The band (display units) the session holds.
fn band(s: &emcraft_engine::Session) -> Option<(f64, f64)> {
    let f = s.display().frequency;
    s.model().job.range().map(|b| (f.from_si(b.start.get()), f.from_si(b.stop.get())))
}
