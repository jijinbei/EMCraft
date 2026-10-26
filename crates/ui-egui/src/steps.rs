//! The workflow (docs/ux/workflow.md §2–3): a step bar under the ribbon, steps 0 Intent to
//! 8 Results each with its status (done, warning, missing, stale) from `project.status`, and the
//! active step's panel at the right: its settings (the step's commands, their dialogs docked in
//! the panel), its checks with a button to the fix, and the 3D view in the step's overlay.
//! Excitation previews the port modes, Mesh shows the mesh's statistics and the solve's
//! estimate (`project.estimate`), Run checks the model first (blocking problems refuse to start,
//! warnings can be accepted), Results marks results of a changed model stale and exports and
//! checks them.

use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, pos2, vec2};
use serde_json::{Value, json};

use crate::scene3d::Overlay;
use crate::theme::Tokens;
use crate::{EmcraftApp, UiError, View, dialogs, glyphs, scenario, ui_kit};

pub use emcraft_engine::workflow::{Check, Status, Step, StepState};

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
        "solver": f.solver.unwrap_or_else(|| Solver::infer(&app.session.model)).id(),
        "check": app.workflow.check.as_ref().map(|c| c.1.clone()),
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
            if app.session.results.sweep.is_some() || app.session.results.time_domain.is_some() {
                View::SParameters
            } else if app.session.results.eigen.is_some() {
                View::Eigenmodes
            } else if app.session.results.radiation.is_some() || app.session.results.td_radiation.is_some() {
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

// ---- the step bar --------------------------------------------------------------------------

const BAR_HEIGHT: f32 = 40.0;

/// The step bar under the ribbon.
pub fn bar(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let states = statuses(app);
    egui::Panel::top("emcraft_steps")
        .exact_size(BAR_HEIGHT)
        .show_separator_line(false)
        .frame(egui::Frame::NONE.fill(t.viewport_top).inner_margin(egui::Margin { left: 6, right: 6, top: 0, bottom: 6 }))
        .show(ui, |ui| {
            let card = ui.max_rect();
            ui.painter().add(t.card(t.toolbar).paint(card));
            scenario::publish_rect("steps", card);
            let font = FontId::proportional(t.font_body);
            let mut x = card.left() + 8.0;
            let mut clicked = None;
            for (i, s) in states.iter().enumerate() {
                let text = format!("{} {}", i, s.step.label());
                let g = ui.painter().layout_no_wrap(text.clone(), font.clone(), t.text);
                let r = Rect::from_min_size(pos2(x, card.top() + 4.0), vec2(g.size().x + 34.0, card.height() - 8.0));
                let resp = ui.interact(r, ui.id().with(("step", i)), Sense::click());
                let tip = if s.messages.is_empty() {
                    format!("{}: {}", s.step.label(), s.status.id())
                } else {
                    s.messages.iter().map(|m| m.text.as_str()).collect::<Vec<_>>().join("\n")
                };
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, s.step.label()));
                let active = app.workflow.step == Some(s.step);
                let fill = if active {
                    t.accent_soft
                } else if resp.hovered() {
                    t.hover
                } else {
                    Color32::TRANSPARENT
                };
                ui.painter().rect_filled(r, 12.0, fill);
                ui.painter().rect_stroke(r, 12.0, Stroke::new(1.0, if active { t.accent } else { t.border }), egui::StrokeKind::Inside);
                mark(ui.painter(), pos2(r.left() + 14.0, r.center().y), s.status, &t);
                ui.painter().galley(pos2(r.left() + 26.0, r.center().y - g.size().y / 2.0), g, if active { t.selected_ink } else { t.text });
                scenario::publish_target(&format!("step:{}", s.step.id()), r);
                if resp.on_hover_text(tip).clicked() {
                    clicked = Some(s.step);
                }
                x = r.right() + 4.0;
                if i + 1 < states.len() {
                    ui.painter().hline(x..=x + 8.0, r.center().y, Stroke::new(1.0, t.border));
                    x += 12.0;
                }
            }
            if let Some(step) = clicked {
                let again = app.workflow.step == Some(step);
                open_step(app, if again { None } else { Some(step) });
            }
        });
}

