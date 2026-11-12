//! The analyses read from a sweep (`results.material`, `.noise`, `.active`, `.breakdown`,
//! `.thermal`): a button for each, opening its dialog, and the last one's chart — or, for a
//! temperature, its numbers.

use super::*;

use emcraft_ui_core::analyses::{BUTTONS, summary};

pub(super) struct Analyses;

impl ChartView for Analyses {
    type Key = SharedChart;
    const HANDLE: Fixed = Fixed::ChartAnalyses;
    const CHART: emcraft_engine::views::ChartId = emcraft_engine::views::ChartId::Analyses;
    const HEIGHT: Height = Height::new(0.75, 260.0, 700.0);
    const BELOW: f32 = 60.0;

    fn controls(app: &EmcraftApp, _st: &mut Parts, ui: &mut egui::Ui, out: &mut Actions) -> Option<Self::Key> {
        let shown = app.replies.analysis.as_ref();
        ui.horizontal_wrapped(|ui| {
            for (id, label, tip) in BUTTONS {
                let on = shown.is_some_and(|a| a.command == id);
                let b = ui.named_enabled(!app.busy(), egui::Button::selectable(on, tr(label)), Handle::Analysis(id)).hint(tip);
                if b.clicked() {
                    out.push(Action::OpenDialog { id: id.to_owned(), values: None });
                }
            }
        });
        let a = shown?;
        let unit = app.session.display().frequency.symbol();
        emcraft_engine::view::analyses::chart(&a.command, &a.reply, unit).map(|_| SharedChart::Analysis(a.run))
    }

    fn chart(app: &EmcraftApp, _key: &Self::Key) -> Result<emcraft_render::chart::Chart, String> {
        let a = app.replies.analysis.as_ref().ok_or("no analysis yet")?;
        emcraft_engine::view::analyses::chart(&a.command, &a.reply, app.session.display().frequency.symbol())
            .ok_or_else(|| "nothing to chart".to_owned())
    }

    fn below(app: &EmcraftApp, _st: &mut Parts, ui: &mut egui::Ui, _chart: &egui::Response, _size: egui::Vec2, _out: &mut Actions) {
        if let Some(text) = app.replies.analysis.as_ref().and_then(|a| summary(&app.session, a)) {
            ui.text(Style::Body, Raw(&text));
        }
    }
}
