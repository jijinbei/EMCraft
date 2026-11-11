//! The system view, a node editor: the band over a canvas of nodes — the source, the stages and
//! the receiver — each with its settings edited in place and, once read, the chain's gain and
//! noise figure at its output; links drawn from an output port to an input port (dragged from
//! one to the other to link, the mark on a link to unlink); nodes dragged by their headers, a
//! right-click on the canvas adding a stage there. Under the canvas the chart and the budget,
//! read again at every change (`solve.system` takes a moment).

mod card;
mod fields;
mod gesture;
mod layout;
mod wires;

use super::*;
use crate::system_ui;
use emcraft_engine::system::{Row, SystemResult};
use emcraft_engine::view::SystemChart;
use fields::Number;
use gesture::{Canvas, Input};
use layout::Layout;

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
/// (`project.setup`) as each is changed; the band the chain was read over until one is.
fn band(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    let fu = app.session.display().frequency;
    let setup = match app.session.model().job.setup() {
        Some(emcraft_model::Setup::System { band, points, frequency }) => {
            Some((band.start.get(), band.stop.get(), points.get(), frequency.map(emcraft_model::Hertz::get)))
        }
        _ => None,
    };
    let read = app
        .session
        .results()
        .system
        .as_deref()
        .and_then(|r| Some((r.points.first()?.frequency, r.points.last()?.frequency, r.points.len(), Some(r.at))));
    let now = setup.or(read);
    let shown = |x: f64| fu.from_si(x);
    let (start, stop, points, at) =
        now.map_or((None, None, None, None), |(a, b, n, f)| (Some(shown(a)), Some(shown(b)), Some(n as f64), f.map(shown)));
    let settings = |key: &str, v: Option<f64>| -> Option<Action> {
        let (mut s0, mut s1, mut n, mut f) = (start, stop, points.unwrap_or(101.0), at);
        match key {
            "start" => s0 = v,
            "stop" => s1 = v,
            "points" => n = v?.round(),
            _ => f = v,
        }
        // One end given alone: the other a fifth of the way off it.
        let (s0, s1) = match (s0, s1) {
            (Some(a), Some(b)) => (a, b),
            (Some(a), None) => (a, a * 1.2),
            (None, Some(b)) => (b / 1.2, b),
            (None, None) => return None,
        };
        let mut p = json!({"kind": "system", "start": s0, "stop": s1, "points": n as u64});
        if let Some(f) = f {
            p["frequency"] = json!(f);
        }
        Some(Action::run("project.setup", p))
    };
    ui.horizontal(|ui| {
        let fields: [(&'static str, crate::lang::Text, Option<f64>, &str, bool); 4] = [
            ("start", t!("start"), start, fu.symbol(), false),
            ("stop", t!("stop"), stop, fu.symbol(), false),
            ("points", t!("Frequency points"), points, "", false),
            ("frequency", t!("budget at"), at, fu.symbol(), true),
        ];
        for (key, label, value, unit, optional) in fields {
            ui.text(Style::Small, label);
            let (rect, _) = ui.allocate_exact_size(vec2(72.0, 20.0), Sense::hover());
            fields::number(ui, Number { rect, handle: Handle::SystemBand(key), value, optional }, |v| settings(key, v), out);
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
    let mut state = Canvas::load(ui.ctx());
    let depth = Layout::of(chain, egui::Pos2::ZERO, state.moving()).depth();
    let (area, bg) = ui.allocate_exact_size(vec2(ui.available_width(), (depth + 56.0).clamp(CANVAS_MIN, CANVAS_MAX)), Sense::click_and_drag());
    scenario::publish_rect(Fixed::SystemCanvas, area);
    let layout = Layout::of(chain, area.min + state.pan, state.moving());
    // The cards' widgets in a child of the canvas' own: placing them leaves the view's layout be.
    let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(area));
    let ui = &mut inner;
    ui.set_clip_rect(area);
    let painter = ui.painter().clone();
    wires::ground(&painter, area, state.pan);
    let dragging = state.linking().and_then(|from| Some((layout.node(from)?.output(), ui.ctx().pointer_latest_pos()?)));
    wires::links(&painter, &layout, dragging);
    wires::unlink_marks(ui, &layout, out);
    let mut inputs: Vec<Input> = layout.nodes.iter().flat_map(|n| card::card(app, ui, n, out)).collect();
    // The canvas itself: dragged where nothing else is, it pans.
    if bg.drag_started() {
        inputs.push(Input::PressEmpty);
    }
    if bg.dragged() {
        inputs.push(Input::Drag(bg.drag_delta()));
    }
    if bg.drag_stopped() {
        inputs.extend(bg.interact_pointer_pos().map(Input::Release));
    }
    out.extend(inputs.into_iter().filter_map(|i| state.step(i, &layout)));
    add_menu(&bg, bg.interact_pointer_pos().map(|p| p - layout.origin), out);
    state.store(ui.ctx());
}

/// The canvas' menu: a stage of each kind added at `at`, off the chain until linked.
fn add_menu(bg: &egui::Response, at: Option<egui::Vec2>, out: &mut Actions) {
    bg.context_menu(|ui| {
        ui.text(Style::SmallDim, t!("Add a stage here"));
        for (kind, label, _) in system_ui::KINDS {
            if ui.button(tr(label)).clicked() {
                let mut v = system_ui::starting(kind);
                if let Some(d) = at {
                    v["at"] = json!([d.x.round(), d.y.round()]);
                    v["linked"] = json!(false);
                }
                out.push(Action::run("system.stage.add", v));
                ui.close();
            }
        }
    });
}

/// The budget at its frequency, a row a stage, and the receiver's reading under it.
fn budget(app: &EmcraftApp, r: &SystemResult, ui: &mut egui::Ui) {
    let Some(p) = r.nearest(r.at) else { return };
    let num = |x: Option<f64>, digits: usize| x.map_or_else(|| "—".to_owned(), |v| format!("{v:.digits$}"));
    let headers: Vec<String> = [
        t!("stage"),
        t!("gain (dB)"),
        t!("total gain (dB)"),
        t!("NF (dB)"),
        t!("Te (K)"),
        t!("signal (dBm)"),
        t!("noise (dBm)"),
        t!("SNR (dB)"),
        t!("IIP3 (dBm)"),
        t!("OIP3 (dBm)"),
        t!("OP1dB (dBm)"),
    ]
    .into_iter()
    .map(|h| tr(h).to_owned())
    .collect();
    let row = |name: &str, x: &Row| {
        vec![
            name.to_owned(),
            num(x.stage_gain_db, 2),
            num(x.gain_db, 2),
            num(x.nf_db, 3),
            num(x.noise_temperature, 1),
            num(x.signal_dbm, 2),
            num(x.noise_dbm, 2),
            num(x.snr_db, 2),
            num(x.iip3_dbm, 2),
            num(x.oip3_dbm, 2),
            num(x.op1db_dbm, 2),
        ]
    };
    let rows: Vec<Vec<String>> = r.stages.iter().zip(&p.rows).map(|((name, _), x)| row(name, x)).collect();
    egui::Grid::new("system_budget").striped(true).num_columns(headers.len()).show(ui, |ui| {
        super::dim_if_stale(ui);
        for h in &headers {
            ui.text(Style::Strong, Raw(h));
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
    if let (Some(t), Some(s)) = (p.system_temperature, p.sensitivity_dbm) {
        ui.text(
            Style::Body,
            Raw(&trf(t!("System temperature {t} K; sensitivity {s} dBm"), crate::args!(t = format!("{t:.1}"), s = format!("{s:.2}")))),
        );
    } else if app.session.model().system.receiver.bandwidth.is_none() {
        ui.text(Style::Dim, t!("Give the receiver a bandwidth (on its node) for its noise, SNR and sensitivity."));
    }
    if let Some(why) = &p.why {
        ui.text(Style::Dim, Raw(why));
    }
}