/// A status mark: a tick, an exclamation, a cross or a turning arrow in a disc.
fn mark(p: &egui::Painter, c: egui::Pos2, s: Status, t: &Tokens) {
    let col = status_color(s, t);
    p.circle_filled(c, 7.0, col);
    let ink = Stroke::new(1.8, Color32::WHITE);
    match s {
        Status::Done => {
            p.line_segment([c + vec2(-3.5, 0.2), c + vec2(-1.0, 2.8)], ink);
            p.line_segment([c + vec2(-1.0, 2.8), c + vec2(3.6, -2.6)], ink);
        }
        Status::Warning => {
            p.line_segment([c + vec2(0.0, -3.8), c + vec2(0.0, 1.0)], ink);
            p.circle_filled(c + vec2(0.0, 3.4), 1.1, Color32::WHITE);
        }
        Status::Missing => {
            p.line_segment([c + vec2(-3.0, -3.0), c + vec2(3.0, 3.0)], ink);
            p.line_segment([c + vec2(-3.0, 3.0), c + vec2(3.0, -3.0)], ink);
        }
        Status::Stale => {
            let pts: Vec<egui::Pos2> = (0..=10).map(|k| 0.6 + 4.6 * k as f32 / 10.0).map(|a| c + vec2(a.cos(), a.sin()) * 3.6).collect();
            p.add(egui::Shape::line(pts, ink));
            let end = c + vec2(5.2f32.cos(), 5.2f32.sin()) * 3.6;
            p.line_segment([end, end + vec2(2.6, 0.4)], ink);
        }
    }
}

// ---- the step panel ------------------------------------------------------------------------

/// The active step's panel at the right (with the open dialog docked in it).
pub fn panel(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    let Some(step) = app.workflow.step else { return };
    let t = Tokens::get();
    let states = statuses(app);
    let Some(state) = states.get(step.index()).cloned() else { return };
    egui::Panel::right("emcraft_step_panel")
        .exact_size(t.form_card_width + 40.0)
        .show_separator_line(false)
        .frame(egui::Frame::NONE.fill(t.viewport_top).inner_margin(egui::Margin { left: 0, right: 6, top: 0, bottom: 6 }))
        .show(ui, |ui| {
            let h = ui.available_height();
            t.card(t.panel).inner_margin(egui::Margin::symmetric(12, 10)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_min_height(h - 22.0);
                scenario::publish_rect("step_panel", ui.max_rect());
                header(ui, &state, &t);
                let dialog_h = if app.dialog.is_some() { (h * 0.58).max(260.0) } else { 0.0 };
                egui::ScrollArea::vertical()
                    .id_salt("step_panel_scroll")
                    .max_height((h - dialog_h - 70.0).max(120.0))
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        checks(app, ui, &state, &t);
                        body(app, ui, step, &states, &t);
                        commands(app, ui, step, &t);
                    });
                if app.dialog.is_some() {
                    ui.add_space(6.0);
                    t.card(t.dialog_body).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        crate::dialog_ui::embedded(app, ui, dialog_h - 10.0);
                    });
                }
            });
        });
}

fn header(ui: &mut egui::Ui, s: &StepState, t: &Tokens) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
        mark(ui.painter(), r.center(), s.status, t);
        ui.label(egui::RichText::new(format!("Step {} · {}", s.step.index(), s.step.label())).size(t.font_heading).strong().color(t.text));
        ui.label(egui::RichText::new(s.status.id()).size(t.font_small).color(status_color(s.status, t)));
    });
    ui.add_space(4.0);
}

/// The step's checks, each with its fix.
fn checks(app: &mut EmcraftApp, ui: &mut egui::Ui, s: &StepState, t: &Tokens) {
    if s.messages.is_empty() {
        ui.label(egui::RichText::new("Nothing to fix here.").size(t.font_small).color(t.success));
        ui.add_space(6.0);
        return;
    }
    let mut chosen = None;
    for (i, m) in s.messages.iter().enumerate() {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(&m.text).size(t.font_small).color(status_color(s.status, t)));
            // A fix that only leads back to this step is left out.
            let here = m.command.as_deref().is_some_and(|c| c.starts_with("solve.") && Step::of_command(c) == Some(s.step));
            if let Some(c) = m.command.as_ref().filter(|_| !here) {
                let r = ui.small_button(fix_label(c)).on_hover_text(format!("{c}: {}", crate::label(c)));
                scenario::publish_target(&format!("step:fix:{i}"), r.rect);
                if r.clicked() {
                    chosen = Some(c.clone());
                }
            }
        });
    }
    ui.add_space(6.0);
    if let Some(c) = chosen {
        fix(app, &c);
    }
}

fn fix_label(command: &str) -> String {
    match Step::of_command(command) {
        Some(step) if command.starts_with("solve.") && command != "solve.port_modes" => format!("Go to {}", step.label()),
        Some(Step::Intent) if command == "project.frequency" => "Set the band".into(),
        _ => format!("Fix: {}", crate::ribbon::button_label(command)),
    }
}

