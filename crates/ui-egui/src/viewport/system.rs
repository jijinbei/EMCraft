//! The system view: the chain as a block diagram from its source on (a click opens a stage's
//! settings, its menu moves or deletes it; a row of buttons adds stages and sets the source and
//! receiver), the chart `solve.system`'s result is read in (along the chain, or over the band)
//! and the budget stage by stage under it.

use super::*;
use emcraft_engine::system::{Row, SystemResult};
use emcraft_engine::view::SystemChart;
use emcraft_model::system::{DataSource, Part, Stage};

/// The stages the buttons add, each with its settings to start from.
const ADD: [(&str, crate::lang::Text, crate::lang::Text); 6] = [
    ("amplifier", t!("Amplifier"), t!("An ideal amplifier: its gain, noise figure, OIP3 and P1dB")),
    ("attenuator", t!("Attenuator"), t!("A matched attenuator: its loss")),
    ("data", t!("S-parameters"), t!("A two-port's S: the sweep or an imported Touchstone file (a filter, a measured LNA)")),
    ("line", t!("Line"), t!("A transmission line: its impedance, length and losses")),
    ("series", t!("Series element"), t!("R, L and C in series along the line")),
    ("shunt", t!("Shunt element"), t!("R, L and C across the line")),
];

/// The settings a new stage of `kind` starts from.
fn starting(kind: &str) -> Value {
    match kind {
        "amplifier" => json!({"type": kind, "gain": 20, "noise_figure": 1}),
        "attenuator" => json!({"type": kind, "loss": 3}),
        "data" => json!({"type": kind, "data": "sweep"}),
        "line" => json!({"type": kind, "impedance": 50, "length": 10}),
        _ => json!({"type": kind, "capacitance": 1}),
    }
}

pub(super) struct SystemView;

impl ChartView for SystemView {
    type Key = SharedChart;
    const HANDLE: Fixed = Fixed::ChartSystem;
    const CHART: emcraft_engine::views::ChartId = emcraft_engine::views::ChartId::System;
    const HEIGHT: Height = Height::new(0.45, 220.0, 520.0);

