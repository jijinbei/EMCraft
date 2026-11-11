//! The system view: the chain as a block diagram from its source on (a click opens a stage's
//! settings in the Chain step, its menu moves or deletes it; the ribbon's SYSTEM tab adds
//! stages), the chart `solve.system`'s result is read in (along the chain, or over the band) and
//! the budget stage by stage under it. With no stage yet, a button for each kind.

use super::*;
use crate::system_ui;
use emcraft_engine::system::{Row, SystemResult};
use emcraft_engine::view::SystemChart;

pub(super) struct SystemView;

impl ChartView for SystemView {
    type Key = SharedChart;
    const HANDLE: Fixed = Fixed::ChartSystem;
    const CHART: emcraft_engine::views::ChartId = emcraft_engine::views::ChartId::System;
    const HEIGHT: Height = Height::new(0.45, 220.0, 520.0);

    fn controls(app: &EmcraftApp, st: &mut Parts, ui: &mut egui::Ui, out: &mut Actions) -> Option<Self::Key> {
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
                    out.extend(system_ui::add(kind));
                }
            }
        });
    });
}

/// The block diagram: the source, each stage a block with its name, kind and numbers, arrows
/// between them, wrapping onto more rows when the card is narrow.
fn diagram(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    let t = &Tokens::get();
    let m = app.session.model();
    let stages = &m.system.stages;
    if stages.is_empty() {
        start(app, ui, out);
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
        p.text(r.center() + vec2(0.0, 2.0), Align2::CENTER_CENTER, tr(system_ui::kind_name(&stage.part)), font(t.font_small), t.text_dim);
        p.text(r.center_bottom() - vec2(0.0, 12.0), Align2::CENTER_CENTER, system_ui::summary(stage), font(t.font_small), t.accent);
        if resp.clicked() {
            out.extend(system_ui::edit(&stage.name));
        }
        let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand).hint(t!("Click to change it; right-click to move or delete it"));
        let (n, k1) = (stages.len(), k + 1);
        resp.context_menu(|ui| {
            if ui.button(tr(t!("Edit…"))).clicked() {
                out.extend(system_ui::edit(&stage.name));
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
