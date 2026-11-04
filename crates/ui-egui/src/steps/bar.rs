//! The step bar under the ribbon: each step with its status mark. Six looks, told apart by shape
//! as well as colour: done (a green disc with a tick), to do (a hollow ring), waiting on an
//! earlier step (a grey disc with dots, the step's name dimmed), a warning (an amber disc with
//! an exclamation), needs attention (a red disc with a cross), out of date (a blue disc with a
//! turning arrow); and Run while a run goes on (a turning arc). A step the chosen kind of
//! analysis does without, with nothing in it, shows a grey ring with a dash, its name dimmed.

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
            // The kind of analysis first, then the steps from the geometry on.
            let row = card.shrink2(vec2(8.0, 4.0));
            let kind = Rect::from_min_size(row.min, vec2(crate::kinds::SWITCH_WIDTH, row.height()));
            crate::kinds::switch(app, ui, kind, out);
            let states: Vec<&StepState> = states.iter().filter(|s| s.step != Step::Intent).collect();
            let list: Vec<Pill> = states
                .iter()
                .map(|s| {
                    let run_going = running && s.step == Step::Run;
                    let status = if run_going { crate::lang::tr("running") } else { shown(s.status) };
                    let mut checks: Vec<String> = s.messages.iter().map(|m| m.text.clone()).collect();
                    if let Some(e) = run_failure(app).filter(|_| s.step == Step::Run) {
                        checks.insert(
                            0,
                            crate::lang::trf("the last run failed: {error}", crate::lang::args!(error = emcraft_engine::text::clip(e, 300))),
                        );
                    }
                    let unneeded = not_needed(app, s.step);
                    if let Some(why) = &unneeded {
                        checks.insert(0, why.clone());
                    }
                    let tip = if checks.is_empty() || run_going {
                        format!("{}: {status}", shown(s.step))
                    } else {
                        format!("{}: {status}\n{}", shown(s.step), checks.join("\n"))
                    };
                    let look = match (run_going, unneeded.is_some(), s.status) {
                        (true, _, _) => Look::Running,
                        (false, true, Status::Done) => Look::Skipped,
                        (false, true, status) => Look::Unneeded(status),
                        (false, false, status) => Look::Plain(status),
                    };
                    Pill {
                        text: format!("{} {}", s.step.index(), shown(s.step)),
                        short: s.step.index().to_string(),
                        said: status.to_owned(),
                        look,
                        active: app.workflow.step == Some(s.step),
                        enabled: true,
                        tip,
                        handle: Handle::Step(s.step),
                    }
                })
                .collect();
            let rest = Rect::from_min_max(pos2(kind.right() + 10.0, row.top()), row.max);
            if let Some(i) = pills(ui, rest, &list) {
                let step = states[i].step;
                let again = app.workflow.step == Some(step);
                out.push(Action::OpenStep(if again { None } else { Some(step) }));
            }
        });
}

/// A stage of a staged bar (the step bar, the PCB window's stages): its text and its short form
/// (when the texts do not fit), its status as a screen reader says it, its mark, whether it is
/// the one open and whether it can be clicked, its tip and its handle.
pub(crate) struct Pill<'a> {
    pub text: String,
    pub short: String,
    pub said: String,
    pub look: Look,
    pub active: bool,
    pub enabled: bool,
    pub tip: String,
    pub handle: Handle<'a>,
}

/// Draws `list` left to right in `row` (each a pill with its mark, joined by short lines; their
/// short forms when the texts do not fit): the one clicked.
pub(crate) fn pills(ui: &mut egui::Ui, row: Rect, list: &[Pill]) -> Option<usize> {
    let t = &Tokens::get();
    let font = FontId::proportional(t.font_body);
    let width = |text: &str| ui.painter().layout_no_wrap(text.to_owned(), font.clone(), t.text).size().x + 34.0 + 16.0;
    let short = list.iter().map(|p| width(&p.text)).sum::<f32>() > row.width();
    let mut x = row.left();
    let mut clicked = None;
    for (i, p) in list.iter().enumerate() {
        let g = ui.painter().layout_no_wrap(if short { p.short.clone() } else { p.text.clone() }, font.clone(), t.text);
        let r = Rect::from_min_size(pos2(x, row.top()), vec2(g.size().x + 34.0, row.height()));
        let resp = ui.hit(r, p.handle, if p.enabled { Sense::click() } else { Sense::hover() });
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, p.enabled, format!("{}: {}", p.text, p.said)));
        let fill = match (p.active, resp.hovered() && p.enabled) {
            (true, _) => t.accent_soft,
            (false, true) => t.hover,
            (false, false) => Color32::TRANSPARENT,
        };
        ui.painter().rect_filled(r, 12.0, fill);
        ui.painter().rect_stroke(r, 12.0, Stroke::new(1.0, if p.active { t.accent } else { t.border }), egui::StrokeKind::Inside);
        let c = pos2(r.left() + 14.0, r.center().y);
        match p.look {
            Look::Running => running_mark(ui, c, t),
            Look::Skipped => skip_mark(ui.painter(), c, t),
            Look::Unneeded(status) | Look::Plain(status) => mark(ui.painter(), c, status, t),
        }
        let ink = match (p.active, p.look) {
            (true, _) => t.selected_ink,
            (false, Look::Skipped | Look::Unneeded(_) | Look::Plain(Status::Waiting)) => t.text_dim,
            (false, _) => t.text,
        };
        ui.painter().galley(pos2(r.left() + 26.0, r.center().y - g.size().y / 2.0), g, ink);
        if resp.on_hover_text(&p.tip).clicked() && p.enabled {
            clicked = Some(i);
        }
        x = r.right() + 4.0;
        if i + 1 < list.len() {
            ui.painter().hline(x..=x + 8.0, r.center().y, Stroke::new(1.0, t.border));
            x += 12.0;
        }
    }
    clicked
}

/// How a step's pill looks: the run under way, a step not needed (skipped when it is done),
/// or its status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Look {
    Running,
    Skipped,
    Unneeded(Status),
    Plain(Status),
}

/// A status mark: a tick, a hollow ring, dots, an exclamation, a cross or a turning arrow.
pub(crate) fn mark(p: &egui::Painter, c: egui::Pos2, s: Status, t: &Tokens) {
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

/// The mark of a step the job's kind of analysis does without: a grey ring with a dash.
pub(crate) fn skip_mark(p: &egui::Painter, c: egui::Pos2, t: &Tokens) {
    p.circle_stroke(c, 6.0, Stroke::new(1.4, t.text_dim));
    p.line_segment([c + vec2(-3.0, 0.0), c + vec2(3.0, 0.0)], Stroke::new(1.6, t.text_dim));
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