/// The step's commands as buttons.
fn commands(app: &mut EmcraftApp, ui: &mut egui::Ui, step: Step, t: &Tokens) {
    let list = step.commands();
    if list.is_empty() {
        return;
    }
    ui_kit::section(ui, "Commands");
    let busy = app.busy();
    let mut chosen = None;
    ui.horizontal_wrapped(|ui| {
        for id in list {
            let (r, resp) = ui.allocate_exact_size(vec2(96.0, 54.0), if busy { Sense::hover() } else { Sense::click() });
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, !busy, crate::ribbon::button_label(id)));
            let active = app.dialog.as_ref().is_some_and(|d| d.command.id == *id);
            if active {
                ui.painter().rect_filled(r, t.radius_sm, t.accent_soft);
            } else if resp.hovered() && !busy {
                ui.painter().rect_filled(r, t.radius_sm, t.hover);
            }
            glyphs::paint(
                ui.painter(),
                Rect::from_center_size(pos2(r.center().x, r.top() + 16.0), vec2(22.0, 22.0)),
                glyphs::for_command(id),
                t.icon,
                t.accent,
            );
            ui.painter().text(
                pos2(r.center().x, r.top() + 40.0),
                Align2::CENTER_CENTER,
                crate::ribbon::button_label(id),
                FontId::proportional(t.font_small),
                t.text,
            );
            scenario::publish_target(&format!("steppanel:{id}"), r);
            if resp.on_hover_text(emcraft_engine::find_command(id).map_or("", |c| c.doc)).clicked() {
                chosen = Some(*id);
            }
        }
    });
    if let Some(id) = chosen
        && let Err(e) = app.open_dialog(id)
    {
        app.message(e, true);
    }
}

