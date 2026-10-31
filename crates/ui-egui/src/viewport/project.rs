//! The project's Home, the first thing an open project shows (step 0 of the step bar and the
//! viewport's first tab): the kind of analysis as four cards with a picture of what each gives,
//! the frequency band with the ports' standard guide bands as presets, the units, and the
//! project at a glance — its geometry, excitation, mesh and results, each with its step's mark,
//! and the next step's button. Each choice runs the command the rest of the window runs
//! (`project.analysis`, `project.frequency`, `units.set`).

use emcraft_model::units::{Frequency, Length};

use super::*;
use crate::kinds::{Look, tile};
use crate::steps::{Fields, Solver, Status, Step, StepState};
use crate::ui_kit;

/// The narrowest card: below four of these in a row, two rows of two.
const CARD_MIN: f32 = 150.0;

pub(super) fn project_home(app: &EmcraftApp, f: &mut Fields, ui: &mut egui::Ui, out: &mut Actions) {
    let t = &Tokens::get();
    scenario::publish_target(Fixed::ProjectHome, ui.max_rect());
    crate::steps::follow_band(app, f);
    let width = ui.available_width().min(1100.0);
    ui.vertical(|ui| {
        ui.set_max_width(width);
        ui_kit::section(ui, "Analysis");
        kinds(app, ui, out);
        ui.add_space(t.space_12);
        let states = crate::steps::statuses_now(app);
        let gap = 24.0;
        let right = 320.0f32.min(width * 0.45);
        let left = width - right - gap;
        if left < 260.0 {
            band(app, f, ui, out);
            units(app, ui, out);
            summary(app, &states, ui, out);
        } else {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(left);
                    band(app, f, ui, out);
                    units(app, ui, out);
                });
                ui.add_space(gap);
                ui.vertical(|ui| {
                    ui.set_width(right);
                    summary(app, &states, ui, out);
                });
            });
        }
    });
}

/// The four kinds as large cards, the job's own marked (`auto` while inferred); a click
/// chooses one (`project.analysis`).
fn kinds(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    let m = app.session.model();
    let (current, chosen) = (m.analysis(), m.analysis);
    let gap = 12.0;
    let w = ui.available_width();
    let per_row = if (w - 3.0 * gap) / 4.0 >= CARD_MIN { 4 } else { 2 };
    let card_w = ((w - (per_row - 1) as f32 * gap) / per_row as f32).clamp(CARD_MIN, 270.0);
    let size = vec2(card_w, (card_w * 0.5).clamp(80.0, 130.0) + 74.0);
    let enabled = !app.busy();
    for row in Solver::ALL.chunks(per_row) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for &kind in row {
                let look = Look { large: true, on: kind == current, auto: chosen.is_none(), enabled };
                if tile(ui, kind, size, look, Handle::Kind(kind)).clicked() && enabled && chosen != Some(kind) {
                    out.push(Action::run("project.analysis", json!({"kind": kind.id()})));
                }
            }
        });
        ui.add_space(gap);
    }
}

/// The band's start and stop (display units) with Apply, and the ports' guide bands as presets.
fn band(app: &EmcraftApp, f: &mut Fields, ui: &mut egui::Ui, out: &mut Actions) {
    let t = &Tokens::get();
    ui_kit::section(ui, "Frequency band");
    let fu = app.session.display().frequency;
    let busy = app.busy();
    let (e0, e1) = (f.error("start").map(str::to_owned), f.error("stop").map(str::to_owned));
    let enter = ui
        .scope(|ui| {
            ui.set_max_width(ui.available_width().min(420.0));
            crate::steps::field(ui, "start", &mut f.start, fu.symbol(), e0.as_deref(), Fixed::IntentStart)
                | crate::steps::field(ui, "stop", &mut f.stop, fu.symbol(), e1.as_deref(), Fixed::IntentStop)
        })
        .inner;
    if enter && !busy {
        out.push(Action::ApplyBand);
    }
    ui.horizontal_wrapped(|ui| {
        ui.add_space(t.form_label_width + 6.0);
        ui.act(!busy, ui_kit::styled("Apply", true), Fixed::IntentApply.into(), "", || Action::ApplyBand, out);
        for b in emcraft_engine::guides::bands(&app.session) {
            let (a, z) = (fu.from_si(b.start), fu.from_si(b.stop));
            let text = format!("{} · {}–{} {}", b.name, crate::steps::fmt(a), crate::steps::fmt(z), fu.symbol());
            let on = app.session.model().band.is_some_and(|m| (m.start.get() - b.start).abs() < 1.0 && (m.stop.get() - b.stop).abs() < 1.0);
            let hint = if b.name == "TE10" { "1.25–1.9 × the TE10 cut-off" } else { "The guide's standard band" };
            let pill = pill(&text, on, 12, PillSize::Small);
            ui.act(!busy, pill, Handle::BandPreset(&b.name), hint, || Action::run("project.frequency", json!({"start": a, "stop": z})), out);
        }
    });
    ui.add_space(t.space_8);
}

