//! The few tokenised controls EMCraft's cards share, after SolidCraft's UI kit
//! (`crates/ui-egui/src/ui_kit/`: the card header, aligned label rows, footer actions, the error
//! band and the progress bar), much reduced. Each publishes a scenario handle where a test or an
//! agent needs to reach it.

use egui::{Align2, Color32, FontId, Rect, Response, Sense, Stroke, Ui, pos2, vec2};

use crate::scenario;
use crate::theme::Tokens;

/// A card's title band: glyph, title, optional dock toggle and a close button. Returns
/// (close clicked, dock clicked).
pub fn header(ui: &mut Ui, glyph: &str, title: &str, docked: Option<bool>, accent: Color32) -> (bool, bool) {
    let t = Tokens::get();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), t.form_header_height), Sense::hover());
    let icon = Rect::from_center_size(pos2(rect.left() + t.space_12 + 10.0, rect.center().y), vec2(20.0, 20.0));
    crate::glyphs::paint(ui.painter(), icon, glyph, t.icon, accent);
    let close = Rect::from_center_size(pos2(rect.right() - t.space_8 - 13.0, rect.center().y), vec2(26.0, 26.0));
    let close_r = icon_button(ui, close, "close", "Close");
    scenario::publish_target("dialog:close", close);
    let mut right = close.left();
    let mut dock_clicked = false;
    if let Some(docked) = docked {
        let dock = Rect::from_center_size(pos2(close.left() - t.space_4 - 13.0, rect.center().y), vec2(26.0, 26.0));
        let r = icon_button(ui, dock, if docked { "undock" } else { "dock" }, if docked { "Float" } else { "Dock" });
        scenario::publish_target("dialog:dock", dock);
        dock_clicked = r.clicked();
        right = dock.left();
    }
    let title_rect = Rect::from_min_max(pos2(icon.right() + t.space_8, rect.top()), pos2(right, rect.bottom()));
    ui.painter().with_clip_rect(title_rect).text(
        pos2(title_rect.left(), title_rect.center().y),
        Align2::LEFT_CENTER,
        title,
        FontId::proportional(t.font_heading),
        t.form_ink,
    );
    let tr = ui.interact(title_rect, ui.id().with("title"), Sense::hover());
    tr.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, title));
    ui.painter().hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, t.dialog_rule));
    (close_r.clicked(), dock_clicked)
}

/// A small square button with a drawn mark (`close`, `dock`, `undock`).
pub fn icon_button(ui: &mut Ui, r: Rect, mark: &str, label: &str) -> Response {
    let t = Tokens::get();
    let resp = ui.interact(r, ui.id().with(("icon_button", mark)), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    if resp.hovered() {
        ui.painter().rect_filled(r, t.radius_sm, t.hover);
    }
    let s = Stroke::new(1.4, t.form_dim);
    let c = r.center();
    let d = 4.5;
    match mark {
        "close" => {
            ui.painter().line_segment([c + vec2(-d, -d), c + vec2(d, d)], s);
            ui.painter().line_segment([c + vec2(d, -d), c + vec2(-d, d)], s);
        }
        "dock" => {
            ui.painter().rect_stroke(Rect::from_center_size(c, vec2(12.0, 10.0)), 1.0, s, egui::StrokeKind::Middle);
            ui.painter().line_segment([c + vec2(2.0, -5.0), c + vec2(2.0, 5.0)], s);
        }
        _ => {
            ui.painter().rect_stroke(Rect::from_center_size(c + vec2(-1.5, 1.5), vec2(9.0, 8.0)), 1.0, s, egui::StrokeKind::Middle);
            ui.painter().rect_stroke(Rect::from_center_size(c + vec2(2.0, -2.0), vec2(9.0, 8.0)), 1.0, s, egui::StrokeKind::Middle);
        }
    }
    resp.on_hover_text(label)
}

/// A form row: the label in the label column, the control after it. The label's text is the
/// row's accessible name.
pub fn row<R>(ui: &mut Ui, label: &str, required: bool, add: impl FnOnce(&mut Ui) -> R) -> R {
    let t = Tokens::get();
    ui.horizontal(|ui| {
        ui.set_min_height(t.control_height);
        let (r, _) = ui.allocate_exact_size(vec2(t.form_label_width, t.control_height), Sense::hover());
        let text = if required { format!("{label} *") } else { label.to_owned() };
        ui.painter().with_clip_rect(r).text(pos2(r.left(), r.center().y), Align2::LEFT_CENTER, &text, FontId::proportional(t.font_label), t.form_dim);
        let lr = ui.interact(r, ui.id().with(("row_label", label)), Sense::hover());
        lr.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, label));
        add(ui)
    })
    .inner
}

