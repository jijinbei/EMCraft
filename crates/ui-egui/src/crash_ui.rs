//! The crash reports' card (crate::crash): each report's kind, when, how often and its
//! message, with Copy (the text of an issue about it, for the person to read and paste:
//! nothing is sent) and Delete; where they are kept, and how a report plays back.

use crate::crash::{Kind, Report};
use crate::handle::{Fixed, Handle};
use crate::lang::{Raw, args, tr, trf};
use crate::theme::Tokens;
use crate::{Action, Actions, EmcraftApp, Style, UiExt, ui_kit};

/// The card over the window.
pub fn show(app: &EmcraftApp, reports: &[Report], ctx: &egui::Context, out: &mut Actions) {
    let t = &Tokens::get();
    let modal = egui::Modal::new(egui::Id::new("emcraft_reports")).frame(t.card(t.dialog_body).inner_margin(16)).show(ctx, |ui| {
        ui.set_width(520.0);
        ui.text(Style::Heading, t!("Crash reports"));
        ui.text(Style::SmallDim, t!("Kept on this computer only. Copy one to report it; nothing is sent."));
        ui.add_space(8.0);
        if reports.is_empty() {
            ui.text(Style::Small, t!("No reports."));
        }
        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
            for (i, r) in reports.iter().enumerate() {
                row(ui, i, r, out);
                ui.separator();
            }
        });
        if let Some(dir) = &app.crashes.dir {
            ui_kit::small(ui, Raw(&dir.display().to_string()), t.text_dim);
        }
        ui.add_space(8.0);
        ui.act(true, ui_kit::styled(t!("Close"), false), Fixed::ReportsClose.into(), Action::Close(crate::ui_state::Layer::Reports), out);
    });
    if modal.should_close() {
        out.push(Action::Close(crate::ui_state::Layer::Reports));
    }
}

/// One report: what, when, how often, its message; Copy and Delete.
fn row(ui: &mut egui::Ui, i: usize, r: &Report, out: &mut Actions) {
    let kind = match r.kind {
        Kind::Panic => t!("Crash"),
        Kind::UncleanExit => t!("Closed unexpectedly"),
        Kind::Report => t!("Report"),
    };
    let days = now_days().saturating_sub(r.last / 86_400);
    let when = match days {
        0 => tr(t!("today")).to_owned(),
        n => trf(t!("{n} days ago"), args!(n = n)),
    };
    let times = if r.count > 1 { format!("  ·  ×{}", r.count) } else { String::new() };
    ui.horizontal(|ui| {
        ui.text(Style::Body, kind);
        ui.text(Style::SmallDim, Raw(&format!("{when}{times}  ·  EMCraft {}", r.emcraft)));
    });
    let first = r.message.lines().next().unwrap_or_default();
    ui.text(Style::Small, Raw(emcraft_engine::text::clip(first, 160).as_str()));
    ui.horizontal(|ui| {
        let copy = ui.named(egui::Button::new(tr(t!("Copy"))), Handle::ReportCopy(i));
        if copy.clicked() {
            ui.ctx().copy_text(r.issue());
            out.push(Action::Message { text: tr(t!("Copied: paste it into an issue.")).to_owned(), error: false });
        }
        ui.act(true, egui::Button::new(tr(t!("Delete"))), Handle::ReportDelete(i), Action::DeleteReport(r.id.clone()), out);
    });
}

/// Days since 1970.
fn now_days() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() / 86_400)
}
