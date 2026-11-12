//! The workflow in the iced window (docs/ux/workflow.md §2–3), as the shared rules decide it
//! ([`emcraft_ui_core::steps`]): under the ribbon, the kind of analysis (a choice that runs
//! `project.analysis`) and the steps from the geometry to the results, each a pill with its
//! status's mark — ✔ done, ○ to do, ⋯ waiting on an earlier step, ! a warning, ✕ needs
//! attention, ↻ out of date, ◌ while a run goes on, ⊖ not needed for the kind chosen — from
//! `project.status`, with what the window knows on top (settings not confirmed, a refused Run,
//! a failed run). A pill opens its step: its panel at the right, the ribbon's tab and the 3D
//! view's overlay for it.

use emcraft_engine::workflow::{Status, Step, StepState};
use emcraft_ui_core::lang::{shown, tr};
use emcraft_ui_core::steps::devices::DeviceSelection;
use emcraft_ui_core::steps::form::Fields;
use emcraft_ui_core::steps::run::Window;
use emcraft_ui_core::steps::{Solver, kinds};
use emcraft_ui_core::view::View;
use iced::widget::{button, container, pick_list, row, text};
use iced::{Alignment, Color, Element, Fill, Widget};
use serde_json::{Value, json};

use crate::app::{App, Message};
use crate::look::{self, Tokens};
use emcraft_ui_core::system::gesture::Command;

/// The workflow's state in the window: the step open, the session's statuses (for the window's
/// generation of the project), the Setup form, why Run was last refused, the last run that
/// failed (its command and why), the devices this machine runs and how the device was chosen.
#[derive(Clone, Debug, Default)]
pub struct Workflow {
    pub step: Option<Step>,
    states: Option<(u64, [StepState; 8])>,
    pub fields: Fields,
    pub refusal: Option<String>,
    pub failure: Option<(String, String)>,
    pub devices: Option<Value>,
    pub selection: DeviceSelection,
    /// `results.check`'s last reply.
    pub check: Option<Value>,
    /// `project.estimate`'s reply for the step's run, with the generation and the request it
    /// was asked for.
    estimate: Option<(u64, Value, Result<Value, String>)>,
}

/// A status's mark and the token colour it is drawn in.
pub fn mark(t: Tokens, status: Status) -> (&'static str, Color) {
    match status {
        Status::Done => ("✔", t.success),
        Status::Todo => ("○", t.text_dim),
        Status::Waiting => ("⋯", t.text_dim),
        Status::Warning => ("!", t.warning),
        Status::Missing => ("✕", t.error),
        Status::Stale => ("↻", t.info),
    }
}

/// How a pill looks: the run under way, a step not needed (skipped once done), or its status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Look {
    Running,
    Skipped,
    Unneeded(Status),
    Plain(Status),
}

impl Workflow {
    /// The workflow for another project: nothing open, refused or failed, its own form; the
    /// machine's devices and how the device was chosen kept.
    pub fn for_another_project(self) -> Workflow {
        Workflow { devices: self.devices, selection: self.selection, ..Workflow::default() }
    }
}

impl App {
    /// Brings the workflow up to date after a change: the session's statuses once per
    /// generation, the Setup form following the confirmed settings, a refusal kept while
    /// something still blocks Run.
    pub(crate) fn refresh_steps(&mut self) {
        let generation = self.generation;
        if self.workflow.states.as_ref().is_none_or(|(g, _)| *g != generation) {
            let states = emcraft_engine::workflow::steps(&self.session);
            self.workflow.states = Some((generation, states));
            let confirmed = self.session.execute("project.values", &json!({"command": "project.setup"})).unwrap_or_else(|_| json!({}));
            self.workflow.fields = std::mem::take(&mut self.workflow.fields).synced(confirmed);
        }
        if self.workflow.refusal.is_some() && emcraft_ui_core::steps::run::preflight(self.solver(), &self.statuses()).blocking.is_empty() {
            self.workflow.refusal = None;
        }
        match self.workflow.step {
            Some(Step::Setup) => {
                let f = std::mem::take(&mut self.workflow.fields).with_defaults();
                self.workflow.fields = emcraft_ui_core::steps::band::with_proposal(f, &self.session);
                self.devices();
            }
            Some(Step::Results) => {
                let ports = self.session.sparameter_entries().len().max(1);
                let f = &mut self.workflow.fields;
                if f.touchstone.is_empty() {
                    f.touchstone = format!("emcraft.s{ports}p");
                }
                if f.format.is_empty() {
                    "ri".clone_into(&mut f.format);
                }
            }
            _ => {}
        }
        self.estimate_run();
    }

