//! The S-parameter chart against the exact curves.

use super::*;

pub(super) fn sparameters(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    if app.session.results.sweep.is_none() && app.session.results.time_domain.is_none() {
        empty(ui, "No S-parameters yet: SIMULATE › Frequency sweep.");
        return;
    }
    let width = picture_width(ui);
    let key = format!("s|{}|{width}|{}", app.revision, crate::theme::is_dark());
    if app.chart.as_ref().is_none_or(|c| c.key != key) {
        match view::sparameter_chart(&app.session) {
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
    let freqs = app.frequencies();
    if let Some(p) = app.chart.as_mut() {
        let r = picture(ui, p, "chart:sparameters");
        // The field view's frequency, marked where the chart puts it.
        if let (Some(f0), Some(f1)) = (freqs.first(), freqs.last())
            && f1 > f0
            && app.field.view.mode.is_none()
            && (*f0..=*f1).contains(&app.field.view.frequency)
        {
            let w = p.size().x;
            let px = 90.0 + ((app.field.view.frequency - f0) / (f1 - f0)) as f32 * (w - 120.0);
            let x = r.rect.min.x + px / w * r.rect.width();
            let painter = ui.painter_at(r.rect);
            let (top, bottom) = (r.rect.top() + r.rect.height() * 0.08, r.rect.bottom() - r.rect.height() * 0.22);
            painter.line_segment([pos2(x, top), pos2(x, bottom)], Stroke::new(1.5, t.accent));
            let (at, align) =
                if x + 60.0 > r.rect.right() { (pos2(x - 4.0, top), Align2::RIGHT_TOP) } else { (pos2(x + 4.0, top), Align2::LEFT_TOP) };
            painter.text(at, align, "fields shown", FontId::proportional(t.font_small), t.accent);
        }
        // A click picks the nearest solved frequency for the field view.
        if r.clicked()
            && let (Some(pos), Some(f0), Some(f1)) = (r.interact_pointer_pos(), freqs.first(), freqs.last())
        {
            let fx = ((pos.x - r.rect.min.x) / r.rect.width()) as f64;
            if let Some(f) = chart_frequency(fx, p.size().x as f64, *f0, *f1).and_then(|f| crate::nearest(&freqs, f)) {
                app.field.view.frequency = f;
                app.field.view.mode = None;
                app.view = View::Field;
            }
        }
    }
    ui.label(
        egui::RichText::new("Click the chart to open the fields at the nearest solved frequency; the blue line marks the one they show.")
            .size(t.font_small)
            .color(t.text_dim),
    );
    // The table.
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
