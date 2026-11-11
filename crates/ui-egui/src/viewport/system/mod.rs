//! The system view, a node editor: the band over a canvas of nodes — the source, the stages and
//! the receiver — each with its settings edited in place and, once read, the chain's gain and
//! noise figure at its output; links drawn from an output port to an input port (dragged from
//! one to the other to link, the mark on a link to unlink); nodes dragged by their headers, a
//! right-click on the canvas adding a stage there. Under the canvas the chart and the budget,
//! read again at every change (`solve.system` takes a moment).

mod bridge;
mod card;
mod fields;
mod wires;

use super::*;
use crate::system_ui;
use bridge::{point, pos};
use emcraft_engine::system::SystemResult;
use emcraft_engine::view::SystemChart;
use emcraft_ui_core::geom;
use emcraft_ui_core::system::band;
use emcraft_ui_core::system::gesture::Input;
use emcraft_ui_core::system::layout::Layout;
use fields::Number;

/// The canvas' least and greatest height (points).
const CANVAS_MIN: f32 = 220.0;
const CANVAS_MAX: f32 = 520.0;

pub(super) struct SystemView;

impl ChartView for SystemView {
    type Key = SharedChart;
    const HANDLE: Fixed = Fixed::ChartSystem;
    const CHART: emcraft_engine::views::ChartId = emcraft_engine::views::ChartId::System;
    const HEIGHT: Height = Height::new(0.35, 200.0, 420.0);

    fn controls(app: &EmcraftApp, st: &mut Parts, ui: &mut egui::Ui, out: &mut Actions) -> Option<Self::Key> {
        let m = app.session.model();
        // Read again at every change, once a revision.
        let r = app.session.results();
        let stale = r.system.is_none() || r.is_stale(emcraft_engine::ResultKind::System);
        if stale && !m.system.stages.is_empty() && !app.busy() && app.auto_solved != Some(app.revision) {
            out.push(Action::AutoSolve);
        }
        band(app, ui, out);
        if let Some(e) = app.auto_error.as_ref().filter(|_| stale) {
            ui.text(Style::SmallDim, Raw(e));
        }
        ui.add_space(4.0);
        if m.system.stages.is_empty() {
            start(app, ui, out);
            return None;
        }
        canvas(app, ui, out);
        ui.add_space(6.0);
        let r = r.system.as_deref()?;
        ui.horizontal(|ui| {
            for c in SystemChart::ALL {
                let on = st.views.system == c;
                if ui.named(egui::Button::selectable(on, tr(chart_label(c))), Handle::SystemChart(c.id())).clicked() {
                    st.views.system = c;
                }
            }
            if let Some(p) = r.nearest(r.at) {
                let unit = app.session.display().frequency;
                ui.text(Style::Dim, Raw(&trf(t!("budget at {f} {unit}"), crate::args!(f = unit.from_si(p.frequency), unit = unit.symbol()))));
            }
        });
        Some(SharedChart::System(st.views.system))
    }

    fn chart(app: &EmcraftApp, key: &Self::Key) -> Result<emcraft_render::chart::Chart, String> {
        let SharedChart::System(what) = key else { return Err("not a system chart".into()) };
        emcraft_engine::view::system_chart(&app.session, *what).map_err(|e| e.to_string())
    }

    fn below(app: &EmcraftApp, _st: &mut Parts, ui: &mut egui::Ui, _chart: &egui::Response, _size: egui::Vec2, _out: &mut Actions) {
        if let Some(r) = app.session.results().system.as_deref() {
            budget(app, r, ui);
        }
    }
}

/// A chart choice's name.
fn chart_label(c: SystemChart) -> crate::lang::Text {
    match c {
        SystemChart::Along => t!("Along the chain"),
        SystemChart::Figure => t!("Noise figure"),
        SystemChart::Gain => t!("Gain"),
    }
}