/// The project's units (`units.set`).
fn units(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    let t = &Tokens::get();
    ui_kit::section(ui, "Units");
    let d = app.session.display();
    let busy = app.busy();
    let choice = |ui: &mut egui::Ui, out: &mut Actions, group: &'static str, id: &'static str, on: bool| {
        let b =
            egui::Button::selectable(on, egui::RichText::new(id).size(t.font_body)).min_size(vec2(52.0, t.control_height)).frame_when_inactive(true);
        let r = ui.named_enabled(!busy, b, Handle::HomeUnit(group, id));
        if r.clicked() && !on {
            out.push(Action::run("units.set", json!({group: id})));
        }
    };
    ui_kit::row(ui, "Length", false, |ui| {
        for u in Length::ALL {
            choice(ui, out, "length", u.id(), d.length == u);
        }
    });
    ui_kit::row(ui, "Frequency", false, |ui| {
        for u in Frequency::ALL {
            choice(ui, out, "frequency", u.id(), d.frequency == u);
        }
    });
}

/// The project at a glance: its name, then its geometry, excitation, mesh and results, each with
/// its step's mark (a click opens the step), and the next step's button.
fn summary(app: &EmcraftApp, states: &[StepState], ui: &mut egui::Ui, out: &mut Actions) {
    let t = &Tokens::get();
    ui_kit::section(ui, "Project");
    t.card(t.panel).inner_margin(egui::Margin::symmetric(12, 10)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.text(Style::Strong, Raw(&app.session.name()));
        ui.add_space(t.space_4);
        let tree = crate::browser::tree(app);
        let rows = |names: &[&str]| -> Vec<String> {
            tree.iter().filter(|(s, _)| names.contains(s)).flat_map(|(_, rows)| rows.iter().map(|r| r.shown.clone())).collect()
        };
        for (step, names) in
            [(Step::Geometry, &["Geometry"][..]), (Step::Excitation, &["Ports", "Sources"]), (Step::Mesh, &["Mesh"]), (Step::Results, &["Results"])]
        {
            let Some(s) = states.get(step.index()) else { continue };
            let found = rows(names);
            let text = match found.as_slice() {
                [] => "—".to_owned(),
                [one] => one.clone(),
                [one, rest @ ..] => format!("{one}  +{}", rest.len()),
            };
            line(app, ui, s, &text, out);
        }
        ui.add_space(t.space_8);
        let next = next_step(app, states);
        let label = trf("Next: {step}", crate::lang::args!(step = shown(next)));
        ui.act(true, ui_kit::styled(Raw(&label), true), Fixed::HomeNext.into(), "", || Action::OpenStep(Some(next)), out);
    });
}

/// A step's line in the summary: its mark, its name and what the model has for it.
fn line(app: &EmcraftApp, ui: &mut egui::Ui, s: &StepState, text: &str, out: &mut Actions) {
    let t = &Tokens::get();
    let h = 24.0;
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    let resp = ui.hit(r, Handle::HomeStep(s.step), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(r, t.radius_sm, t.hover);
    }
    let p = ui.painter().with_clip_rect(r);
    let c = pos2(r.left() + 10.0, r.center().y);
    if crate::steps::not_needed(app, s.step).is_some() && s.status == Status::Done {
        crate::steps::skip_mark(&p, c, t);
    } else {
        crate::steps::mark(&p, c, s.status, t);
    }
    p.text(pos2(r.left() + 24.0, r.center().y), Align2::LEFT_CENTER, shown(s.step), FontId::proportional(t.font_small), t.text_dim);
    p.text(pos2(r.left() + 110.0, r.center().y), Align2::LEFT_CENTER, text, FontId::proportional(t.font_small), t.text);
    let tip = format!("{}: {}", shown(s.step), shown(s.status));
    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tip).clicked() {
        out.push(Action::OpenStep(Some(s.step)));
    }
}

/// The first step after Home with something to do (not done, not one the kind does without);
/// Results when every one is done.
fn next_step(app: &EmcraftApp, states: &[StepState]) -> Step {
    states
        .iter()
        .filter(|s| s.step != Step::Intent && s.status != Status::Done && crate::steps::not_needed(app, s.step).is_none())
        .find(|s| s.status != Status::Waiting)
        .map_or(Step::Results, |s| s.step)
}
