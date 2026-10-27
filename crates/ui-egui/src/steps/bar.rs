//! The step bar under the ribbon: each step with its status mark.

use super::*;

const BAR_HEIGHT: f32 = 40.0;

/// The step bar under the ribbon.
pub fn bar(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    // A row's hover highlight in 3D lasts one frame: the panel drawn after this sets it again.
    app.view3d.hover = None;
    let states = statuses(app);
    egui::Panel::top("emcraft_steps")
        .exact_size(BAR_HEIGHT)
        .show_separator_line(false)
        .frame(egui::Frame::NONE.fill(t.viewport_top).inner_margin(egui::Margin { left: 6, right: 6, top: 0, bottom: 6 }))
        .show(ui, |ui| {
            let card = ui.max_rect();
            ui.painter().add(t.card(t.toolbar).paint(card));
            scenario::publish_rect("steps", card);
            let font = FontId::proportional(t.font_body);
            let mut x = card.left() + 8.0;
            let mut clicked = None;
            for (i, s) in states.iter().enumerate() {
                let text = format!("{} {}", i, s.step.label());
                let g = ui.painter().layout_no_wrap(text.clone(), font.clone(), t.text);
                let r = Rect::from_min_size(pos2(x, card.top() + 4.0), vec2(g.size().x + 34.0, card.height() - 8.0));
                let resp = ui.interact(r, ui.id().with(("step", i)), Sense::click());
                let tip = if s.messages.is_empty() {
                    format!("{}: {}", s.step.label(), s.status.id())
                } else {
                    s.messages.iter().map(|m| m.text.as_str()).collect::<Vec<_>>().join("\n")
                };
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, s.step.label()));
                let active = app.workflow.step == Some(s.step);
                let fill = if active {
                    t.accent_soft
                } else if resp.hovered() {
                    t.hover
                } else {
                    Color32::TRANSPARENT
                };
                ui.painter().rect_filled(r, 12.0, fill);
                ui.painter().rect_stroke(r, 12.0, Stroke::new(1.0, if active { t.accent } else { t.border }), egui::StrokeKind::Inside);
                mark(ui.painter(), pos2(r.left() + 14.0, r.center().y), s.status, &t);
                ui.painter().galley(pos2(r.left() + 26.0, r.center().y - g.size().y / 2.0), g, if active { t.selected_ink } else { t.text });
                scenario::publish_target(&format!("step:{}", s.step.id()), r);
                if resp.on_hover_text(tip).clicked() {
                    clicked = Some(s.step);
                }
                x = r.right() + 4.0;
                if i + 1 < states.len() {
                    ui.painter().hline(x..=x + 8.0, r.center().y, Stroke::new(1.0, t.border));
                    x += 12.0;
                }
            }
            if let Some(step) = clicked {
                let again = app.workflow.step == Some(step);
                open_step(app, if again { None } else { Some(step) });
            }
        });
}

/// A status mark: a tick, an exclamation, a cross or a turning arrow in a disc.
pub(super) fn mark(p: &egui::Painter, c: egui::Pos2, s: Status, t: &Tokens) {
    let col = status_color(s, t);
    p.circle_filled(c, 7.0, col);
    let ink = Stroke::new(1.8, Color32::WHITE);
    match s {
        Status::Done => {
            p.line_segment([c + vec2(-3.5, 0.2), c + vec2(-1.0, 2.8)], ink);
            p.line_segment([c + vec2(-1.0, 2.8), c + vec2(3.6, -2.6)], ink);
        }
        Status::Warning => {
            p.line_segment([c + vec2(0.0, -3.8), c + vec2(0.0, 1.0)], ink);
            p.circle_filled(c + vec2(0.0, 3.4), 1.1, Color32::WHITE);
        }
        Status::Missing => {
            p.line_segment([c + vec2(-3.0, -3.0), c + vec2(3.0, 3.0)], ink);
            p.line_segment([c + vec2(-3.0, 3.0), c + vec2(3.0, -3.0)], ink);
        }
        Status::Stale => {
            let pts: Vec<egui::Pos2> = (0..=10).map(|k| 0.6 + 4.6 * k as f32 / 10.0).map(|a| c + vec2(a.cos(), a.sin()) * 3.6).collect();
            p.add(egui::Shape::line(pts, ink));
            let end = c + vec2(5.2f32.cos(), 5.2f32.sin()) * 3.6;
            p.line_segment([end, end + vec2(2.6, 0.4)], ink);
        }
    }
}