/// The band over the canvas: start, stop, points and the budget's frequency, confirmed
/// (`project.setup`) as each is changed (`system::band`); the band the chain was read over
/// until one is.
fn band(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    let fu = app.session.display().frequency;
    let now = band::shown(&app.session);
    ui.horizontal(|ui| {
        let fields: [(&'static str, crate::lang::Text, &str); 4] = [
            ("start", t!("start"), fu.symbol()),
            ("stop", t!("stop"), fu.symbol()),
            ("points", t!("Frequency points"), ""),
            ("frequency", t!("budget at"), fu.symbol()),
        ];
        for (key, label, unit) in fields {
            ui.text(Style::Small, label);
            let (rect, _) = ui.allocate_exact_size(vec2(72.0, 20.0), Sense::hover());
            let field = Number { rect, handle: Handle::SystemBand(key), value: band::field(now, key), optional: key == "frequency" };
            fields::number(ui, field, |v| band::setup(now, key, v).map(|p| Action::run("project.setup", p)), out);
            if !unit.is_empty() {
                ui.text(Style::SmallDim, Raw(unit));
            }
            ui.add_space(6.0);
        }
    });
}

/// A chain with no stage yet: what a system is, and a large button for each kind of stage.
fn start(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    let t = &Tokens::get();
    ui.add_space(24.0);
    ui.vertical_centered(|ui| {
        ui.text(Style::Heading, t!("A chain from the antenna to the receiver"));
        ui.add_space(4.0);
        ui.text(Style::Dim, t!("Add its stages in order: an LNA, a filter's S-parameters (a Touchstone file or a sweep), lines, more amplifiers."));
        ui.add_space(12.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            for (kind, label, tip) in system_ui::KINDS {
                let b = egui::Button::new(egui::RichText::new(format!("+ {}", tr(label))).size(t.font_heading)).min_size(vec2(170.0, 44.0));
                if ui.named_enabled(!app.busy(), b, Handle::SystemStart(kind)).on_hover_text(tr(tip)).clicked() {
                    out.push(Action::run("system.stage.add", system_ui::starting(kind)));
                }
            }
        });
    });
}

/// The canvas: laid out from the model (`layout`), drawn under the cards (`wires`), each card
/// with its rows edited in place (`card`), the pointer's gestures — pan, move, link — turned
/// into commands (`gesture`); a right-click adds a stage there.
fn canvas(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    let chain = &app.session.model().system;
    let state = bridge::load(ui.ctx());
    let depth = Layout::of(chain, geom::Point::ORIGIN, state.moving()).depth();
    let (area, bg) = ui.allocate_exact_size(vec2(ui.available_width(), (depth + 56.0).clamp(CANVAS_MIN, CANVAS_MAX)), Sense::click_and_drag());
    scenario::publish_rect(Fixed::SystemCanvas, area);
    let layout = Layout::of(chain, point(area.min) + state.pan, state.moving());
    // The cards' widgets in a child of the canvas' own: placing them leaves the view's layout be.
    let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(area));
    let ui = &mut inner;
    ui.set_clip_rect(area);
    let painter = ui.painter().clone();
    wires::ground(&painter, area, egui::vec2(state.pan.x, state.pan.y));
    let dragging = state.linking().and_then(|from| Some((pos(layout.node(from)?.output()), ui.ctx().pointer_latest_pos()?)));
    wires::links(&painter, &layout, dragging);
    wires::unlink_marks(ui, &layout, out);
    // What the cards and the empty canvas (dragged where nothing else is: a pan) report, in turn.
    let cards: Vec<Input> = layout.nodes.iter().flat_map(|n| card::card(app, ui, n, out)).collect();
    let inputs = cards.into_iter().chain(bridge::drag_inputs(&bg, Input::PressEmpty));
    let (state, commands) = state.steps(inputs, &layout);
    out.extend(commands.into_iter().map(|c| Action::run(c.id, c.params)));
    add_menu(&bg, bg.interact_pointer_pos().map(|p| point(p) - layout.origin), out);
    bridge::store(ui.ctx(), state);
}

/// The canvas' menu: a stage of each kind added at `at`, off the chain until linked.
fn add_menu(bg: &egui::Response, at: Option<geom::Vector>, out: &mut Actions) {
    bg.context_menu(|ui| {
        ui.text(Style::SmallDim, t!("Add a stage here"));
        for (kind, label, _) in system_ui::KINDS {
            if ui.button(tr(label)).clicked() {
                let v = at.map_or_else(|| system_ui::starting(kind), |d| system_ui::placed(kind, [d.x.round().into(), d.y.round().into()]));
                out.push(Action::run("system.stage.add", v));
                ui.close();
            }
        }
    });
}

/// The budget at its frequency, a row a stage, and the receiver's reading under it.
fn budget(app: &EmcraftApp, r: &SystemResult, ui: &mut egui::Ui) {
    let Some((p, rows)) = system_ui::budget::table(r) else { return };
    egui::Grid::new("system_budget").striped(true).num_columns(system_ui::budget::HEADERS.len()).show(ui, |ui| {
        super::dim_if_stale(ui);
        for h in system_ui::budget::HEADERS {
            ui.text(Style::Strong, h);
        }
        ui.end_row();
        for cells in &rows {
            for c in cells {
                ui.label(c);
            }
            ui.end_row();
        }
    });
    ui.add_space(4.0);
    for (line, dim) in system_ui::budget::summary(&app.session, p) {
        ui.text(if dim { Style::Dim } else { Style::Body }, Raw(&line));
    }
}