    /// The devices this machine runs, asked once; the first time, the one recommended chosen.
    fn devices(&mut self) {
        if self.workflow.devices.is_some() {
            return;
        }
        let v = self.session.execute("solve.devices", &json!({})).unwrap_or_else(|e| json!({"error": e.to_string()}));
        if self.workflow.selection == DeviceSelection::Pending {
            let (device, precision) = emcraft_ui_core::steps::devices::recommended(&v, self.solver());
            let _ = self.session.execute("solve.device", &json!({"device": device.id(), "precision": precision.id()}));
            self.workflow.selection = DeviceSelection::Automatic;
        }
        self.workflow.devices = Some(v);
    }

    /// The step's estimate (Mesh: one solve at the form's order; Setup and Run: the run as the
    /// form sets it up), asked again only when the project or the request changed.
    fn estimate_run(&mut self) {
        let f = &self.workflow.fields;
        let asked = match self.workflow.step {
            Some(Step::Mesh) => {
                let order = f.order.trim().parse::<u64>().ok().filter(|o| (1..=2).contains(o)).unwrap_or(2);
                Some(json!({"command": "solve.frequency", "params": {"points": 1}, "order": order}))
            }
            Some(Step::Setup | Step::Run) => {
                let (params, _) = emcraft_ui_core::steps::form::checked(self.solver(), self.session.limits().frequencies as u64, f.clone());
                params.ok().map(|p| json!({"command": "solve.run", "params": p}))
            }
            _ => None,
        };
        let Some(asked) = asked else { return };
        if self.workflow.estimate.as_ref().is_some_and(|(g, a, _)| *g == self.generation && *a == asked) {
            return;
        }
        let reply = self.session.execute("project.estimate", &asked).map_err(|e| e.to_string());
        self.workflow.estimate = Some((self.generation, asked, reply));
    }

    /// The step's estimate, when it is the project's as it stands.
    pub(crate) fn estimate(&self) -> Option<&Result<Value, String>> {
        self.workflow.estimate.as_ref().filter(|(g, _, _)| *g == self.generation).map(|(_, _, r)| r)
    }

    /// The Setup step's band diagram as the form stands.
    pub(crate) fn band_diagram(&self) -> Option<emcraft_ui_core::steps::band::Diagram> {
        let f = &self.workflow.fields;
        let solves = emcraft_ui_core::steps::band::solves(self.solver(), f, self.session.limits().frequencies as u64);
        emcraft_ui_core::steps::band::diagram(&self.session, f, solves)
    }

    /// What a step panel's control asks for.
    pub(crate) fn step_act(&mut self, a: crate::step_panel::Act) {
        use crate::step_panel::Act;
        match a {
            Act::Fix(command) => {
                let (step, dialog) = emcraft_ui_core::steps::fix(&command);
                if step.is_some() {
                    self.open_step(step);
                }
                if dialog {
                    match self.open(&command, &json!({})) {
                        Ok(d) => self.dialog = Some(d),
                        Err(e) => self.error = Some(e),
                    }
                }
            }
            Act::Typed(name, s) => self.workflow.fields = std::mem::take(&mut self.workflow.fields).typed(name, &s),
            Act::Format(v) => self.workflow.fields = std::mem::take(&mut self.workflow.fields).typed("format", v),
            Act::Confirm => self.confirm_setup(),
            Act::Run => self.start_run(),
            Act::Device(device, precision) => {
                self.workflow.selection = DeviceSelection::Explicit;
                self.run_now("solve.device", &json!({"device": device.id(), "precision": precision.id()}));
            }
            Act::PreviewModes if self.session.model().job.range().is_some() => self.start("solve.port_modes", json!({})),
            Act::PreviewModes => match self.open("solve.port_modes", &json!({})) {
                Ok(d) => self.dialog = Some(d),
                Err(e) => self.error = Some(e),
            },
            Act::CheckResults => match self.session.execute("results.check", &json!({})) {
                Ok(v) => self.workflow.check = Some(v),
                Err(e) => self.error = Some(e.to_string()),
            },
            Act::Export => self.export_touchstone(),
        }
    }

    /// The Setup form confirmed (`project.setup`), each bad field marked.
    fn confirm_setup(&mut self) {
        let f = Fields { tried: true, ..std::mem::take(&mut self.workflow.fields) };
        let (params, f) = emcraft_ui_core::steps::form::checked(self.solver(), self.session.limits().frequencies as u64, f);
        self.workflow.fields = f;
        match params {
            Ok(p) => self.run_now("project.setup", &p),
            Err(e) => self.error = Some(e),
        }
    }

