//! The step bar under the ribbon: each step with its status mark. Six looks, told apart by shape
//! as well as colour: done (a green disc with a tick), to do (a hollow ring), waiting on an
//! earlier step (a grey disc with dots, the step's name dimmed), a warning (an amber disc with
//! an exclamation), needs attention (a red disc with a cross), out of date (a blue disc with a
//! turning arrow); and Run while a run goes on (a turning arc).

use super::*;
use crate::lang::shown;

const BAR_HEIGHT: f32 = 40.0;

/// The step bar under the ribbon.
pub fn bar(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    let t = &Tokens::get();
    let states = statuses_now(app);
    let running = running(app);
    egui::Panel::top("emcraft_steps")
        .exact_size(BAR_HEIGHT)
        .show_separator_line(false)
        .frame(egui::Frame::NONE.fill(t.viewport_top).inner_margin(egui::Margin { left: 6, right: 6, top: 0, bottom: 6 }))
        .show(ui, |ui| {
            let card = ui.max_rect();
            ui.painter().add(t.card(t.toolbar).paint(card));
            scenario::publish_rect(Fixed::Steps, card);
            let font = FontId::proportional(t.font_body);
            let mut x = card.left() + 8.0;
            let mut clicked = None;
            for (i, s) in states.iter().enumerate() {
                let text = format!("{} {}", i, shown(s.step));
                let g = ui.painter().layout_no_wrap(text.clone(), font.clone(), t.text);
                let r = Rect::from_min_size(pos2(x, card.top() + 4.0), vec2(g.size().x + 34.0, card.height() - 8.0));
                let resp = ui.hit(r, Handle::Step(s.step), Sense::click());
                let run_going = running && s.step == Step::Run;
                let status = if run_going { crate::lang::tr("running") } else { shown(s.status) };
                let mut checks: Vec<String> = s.messages.iter().map(|m| m.text.clone()).collect();
                if let Some(e) = run_failure(app).filter(|_| s.step == Step::Run) {
                    checks
                        .insert(0, crate::lang::trf("the last run failed: {error}", crate::lang::args!(error = emcraft_engine::text::clip(e, 300))));
                }
                let tip = if checks.is_empty() || run_going {
                    format!("{}: {status}", shown(s.step))
                } else {
                    format!("{}: {status}\n{}", shown(s.step), checks.join("\n"))
                };
                // A screen reader hears the step and where it stands.
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("{text}: {status}")));
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
                let c = pos2(r.left() + 14.0, r.center().y);
                if run_going {
                    running_mark(ui, c, t);
                } else {
                    mark(ui.painter(), c, s.status, t);
                }
                let ink = if active {
                    t.selected_ink
                } else if s.status == Status::Waiting && !run_going {
                    t.text_dim
                } else {
                    t.text
                };
                ui.painter().galley(pos2(r.left() + 26.0, r.center().y - g.size().y / 2.0), g, ink);
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
                out.push(Action::OpenStep(if again { None } else { Some(step) }));
            }
        });
}

/// A status mark: a tick, a hollow ring, dots, an exclamation, a cross or a turning arrow.
pub(super) fn mark(p: &egui::Painter, c: egui::Pos2, s: Status, t: &Tokens) {
    let col = status_color(s, t);
    let ink = Stroke::new(1.8, t.on_status);
    match s {
        Status::Done => {
            p.circle_filled(c, 7.0, col);
            p.line_segment([c + vec2(-3.5, 0.2), c + vec2(-1.0, 2.8)], ink);
            p.line_segment([c + vec2(-1.0, 2.8), c + vec2(3.6, -2.6)], ink);
        }
        Status::Todo => {
            p.circle_stroke(c, 6.0, Stroke::new(1.6, col));
        }
        Status::Waiting => {
            p.circle_filled(c, 7.0, t.border);
            for dx in [-3.0, 0.0, 3.0] {
                p.circle_filled(c + vec2(dx, 0.0), 1.1, t.text_dim);
            }
        }
        Status::Warning => {
            p.circle_filled(c, 7.0, col);
            p.line_segment([c + vec2(0.0, -3.8), c + vec2(0.0, 1.0)], ink);
            p.circle_filled(c + vec2(0.0, 3.4), 1.1, t.on_status);
        }
        Status::Missing => {
            p.circle_filled(c, 7.0, col);
            p.line_segment([c + vec2(-3.0, -3.0), c + vec2(3.0, 3.0)], ink);
            p.line_segment([c + vec2(-3.0, 3.0), c + vec2(3.0, -3.0)], ink);
        }
        Status::Stale => {
            p.circle_filled(c, 7.0, col);
            let pts: Vec<egui::Pos2> = (0..=10).map(|k| 0.6 + 4.6 * k as f32 / 10.0).map(|a| c + vec2(a.cos(), a.sin()) * 3.6).collect();
            p.add(egui::Shape::line(pts, ink));
            let end = c + vec2(5.2f32.cos(), 5.2f32.sin()) * 3.6;
            p.line_segment([end, end + vec2(2.6, 0.4)], ink);
        }
    }
}

/// Run's mark while a run goes on: an arc of the accent turning on a faint ring.
pub(super) fn running_mark(ui: &egui::Ui, c: egui::Pos2, t: &Tokens) {
    let p = ui.painter();
    p.circle_stroke(c, 6.0, Stroke::new(1.6, t.accent_soft));
    let start = (ui.input(|i| i.time) * 4.0) as f32;
    let pts: Vec<egui::Pos2> = (0..=12).map(|k| start + 4.0 * k as f32 / 12.0).map(|a| c + vec2(a.cos(), a.sin()) * 6.0).collect();
    p.add(egui::Shape::line(pts, Stroke::new(2.0, t.accent)));
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
}
