//! The message area at the bottom: the last messages (a message repeated in a row counted, not
//! repeated), and the running job's progress with its Cancel.

use crate::handle::Fixed;
use crate::lang::{args, tr, trf};

use crate::{Action, Actions, EmcraftApp, UiExt, label, scenario, theme, ui_kit};

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
    pub fn info_with(&mut self, template: &str, args: &[(&str, &dyn std::fmt::Display)]) {
        self.message(crate::lang::trf(template, args), false);
    }

    /// [`EmcraftApp::info_with`] as an error.
    pub fn error_with(&mut self, template: &str, args: &[(&str, &dyn std::fmt::Display)]) {
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

/// The message card at the bottom: the last messages, and the running job's progress and Cancel.
pub(crate) fn area(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    let t = &theme::Tokens::get();
    // Taller while a job runs: its progress row sits above the messages; two lines of them in a
    // short window, which leaves the room to the views.
    let lines = if ui.ctx().content_rect().height() < SHORT { 36.0 } else { 72.0 };
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
                let rest = (height - if app.job.is_some() { 36.0 } else { 0.0 }).max(16.0);
                egui::ScrollArea::vertical().stick_to_bottom(true).max_height(rest).min_scrolled_height(rest).auto_shrink([false, false]).show(
                    ui,
                    |ui| {
                        let start = app.messages.len().saturating_sub(40);
                        for m in &app.messages[start..] {
                            let text = if m.count > 1 { format!("{} (×{})", m.text, m.count) } else { m.text.clone() };
                            ui.label(egui::RichText::new(text).size(t.font_body).color(if m.error { t.error } else { t.text }));
                        }
                    },
                );
            });
        });
}

/// The running job's row: Cancel, what it does and how far it is.
fn progress(app: &EmcraftApp, ui: &mut egui::Ui, t: &theme::Tokens, out: &mut Actions) {
    let Some(job) = &app.job else { return };
    ui.horizontal(|ui| {
        // Cancel first, where it stays put while the text beside it changes.
        ui.act(true, ui_kit::styled("Cancel", false), Fixed::StatusCancel.into(), "", || Action::CancelJob, out);
        ui.spinner();
        ui.label(egui::RichText::new(job_text(job)).size(t.font_body));
        ui_kit::progress(ui, job.snapshot().fraction(), 180.0);
    });
}

/// A running job in brief: what runs, how far it is (its count, from the job's progress) and
/// for how long — `Frequency sweep · 3/11 · 4 s`.
pub(crate) fn job_text(job: &crate::worker::Job) -> String {
    let snap = job.snapshot();
    let command = if job.command == "engine.script" { tr("Script") } else { label(&job.command) };
    let secs = format!("{:.0}", job.elapsed());
    let (done, total) = (snap.done.min(snap.total) as usize, snap.total as usize);
    let what = match snap.stage.as_str() {
        _ if total == 0 => tr("starting").to_owned(),
        "fast sweep" => tr("fast").to_owned(),
        "frequency" | "phase" | "adaptive pass" => format!("{done}/{total}"),
        // A study's points (each a thousand parts) and a script's steps: the engine's stage.
        stage if total >= 1000 => format!("{stage}, {} %", (100 * done) / total),
        stage => format!("{stage} {done} / {total}"),
    };
    trf("{command} · {what} · {secs} s", args!(command = command, what = what, secs = secs))
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
            ui.add(egui::Label::new(egui::RichText::new(job_text(job)).size(t.font_body).color(t.selected_ink)).wrap());
        });
        ui_kit::progress(ui, job.snapshot().fraction(), ui.available_width());
        ui.add_space(2.0);
        ui.act(true, ui_kit::styled("Cancel", false), Fixed::JobCancel.into(), "", || Action::CancelJob, out);
    });
    scenario::publish_rect(Fixed::Job, r.response.rect);
}
