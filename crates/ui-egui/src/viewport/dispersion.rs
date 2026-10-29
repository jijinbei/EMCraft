//! A periodic cell's dispersion diagram: the bands against the phase per period, over the
//! closed form where it applies and the light line, with their table.

use super::*;

pub(super) fn dispersion(app: &EmcraftApp, st: &mut Parts, ui: &mut egui::Ui) {
    let session = &app.session;
    let height = Height::new(0.62, 260.0, 700.0);
    let drawn = chart_view(ui, &mut st.views.chart, app.revision, ChartView::Dispersion, height, "chart:dispersion", || {
        emcraft_engine::periodic::dispersion_chart(session)
    });
    if let Err(e) = drawn {
        return empty(ui, &e);
    }
    table(app, ui);
}

/// The bands' frequencies at each phase.
fn table(app: &EmcraftApp, ui: &mut egui::Ui) {
    let Some(d) = app.session.results().dispersion.as_ref() else { return };
    let (fu, lu) = (app.session.display().frequency, app.session.display().length);
    let bands = d.points.iter().map(|p| p.frequencies.len()).max().unwrap_or(0);
    crate::ui_kit::caption(ui, &format!("{} unknowns after the periodic reduction; period {:.4} {}", d.unknowns, lu.from_si(d.period), lu.symbol()));
    egui::ScrollArea::vertical().id_salt("dispersion_table").show(ui, |ui| {
        egui::Grid::new("dispersion_grid").striped(true).num_columns(2 + bands).show(ui, |ui| {
            ui.label(egui::RichText::new("ψ (°)").strong());
            ui.label(egui::RichText::new(format!("β (rad/{})", lu.symbol())).strong());
            for b in 0..bands {
                ui.label(egui::RichText::new(format!("band {} ({})", b + 1, fu.symbol())).strong());
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
