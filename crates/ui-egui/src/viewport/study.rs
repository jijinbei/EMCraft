//! The last parameter study (`study.sweep`, `study.optimize`): its chart — a sweep's |S| against
//! frequency, one curve per value, or an optimization's goal at each solve with the best so far —
//! the S-parameter shown, and the table of its points. A sweep over a grid of several variables
//! is charted a slice at a time: the variable charted along picked from pills, and each other
//! variable held at a value picked from its own row of pills (`study:along:<name>`,
//! `study:at:<name>:<index>`).

use emcraft_engine::study::{Kind, Slice, Study};

use super::*;
use crate::lang::args;

/// One-line account of a study.
pub fn summary(study: &Study) -> String {
    let names = study.variables.join(", ");
    let failed = study.rows.iter().filter(|r| r.error.is_some()).count();
    let failed = if failed > 0 { trf(", {n} failed", args!(n = failed)) } else { String::new() };
    let (n, solve) = (study.rows.len(), &study.solve);
    match study.kind {
        Kind::Sweep if study.is_grid() => {
            let sizes: Vec<String> = study.grid.iter().map(|v| v.len().to_string()).collect();
            trf(
                "Sweep of {names} on a grid: {sizes} = {n} points by {solve}{failed}",
                args!(names = names, sizes = sizes.join(" × "), n = n, solve = solve, failed = failed),
            )
        }
        Kind::Sweep => trf("Sweep of {names}: {n} values by {solve}{failed}", args!(names = names, n = n, solve = solve, failed = failed)),
        Kind::Optimize => {
            let best = study.best.and_then(|b| study.rows.get(b)).map(|r| {
                let at: Vec<String> = study.variables.iter().zip(&r.values).map(|(n, v)| format!("{n} = {}", short(*v))).collect();
                trf("best {at} (goal {goal})", args!(at = at.join(", "), goal = r.objective.map_or("–".into(), short)))
            });
            let state = tr(if study.converged { "converged" } else { "stopped at its budget" });
            let goal = study.goal.clone().unwrap_or_default();
            trf(
                "Optimization of {names}: {goal}, {n} solves, {state}; {best}",
                args!(names = names, goal = goal, n = n, state = state, best = best.unwrap_or_default()),
            )
        }
    }
}

