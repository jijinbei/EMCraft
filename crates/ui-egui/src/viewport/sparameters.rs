//! The S-parameter chart against the exact curves.

use super::*;

pub(super) fn sparameters(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    let width = picture_width(ui);
    let key = format!("s|{}|{width}|{}", app.revision, crate::theme::is_dark());
    let freqs = app.frequencies();
    let session = &app.session;
    let p = match cached_chart(ui, &mut app.views.chart, key, width, Height::new(0.62, 260.0, 700.0), || view::sparameter_chart(session)) {
        Ok(p) => p,
        Err(e) => return empty(ui, &e),
    };
    let r = picture(ui, p, "chart:sparameters");
    let chart_width = p.size().x;
    frequency_marker(app, ui, &r, chart_width, &freqs);
    note(ui, "Click the chart to open the fields at the nearest solved frequency; the blue line marks the one they show.");
    table(app, ui);
}

/// The field view's frequency, marked where the chart (`chart_width` pixels wide) puts it; a
/// click picks the nearest solved frequency for the field view and opens it.
fn frequency_marker(app: &mut EmcraftApp, ui: &egui::Ui, r: &egui::Response, chart_width: f32, freqs: &[f64]) {
    let t = Tokens::get();
    let (Some(f0), Some(f1)) = (freqs.first().copied(), freqs.last().copied()) else { return };
    let f = app.field.view.frequency;
    if f1 > f0 && app.field.view.mode.is_none() && (f0..=f1).contains(&f) {
        let px = 90.0 + ((f - f0) / (f1 - f0)) as f32 * (chart_width - 120.0);
        let x = r.rect.min.x + px / chart_width * r.rect.width();
        let painter = ui.painter_at(r.rect);
        let (top, bottom) = (r.rect.top() + r.rect.height() * 0.08, r.rect.bottom() - r.rect.height() * 0.22);
        painter.line_segment([pos2(x, top), pos2(x, bottom)], Stroke::new(1.5, t.accent));
        let (at, align) = if x + 60.0 > r.rect.right() { (pos2(x - 4.0, top), Align2::RIGHT_TOP) } else { (pos2(x + 4.0, top), Align2::LEFT_TOP) };
        painter.text(at, align, "fields shown", FontId::proportional(t.font_small), t.accent);
    }
    if r.clicked()
        && let Some(pos) = r.interact_pointer_pos()
    {
        let fx = ((pos.x - r.rect.min.x) / r.rect.width()) as f64;
        if let Some(f) = chart_frequency(fx, chart_width as f64, f0, f1).and_then(|f| crate::nearest(freqs, f)) {
            app.field.view.frequency = f;
            app.field.view.mode = None;
            app.view = View::Field;
        }
    }
}

/// The sweep's S11 and S21 at each frequency.
fn table(app: &EmcraftApp, ui: &mut egui::Ui) {
    let Some(sw) = app.session.results.sweep.as_ref() else { return };
    let fu = app.session.display.frequency;
    egui::ScrollArea::vertical().id_salt("s_table").show(ui, |ui| {
        egui::Grid::new("s_grid").striped(true).num_columns(5).show(ui, |ui| {
            for h in [format!("f ({})", fu.symbol()), "|S11| dB".into(), "arg S11 (°)".into(), "|S21| dB".into(), "arg S21 (°)".into()] {
                ui.label(egui::RichText::new(h).strong());
            }
            ui.end_row();
            for r in &sw.results {
                ui.label(format!("{:.4}", fu.from_si(r.frequency)));
                let s11 = r.s.get(0, 0);
                let s21 = r.s.get(1, 0);
                for c in [s11, s21] {
                    match c {
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

/// Maps a click at fraction `fx` across the S-parameter chart to a frequency, with the chart's
/// layout (`emcraft_render::chart::draw`: the plot runs from x = 90 to w − 30).
pub fn chart_frequency(fx: f64, width_px: f64, f0: f64, f1: f64) -> Option<f64> {
    let (x0, pw) = (90.0, width_px - 120.0);
    let t = (fx * width_px - x0) / pw;
    (pw > 0.0 && (0.0..=1.0).contains(&t)).then_some(f0 + t * (f1 - f0))
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
