//! A periodic cell's dispersion diagram: the bands against the phase per period, over the
//! closed form where it applies and the light line, with their table.

use super::*;
use crate::lang::args;

pub(super) struct Dispersion;

impl ChartView for Dispersion {
    type Key = SharedChart;
    const HANDLE: Fixed = Fixed::ChartDispersion;
    const CHART: emcraft_engine::views::ChartId = emcraft_engine::views::ChartId::Dispersion;
    const HEIGHT: Height = OVER_TABLE;

    fn controls(_: &EmcraftApp, _: &mut Parts, _: &mut egui::Ui, _: &mut Actions) -> Option<SharedChart> {
        Some(SharedChart::Dispersion)
    }

    fn chart(app: &EmcraftApp, _: &SharedChart) -> Result<emcraft_render::chart::Chart, String> {
        emcraft_engine::periodic::dispersion_chart(&app.session).map_err(|e| e.to_string())
    }

    /// The bands' table, packed beside or under the chart.
    fn blocks(app: &EmcraftApp, _: &Parts, ui: &egui::Ui) -> Vec<egui::Vec2> {
        let Some(t) = Table::of(app) else { return Vec::new() };
        let font = egui::TextStyle::Body.resolve(ui.style());
        let columns = super::grid_width(ui, &super::column_widths(ui, &font, &t.headers, &t.rows), ui.spacing().item_spacing.x);
        let small = FontId::proportional(Tokens::get().font_small);
        let caption = ui.painter().layout_no_wrap(t.caption.clone(), small, Color32::PLACEHOLDER).size().x;
        vec![table_size(columns.max(caption), t.rows.len())]
    }

    fn block(app: &EmcraftApp, _: &mut Parts, ui: &mut egui::Ui, _: usize, _: &mut Actions) {
        table(app, ui);
    }
}

/// The size the table `width` wide (measured) at `points` phases wants (points): a caption, a
/// header and a row per phase.
pub(super) fn table_size(width: f32, points: usize) -> egui::Vec2 {
    vec2(width, 56.0 + 24.0 * points as f32)
}

/// The bands' table as drawn: its caption, headers (the phase, β, a column per band) and rows.
struct Table {
    caption: String,
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
}

impl Table {
    fn of(app: &EmcraftApp) -> Option<Table> {
        let d = app.session.results().dispersion.as_ref()?;
        let (fu, lu) = (app.session.display().frequency, app.session.display().length);
        let bands = d.points.iter().map(|p| p.frequencies.len()).max().unwrap_or(0);
        let period = format!("{:.4} {}", lu.from_si(d.period), lu.symbol());
        let caption = trf("{n} unknowns after the periodic reduction; period {period}", args!(n = d.unknowns, period = period));
        let headers = ["ψ (°)".to_owned(), format!("β (rad/{})", lu.symbol())]
            .into_iter()
            .chain((0..bands).map(|b| trf("band {n} ({unit})", args!(n = (b + 1), unit = fu.symbol()))))
            .collect();
        let rows = d
            .points
            .iter()
            .map(|p| {
                let bands = (0..bands).map(|b| p.frequencies.get(b).map_or(String::new(), |f| format!("{:.4}", fu.from_si(*f))));
                [format!("{:.1}", p.psi.to_degrees()), format!("{:.4}", p.beta * lu.to_si(1.0))].into_iter().chain(bands).collect()
            })
            .collect();
        Some(Table { caption, headers, rows })
    }
}

/// The bands' frequencies at each phase.
fn table(app: &EmcraftApp, ui: &mut egui::Ui) {
    let Some(t) = Table::of(app) else { return };
    crate::ui_kit::caption(ui, Raw(&t.caption));
    egui::ScrollArea::both().id_salt("dispersion_table").show(ui, |ui| {
        egui::Grid::new("dispersion_grid").striped(true).num_columns(t.headers.len()).show(ui, |ui| {
            super::dim_if_stale(ui);
            for h in &t.headers {
                ui.label(egui::RichText::new(h).strong());
            }
            ui.end_row();
            for row in &t.rows {
                for c in row {
                    ui.label(c);
                }
                ui.end_row();
            }
        });
    });
}