/// A one-line text field with its error under it; `true` when Enter was pressed in it.
fn field(ui: &mut egui::Ui, label: &str, text: &mut String, unit: &str, error: Option<&str>, handle: &str) -> bool {
    let t = Tokens::get();
    let mut enter = false;
    ui_kit::row(ui, label, false, |ui| {
        let mut edit = egui::TextEdit::singleline(text).desired_width((ui.available_width() - 44.0).max(60.0)).char_limit(64);
        if error.is_some() {
            edit = edit.background_color(t.form_error_fill).text_color(t.form_error);
        }
        let r = ui.add(edit);
        scenario::publish_target(handle, r.rect);
        enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if !unit.is_empty() {
            ui.label(egui::RichText::new(unit).size(t.font_small).color(t.text_dim));
        }
    });
    if let Some(e) = error {
        ui.horizontal(|ui| {
            ui.add_space(t.form_label_width + 6.0);
            ui.label(egui::RichText::new(e).size(t.font_small).color(t.form_error));
        });
    }
    enter
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

fn kv(ui: &mut egui::Ui, k: &str, v: String, t: &Tokens) {
    ui.horizontal(|ui| {
        ui.add_sized(vec2(t.form_label_width, 16.0), egui::Label::new(egui::RichText::new(k).size(t.font_small).color(t.text_dim)));
        ui.label(egui::RichText::new(v).size(t.font_small).color(t.text));
    });
}

/// The band (display units) the session holds.
fn band(app: &EmcraftApp) -> Option<(f64, f64)> {
    let f = app.session.display.frequency;
    app.session.model.band.map(|b| (f.from_si(b.start), f.from_si(b.stop)))
}

fn fmt(v: f64) -> String {
    let s = format!("{v:.6}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// The step's own settings and information.
fn body(app: &mut EmcraftApp, ui: &mut egui::Ui, step: Step, states: &[StepState], t: &Tokens) {
    let fu = app.session.display.frequency.symbol();
    let lu = app.session.display.length.symbol();
    // The band fields follow the session's band when it changes.
    let b = band(app);
    if app.workflow.fields.band_seen != b {
        if let Some((a, z)) = b {
            app.workflow.fields.start = fmt(a);
            app.workflow.fields.stop = fmt(z);
        }
        app.workflow.fields.band_seen = b;
    }
    match step {
        Step::Intent => intent(app, ui, fu, t),
        Step::Geometry => {
            ui_kit::section(ui, "Bodies");
            let tree = crate::browser::tree(app);
            for (_, rows) in tree.iter().filter(|(s, _)| *s == "Geometry") {
                for r in rows {
                    ui.label(egui::RichText::new(&r.text).size(t.font_small).color(t.text));
                }
            }
            if let Some(emcraft_model::Aabb { min: lo, max: hi }) = app.session.model.bounds() {
                let l = app.session.display.length;
                kv(
                    ui,
                    "extent",
                    format!("{} × {} × {} {lu}", fmt(l.from_si(hi.x - lo.x)), fmt(l.from_si(hi.y - lo.y)), fmt(l.from_si(hi.z - lo.z))),
                    t,
                );
            }
        }
        Step::Materials => {
            ui_kit::section(ui, "Materials");
            let m = &app.session.model;
            for (i, mat) in m.materials.iter().enumerate() {
                ui.horizontal(|ui| {
                    let c = crate::scene3d::material_rgb(m, i);
                    let (r, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                    ui.painter().rect_filled(r, 2.0, Color32::from_rgb(c.0, c.1, c.2));
                    let loss = if mat.loss_tangent > 0.0 { format!(", tan δ {}", mat.loss_tangent) } else { String::new() };
                    ui.label(egui::RichText::new(format!("{}: εr {}, μr {}{loss}", mat.name, mat.eps_r, mat.mu_r)).size(t.font_small).color(t.text));
                });
            }
        }
        Step::Excitation => excitation(app, ui, fu, t),
        Step::Boundaries => {
            ui_kit::section(ui, "Boundaries");
            let m = &app.session.model;
            kv(ui, "walls", m.wall_conductivity.map_or("perfect conductor".into(), |s| format!("σ = {s:.3e} S/m")), t);
            let l = app.session.display.length;
            kv(ui, "open", m.open.map_or("closed (no PML)".into(), |o| format!("PML {} {lu} thick", fmt(l.from_si(o.thickness)))), t);
        }
        Step::Mesh => mesh(app, ui, lu, t),
        Step::Setup => setup(app, ui, fu, t),
        Step::Run => run_panel(app, ui, states, t),
        Step::Results => results(app, ui, states, t),
    }
}

fn intent(app: &mut EmcraftApp, ui: &mut egui::Ui, fu: &str, t: &Tokens) {
    ui_kit::section(ui, "Frequency band");
    ui_kit::caption(ui, "Set once, early: it sizes the mesh (elements per wavelength at its top), the sweep and the port-mode preview.");
    let f = &mut app.workflow.fields;
    let (e0, e1) = (f.error("start").map(str::to_owned), f.error("stop").map(str::to_owned));
    let mut go = field(ui, "start", &mut f.start, fu, e0.as_deref(), "intent:start");
    go |= field(ui, "stop", &mut f.stop, fu, e1.as_deref(), "intent:stop");
    let busy = app.busy();
    go |= ui_kit::button(ui, "Apply", true, !busy, "intent:apply").clicked();
    if go && !busy {
        apply_band(app);
    }
    ui_kit::section(ui, "Problem");
    let kind = match Solver::infer(&app.session.model) {
        Solver::Eigenmodes => "a resonator (no excitation: its eigenmodes)",
        Solver::Radiation => "an antenna or a scatterer (an open problem)",
        _ => "a guided-wave component (S-parameters between ports)",
    };
    ui.label(egui::RichText::new(kind).size(t.font_small).color(t.text));
    let d = app.session.display;
    kv(ui, "units", format!("{}, {}", d.length.symbol(), d.frequency.symbol()), t);
}

/// Checks the band's fields one by one and sets the band (`project.frequency`).
pub fn apply_band(app: &mut EmcraftApp) {
    let f = &mut app.workflow.fields;
    let (a, z) = (positive(&f.start), positive(&f.stop));
    f.set_error("start", a.as_ref().err().cloned());
    f.set_error("stop", z.as_ref().err().cloned());
    let (Ok(a), Ok(z)) = (a, z) else { return };
    if z < a {
        f.set_error("stop", Some("must not be below the start".into()));
        return;
    }
    if let Err(e) = app.run("project.frequency", json!({"start": a, "stop": z})) {
        app.workflow.fields.set_error("stop", Some(e.to_string()));
    }
}

fn excitation(app: &mut EmcraftApp, ui: &mut egui::Ui, fu: &str, t: &Tokens) {
    ui_kit::section(ui, "Ports and sources");
    let m = &app.session.model;
    if m.ports.is_empty() && m.lumped.is_empty() && m.sources.is_empty() {
        ui.label(egui::RichText::new("none yet").size(t.font_small).color(t.text_dim));
    }
    for p in &m.ports {
        let at = match p.face {
            Some(_) => "on a face".to_owned(),
            None => format!("{} end", if p.end == emcraft_model::End::Start { "start" } else { "far" }),
        };
        ui.label(egui::RichText::new(format!("Port {}: {at}, {} modes", p.name, p.modes)).size(t.font_small).color(t.text));
    }
    for p in &m.lumped {
        ui.label(egui::RichText::new(format!("Lumped port {}: {} Ω", p.name, p.impedance)).size(t.font_small).color(t.text));
    }
    if !m.sources.is_empty() {
        ui.label(egui::RichText::new(format!("{} source(s)", m.sources.len())).size(t.font_small).color(t.text));
    }
    if m.ports.is_empty() {
        return;
    }
    ui_kit::section(ui, "Port modes");
    let ready = m.mesh.is_some();
    let at = band(app).map(|(_, z)| format!(" at {} {fu}", fmt(z))).unwrap_or_default();
    ui_kit::caption(ui, &format!("Each port's modes{at}: propagation, effective permittivity, impedance (seconds, no full solve)."));
    let busy = app.busy();
    if ui_kit::button(ui, "Preview port modes", true, ready && !busy, "excitation:preview")
        .on_hover_text(if ready { "" } else { "needs a mesh (step 5)" })
        .clicked()
    {
        let r =
            if app.session.model.band.is_some() { app.run("solve.port_modes", json!({})).map(|_| ()) } else { app.open_dialog("solve.port_modes") };
        if let Err(e) = r {
            app.message(e, true);
        }
    }
    let Some(reply) = app.port_modes.clone() else { return };
    let f = reply.get("frequency").and_then(Value::as_f64).unwrap_or(f64::NAN);
    ui.label(egui::RichText::new(format!("at {} {fu}", fmt(f))).size(t.font_small).color(t.text_dim));
    egui::Grid::new("port_modes_preview").num_columns(4).spacing(vec2(8.0, 2.0)).striped(true).show(ui, |ui| {
        let per = format!("β (rad/{})", reply.get("length_unit").and_then(Value::as_str).unwrap_or("m"));
        for h in ["port / mode", per.as_str(), "εeff", "Z (Ω)"] {
            ui.label(egui::RichText::new(h).size(t.font_small).color(t.text_dim));
        }
        ui.end_row();
        for p in reply.get("ports").and_then(Value::as_array).into_iter().flatten() {
            let name = p.get("name").and_then(Value::as_str).unwrap_or("?");
            for mode in p.get("modes").and_then(Value::as_array).into_iter().flatten().take(8) {
                let n = |k: &str| mode.get(k).and_then(Value::as_f64);
                let txt = |s: String| egui::RichText::new(s).size(t.font_small).color(t.text);
                ui.label(txt(format!("{name} · {}", mode.get("name").and_then(Value::as_str).unwrap_or(""))));
                if mode.get("propagating").and_then(Value::as_bool) == Some(true) {
                    ui.label(txt(n("beta").map_or("—".into(), |v| format!("{v:.4}"))));
                    ui.label(txt(n("eps_eff").map_or("—".into(), |v| format!("{v:.4}"))));
                    ui.label(txt(n("impedance").map_or("—".into(), |v| format!("{v:.2}"))));
                } else {
                    let unit = reply.get("length_unit").and_then(Value::as_str).unwrap_or("m");
                    ui.label(
                        egui::RichText::new(format!("cut off, {:.1} dB/{unit}", n("attenuation_db").unwrap_or(f64::NAN)))
                            .size(t.font_small)
                            .color(t.text_dim),
                    );
                    ui.label("");
                    ui.label("");
                }
                ui.end_row();
            }
        }
    });
    if ui_kit::button(ui, "Show the mode patterns", false, true, "excitation:patterns").clicked() {
        app.view = View::PortModes;
    }
}

fn mesh(app: &mut EmcraftApp, ui: &mut egui::Ui, lu: &str, t: &Tokens) {
    ui_kit::section(ui, "Mesh");
    let l = app.session.display.length;
    let Some(m) = app.session.model.mesh.as_ref() else {
        ui.label(egui::RichText::new("not meshed yet").size(t.font_small).color(t.text_dim));
        return;
    };
    kv(ui, "nodes", m.nodes.len().to_string(), t);
    kv(ui, "tetrahedra", m.tets.len().to_string(), t);
    kv(ui, "boundary faces", m.boundary.len().to_string(), t);
    let volume: f64 = (0..m.tets.len()).filter_map(|i| m.volume(i)).sum();
    let h = (6.0 * std::f64::consts::SQRT_2 * volume / m.tets.len().max(1) as f64).cbrt();
    kv(ui, "mean element", format!("{:.3} {lu}", l.from_si(h)), t);
    if let Some(b) = app.session.model.band {
        let index = app.session.model.materials.iter().map(|x| (x.eps_r * x.mu_r).sqrt()).fold(1.0, f64::max);
        let lambda = emcraft_model::units::C0 / (b.stop * index);
        kv(ui, "per wavelength", format!("{:.1} at the band's top", lambda / h.max(1e-300)), t);
    }
    ui_kit::section(ui, "Estimate");
    match estimate(app) {
        Ok(e) => {
            let n = |k: &str| e.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN);
            kv(ui, "unknowns", format!("{:.0} (order {})", n("unknowns"), e.get("order").and_then(Value::as_u64).unwrap_or(2)), t);
            kv(ui, "memory", format!("{:.0} MB", n("memory_mb")), t);
            let measured = e.get("from_this_session").and_then(Value::as_bool) == Some(true);
            kv(ui, "time", format!("{:.2} s per frequency{}", n("seconds_per_frequency"), if measured { "" } else { " (reference machine)" }), t);
        }
        Err(e) => {
            ui.label(egui::RichText::new(e).size(t.font_small).color(t.text_dim));
        }
    }
}

fn setup(app: &mut EmcraftApp, ui: &mut egui::Ui, fu: &str, t: &Tokens) {
    ui_kit::section(ui, "Solver");
    let inferred = Solver::infer(&app.session.model);
    let f = &mut app.workflow.fields;
    let mut solver = f.solver.unwrap_or(inferred);
    for s in Solver::ALL {
        let r = ui.radio_value(&mut solver, s, egui::RichText::new(s.label()).size(t.font_small));
        scenario::publish_target(&format!("setup:solver:{}", s.id()), r.rect);
    }
    if solver != f.solver.unwrap_or(inferred) || f.solver.is_some() {
        f.solver = Some(solver);
    }
    if f.points.is_empty() {
        f.points = "11".into();
    }
    if f.order.is_empty() {
        f.order = "2".into();
    }
    if f.count.is_empty() {
        f.count = "5".into();
    }
    ui_kit::section(ui, "Settings");
    let e = |f: &Fields, k: &str| f.error(k).map(str::to_owned);
    match solver {
        Solver::Frequency | Solver::TimeDomain => {
            let band = app.session.model.band.map(|b| {
                let u = app.session.display.frequency;
                format!("{} to {} {fu}", fmt(u.from_si(b.start)), fmt(u.from_si(b.stop)))
            });
            let f = &mut app.workflow.fields;
            ui.label(egui::RichText::new(format!("band: {}", band.unwrap_or_else(|| "not set (step 0)".into()))).size(t.font_small).color(t.text));
            let ep = e(f, "points");
            field(ui, "points", &mut f.points, "", ep.as_deref(), "setup:points");
        }
        Solver::Eigenmodes => {
            let f = &mut app.workflow.fields;
            let ec = e(f, "count");
            field(ui, "modes", &mut f.count, "", ec.as_deref(), "setup:count");
            let ef = e(f, "frequency");
            field(ui, "near", &mut f.frequency, fu, ef.as_deref(), "setup:frequency");
        }
        Solver::Radiation => {
            let f = &mut app.workflow.fields;
            let ef = e(f, "frequency");
            field(ui, "frequency", &mut f.frequency, fu, ef.as_deref(), "setup:frequency");
        }
    }
    if solver != Solver::TimeDomain {
        let f = &mut app.workflow.fields;
        let eo = e(f, "order");
        field(ui, "order", &mut f.order, "", eo.as_deref(), "setup:order");
    }
    ui_kit::caption(ui, "Run (step 7) starts this solve after checking the model.");
    // The fields' errors as typed.
    let _ = run_params(app);
}

/// The solver and its parameters from the setup's fields; each bad field is marked.
pub fn run_params(app: &mut EmcraftApp) -> Result<(Solver, Value), String> {
    let solver = app.workflow.fields.solver.unwrap_or_else(|| Solver::infer(&app.session.model));
    let band = band(app);
    let limit = app.session.limits.frequencies as u64;
    let f = &mut app.workflow.fields;
    if f.points.is_empty() {
        f.points = "11".into();
    }
    if f.order.is_empty() {
        f.order = "2".into();
    }
    if f.count.is_empty() {
        f.count = "5".into();
    }
    let order = integer(&f.order, 1, 2);
    let mut bad = Vec::new();
    let mut take = |name: &'static str, r: Result<Value, String>, f: &mut Fields| match r {
        Ok(v) => {
            f.set_error(name, None);
            Some(v)
        }
        Err(e) => {
            bad.push(format!("{name}: {e}"));
            f.set_error(name, Some(e));
            None
        }
    };
    // A frequency field left empty takes the band's middle.
    let freq = |text: &str| {
        if text.trim().is_empty() {
            band.map(|(a, z)| 0.5 * (a + z)).ok_or_else(|| "give a frequency (or set the band, step 0)".to_owned())
        } else {
            positive(text)
        }
    };
    let params = match solver {
        Solver::Frequency | Solver::TimeDomain => {
            let points = take("points", integer(&f.points.clone(), 1, limit).map(|v| json!(v)), f);
            let order = if solver == Solver::Frequency { take("order", order.map(|v| json!(v)), f) } else { Some(Value::Null) };
            if band.is_none() {
                bad.push("no frequency band: set it in step 0 (Intent)".into());
            }
            match (points, order) {
                (Some(p), Some(o)) if solver == Solver::Frequency => json!({"points": p, "order": o}),
                (Some(p), Some(_)) => match band {
                    Some((a, z)) => json!({"start": a, "stop": z, "points": p}),
                    None => json!({"points": p}),
                },
                _ => Value::Null,
            }
        }
        Solver::Eigenmodes => {
            let count = take("count", integer(&f.count.clone(), 1, 50).map(|v| json!(v)), f);
            let near = take("frequency", freq(&f.frequency.clone()).map(|v| json!(v)), f);
            let order = take("order", order.map(|v| json!(v)), f);
            match (count, near, order) {
                (Some(c), Some(n), Some(o)) => json!({"near": n, "count": c, "order": o}),
                _ => Value::Null,
            }
        }
        Solver::Radiation => {
            let fr = take("frequency", freq(&f.frequency.clone()).map(|v| json!(v)), f);
            let order = take("order", order.map(|v| json!(v)), f);
            match (fr, order) {
                (Some(fr), Some(o)) => json!({"frequency": fr, "order": o}),
                _ => Value::Null,
            }
        }
    };
    if bad.is_empty() { Ok((solver, params)) } else { Err(bad.join("; ")) }
}

/// The preflight: what blocks the run and what it may accept.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Preflight {
    pub blocking: Vec<(Step, String)>,
    pub warnings: Vec<(Step, String)>,
}

pub fn preflight(app: &mut EmcraftApp, states: &[StepState]) -> Preflight {
    let mut out = Preflight::default();
    let solver = app.workflow.fields.solver.unwrap_or_else(|| Solver::infer(&app.session.model));
    for s in states.iter().filter(|s| s.step.index() <= Step::Setup.index()) {
        // A resonator needs no excitation.
        if s.step == Step::Excitation && solver == Solver::Eigenmodes && s.status == Status::Missing {
            continue;
        }
        let list = match s.status {
            Status::Missing => &mut out.blocking,
            Status::Warning | Status::Stale => &mut out.warnings,
            Status::Done => continue,
        };
        if s.messages.is_empty() {
            list.push((s.step, format!("{}: {}", s.step.label(), s.status.id())));
        }
        for m in &s.messages {
            list.push((s.step, m.text.clone()));
        }
    }
    if let Err(e) = run_params(app) {
        out.blocking.push((Step::Setup, e));
    }
    out
}

/// Run: refuses on blocking problems; warnings are accepted by pressing Run.
pub fn start_run(app: &mut EmcraftApp) -> Result<Value, UiError> {
    let states = statuses(app);
    let pre = preflight(app, &states);
    if !pre.blocking.is_empty() {
        let e = UiError::Refused(format!(
            "{} problem(s) block it — {}",
            pre.blocking.len(),
            pre.blocking.iter().map(|(s, m)| format!("{} ({})", m, s.label())).collect::<Vec<_>>().join("; ")
        ));
        app.workflow.fields.refused = Some(e.to_string());
        app.message(&e, true);
        return Err(e);
    }
    app.workflow.fields.refused = None;
    let (solver, params) = run_params(app).map_err(UiError::Input)?;
    if !pre.warnings.is_empty() {
        app.message(format!("Running with {} accepted warning(s).", pre.warnings.len()), false);
    }
    app.run(solver.command(), params)
}

fn run_panel(app: &mut EmcraftApp, ui: &mut egui::Ui, states: &[StepState], t: &Tokens) {
    ui_kit::section(ui, "Preflight");
    let pre = preflight(app, states);
    if pre.blocking.is_empty() && pre.warnings.is_empty() {
        ui.label(egui::RichText::new("Every step is ready.").size(t.font_small).color(t.success));
    }
    let mut go = None;
    for (list, color, what) in [(&pre.blocking, t.error, "blocks"), (&pre.warnings, t.warning, "warning")] {
        for (i, (step, text)) in list.iter().enumerate() {
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(format!("{} · {text}", step.label())).size(t.font_small).color(color));
                let r = ui.small_button(format!("Go to {}", step.label()));
                scenario::publish_target(&format!("preflight:{what}:{i}"), r.rect);
                if r.clicked() {
                    go = Some(*step);
                }
            });
        }
    }
    if let Some(s) = go {
        open_step(app, Some(s));
        return;
    }
    let solver = app.workflow.fields.solver.unwrap_or_else(|| Solver::infer(&app.session.model));
    ui.add_space(6.0);
    ui.label(egui::RichText::new(format!("Solver: {}", solver.label())).size(t.font_small).color(t.text));
    let label = if !pre.blocking.is_empty() {
        format!("Run — {} problem(s) to fix first", pre.blocking.len())
    } else if pre.warnings.is_empty() {
        "Run".to_owned()
    } else {
        format!("Run, accepting {} warning(s)", pre.warnings.len())
    };
    let busy = app.busy();
    if ui_kit::button(ui, &label, true, !busy, "run:start").clicked() {
        let _ = start_run(app);
    }
    if let Some(e) = &app.workflow.fields.refused {
        ui_kit::error_band(ui, e);
    }
    if busy {
        ui.label(egui::RichText::new("Running — progress and Cancel below.").size(t.font_small).color(t.text_dim));
    }
    if let Some(e) = estimate(app).ok().filter(|_| solver == Solver::Frequency) {
        let n = |k: &str| e.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN);
        let points = app.workflow.fields.points.trim().parse::<f64>().unwrap_or(11.0);
        ui_kit::caption(
            ui,
            &format!(
                "About {:.0} unknowns, {:.0} MB, {:.1} s for {points} frequencies (fast sweep: less).",
                n("unknowns"),
                n("memory_mb"),
                n("seconds_per_frequency") * points
            ),
        );
    }
}

