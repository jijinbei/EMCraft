//! What the canvas edits in place: a number typed into a field, a name chosen from a list.

use egui::{FontId, Rect, vec2};

use crate::handle::Handle;
use crate::theme::Tokens;
use crate::{Action, Actions, scenario};

/// A number edited in place: where, its name on screen, the model's value and whether it may be
/// left empty.
pub(super) struct Number<'a> {
    pub rect: Rect,
    pub handle: Handle<'a>,
    pub value: Option<f64>,
    pub optional: bool,
}

/// A number edited in place: shown as the model has it, typed into while it has the keyboard,
/// committed when it loses it (Enter, a click elsewhere) if changed — `commit` with the number,
/// or `None` for an empty field when `optional`. Its text while typed lives in the context's
/// memory.
pub(super) fn number(ui: &mut egui::Ui, field: Number, commit: impl FnOnce(Option<f64>) -> Option<Action>, out: &mut Actions) {
    let Number { rect, handle, value, optional } = field;
    let t = &Tokens::get();
    let id = egui::Id::new(("system_field", handle.to_string()));
    let shown = emcraft_ui_core::system::card::shown(value);
    let mut text = ui.ctx().data_mut(|d| d.get_temp::<String>(id)).unwrap_or_else(|| shown.clone());
    let edit =
        egui::TextEdit::singleline(&mut text).font(FontId::proportional(t.font_small)).horizontal_align(egui::Align::RIGHT).margin(vec2(4.0, 1.0));
    let r = ui.put(rect, edit);
    scenario::publish_target(handle, r.rect);
    if r.has_focus() {
        ui.ctx().data_mut(|d| d.insert_temp(id, text.clone()));
    }
    if !r.lost_focus() {
        return;
    }
    ui.ctx().data_mut(|d| d.remove::<String>(id));
    if let Some(a) = emcraft_ui_core::system::card::committed(&text, value, optional).and_then(commit) {
        out.push(a);
    }
}

/// A choice in place at `rect` among `options` (each its value and its text): the value picked
/// when another than `current` is.
pub(super) fn choice(ui: &mut egui::Ui, rect: Rect, id: egui::Id, options: &[(String, String)], current: &str) -> Option<String> {
    let mut chosen = current.to_owned();
    let shown = options.iter().find(|(v, _)| v == current).map_or(current, |(_, s)| s.as_str()).to_owned();
    ui.put(rect, |ui: &mut egui::Ui| {
        egui::ComboBox::from_id_salt(id)
            .selected_text(shown)
            .width(rect.width())
            .show_ui(ui, |ui| {
                for (v, s) in options {
                    ui.selectable_value(&mut chosen, v.clone(), s);
                }
            })
            .response
    });
    (chosen != current).then_some(chosen)
}