    /// Run: refused while something blocks it (what blocks marked), warnings accepted; a
    /// board's run is its diagnosis.
    fn start_run(&mut self) {
        if self.solver() == Solver::Pcb {
            self.page = View::Pcb;
            self.start("pcb.inspect", json!({}));
            return;
        }
        let pre = emcraft_ui_core::steps::run::preflight(self.solver(), &self.statuses());
        if let Some(refusal) = pre.refusal() {
            self.error = Some(refusal.clone());
            self.workflow.refusal = Some(refusal);
            return;
        }
        self.workflow.refusal = None;
        self.workflow.failure = None;
        self.start("solve.run", json!({}));
    }

    /// The Touchstone file the Results step names, written.
    fn export_touchstone(&mut self) {
        let f = &self.workflow.fields;
        let (path, format) = (f.touchstone.trim().to_owned(), f.format.clone());
        let error = if path.is_empty() {
            Some(tr(t!("give a file")).to_owned())
        } else {
            self.session.execute("results.touchstone", &json!({"path": path, "format": format})).err().map(|e| e.to_string())
        };
        self.workflow.fields = std::mem::take(&mut self.workflow.fields).with_error("touchstone", error);
    }

    /// The analysis the project runs.
    pub(crate) fn solver(&self) -> Solver {
        self.session.model().analysis()
    }

    /// Whether the Setup form differs from the confirmed settings (or none are confirmed).
    pub(crate) fn unconfirmed(&self) -> bool {
        let f = &self.workflow.fields;
        let confirmed = f.seen.as_ref().filter(|v| v.as_object().is_some_and(|o| !o.is_empty()));
        let (params, _) = emcraft_ui_core::steps::form::checked(self.solver(), self.session.limits().frequencies as u64, f.clone());
        match (confirmed, params) {
            (Some(c), Ok(p)) => !emcraft_ui_core::steps::same(c, &p),
            _ => true,
        }
    }

    /// Whether a run is going on.
    pub(crate) fn running(&self) -> bool {
        self.job.as_ref().is_some_and(|j| emcraft_ui_core::steps::is_run(&j.command))
    }

    /// Why the last run failed, while that is the last word.
    pub(crate) fn run_failure(&self) -> Option<&str> {
        self.workflow.failure.as_ref().filter(|(c, _)| emcraft_ui_core::steps::is_run(c) && self.job.is_none()).map(|(_, e)| e.as_str())
    }

    /// The steps' statuses as the window shows them.
    pub(crate) fn statuses(&self) -> [StepState; 8] {
        let states = match &self.workflow.states {
            Some((g, s)) if *g == self.generation => s.clone(),
            _ => emcraft_engine::workflow::steps(&self.session),
        };
        let window = Window {
            unconfirmed: self.session.model().job.setup().is_some() && self.unconfirmed(),
            refused: self.workflow.refusal.is_some(),
            failed: self.run_failure().is_some(),
        };
        emcraft_ui_core::steps::run::shown(states, window, self.solver())
    }

    /// Opens `step` (none: closes the one open): its panel, the ribbon's tab and the view it
    /// calls for; a board's steps are its PCB page, a system's its chain, the kind (step 0) the
    /// model with no panel. A dialog of another step closes.
    pub(crate) fn open_step(&mut self, step: Option<Step>) {
        let (home, settings) = (false, false);
        (self.home, self.settings) = (home, settings);
        match (self.solver(), step) {
            (_, None) => self.workflow.step = None,
            (Solver::Pcb, Some(_)) => self.page = View::Pcb,
            (Solver::System, Some(_)) => self.page = View::System,
            (_, Some(Step::Intent)) => {
                self.workflow.step = None;
                self.page = View::Model;
            }
            (_, Some(step)) => {
                self.workflow.step = Some(step);
                self.tab = emcraft_ui_core::steps::tab(step);
                self.page = if step == Step::Results { emcraft_ui_core::after::results_view(&self.session) } else { View::Model };
                if self.page == View::Model {
                    let overlay = emcraft_ui_core::steps::overlay(step);
                    self.view3d = std::mem::take(&mut self.view3d).navigated(crate::view3d::Nav::Overlay(overlay));
                }
                if self.dialog.as_ref().is_some_and(|d| Step::of_command(d.command.id) != Some(step)) {
                    self.dialog = None;
                }
            }
        }
    }
}

