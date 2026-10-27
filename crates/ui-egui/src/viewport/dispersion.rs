//! A periodic cell's dispersion diagram: the bands against the phase per period, over the
//! closed form where it applies and the light line, with their table.

use super::*;

pub(super) fn dispersion(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    if app.session.results.dispersion.is_none() {
        empty(ui, "No dispersion diagram yet: MATERIALS › Periodic, then SIMULATE › Dispersion.");
        return;
    }
    let width = picture_width(ui);
    let key = format!("d|{}|{width}|{}", app.revision, crate::theme::is_dark());
    if app.chart.as_ref().is_none_or(|c| c.key != key) {
        match emcraft_engine::periodic::dispersion_chart(&app.session) {
            Ok(chart) => {
                let height = (ui.available_height() * 0.62).clamp(260.0, 700.0) as usize;
                app.chart = Some(Picture::new(key, &emcraft_render::chart::draw(&chart, width, height)));
            }
            Err(e) => {
                empty(ui, &e.to_string());
                return;
            }
        }
    }
    if let Some(p) = app.chart.as_mut() {
        picture(ui, p, "chart:dispersion");
    }
    let Some(d) = app.session.results.dispersion.as_ref() else { return };
    let (fu, lu) = (app.session.display.frequency, app.session.display.length);
    let bands = d.points.iter().map(|p| p.frequencies.len()).max().unwrap_or(0);
    ui.label(
        egui::RichText::new(format!("{} unknowns after the periodic reduction; period {:.4} {}", d.unknowns, lu.from_si(d.period), lu.symbol()))
            .size(t.font_small)
            .color(t.text_dim),
    );
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