fn results(app: &mut EmcraftApp, ui: &mut egui::Ui, states: &[StepState], t: &Tokens) {
    let stale = states.get(Step::Results.index()).is_some_and(|s| s.status == Status::Stale);
    if stale {
        ui_kit::error_band(ui, "These results belong to an earlier state of the model.");
    }
    let s = &app.session;
    let views: Vec<View> = [
        (s.results.sweep.is_some() || s.results.time_domain.is_some(), View::SParameters),
        (s.results.sweep.is_some() || s.results.eigen.is_some() || s.results.radiation.is_some(), View::Field),
        (s.results.sweep.is_some() || s.results.eigen.is_some() || s.results.radiation.is_some(), View::Model),
        (s.results.eigen.is_some(), View::Eigenmodes),
        (s.results.radiation.is_some() || s.results.td_radiation.is_some(), View::FarField),
        (s.results.sweep.is_some(), View::Compare),
    ]
    .into_iter()
    .filter_map(|(on, v)| on.then_some(v))
    .collect();
    if views.is_empty() {
        ui.label(egui::RichText::new("No results yet: Run (step 7).").size(t.font_small).color(t.text_dim));
        return;
    }
    ui_kit::section(ui, "Show");
    ui.horizontal_wrapped(|ui| {
        for v in views {
            let r = ui.selectable_label(app.view == v, if v == View::Model { "Field in 3D" } else { v.label() });
            scenario::publish_target(&format!("results:show:{}", v.id()), r.rect);
            if r.clicked() {
                crate::ribbon::show_view(app, v);
            }
        }
    });
    if app.session.results.sweep.is_none() {
        return;
    }
    ui_kit::section(ui, "Checks");
    let busy = app.busy();
    if ui_kit::button(ui, "Passivity and reciprocity", false, !busy, "results:check").clicked() {
        match app.run("results.check", json!({})) {
            Ok(v) => app.workflow.check = Some((app.revision, v)),
            Err(e) => app.message(e, true),
        }
    }
    if let Some((_, c)) = &app.workflow.check {
        let n = |k: &str| c.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN);
        let passive = c.get("passive").and_then(Value::as_bool) == Some(true);
        kv(ui, "largest σ(S)", format!("{:.6} — {}", n("largest_singular_value"), if passive { "passive" } else { "NOT passive" }), t);
        kv(ui, "max |S − S^T|", format!("{:.3e}", n("reciprocity_error")), t);
    }
    ui_kit::section(ui, "Touchstone");
    let ports = app.session.model.ports.len().max(1);
    let f = &mut app.workflow.fields;
    if f.touchstone.is_empty() {
        f.touchstone = format!("emcraft.s{ports}p");
    }
    if f.format.is_empty() {
        f.format = "ri".into();
    }
    let ep = f.error("touchstone").map(str::to_owned);
    field(ui, "path", &mut f.touchstone, "", ep.as_deref(), "results:touchstone:path");
    ui.horizontal(|ui| {
        ui.add_space(t.form_label_width + 6.0);
        for (v, label) in [("ri", "real, imaginary"), ("ma", "magnitude, angle")] {
            let r = ui.radio_value(&mut f.format, v.to_owned(), egui::RichText::new(label).size(t.font_small));
            scenario::publish_target(&format!("results:touchstone:{v}"), r.rect);
        }
    });
    if ui_kit::button(ui, "Export", true, !busy, "results:touchstone").clicked() {
        let (path, format) = (f.touchstone.trim().to_owned(), f.format.clone());
        if path.is_empty() {
            f.set_error("touchstone", Some("give a file".into()));
        } else {
            f.set_error("touchstone", None);
            if let Err(e) = app.run("results.touchstone", json!({"path": path, "format": format})) {
                app.workflow.fields.set_error("touchstone", Some(e.to_string()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_read_their_ids_and_commands() {
        for s in Step::ALL {
            assert_eq!(s.id().parse::<Step>(), Ok(s));
        }
        assert_eq!(Step::of_command("geometry.solid"), Some(Step::Geometry));
        assert_eq!(Step::of_command("solve.port_modes"), Some(Step::Excitation));
        assert_eq!(overlay(Step::Mesh), Overlay::Mesh);
    }
}