/// Whether the bar shows: a project with steps (not a board's nor a system's), outside the start
/// and settings pages.
pub fn shown_bar(app: &App) -> bool {
    !app.home && !app.settings && !matches!(app.solver(), Solver::Pcb | Solver::System)
}

/// The kind of analysis: the job's, marked "auto" while it is inferred; another chosen runs
/// `project.analysis`.
fn kind(app: &App) -> Element<'_, Message> {
    let t = app.look.tokens;
    let m = app.session.model();
    let (current, chosen) = (m.analysis(), m.job.chosen());
    let name = move |k: Solver| {
        let n = tr(kinds::card(k).name);
        if chosen.is_none() && k == current { format!("{n} ({})", tr(t!("auto"))) } else { n.to_owned() }
    };
    let idle = app.job.is_none();
    let list = pick_list(Some(current), Solver::ALL, move |k: &Solver| name(*k)).text_size(t.font_button).style(look::choice(t));
    let choose = move |k: Solver| Message::Run(Command { id: "project.analysis", params: json!({"kind": k.id()}) });
    let list = if idle { list.on_select(choose) } else { list };
    list.boxed()
}

/// The step bar: the kind of analysis, then a pill a step from the geometry on.
pub fn bar(app: &App) -> Element<'_, Message> {
    let t = app.look.tokens;
    let states = app.statuses();
    let running = app.running();
    let m = app.session.model();
    let pills = states.iter().filter(|s| s.step != Step::Intent).map(|s| {
        let unneeded = emcraft_ui_core::steps::not_needed(m, s.step).is_some();
        let look = match (running && s.step == Step::Run, unneeded, s.status) {
            (true, _, _) => Look::Running,
            (false, true, Status::Done) => Look::Skipped,
            (false, true, status) => Look::Unneeded(status),
            (false, false, status) => Look::Plain(status),
        };
        let (symbol, ink) = match look {
            Look::Running => ("◌", t.accent),
            Look::Skipped => ("⊖", t.text_dim),
            Look::Unneeded(status) | Look::Plain(status) => mark(t, status),
        };
        let open = app.workflow.step == Some(s.step);
        let dim = matches!(look, Look::Skipped | Look::Unneeded(_) | Look::Plain(Status::Waiting));
        let (ink, name_ink) = match (open, dim) {
            (true, _) => (t.on_accent, t.on_accent),
            (false, true) => (ink, t.text_dim),
            (false, false) => (ink, t.text),
        };
        let name = text(format!("{} {}", s.step.index(), shown(s.step))).size(t.font_button).color(name_ink);
        let content = row![text(symbol).size(t.font_button).color(ink), name].spacing(6).align_y(Alignment::Center);
        button(content).style(look::button(t, open)).on_press(Message::Step(if open { None } else { Some(s.step) })).boxed()
    });
    let line = row(std::iter::once(kind(app)).chain(pills)).spacing(6).align_y(Alignment::Center).wrap().vertical_spacing(6);
    container(line).padding([4, 14]).width(Fill).boxed()
}

/// The workflow's state for `ui.inspect`: the step open, each step's status and checks, the
/// kind, the Setup form, whether its settings are confirmed, a refusal and a failure.
pub fn inspect(app: &App) -> Value {
    let states = app.statuses();
    let steps: serde_json::Map<String, Value> = states.iter().map(|s| (s.step.id().to_owned(), json!(s.status.id()))).collect();
    let messages: serde_json::Map<String, Value> = states
        .iter()
        .map(|s| (s.step.id().to_owned(), json!(s.messages.iter().map(|m| json!({"text": m.text, "command": m.command})).collect::<Vec<_>>())))
        .collect();
    let f = &app.workflow.fields;
    let fields: serde_json::Map<String, Value> = emcraft_ui_core::steps::form::NAMES.iter().map(|n| ((*n).to_owned(), json!(f.text(n)))).collect();
    json!({
        "step": app.workflow.step.map(Step::id),
        "steps": steps,
        "messages": messages,
        "running": app.running(),
        "solver": app.solver().id(),
        "setup": fields,
        "errors": f.errors.iter().map(|(k, v)| ((*k).to_owned(), json!(v))).collect::<serde_json::Map<_, _>>(),
        "confirmed": !app.unconfirmed(),
        "refused": app.workflow.refusal,
        "failure": app.workflow.failure.as_ref().map(|(c, e)| json!({"command": c, "error": e})),
        "check": app.workflow.check,
    })
}
