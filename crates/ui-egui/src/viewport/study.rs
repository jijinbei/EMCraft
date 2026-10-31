//! The last parameter study (`study.sweep`, `study.optimize`): its chart — a sweep's |S| against
//! frequency, one curve per value, or an optimization's goal at each solve with the best so far —
//! the S-parameter shown, and the table of its points. A sweep over a grid of several variables
//! is charted a slice at a time: the variable charted along picked from pills, and each other
//! variable held at a value picked from its own row of pills (`study:along:<name>`,
//! `study:at:<name>:<index>`).

use emcraft_engine::study::{Kind, Slice, Study};

use super::*;
use crate::lang::args;

/// One-line account of a study, with its surrogate's where it had one.
pub fn summary(study: &Study) -> String {
    let Some(g) = &study.surrogate else { return plain_summary(study) };
    let extra = match study.kind {
        Kind::Sweep => {
            let predicted = study.rows.iter().filter(|r| r.is_predicted()).count();
            trf(", {solved} solved, {predicted} predicted", args!(solved = g.solves, predicted = predicted))
        }
        Kind::Optimize => match g.at_best {
            Some((m, s)) => trf(" · Bayesian, surrogate {mean} ± {sigma}", args!(mean = short(m), sigma = short(s))),
            None => tr(" · Bayesian").to_owned(),
        },
    };
    format!("{}{extra}", plain_summary(study))
}

