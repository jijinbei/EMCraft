//! The S-parameter chart against the exact curves.

use super::*;
use crate::lang::args;

pub(super) struct SParameters;

impl ChartView for SParameters {
    type Key = SharedChart;
    const HANDLE: Fixed = Fixed::ChartSParameters;
    const HEIGHT: Height = OVER_TABLE;

    /// The switch between |S| in dB, its magnitude and its phase.
    fn controls(_: &EmcraftApp, st: &mut Parts, ui: &mut egui::Ui) -> Option<SharedChart> {
        ui.horizontal(|ui| {
            ui.text(Style::SmallDim, "Show");
            for d in view::SDisplay::ALL {
                let on = st.views.sdisplay == d;
                if ui.named(pill(shown(d), on, 10, PillSize::Small), Handle::SParameters(d.id())).clicked() {
                    st.views.sdisplay = d;
                }
            }
        });
        Some(SharedChart::SParameters(st.views.sdisplay))
    }

    fn chart(app: &EmcraftApp, key: &SharedChart) -> Result<emcraft_render::chart::Chart, String> {
        let display = match key {
            SharedChart::SParameters(d) => *d,
            _ => view::SDisplay::Db,
        };
        view::sparameter_chart_as(&app.session, display).map_err(|e| e.to_string())
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
    let freqs: Vec<f64> = sw.results.iter().map(|r| fu.from_si(r.frequency)).collect();
    let fd = frequency_decimals(&freqs);
    // Each column's numbers right-aligned under its header, at the column's own decimals.
    let t = &Tokens::get();
    let font = FontId::monospace(t.font_body);
    let width = |text: &str| ui.painter().layout_no_wrap(text.to_owned(), font.clone(), t.text).size().x;
    let headers: Vec<String> =
        std::iter::once(format!("f ({})", fu.symbol())).chain((1..=n).flat_map(|p| [format!("|S{p}1| (dB)"), format!("arg S{p}1 (°)")])).collect();
    let rows: Vec<Vec<String>> = sw
        .results
        .iter()
        .zip(&freqs)
        .map(|(r, f)| {
            let mut row = vec![format!("{f:.fd$}")];
            for p in 0..n {
                match r.s.get(p, 0) {
                    Some(c) => row.extend([format!("{:.2}", emcraft_model::units::decibels(c.norm())), format!("{:.1}", c.arg().to_degrees())]),
                    None => row.extend(["–".to_owned(), "–".to_owned()]),
                }
            }
            row
        })
        .collect();
    let widths: Vec<f32> =
        (0..headers.len()).map(|k| rows.iter().map(|r| r.get(k).map_or(0.0, |c| width(c))).fold(width(&headers[k]), f32::max) + 14.0).collect();
    let cell = |ui: &mut egui::Ui, w: f32, text: egui::RichText| {
        ui.allocate_ui_with_layout(vec2(w, 18.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.set_min_width(w);
            ui.label(text);
        });
    };
    egui::ScrollArea::vertical().id_salt("s_table").show(ui, |ui| {
        egui::Grid::new("s_grid").striped(true).num_columns(headers.len()).spacing(vec2(0.0, 2.0)).show(ui, |ui| {
            for (h, w) in headers.iter().zip(&widths) {
                cell(ui, *w, egui::RichText::new(h).font(font.clone()).strong());
            }
            ui.end_row();
            for row in &rows {
                for (c, w) in row.iter().zip(&widths) {
                    cell(ui, *w, egui::RichText::new(c).font(font.clone()));
                }
                ui.end_row();
            }
        });
    });
}

/// The decimals that tell a sweep's frequencies (display units) apart: enough for their
/// smallest step, from 1 to 6.
fn frequency_decimals(freqs: &[f64]) -> usize {
    let step = freqs.windows(2).map(|w| (w[1] - w[0]).abs()).filter(|d| *d > 0.0).fold(f64::INFINITY, f64::min);
    if !step.is_finite() {
        return 3;
    }
    (-step.log10()).ceil().clamp(1.0, 6.0) as usize
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
        // A sweep's frequencies with as many decimals as their step needs.
        assert_eq!(frequency_decimals(&[6.0, 7.0, 8.0]), 1);
        assert_eq!(frequency_decimals(&[6.0, 6.025, 6.05]), 2);
        assert_eq!(frequency_decimals(&[9.0]), 3);
        assert_eq!(frequency_decimals(&[1.0, 1.0 + 1e-12]), 6);
    }
}