/// A caption: small dim wrapping text.
pub fn caption(ui: &mut Ui, text: &str) {
    let t = Tokens::get();
    ui.label(egui::RichText::new(text).size(t.font_small).color(t.text_dim));
}

/// A section heading in small capitals with a hairline after it.
pub fn section(ui: &mut Ui, text: &str) {
    let t = Tokens::get();
    ui.add_space(t.space_4);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 18.0), Sense::hover());
    let g = ui.painter().layout_no_wrap(text.to_uppercase(), FontId::proportional(t.font_small), t.text_dim);
    let w = g.size().x;
    ui.painter().galley(pos2(r.left(), r.center().y - g.size().y / 2.0), g, t.text_dim);
    ui.painter().hline((r.left() + w + t.space_6)..=r.right(), r.center().y, Stroke::new(1.0, t.dialog_rule));
}

/// A full-width error band.
pub fn error_band(ui: &mut Ui, text: &str) {
    let t = Tokens::get();
    egui::Frame::NONE.fill(t.form_error_fill).corner_radius(t.radius_sm).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let r = ui.label(egui::RichText::new(text).size(t.font_body).color(t.form_error));
        scenario::publish_rect("dialog:error", r.rect);
    });
}

/// A button: secondary, or `primary` in the accent colour. Publishes `handle` at its centre.
pub fn button(ui: &mut Ui, label: &str, primary: bool, enabled: bool, handle: &str) -> Response {
    let t = Tokens::get();
    let text = egui::RichText::new(label).size(t.font_button).color(if primary { t.on_accent } else { t.form_ink });
    let mut b = egui::Button::new(text).min_size(vec2(72.0, t.control_height)).corner_radius(t.control_radius);
    b = if primary { b.fill(t.accent) } else { b.fill(t.control_fill).stroke(Stroke::new(1.0, t.control_border)) };
    let r = ui.add_enabled(enabled, b);
    scenario::publish_target(handle, r.rect);
    r
}

/// A determinate (`Some`) or running (`None`) progress bar.
pub fn progress(ui: &mut Ui, fraction: Option<f64>, width: f32) -> Response {
    let t = Tokens::get();
    let (r, resp) = ui.allocate_exact_size(vec2(width, 6.0), Sense::hover());
    ui.painter().rect_filled(r, 3, t.dialog_rule);
    match fraction {
        Some(f) => {
            let w = r.width() * f.clamp(0.0, 1.0) as f32;
            ui.painter().rect_filled(Rect::from_min_size(r.min, vec2(w, r.height())), 3, t.accent);
        }
        None => {
            // A running segment sliding along the bar.
            let time = ui.input(|i| i.time);
            let phase = (time * 0.8).fract() as f32;
            let seg = r.width() * 0.3;
            let x0 = r.left() + (r.width() + seg) * phase - seg;
            let s = Rect::from_min_max(pos2(x0.max(r.left()), r.top()), pos2((x0 + seg).min(r.right()), r.bottom()));
            if s.is_positive() {
                ui.painter().rect_filled(s, 3, t.accent);
            }
            ui.ctx().request_repaint();
        }
    }
    scenario::publish_rect("progress", r);
    resp
}
