//! The message area at the bottom: the last messages (a message repeated in a row counted, not
//! repeated), and the running job's progress with its Cancel.

use crate::{EmcraftApp, label, scenario, theme, ui_kit};

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
    pub fn message(&mut self, text: impl std::fmt::Display, error: bool) {
        let text: String = text.to_string().chars().take(2000).collect();
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

/// The message card at the bottom: the last messages, and the running job's progress and Cancel.
pub(crate) fn area(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    let t = theme::Tokens::get();
    // Taller while a job runs: its progress row sits above the messages.
    egui::Panel::bottom("emcraft_status")
        .exact_size(if app.job.is_some() { 130.0 } else { 96.0 })
        .show_separator_line(false)
        .frame(egui::Frame::NONE.fill(t.viewport_bottom).inner_margin(egui::Margin { left: 6, right: 6, top: 0, bottom: 6 }))
        .show(ui, |ui| {
            let height = ui.available_height() - 12.0;
            t.card(t.panel).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_min_height(height);
                scenario::publish_rect("status", ui.max_rect());
                if progress(app, ui, &t) {
                    app.cancel_job();
                }
                let rest = (height - if app.job.is_some() { 36.0 } else { 0.0 }).max(16.0);
                egui::ScrollArea::vertical().stick_to_bottom(true).max_height(rest).auto_shrink([false, false]).show(ui, |ui| {
                    let start = app.messages.len().saturating_sub(40);
                    for m in &app.messages[start..] {
                        let text = if m.count > 1 { format!("{} (×{})", m.text, m.count) } else { m.text.clone() };
                        ui.label(egui::RichText::new(text).size(t.font_body).color(if m.error { t.error } else { t.text }));
                    }
                });
            });
        });
}

/// The running job's row: Cancel, what it does and how far it is. `true` when Cancel was
/// clicked.
fn progress(app: &EmcraftApp, ui: &mut egui::Ui, t: &theme::Tokens) -> bool {
    let Some(job) = &app.job else { return false };
    let snap = job.snapshot();
    let mut cancel = false;
    ui.horizontal(|ui| {
        // Cancel first, where it stays put while the text beside it changes.
        if ui_kit::button(ui, "Cancel", false, true, "status:cancel").clicked() {
            cancel = true;
        }
        ui.spinner();
        let what = if snap.total > 0 {
            format!("{} — {} {} of {}", label(&job.command), snap.stage, (snap.done + 1).min(snap.total), snap.total)
        } else {
            format!("{} — running", label(&job.command))
        };
        ui.label(egui::RichText::new(format!("{what} ({:.0} s)", job.elapsed())).size(t.font_body));
        ui_kit::progress(ui, snap.fraction(), 180.0);
    });
    cancel
}
