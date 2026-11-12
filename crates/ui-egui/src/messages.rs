//! The message area at the bottom: the last messages (a message repeated in a row counted, not
//! repeated), and the running job's progress with its Cancel.

use crate::handle::Fixed;

use crate::{Action, Actions, EmcraftApp, Hint, UiExt, scenario, theme, ui_kit};

/// A line of the message area.
#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    pub text: String,
    pub error: bool,
    /// The same message repeated in a row.
    pub count: u32,
}

/// Messages kept.
const MAX_MESSAGES: usize = 200;

impl EmcraftApp {
    /// Reports `text` in the message area.
    pub fn info(&mut self, text: impl std::fmt::Display) {
        self.message(text, false);
    }

    /// Reports `text` in the message area as an error.
    pub fn error(&mut self, text: impl std::fmt::Display) {
        self.message(text, true);
    }

    /// Reports `template` in the current language, its placeholders filled with `args`.
    pub fn info_with(&mut self, template: crate::lang::Text, args: &[(&str, &dyn std::fmt::Display)]) {
        self.message(crate::lang::trf(template, args), false);
    }

    /// [`EmcraftApp::info_with`] as an error.
    pub fn error_with(&mut self, template: crate::lang::Text, args: &[(&str, &dyn std::fmt::Display)]) {
        self.message(crate::lang::trf(template, args), true);
    }

    fn message(&mut self, text: impl std::fmt::Display, error: bool) {
        let text = emcraft_engine::text::clip(&text.to_string(), 2000);
        if let Some(last) = self.messages.last_mut()
            && last.text == text
            && last.error == error
        {
            last.count = last.count.saturating_add(1);
            return;
        }
        self.messages.push(Message { text, error, count: 1 });
        if self.messages.len() > MAX_MESSAGES {
            let extra = self.messages.len() - MAX_MESSAGES;
            self.messages.drain(..extra);
        }
    }

    pub fn status(&self) -> Option<&Message> {
        self.messages.last()
    }
}

/// In a window less tall than this, the message card shows two lines.
const SHORT: f32 = 720.0;

/// The message card at the bottom: the last message on one line, the earlier ones a click away
/// (the button at its end opens two to four lines of them, scrolled to the newest), and the
/// running job's progress and Cancel.
pub(crate) fn area(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    let t = &theme::Tokens::get();
    let open_id = egui::Id::new("emcraft_messages_open");
    let open = ui.ctx().data(|d| d.get_temp::<bool>(open_id)).unwrap_or(false);
    // Opened: two lines of history in a short window, four in a tall one.
    let lines = match (open, ui.ctx().content_rect().height() < SHORT) {
        (false, _) => 20.0,
        (true, true) => 40.0,
        (true, false) => 80.0,
    };
    egui::Panel::bottom("emcraft_status")
        .exact_size(lines + if app.job.is_some() { 58.0 } else { 24.0 })
        .show_separator_line(false)
        .frame(egui::Frame::NONE.fill(t.viewport_bottom).inner_margin(egui::Margin { left: 6, right: 6, top: 0, bottom: 6 }))
        .show(ui, |ui| {
            let height = ui.available_height() - 12.0;
            t.card(t.panel).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_min_height(height);
                scenario::publish_rect(Fixed::Status, ui.max_rect());
                progress(app, ui, t, out);
                let line = |m: &Message| {
                    let text = if m.count > 1 { format!("{} (×{})", m.text, m.count) } else { m.text.clone() };
                    egui::RichText::new(text).size(t.font_body).color(if m.error { t.error } else { t.text })
                };
                // The toggle at the line's end: the history's count, and which way it goes.
                let toggle = |ui: &mut egui::Ui| {
                    let n = app.messages.len();
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(40.0, 18.0), egui::Sense::hover());
                    let r = ui.hit(rect, Fixed::MessagesToggle.into(), egui::Sense::click());
                    if r.hovered() {
                        ui.painter().rect_filled(rect, t.radius_sm, t.hover);
                    }
                    // A triangle pointing the way it goes (up: open), and how many there are.
                    let c = egui::pos2(rect.left() + 10.0, rect.center().y);
                    let (a, b) = if open { (4.0, -3.0) } else { (-3.0, 4.0) };
                    let tri = vec![c + egui::vec2(-5.0, b), c + egui::vec2(5.0, b), c + egui::vec2(0.0, a)];
                    ui.painter().add(egui::Shape::convex_polygon(tri, t.text_dim, egui::Stroke::NONE));
                    ui.painter().text(
                        egui::pos2(rect.left() + 20.0, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        n.to_string(),
                        egui::FontId::proportional(t.font_small),
                        t.text_dim,
                    );
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, crate::lang::tr(t!("Earlier messages"))));
                    let r = r.on_hover_cursor(egui::CursorIcon::PointingHand).hint(if open {
                        t!("Hide earlier messages")
                    } else {
                        t!("Earlier messages")
                    });
                    if r.clicked() {
                        ui.ctx().data_mut(|d| d.insert_temp(open_id, !open));
                    }
                };
                if !open {
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            toggle(ui);
                            if let Some(m) = app.messages.last() {
                                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| ui.add(egui::Label::new(line(m)).truncate()));
                            }
                        });
                    });
                    return;
                }
                let rest = (height - if app.job.is_some() { 36.0 } else { 0.0 }).max(16.0);
                ui.horizontal_top(|ui| {
                    let w = ui.available_width() - 48.0;
                    ui.vertical(|ui| {
                        ui.set_width(w);
                        egui::ScrollArea::vertical()
                            .stick_to_bottom(true)
                            .max_height(rest)
                            .min_scrolled_height(rest)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                let start = app.messages.len().saturating_sub(40);
                                for m in &app.messages[start..] {
                                    ui.label(line(m));
                                }
                            });
                    });
                    toggle(ui);
                });
            });
        });
}