fn plain_summary(study: &Study) -> String {
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
    fn controls(app: &EmcraftApp, st: &mut Parts, ui: &mut egui::Ui, _out: &mut Actions) -> Option<StudyKey> {
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

    /// The table of its points, packed beside or under the chart.
    fn blocks(app: &EmcraftApp, st: &Parts, ui: &egui::Ui) -> Vec<egui::Vec2> {
        let Some(t) = Points::of(app, st) else { return Vec::new() };
        let body = egui::TextStyle::Body.resolve(ui.style());
        let mut widths = super::column_widths(ui, &body, &t.headers, &t.rows);
        let notes: Vec<Vec<String>> = t.notes.iter().map(|(n, _)| vec![n.clone()]).collect();
        widths.extend(super::column_widths(ui, &FontId::proportional(Tokens::get().font_small), &[String::new()], &notes));
        vec![points_size(super::grid_width(ui, &widths, ui.spacing().item_spacing.x), t.rows.len())]
    }

    fn block(app: &EmcraftApp, st: &mut Parts, ui: &mut egui::Ui, _: usize, _: &mut Actions) {
        study_table(app, st, ui);
    }

    /// Why, in a line where the chart goes (the table stays in its block).
    fn failed(_: &EmcraftApp, _: &mut Parts, ui: &mut egui::Ui, error: &str) {
        let t = &Tokens::get();
        ui.label(egui::RichText::new(error).size(t.font_body).color(t.text_dim));
    }
}

/// The S-parameters a sweep's chart may show (none for an optimization).
fn parameters(study: &Study) -> Vec<String> {
    if study.kind == Kind::Sweep { study.parameters() } else { Vec::new() }
}

/// The table under the chart, of the S-parameter shown.
fn study_table(app: &EmcraftApp, st: &Parts, ui: &mut egui::Ui) {
    if let Some(t) = Points::of(app, st) {
        table(ui, &t);
    }
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

/// The most rows the table shows.
const MAX_ROWS: usize = 500;

/// The size the table `width` wide (measured) of `rows` points wants (points): a header and a
/// row per point.
pub(super) fn points_size(width: f32, rows: usize) -> egui::Vec2 {
    vec2(width, 30.0 + 24.0 * rows as f32)
}

/// How a row's note is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Note {
    Error,
    Best,
    Predicted,
}

/// The study's points as drawn: the headers and each row's cells (the number when optimizing,
/// the values, the goal or the band's extremes of the S-parameter shown, the modes where any
/// were found), and each row's note (why it failed, best, predicted) in small text.
struct Points {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
    notes: Vec<(String, Option<Note>)>,
}

impl Points {
    fn of(app: &EmcraftApp, st: &Parts) -> Option<Points> {
        let study = app.session.results().study.as_ref()?;
        let parameters = parameters(study);
        let shown = st.views.study.parameter.as_deref().filter(|p| parameters.iter().any(|q| q == p));
        let parameter = shown.or(parameters.first().map(String::as_str));
        let units = emcraft_engine::view::variable_units(&app.session, study);
        let optimize = study.kind == Kind::Optimize;
        let modes = !study.rows.iter().all(|r| r.modes.is_empty());
        let mut headers: Vec<String> = Vec::new();
        if optimize {
            headers.push("#".into());
        }
        // Each variable with its unit, where the model says it.
        for (n, u) in study.variables.iter().zip(units.iter().chain(std::iter::repeat(&None))) {
            headers.push(u.as_ref().map_or_else(|| n.clone(), |u| format!("{n} ({u})")));
        }
        if optimize {
            headers.push(match emcraft_engine::view::goal_unit(study) {
                Some(u) => trf("goal ({unit})", args!(unit = u)),
                None => tr("goal").to_owned(),
            });
        } else if let Some(p) = parameter {
            let p = p.to_uppercase();
            headers.extend([trf("min |{p}| dB", args!(p = p)), trf("max |{p}| dB", args!(p = p))]);
        }
        if modes {
            headers.push(trf("modes ({unit})", args!(unit = study.frequency_unit)));
        }
        let (mut rows, mut notes) = (Vec::new(), Vec::new());
        for (i, r) in study.rows.iter().enumerate().take(MAX_ROWS) {
            let mut row: Vec<String> = Vec::new();
            if optimize {
                row.push(format!("{}", i + 1));
            }
            row.extend(r.values.iter().map(|v| short(*v)));
            if optimize {
                row.push(r.objective.map_or("–".into(), |o| format!("{o:.4}")));
            } else if let Some(p) = parameter {
                let db = r.db(p).unwrap_or_default();
                let lo = db.iter().copied().fold(f64::INFINITY, f64::min);
                let hi = db.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                let cell = |x: f64| if x.is_finite() { format!("{x:.3}") } else { "–".into() };
                row.extend([cell(lo), cell(hi)]);
            }
            if modes {
                row.push(r.modes.iter().take(6).map(|f| format!("{f:.4}")).collect::<Vec<_>>().join(", "));
            }
            // A surrogate's prediction, with the largest σ of the |S| shown.
            let sigma = parameter.and_then(|p| r.db_sigma(p)).map(|s| s.into_iter().fold(0.0, f64::max));
            notes.push(match (&r.error, study.best == Some(i), r.is_predicted(), sigma) {
                (Some(e), _, _, _) => (emcraft_engine::text::clip(e, 120), Some(Note::Error)),
                (None, true, _, _) => (tr("best").to_owned(), Some(Note::Best)),
                (None, false, true, Some(s)) => (trf("predicted ± {sigma} dB", args!(sigma = format!("{s:.3}"))), Some(Note::Predicted)),
                (None, false, true, None) => (tr("predicted").to_owned(), Some(Note::Predicted)),
                (None, false, false, _) => (String::new(), None),
            });
            rows.push(row);
        }
        Some(Points { headers, rows, notes })
    }
}

/// The most values offered as pills for one held variable.
const MAX_PILLS: usize = 101;

/// The study's points, each row's note last.
fn table(ui: &mut egui::Ui, p: &Points) {
    let t = &Tokens::get();
    egui::ScrollArea::both().id_salt("study_table").show(ui, |ui| {
        egui::Grid::new("study_grid").striped(true).show(ui, |ui| {
            super::dim_if_stale(ui);
            for h in &p.headers {
                ui.label(egui::RichText::new(h).strong());
            }
            ui.label("");
            ui.end_row();
            for (row, (note, kind)) in p.rows.iter().zip(&p.notes) {
                for c in row {
                    ui.label(c);
                }
                let colour = match kind {
                    Some(Note::Error) => t.error,
                    Some(Note::Best) => t.accent,
                    Some(Note::Predicted) | None => t.text_dim,
                };
                crate::ui_kit::small(ui, Raw(note), colour);
                ui.end_row();
            }
        });
    });
}
