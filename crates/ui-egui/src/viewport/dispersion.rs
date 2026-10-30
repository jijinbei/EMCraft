//! A periodic cell's dispersion diagram: the bands against the phase per period, over the
//! closed form where it applies and the light line, with their table.

use super::*;
use crate::lang::args;

pub(super) struct Dispersion;

impl ChartView for Dispersion {
    type Key = SharedChart;
    const HANDLE: Fixed = Fixed::ChartDispersion;
    const HEIGHT: Height = OVER_TABLE;

    fn controls(_: &EmcraftApp, _: &mut Parts, _: &mut egui::Ui) -> Option<SharedChart> {
        Some(SharedChart::Dispersion)
    }

    fn chart(app: &EmcraftApp, _: &SharedChart) -> Result<emcraft_render::chart::Chart, String> {
        emcraft_engine::periodic::dispersion_chart(&app.session).map_err(|e| e.to_string())
    }

    fn below(app: &EmcraftApp, _: &mut Parts, ui: &mut egui::Ui, _: &egui::Response, _: egui::Vec2, _: &mut Actions) {
        table(app, ui);
    }
}

/// The bands' frequencies at each phase.
fn table(app: &EmcraftApp, ui: &mut egui::Ui) {
    let Some(d) = app.session.results().dispersion.as_ref() else { return };
    let (fu, lu) = (app.session.display().frequency, app.session.display().length);
    let bands = d.points.iter().map(|p| p.frequencies.len()).max().unwrap_or(0);
    let period = format!("{:.4} {}", lu.from_si(d.period), lu.symbol());
    crate::ui_kit::caption(ui, trf("{n} unknowns after the periodic reduction; period {period}", args!(n = d.unknowns, period = period)));
    egui::ScrollArea::vertical().id_salt("dispersion_table").show(ui, |ui| {
        egui::Grid::new("dispersion_grid").striped(true).num_columns(2 + bands).show(ui, |ui| {
            super::dim_if_stale(ui);
            ui.label(egui::RichText::new("ψ (°)").strong());
            ui.label(egui::RichText::new(format!("β (rad/{})", lu.symbol())).strong());
            for b in 0..bands {
                ui.text(Style::Strong, trf("band {n} ({unit})", args!(n = (b + 1), unit = fu.symbol())));
            }
            ui.end_row();
            for p in &d.points {
                ui.label(format!("{:.1}", p.psi.to_degrees()));
                ui.label(format!("{:.4}", p.beta * lu.to_si(1.0)));
                for b in 0..bands {
                    ui.label(p.frequencies.get(b).map_or(String::new(), |f| format!("{:.4}", fu.from_si(*f))));
                }
                ui.end_row();
            }
        });
    });
}