fn short(v: f64) -> String {
    let s = format!("{v:.5}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

pub(super) struct StudyView;

/// The S-parameter a study's chart shows and its slice of a grid.
type StudyKey = (Option<String>, Slice);

impl ChartView for StudyView {
    type Key = StudyKey;
    const HANDLE: Fixed = Fixed::ChartStudy;
    const CHART: emcraft_engine::views::ChartId = emcraft_engine::views::ChartId::Study;
    const HEIGHT: Height = Height::new(0.6, 240.0, 640.0);

    /// The study in a line, the S-parameter a sweep shows, and a grid's slice.
    fn controls(app: &EmcraftApp, st: &mut Parts, ui: &mut egui::Ui) -> Option<StudyKey> {
        let t = &Tokens::get();
        let study = app.session.results().study.as_ref()?;
        let state = &mut st.views.study;
        ui.label(egui::RichText::new(summary(study)).size(t.font_body).color(t.text));
        let parameters = parameters(study);
        if let [first, _, ..] = parameters.as_slice() {
            ui.horizontal(|ui| {
                crate::ui_kit::small(ui, "Show", t.text_dim);
                for p in &parameters {
                    let on = state.parameter.as_ref().unwrap_or(first) == p;
                    if ui.named(pill(&format!("|{}|", p.to_uppercase()), on, 10, PillSize::Small), Handle::StudyParameter(p)).clicked() && !on {
                        state.parameter = Some(p.clone());
                    }
                }
            });
        }
        if study.is_grid() {
            slice_picker(ui, study, &mut state.slice);
        }
        Some((state.parameter.clone().filter(|p| parameters.contains(p)), state.slice.clone()))
    }

    fn chart(app: &EmcraftApp, (parameter, slice): &StudyKey) -> Result<emcraft_render::chart::Chart, String> {
        let study = app.session.results().study.as_ref().ok_or("no study")?;
        let units = emcraft_engine::view::variable_units(&app.session, study);
        emcraft_engine::view::chart_with_units(study, parameter.as_deref(), slice, &units).map_err(|e| e.to_string())
    }

    fn below(app: &EmcraftApp, st: &mut Parts, ui: &mut egui::Ui, _: &egui::Response, _: egui::Vec2, _: &mut Actions) {
        study_table(app, st, ui);
    }

    /// Why, in a line over the table.
    fn failed(app: &EmcraftApp, st: &mut Parts, ui: &mut egui::Ui, error: &str) {
        let t = &Tokens::get();
        ui.label(egui::RichText::new(error).size(t.font_body).color(t.text_dim));
        study_table(app, st, ui);
    }
}

/// The S-parameters a sweep's chart may show (none for an optimization).
fn parameters(study: &Study) -> Vec<String> {
    if study.kind == Kind::Sweep { study.parameters() } else { Vec::new() }
}

/// The table under the chart, of the S-parameter shown.
fn study_table(app: &EmcraftApp, st: &Parts, ui: &mut egui::Ui) {
    let Some(study) = app.session.results().study.as_ref() else { return };
    let parameters = parameters(study);
    let shown = st.views.study.parameter.as_deref().filter(|p| parameters.iter().any(|q| q == p));
    let units = emcraft_engine::view::variable_units(&app.session, study);
    table(ui, study, shown.or(parameters.first().map(String::as_str)), &units);
}

/// A grid sweep's slice: the variable charted along, and the value each other variable is held
/// at (a row of pills each).
fn slice_picker(ui: &mut egui::Ui, study: &Study, slice: &mut Slice) {
    let t = &Tokens::get();
    slice.at.resize(study.variables.len(), 0);
    ui.horizontal_wrapped(|ui| {
        crate::ui_kit::small(ui, "Along", t.text_dim);
        for (k, n) in study.variables.iter().enumerate() {
            let on = slice.free == k;
            if ui.named(pill(n, on, 10, PillSize::Small), Handle::StudyAlong(n)).clicked() && !on {
                slice.free = k;
            }
        }
    });
    for (k, n) in study.variables.iter().enumerate() {
        if k == slice.free {
            continue;
        }
        let Some(axis) = study.grid.get(k) else { continue };
        ui.horizontal_wrapped(|ui| {
            crate::ui_kit::small(ui, trf("{name} at", args!(name = n)), t.text_dim);
            for (i, v) in axis.iter().enumerate().take(MAX_PILLS) {
                let on = slice.at.get(k) == Some(&i);
                if ui.named(pill(&short(*v), on, 10, PillSize::Small), Handle::StudyAt(n, i)).clicked()
                    && !on
                    && let Some(slot) = slice.at.get_mut(k)
                {
                    *slot = i;
                }
            }
        });
    }
}

/// The most values offered as pills for one held variable.
const MAX_PILLS: usize = 101;

/// The study's points: the values, the goal or the band's extremes of the S-parameter shown, and
/// why a point failed.
fn table(ui: &mut egui::Ui, study: &Study, parameter: Option<&str>, units: &[Option<String>]) {
    let t = &Tokens::get();
    let optimize = study.kind == Kind::Optimize;
    egui::ScrollArea::vertical().id_salt("study_table").show(ui, |ui| {
        egui::Grid::new("study_grid").striped(true).show(ui, |ui| {
            super::dim_if_stale(ui);
            if optimize {
                ui.label(egui::RichText::new("#").strong());
            }
            // Each variable with its unit, where the model says it.
            for (n, u) in study.variables.iter().zip(units.iter().chain(std::iter::repeat(&None))) {
                let head = u.as_ref().map_or_else(|| n.clone(), |u| format!("{n} ({u})"));
                ui.label(egui::RichText::new(head).strong());
            }
            if optimize {
                match emcraft_engine::view::goal_unit(study) {
                    Some(u) => ui.text(Style::Strong, trf("goal ({unit})", args!(unit = u))),
                    None => ui.text(Style::Strong, "goal"),
                };
            } else if let Some(p) = parameter {
                let p = p.to_uppercase();
                ui.text(Style::Strong, trf("min |{p}| dB", args!(p = p)));
                ui.text(Style::Strong, trf("max |{p}| dB", args!(p = p)));
            }
            if !study.rows.iter().all(|r| r.modes.is_empty()) {
                ui.text(Style::Strong, trf("modes ({unit})", args!(unit = study.frequency_unit)));
            }
            ui.label(egui::RichText::new("").strong());
            ui.end_row();
            for (i, r) in study.rows.iter().enumerate().take(500) {
                let best = study.best == Some(i);
                if optimize {
                    ui.label(format!("{}", i + 1));
                }
                for v in &r.values {
                    ui.label(short(*v));
                }
                if optimize {
                    ui.label(r.objective.map_or("–".into(), |o| format!("{o:.4}")));
                } else if let Some(p) = parameter {
                    let db = r.db(p).unwrap_or_default();
                    let lo = db.iter().copied().fold(f64::INFINITY, f64::min);
                    let hi = db.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                    let cell = |x: f64| if x.is_finite() { format!("{x:.3}") } else { "–".into() };
                    ui.label(cell(lo));
                    ui.label(cell(hi));
                }
                if !study.rows.iter().all(|r| r.modes.is_empty()) {
                    ui.label(r.modes.iter().take(6).map(|f| format!("{f:.4}")).collect::<Vec<_>>().join(", "));
                }
                match (&r.error, best) {
                    (Some(e), _) => ui.label(egui::RichText::new(emcraft_engine::text::clip(e, 120)).size(t.font_small).color(t.error)),
                    (None, true) => crate::ui_kit::small(ui, "best", t.accent),
                    (None, false) => ui.label(""),
                };
                ui.end_row();
            }
        });
    });
}
