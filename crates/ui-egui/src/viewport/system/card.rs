//! A node's card: its box and header (dragged to move a stage, its menu deleting it), its ports
//! (the output dragged to link), its rows of settings edited in place and the chain's reading
//! at its output. What a card holds is data ([`rows`]); drawing it is one loop.

use egui::{Align2, FontId, Rect, Sense, Stroke, pos2, vec2};
use emcraft_model::system::{DataSource, Part, Stage};
use serde_json::{Value, json};

use super::bridge::{drag_inputs, pos, rect};
use super::fields::{self, Number};
use crate::handle::Handle;
use crate::lang::{tr, trf};
use crate::theme::Tokens;
use crate::{Action, Actions, EmcraftApp, Hint, UiExt, scenario, system_ui};
use emcraft_ui_core::system::gesture::Input;
use emcraft_ui_core::system::layout::{Kind, NodeBox};

/// A port's radius, and the room around it the pointer may press.
const PORT_R: f32 = 6.0;
const PORT_REACH: f32 = 16.0;

/// What a row edits.
enum Edit {
    /// A number in `unit`, which may be left empty when `optional`.
    Number { value: Option<f64>, unit: String, optional: bool },
    /// A name among `options` (each its value and its text).
    Choice { options: Vec<(String, String)>, current: String },
}

/// A row of a card: its label, the parameter it sets and what it edits.
struct Row {
    label: String,
    key: &'static str,
    value: Edit,
}

fn number(label: &str, key: &'static str, value: Option<f64>, unit: &str, optional: bool) -> Row {
    Row { label: label.to_owned(), key, value: Edit::Number { value, unit: unit.to_owned(), optional } }
}

/// The measurements of `ports` ports, as choices.
fn measured(app: &EmcraftApp, ports: usize) -> impl Iterator<Item = (String, String)> + '_ {
    app.session.views().measured.iter().filter(move |x| x.data.outputs() == ports).map(|x| (x.name.clone(), x.name.clone()))
}

/// A stage's rows: its own settings (the rest in its dialog, All settings…).
fn stage_rows(app: &EmcraftApp, stage: &Stage) -> Vec<Row> {
    let v = emcraft_engine::system::stage_params(&app.session, &stage.part);
    let n = |label: &str, key: &'static str, unit: &str, optional: bool| number(label, key, v[key].as_f64(), unit, optional);
    let lu = app.session.display().length.symbol();
    match &stage.part {
        Part::Amplifier { .. } => {
            vec![n("G", "gain", "dB", false), n("NF", "noise_figure", "dB", false), n("OIP3", "oip3", "dBm", true), n("P1dB", "p1db", "dBm", true)]
        }
        Part::Attenuator { .. } => vec![n("L", "loss", "dB", false)],
        Part::Line { .. } => vec![n("Z₀", "impedance", "Ω", false), n("ℓ", "length", lu, false), n("εr", "eps_r", "", false)],
        Part::Element { .. } => vec![n("R", "resistance", "Ω", false), n("L", "inductance", "nH", false), n("C", "capacitance", "pF", false)],
        Part::Data { data, .. } => {
            let current = match data {
                DataSource::Sweep => "sweep".to_owned(),
                DataSource::Measurement { name } => name.clone(),
            };
            let options = std::iter::once(("sweep".to_owned(), tr(t!("the sweep")).to_owned())).chain(measured(app, 2)).collect();
            vec![Row { label: "S".into(), key: "data", value: Edit::Choice { options, current } }]
        }
    }
}

/// The rows `node`'s card shows.
fn rows(app: &EmcraftApp, node: &NodeBox) -> Vec<Row> {
    let m = app.session.model();
    let fu = app.session.display().frequency;
    match node.kind {
        Kind::Source => {
            let src = &m.system.source;
            let options = std::iter::once((String::new(), format!("{} Ω", m.system.receiver.impedance))).chain(measured(app, 1)).collect();
            let current = src.antenna.clone().unwrap_or_default();
            vec![
                Row { label: tr(t!("antenna")).to_owned(), key: "antenna", value: Edit::Choice { options, current } },
                number("T", "temperature", Some(src.temperature), "K", false),
                number(tr(t!("signal")), "power", src.power, "dBm", true),
            ]
        }
        Kind::Receiver => {
            let r = &m.system.receiver;
            vec![
                number("B", "bandwidth", r.bandwidth.map(|b| fu.from_si(b)), fu.symbol(), true),
                number("SNR", "snr", Some(r.snr), "dB", false),
                number("Z₀", "impedance", Some(r.impedance), "Ω", false),
            ]
        }
        Kind::Stage(i) => stage_rows(app, &m.system.stages[i]),
    }
}

/// The command a row's new value runs: the source's, the receiver's or the stage's settings.
fn command(node: &NodeBox, key: &str, value: Value) -> Action {
    match node.kind {
        Kind::Source => Action::run("system.source", json!({ key: value })),
        Kind::Receiver => Action::run("system.receiver", json!({ key: value })),
        Kind::Stage(_) => Action::run("system.stage.edit", json!({"stage": node.name, key: value})),
    }
}

/// What the chain reads at `node`'s output: a stage's gain and noise figure, the receiver's
/// sensitivity.
fn reading(app: &EmcraftApp, node: &NodeBox) -> Option<String> {
    let f = |x: Option<f64>| x.map_or_else(|| "—".to_owned(), |v| format!("{v:.2}"));
    match node.kind {
        Kind::Source => None,
        Kind::Receiver => {
            let r = app.session.results().system.as_deref()?;
            let s = r.nearest(r.at)?.sensitivity_dbm?;
            Some(trf(t!("sensitivity {s} dBm"), crate::args!(s = format!("{s:.1}"))))
        }
        Kind::Stage(_) => {
            let (g, nf) = system_ui::reading(&app.session, &node.name)?;
            Some(trf(t!("total {gain} dB · NF {nf} dB"), crate::args!(gain = f(g), nf = f(nf))))
        }
    }
}

