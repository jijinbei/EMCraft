//! A node's card: its box and header (dragged to move a stage, its menu deleting it), its ports
//! (the output dragged to link), its rows of settings edited in place and the chain's reading
//! at its output. What a card holds is data ([`rows`]); drawing it is one loop.

use egui::{Align2, FontId, Rect, Sense, Stroke, pos2, vec2};
use serde_json::json;

use super::bridge::{drag_inputs, pos, rect};
use super::fields::{self, Number};
use crate::handle::Handle;
use crate::lang::tr;
use crate::theme::Tokens;
use crate::{Action, Actions, EmcraftApp, Hint, UiExt, scenario, system_ui};
use emcraft_ui_core::system::card::{Edit, command, reading, rows, title};
use emcraft_ui_core::system::gesture::Input;
use emcraft_ui_core::system::layout::{Kind, NodeBox};

/// A port's radius, and the room around it the pointer may press.
const PORT_R: f32 = 6.0;
const PORT_REACH: f32 = 16.0;

/// A command a card's row runs, as an action.
fn run(c: emcraft_ui_core::system::gesture::Command) -> Action {
    Action::run(c.id, c.params)
}

/// Draws `node`'s card and edits its rows in place; what the pointer did on its header and ports.
pub(super) fn card(app: &EmcraftApp, ui: &mut egui::Ui, node: &NodeBox, out: &mut Actions) -> Vec<Input> {
    let t = &Tokens::get();
    let r = rect(node.rect);
    let p = ui.painter().clone();
    p.rect(r, 8.0, if node.on_chain { t.dialog_body } else { t.panel_header }, Stroke::new(1.2, t.border), egui::StrokeKind::Inside);
    let head = rect(node.header());
    p.rect_filled(head, egui::CornerRadius { nw: 8, ne: 8, sw: 0, se: 0 }, if node.on_chain { t.accent_soft } else { t.border });
    p.text(head.left_center() + vec2(10.0, 0.0), Align2::LEFT_CENTER, title(&app.session, node), FontId::proportional(t.font_small), t.text);
    let inputs: Vec<Input> = header(ui, node, out).into_iter().chain(ports(ui, node)).collect();
    let font = FontId::proportional(t.font_small);
    let rows = rows(&app.session, node);
    for (k, row) in rows.iter().enumerate() {
        let line = rect(node.row(k));
        p.text(line.left_center(), Align2::LEFT_CENTER, &row.label, font.clone(), t.text_dim);
        let value_at = Rect::from_min_max(pos2(line.left() + 56.0, line.top()), line.max);
        match &row.value {
            Edit::Number { value, unit, optional } => {
                p.text(line.right_center(), Align2::RIGHT_CENTER, unit, font.clone(), t.text_dim);
                let rect = value_at.with_max_x(line.right() - 36.0);
                let field = Number { rect, handle: Handle::SystemField(&node.name, row.key), value: *value, optional: *optional };
                fields::number(ui, field, |v| Some(run(command(node, row.key, json!(v)))), out);
            }
            Edit::Choice { options, current } => {
                if let Some(v) = fields::choice(ui, value_at, egui::Id::new(("system_choice", &node.name, row.key)), options, current) {
                    out.push(run(command(node, row.key, json!(v))));
                }
            }
        }
    }
    if let Some(text) = reading(&app.session, node) {
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
