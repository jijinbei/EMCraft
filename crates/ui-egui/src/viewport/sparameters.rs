//! The S-parameter chart against the exact curves.

use super::*;
use crate::lang::args;

pub(super) struct SParameters;

impl ChartView for SParameters {
    type Key = SharedChart;
    const HANDLE: Fixed = Fixed::ChartSParameters;
    const HEIGHT: Height = OVER_TABLE;

    fn controls(_: &EmcraftApp, _: &mut Parts, _: &mut egui::Ui) -> Option<SharedChart> {
        Some(SharedChart::SParameters)
    }

    fn chart(app: &EmcraftApp, _: &SharedChart) -> Result<emcraft_render::chart::Chart, String> {
        view::sparameter_chart(&app.session).map_err(|e| e.to_string())
    }

    /// The field view's frequency on the chart, and the table of S.
    fn below(app: &EmcraftApp, st: &mut Parts, ui: &mut egui::Ui, chart: &egui::Response, size: egui::Vec2, out: &mut Actions) {
        frequency_marker(&mut st.field, ui, chart, size, &app.frequencies(), out);
        crate::ui_kit::caption(ui, "Click the chart to open the fields at the nearest solved frequency; the blue line marks the one they show.");
        table(app, ui);
    }
}

/// The field view's frequency, marked where the chart (`size` pixels) puts it; a click picks
/// the nearest solved frequency for the field view and opens it.
fn frequency_marker(field: &mut FieldState, ui: &egui::Ui, r: &egui::Response, size: egui::Vec2, freqs: &[f64], out: &mut Actions) {
    let t = &Tokens::get();
    let (Some(f0), Some(f1)) = (freqs.first().copied(), freqs.last().copied()) else { return };
    let layout = chart_layout(f64::from(size.x), f64::from(size.y), f0, f1);
    let f = field.view.frequency;
    if f1 > f0 && field.view.mode.is_none() && (f0..=f1).contains(&f) {
        // The chart's pixels to the response's rectangle.
        let (sx, sy) = (r.rect.width() / size.x.max(1.0), r.rect.height() / size.y.max(1.0));
        let [_, y0, _, ph] = layout.plane;
        let x = r.rect.min.x + layout.to_pixels([f, 0.0])[0] as f32 * sx;
        let painter = ui.painter_at(r.rect);
        let (top, bottom) = (r.rect.top() + y0 as f32 * sy, r.rect.top() + (y0 + ph) as f32 * sy);
        painter.line_segment([pos2(x, top), pos2(x, bottom)], Stroke::new(1.5, t.accent));
        let (at, align) = if x + 60.0 > r.rect.right() { (pos2(x - 4.0, top), Align2::RIGHT_TOP) } else { (pos2(x + 4.0, top), Align2::LEFT_TOP) };
        painter.text(at, align, tr("fields shown"), FontId::proportional(t.font_small), t.accent);
    }
    if r.clicked()
        && let Some(pos) = r.interact_pointer_pos()
    {
        let fx = ((pos.x - r.rect.min.x) / r.rect.width()) as f64;
        if let Some(f) = chart_frequency(fx, f64::from(size.x), f0, f1).and_then(|f| crate::nearest(freqs, f)) {
            field.view.frequency = f;
            field.view.mode = None;
            out.push(Action::SetView(View::Field));
        }
    }
}

/// Most entries of S's first column the table shows.
const TABLE_ENTRIES: usize = 8;

/// The sweep's first column of S (S11, S21, and on with more ports or terminals) at each
/// frequency.
fn table(app: &EmcraftApp, ui: &mut egui::Ui) {
    let Some(sw) = app.session.results().sweep.as_ref() else { return };
    let fu = app.session.display().frequency;
    let entries = sw.entries();
    let n = entries.len().clamp(2, TABLE_ENTRIES);
    if entries.iter().any(|e| e.terminal.is_some()) {
        let names: Vec<String> = entries.iter().enumerate().map(|(i, e)| format!("{} = {}", i + 1, e.name)).collect();
        crate::ui_kit::caption(ui, trf("Entries of S: {entries}", args!(entries = names.join(", "))));
    }
    egui::ScrollArea::vertical().id_salt("s_table").show(ui, |ui| {
        egui::Grid::new("s_grid").striped(true).num_columns(1 + 2 * n).show(ui, |ui| {
            ui.label(egui::RichText::new(format!("f ({})", fu.symbol())).strong());
            for p in 1..=n {
                ui.label(egui::RichText::new(format!("|S{p}1| dB")).strong());
                ui.label(egui::RichText::new(format!("arg S{p}1 (°)")).strong());
            }
            ui.end_row();
            for r in &sw.results {
                ui.label(format!("{:.4}", fu.from_si(r.frequency)));
                for p in 0..n {
                    match r.s.get(p, 0) {
                        Some(c) => {
                            ui.label(format!("{:.3}", emcraft_model::units::decibels(c.norm())));
                            ui.label(format!("{:.2}", c.arg().to_degrees()));
                        }
                        None => {
                            ui.label("–");
                            ui.label("–");
                        }
                    }
                }
                ui.end_row();
            }
        });
    });
}

/// The S-parameter chart's layout at `width` × `height` pixels, its x axis the sweep's
/// frequencies `f0..=f1` (its y axis does not matter here).
fn chart_layout(width: f64, height: f64, f0: f64, f1: f64) -> emcraft_render::chart::Layout {
    emcraft_render::chart::Layout::new(width.max(0.0) as usize, height.max(0.0) as usize, (f0, f1), (0.0, 1.0))
}

/// Maps a click at fraction `fx` across the S-parameter chart (`width_px` wide) to a
/// frequency, with the chart's layout.
fn chart_frequency(fx: f64, width_px: f64, f0: f64, f1: f64) -> Option<f64> {
    chart_layout(width_px, 0.0, f0, f1).x_at(fx * width_px)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chart_clicks_map_to_frequencies() {
        assert_eq!(chart_frequency(90.0 / 1000.0, 1000.0, 5.0, 9.0), Some(5.0));
        assert!((chart_frequency(530.0 / 1000.0, 1000.0, 5.0, 9.0).unwrap_or(0.0) - 7.0).abs() < 1e-12);
        assert_eq!(chart_frequency(0.01, 1000.0, 5.0, 9.0), None);
        assert_eq!(crate::nearest(&[5.0, 6.0, 7.0], 6.4), Some(6.0));
        assert_eq!(crate::nearest(&[], 6.4), None);
    }
}