/// The running job's row: Cancel, what it does and how far it is.
fn progress(app: &EmcraftApp, ui: &mut egui::Ui, t: &theme::Tokens, out: &mut Actions) {
    let Some(job) = &app.job else { return };
    ui.horizontal(|ui| {
        // Cancel first, where it stays put while the text beside it changes.
        ui.act(true, ui_kit::styled(t!("Cancel"), false), Fixed::StatusCancel.into(), Action::CancelJob, out);
        ui.spinner();
        if let Some(w) = job.place() {
            device_chip(ui, &w, t);
        }
        ui.label(egui::RichText::new(job.text()).size(t.font_body));
        ui_kit::progress(ui, job.snapshot().fraction(), 180.0);
    });
}

/// The solve's device as a chip: the accent's while a GPU works, dim on the CPU.
fn device_chip(ui: &mut egui::Ui, w: &crate::job::Where, t: &theme::Tokens) {
    let on = w.gpu() && (w.stepped || w.fronts > 0);
    let text = w.text();
    let (fill, ink) = if on { (t.accent, t.on_accent) } else { (t.panel_header, t.text_dim) };
    let hint = match (w.gpu(), w.stepped) {
        (false, _) => t!("The CPU's threads (Setup › Compute picks a GPU)"),
        (true, true) => t!("The time domain's steps on the GPU"),
        (true, false) => t!("The factorization on the CPU, its large fronts on the GPU"),
    };
    let r = egui::Frame::NONE.fill(fill).corner_radius(t.radius_sm).inner_margin(egui::Margin::symmetric(6, 2)).show(ui, |ui| {
        ui.label(egui::RichText::new(text).size(t.font_small).color(ink));
    });
    scenario::publish_target(Fixed::JobDevice, r.response.rect);
    r.response.hint(hint);
}

/// The running job in the right column (the step panel, the docked dialog that started it): what
/// runs and how far it is, its bar and Cancel. Escape does not cancel: Cancel is a click.
pub(crate) fn job_card(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    let Some(job) = &app.job else { return };
    let t = &theme::Tokens::get();
    let r = egui::Frame::NONE.fill(t.accent_soft).corner_radius(t.radius_sm).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.spinner();
            ui.add(egui::Label::new(egui::RichText::new(job.text()).size(t.font_body).color(t.selected_ink)).wrap());
        });
        ui_kit::progress(ui, job.snapshot().fraction(), ui.available_width());
        ui.add_space(2.0);
        ui.act(true, ui_kit::styled(t!("Cancel"), false), Fixed::JobCancel.into(), Action::CancelJob, out);
    });
    scenario::publish_rect(Fixed::Job, r.response.rect);
}