    fn controls(app: &EmcraftApp, st: &mut Parts, ui: &mut egui::Ui, out: &mut Actions) -> Option<Self::Key> {
        buttons(app, ui, out);
        ui.add_space(6.0);
        diagram(app, ui, out);
        ui.add_space(6.0);
        let r = app.session.results().system.as_deref()?;
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

/// The row of buttons: a stage of each kind, the source, the receiver, and Run.
fn buttons(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    let busy = app.busy();
    ui.horizontal_wrapped(|ui| {
        for (kind, label, tip) in ADD {
            let b = egui::Button::new(format!("+ {}", tr(label)));
            ui.act(!busy, b, Handle::SystemAdd(kind), Action::OpenDialog { id: "system.stage.add".into(), values: Some(starting(kind)) }, out).hint(tip);
        }
        ui.separator();
        let m = app.session.model();
        let source = serde_json::to_value(&m.system.source).unwrap_or_else(|_| json!({}));
        ui.act(!busy, egui::Button::new(tr(t!("Source…"))), Handle::SystemAdd("source"), Action::OpenDialog { id: "system.source".into(), values: Some(source) }, out)
            .hint(t!("The antenna whose impedance drives the chain, its noise temperature and the signal it gives"));
        let r = &m.system.receiver;
        let receiver = json!({"impedance": r.impedance, "temperature": r.temperature, "bandwidth": r.bandwidth.map(|b| app.session.display().frequency.from_si(b)), "snr": r.snr});
        ui.act(!busy, egui::Button::new(tr(t!("Receiver…"))), Handle::SystemAdd("receiver"), Action::OpenDialog { id: "system.receiver".into(), values: Some(receiver) }, out)
            .hint(t!("The chain's impedance and temperature, the receiver's bandwidth and the SNR it needs"));
        ui.separator();
        let ready = !m.system.stages.is_empty();
        let run = if m.job.setup().is_some() { Action::run("solve.run", json!({})) } else { Action::OpenStep(Some(crate::steps::Step::Setup)) };
        ui.act(ready && !busy, egui::Button::new(tr(t!("Solve"))), Handle::SystemAdd("solve"), run, out)
            .hint(t!("Read the chain over the band (solve.system); the band is the Setup step's"));
    });
}

/// A stage's numbers as its block shows them, in a line.
fn summary(stage: &Stage) -> String {
    let g = |x: f64| format!("{x:.1} dB");
    match &stage.part {
        Part::Amplifier { gain, noise_figure, .. } => format!("G {}  NF {}", g(*gain), g(*noise_figure)),
        Part::Attenuator { loss } => format!("−{}", g(*loss)),
        Part::Data { data: DataSource::Sweep, .. } => "S: sweep".to_owned(),
        Part::Data { data: DataSource::Measurement { name }, .. } => format!("S: {}", emcraft_engine::text::clip(name, 16)),
        Part::Line { impedance, .. } => format!("{impedance:.0} Ω"),
        Part::Element { element, .. } => {
            let mut parts = Vec::new();
            if element.resistance > 0.0 {
                parts.push(format!("{:.3} Ω", element.resistance));
            }
            if element.inductance > 0.0 {
                parts.push(format!("{:.3} nH", element.inductance * 1e9));
            }
            if element.capacitance > 0.0 {
                parts.push(format!("{:.3} pF", element.capacitance * 1e12));
            }
            parts.join(" ")
        }
    }
}

/// A stage's kind as its block names it.
fn kind_name(part: &Part) -> crate::lang::Text {
    match part {
        Part::Amplifier { .. } => t!("Amplifier"),
        Part::Attenuator { .. } => t!("Attenuator"),
        Part::Data { .. } => t!("S-parameters"),
        Part::Line { .. } => t!("Line"),
        Part::Element { shunt: false, .. } => t!("Series element"),
        Part::Element { shunt: true, .. } => t!("Shunt element"),
    }
}

/// The block diagram: the source, each stage a block with its name, kind and numbers, arrows
/// between them, wrapping onto more rows when the card is narrow.
fn diagram(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    let t = &Tokens::get();
    let m = app.session.model();
    let stages = &m.system.stages;
    if stages.is_empty() {
        ui.text(Style::Dim, t!("No stages yet: add an amplifier, a filter's S-parameters or a line with the buttons above."));
        return;
    }
    let (w, h, gap) = (150.0, 64.0, 34.0);
    let per_row = (((ui.available_width() + gap) / (w + gap)).floor() as usize).max(2);
    let rows = (stages.len() + 1).div_ceil(per_row);
    let (area, _) = ui.allocate_exact_size(vec2(ui.available_width(), rows as f32 * (h + 18.0)), Sense::hover());
    let at = |k: usize| {
        let (r, c) = (k / per_row, k % per_row);
        Rect::from_min_size(pos2(area.left() + c as f32 * (w + gap), area.top() + r as f32 * (h + 18.0)), vec2(w, h))
    };
    let p = ui.painter().with_clip_rect(area);
    let font = |size: f32| FontId::proportional(size);
    // The source.
    let src = at(0);
    p.rect(src, 8.0, t.panel_header, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    let name = m.system.source.antenna.clone().unwrap_or_else(|| format!("{} Ω", m.system.receiver.impedance));
    p.text(src.center_top() + vec2(0.0, 14.0), Align2::CENTER_CENTER, tr(t!("Source")), font(t.font_body), t.text);
    p.text(src.center() + vec2(0.0, 8.0), Align2::CENTER_CENTER, emcraft_engine::text::clip(&name, 20), font(t.font_small), t.text_dim);
    for (k, stage) in stages.iter().enumerate() {
        let r = at(k + 1);
        let prev = at(k);
        // The arrow from the block before (along the row; across rows, from the row's end).
        let (from, to) =
            if prev.top() == r.top() { (prev.right_center(), r.left_center()) } else { (r.left_center() - vec2(gap * 0.8, 0.0), r.left_center()) };
        p.line_segment([from, to], Stroke::new(1.5, t.text_dim));
        p.line_segment([to, to + vec2(-7.0, -4.0)], Stroke::new(1.5, t.text_dim));
        p.line_segment([to, to + vec2(-7.0, 4.0)], Stroke::new(1.5, t.text_dim));
        let resp = ui.hit(r, Handle::SystemStage(&stage.name), Sense::click());
        let fill = if resp.hovered() { t.hover } else { t.panel };
        p.rect(r, 8.0, fill, Stroke::new(1.5, if resp.hovered() { t.accent } else { t.border }), egui::StrokeKind::Inside);
        p.text(r.center_top() + vec2(0.0, 13.0), Align2::CENTER_CENTER, emcraft_engine::text::clip(&stage.name, 20), font(t.font_body), t.text);
        p.text(r.center() + vec2(0.0, 2.0), Align2::CENTER_CENTER, tr(kind_name(&stage.part)), font(t.font_small), t.text_dim);
        p.text(r.center_bottom() - vec2(0.0, 12.0), Align2::CENTER_CENTER, summary(stage), font(t.font_small), t.accent);
        let mut values = emcraft_engine::system::stage_params(&app.session, &stage.part);
        values["stage"] = json!(stage.name);
        if resp.clicked() {
            out.push(Action::OpenDialog { id: "system.stage.edit".into(), values: Some(values.clone()) });
        }
        let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand).hint(t!("Click to change it; right-click to move or delete it"));
        let (n, k1) = (stages.len(), k + 1);
        resp.context_menu(|ui| {
            if ui.button(tr(t!("Edit…"))).clicked() {
                out.push(Action::OpenDialog { id: "system.stage.edit".into(), values: Some(values.clone()) });
                ui.close();
            }
            if ui.add_enabled(k > 0, egui::Button::new(tr(t!("Move toward the source")))).clicked() {
                out.push(Action::run("system.stage.move", json!({"stage": stage.name, "position": k1 - 1})));
                ui.close();
            }
            if ui.add_enabled(k1 < n, egui::Button::new(tr(t!("Move toward the receiver")))).clicked() {
                out.push(Action::run("system.stage.move", json!({"stage": stage.name, "position": k1 + 1})));
                ui.close();
            }
            if ui.button(tr(t!("Delete"))).clicked() {
                out.push(Action::run("system.stage.delete", json!({"stage": stage.name})));
                ui.close();
            }
        });
    }
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
        ui.text(Style::Dim, t!("Give the receiver a bandwidth (Receiver…) for its noise, SNR and sensitivity."));
    }
    if let Some(why) = &p.why {
        ui.text(Style::Dim, Raw(why));
    }
}