/// The card's title: the source, the receiver, or a stage's name and kind.
fn title(app: &EmcraftApp, node: &NodeBox) -> String {
    match node.kind {
        Kind::Source => tr(t!("Source")).to_owned(),
        Kind::Receiver => tr(t!("Receiver")).to_owned(),
        Kind::Stage(i) => {
            let s = &app.session.model().system.stages[i];
            format!("{}  ·  {}", emcraft_engine::text::clip(&s.name, 18), tr(system_ui::kind_name(&s.part)))
        }
    }
}

/// Draws `node`'s card and edits its rows in place; what the pointer did on its header and ports.
pub(super) fn card(app: &EmcraftApp, ui: &mut egui::Ui, node: &NodeBox, out: &mut Actions) -> Vec<Input> {
    let t = &Tokens::get();
    let r = rect(node.rect);
    let p = ui.painter().clone();
    p.rect(r, 8.0, if node.on_chain { t.dialog_body } else { t.panel_header }, Stroke::new(1.2, t.border), egui::StrokeKind::Inside);
    let head = rect(node.header());
    p.rect_filled(head, egui::CornerRadius { nw: 8, ne: 8, sw: 0, se: 0 }, if node.on_chain { t.accent_soft } else { t.border });
    p.text(head.left_center() + vec2(10.0, 0.0), Align2::LEFT_CENTER, title(app, node), FontId::proportional(t.font_small), t.text);
    let inputs: Vec<Input> = header(ui, node, out).into_iter().chain(ports(ui, node)).collect();
    let font = FontId::proportional(t.font_small);
    let rows = rows(app, node);
    for (k, row) in rows.iter().enumerate() {
        let line = rect(node.row(k));
        p.text(line.left_center(), Align2::LEFT_CENTER, &row.label, font.clone(), t.text_dim);
        let value_at = Rect::from_min_max(pos2(line.left() + 56.0, line.top()), line.max);
        match &row.value {
            Edit::Number { value, unit, optional } => {
                p.text(line.right_center(), Align2::RIGHT_CENTER, unit, font.clone(), t.text_dim);
                let rect = value_at.with_max_x(line.right() - 36.0);
                let field = Number { rect, handle: Handle::SystemField(&node.name, row.key), value: *value, optional: *optional };
                fields::number(ui, field, |v| Some(command(node, row.key, json!(v))), out);
            }
            Edit::Choice { options, current } => {
                if let Some(v) = fields::choice(ui, value_at, egui::Id::new(("system_choice", &node.name, row.key)), options, current) {
                    out.push(command(node, row.key, json!(v)));
                }
            }
        }
    }
    if let Some(text) = reading(app, node) {
        p.text(rect(node.row(rows.len())).left_center(), Align2::LEFT_CENTER, text, font.clone(), t.accent);
    }
    if !node.on_chain {
        p.text(
            r.right_bottom() + vec2(-8.0, -6.0),
            Align2::RIGHT_BOTTOM,
            tr(t!("off the chain")),
            FontId::proportional(t.font_small - 1.0),
            t.text_dim,
        );
    }
    inputs
}

/// The header: a stage's dragged to move it, its menu deleting it.
fn header(ui: &mut egui::Ui, node: &NodeBox, out: &mut Actions) -> Vec<Input> {
    let Kind::Stage(_) = node.kind else {
        ui.hit(rect(node.header()), Handle::SystemNode(&node.name), Sense::hover());
        return Vec::new();
    };
    let resp = ui.hit(rect(node.header()), Handle::SystemNode(&node.name), Sense::click_and_drag());
    let inputs = drag_inputs(&resp, Input::Grab(node.name.clone()));
    let resp = resp.on_hover_cursor(egui::CursorIcon::Grab).hint(t!("Drag to move it; right-click for more"));
    resp.context_menu(|ui| {
        if ui.button(tr(t!("All settings…"))).clicked() {
            out.push(Action::EditItem(system_ui::item(&node.name)));
            ui.close();
        }
        if ui.button(tr(t!("Delete"))).clicked() {
            out.push(Action::run("system.stage.delete", json!({"stage": node.name})));
            ui.close();
        }
    });
    inputs
}

/// The ports: an input (but the source's), an output (but the receiver's) dragged to link.
fn ports(ui: &mut egui::Ui, node: &NodeBox) -> Vec<Input> {
    let t = &Tokens::get();
    let square = |c: egui::Pos2, side: f32| Rect::from_center_size(c, vec2(side, side));
    if node.kind != Kind::Source {
        ui.painter().circle(pos(node.input()), PORT_R, t.panel, Stroke::new(1.5, t.accent));
        scenario::publish_target(Handle::SystemIn(&node.name), square(pos(node.input()), 2.0 * PORT_R));
    }
    if node.kind == Kind::Receiver {
        return Vec::new();
    }
    ui.painter().circle_filled(pos(node.output()), PORT_R, t.accent);
    let port = ui.hit(square(pos(node.output()), 2.0 * PORT_REACH), Handle::SystemOut(&node.name), Sense::drag());
    let inputs = drag_inputs(&port, Input::StartLink(node.name.clone()));
    port.on_hover_cursor(egui::CursorIcon::Crosshair).hint(t!("Drag to another node's input to link them"));
    inputs
}
