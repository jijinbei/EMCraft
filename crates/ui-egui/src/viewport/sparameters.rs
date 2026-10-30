//! The S-parameter chart against the exact curves.

use super::*;
use crate::lang::args;

pub(super) struct SParameters;

impl ChartView for SParameters {
    type Key = SharedChart;
    const HANDLE: Fixed = Fixed::ChartSParameters;
    const CHART: emcraft_engine::views::ChartId = emcraft_engine::views::ChartId::SParameters;
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

    /// The field view's frequency on the chart, the band to refine, the ports' reference
    /// planes and the table of S.
    fn below(app: &EmcraftApp, st: &mut Parts, ui: &mut egui::Ui, chart: &egui::Response, size: egui::Vec2, out: &mut Actions) {
        if let Some(layout) = st.views.layout {
            let map = ChartMap { rect: chart.rect, size, layout, unit: app.session.display().frequency };
            frequency_marker(&mut st.field, ui, chart, &map, &app.frequencies(), out);
            refine_band(app, &mut st.views.refine, ui, chart, &map, out);
        }
        reference_planes(app, &mut st.views.deembed, ui, out);
        crate::ui_kit::caption(ui, "Click: the fields there · drag: a band to solve more finely");
        table(app, ui);
    }
}

/// Where the S-parameter chart's frequencies are on screen: the picture's rectangle and size
/// (pixels), the chart's layout (its x axis in the display unit `unit`).
struct ChartMap {
    rect: Rect,
    size: egui::Vec2,
    layout: emcraft_render::axes::Layout,
    unit: emcraft_model::units::Frequency,
}

impl ChartMap {
    /// The frequency (Hz) under a screen x; `None` off the plot area.
    fn frequency(&self, x: f32) -> Option<f64> {
        let px = f64::from((x - self.rect.min.x) / self.rect.width().max(1.0) * self.size.x);
        self.layout.x_at(px).map(|f| self.unit.to_si(f))
    }

    /// The screen x of frequency `f` (Hz).
    fn x(&self, f: f64) -> f32 {
        let [px, _] = self.layout.to_pixels([self.unit.from_si(f), self.layout.y.0]);
        self.rect.min.x + px as f32 / self.size.x.max(1.0) * self.rect.width()
    }

    /// The plot area's top and bottom on screen.
    fn rows(&self) -> (f32, f32) {
        let [_, y0, _, ph] = self.layout.plane;
        let sy = self.rect.height() / self.size.y.max(1.0);
        (self.rect.top() + y0 as f32 * sy, self.rect.top() + (y0 + ph) as f32 * sy)
    }
}

/// The field view's frequency, marked where the chart puts it; a click picks the nearest solved
/// frequency for the field view and opens it.
fn frequency_marker(field: &mut FieldState, ui: &egui::Ui, r: &egui::Response, map: &ChartMap, freqs: &[f64], out: &mut Actions) {
    let t = &Tokens::get();
    let (Some(f0), Some(f1)) = (freqs.first().copied(), freqs.last().copied()) else { return };
    let f = field.view.frequency;
    if f1 > f0 && field.view.mode.is_none() && (f0..=f1).contains(&f) {
        let x = map.x(f);
        let (top, bottom) = map.rows();
        if (r.rect.left()..=r.rect.right()).contains(&x) {
            let painter = ui.painter_at(r.rect);
            painter.line_segment([pos2(x, top), pos2(x, bottom)], Stroke::new(1.5, t.accent));
            let (at, align) =
                if x + 60.0 > r.rect.right() { (pos2(x - 4.0, top), Align2::RIGHT_TOP) } else { (pos2(x + 4.0, top), Align2::LEFT_TOP) };
            painter.text(at, align, tr("fields shown"), FontId::proportional(t.font_small), t.accent);
        }
    }
    if r.clicked()
        && let Some(pos) = r.interact_pointer_pos()
        && let Some(f) = map.frequency(pos.x).and_then(|f| crate::nearest(freqs, f))
    {
        field.view.frequency = f;
        field.view.mode = None;
        out.push(Action::SetView(View::Field));
    }
}

/// Most points a band adds.
const MAX_REFINE: usize = 201;

/// A drag across the chart picks a band (shaded); under the chart, its ends, the points to add
/// and Refine (`solve.refine`), or ✕.
fn refine_band(app: &EmcraftApp, refine: &mut crate::state::Refine, ui: &mut egui::Ui, r: &egui::Response, map: &ChartMap, out: &mut Actions) {
    let t = &Tokens::get();
    if r.drag_started()
        && let Some(pos) = r.interact_pointer_pos()
    {
        refine.from = map.frequency(pos.x);
    }
    if let (Some(a), Some(pos)) = (refine.from, r.interact_pointer_pos().filter(|_| r.dragged())) {
        let b = map.frequency(pos.x.clamp(map.x(map.unit.to_si(map.layout.x.0)), map.x(map.unit.to_si(map.layout.x.1)))).unwrap_or(a);
        refine.band = Some((a.min(b), a.max(b)));
    }
    if r.drag_stopped() {
        refine.from = None;
        if refine.band.is_some_and(|(a, b)| (map.x(b) - map.x(a)).abs() < 4.0) {
            refine.band = None;
        }
    }
    let Some((a, b)) = refine.band else { return };
    let (top, bottom) = map.rows();
    let band = Rect::from_min_max(pos2(map.x(a), top), pos2(map.x(b), bottom)).intersect(r.rect);
    ui.painter().rect_filled(band, 0.0, t.accent.gamma_multiply(0.18));
    scenario::publish_target(Fixed::RefineBand, band);
    let fu = app.session.display().frequency;
    ui.horizontal(|ui| {
        let ends = format!("{} – {} {}", emcraft_render::axes::format(fu.from_si(a)), emcraft_render::axes::format(fu.from_si(b)), fu.symbol());
        ui.text(Style::Small, ends);
        let r = ui.named(egui::DragValue::new(&mut refine.points).range(2..=MAX_REFINE).suffix(" pts"), Fixed::RefinePoints.into());
        r.hint("frequencies in the band");
        let params = json!({"start": fu.from_si(a), "stop": fu.from_si(b), "points": refine.points});
        let busy = app.busy();
        let hint = if busy { "a job is running" } else { "Solve these frequencies and merge them into the sweep (solve.refine)" };
        let r = ui.act(!busy, crate::ui_kit::styled("Refine", true), Fixed::RefineStart.into(), hint, || Action::run("solve.refine", params), out);
        let (x, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
        if crate::ui_kit::icon_button(ui, x, crate::ui_kit::Mark::Close, "Clear the band", Fixed::RefineClear).clicked() || r.clicked() {
            refine.band = None;
        }
    });
}

/// Each port's reference plane (its de-embedding, display units): dragged or typed, the
/// S-parameters follow at once (`model.port.deembed`); with terminal ports, a new solve.
fn reference_planes(app: &EmcraftApp, edits: &mut Vec<f64>, ui: &mut egui::Ui, out: &mut Actions) {
    let s = &app.session;
    let ports = &s.model().ports;
    let Some(sw) = s.results().sweep.as_ref() else { return };
    if ports.is_empty() {
        return;
    }
    let l = s.display().length;
    edits.resize(ports.len(), 0.0);
    let instant = !sw.driven.has_terminals() && sw.driven.periodic.is_none();
    let busy = app.busy();
    ui.horizontal_wrapped(|ui| {
        ui.text(Style::SmallDim, "Ref. plane").on_hover_text(tr("Each port's reference plane: its distance inside the model (de-embedding)"));
        for (i, port) in ports.iter().enumerate() {
            let Some(value) = edits.get_mut(i) else { continue };
            let now = l.from_si(port.deembed);
            ui.text(Style::Small, port.name.clone());
            let drag = egui::DragValue::new(value).speed(0.1).suffix(format!(" {}", l.symbol())).max_decimals(4);
            let r = ui.named_enabled(!busy, drag, Handle::Deembed(&port.name));
            let r = if instant { r.hint("S follows at once (a phase shift of the port's mode)") } else { r.hint("terminal ports: solved again") };
            if (r.drag_stopped() || r.lost_focus()) && (*value - now).abs() > 1e-12 {
                out.push(Action::run("model.port.deembed", json!({"name": port.name, "deembed": *value})));
            } else if !(r.dragged() || r.has_focus()) {
                *value = now;
            }
        }
    });
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
    egui::Grid::new("s_grid").striped(true).num_columns(headers.len()).spacing(vec2(0.0, 2.0)).show(ui, |ui| {
        super::dim_if_stale(ui);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chart_clicks_map_to_frequencies() {
        let layout = emcraft_render::chart::layout(1000, 600, (5.0, 9.0), (0.0, 1.0));
        let map =
            ChartMap { rect: Rect::from_min_size(pos2(100.0, 0.0), vec2(500.0, 300.0)), size: vec2(1000.0, 600.0), layout, unit: Default::default() };
        // 90 px of the picture's 1000 is the plot's left (5 GHz); half-scale on screen.
        assert_eq!(map.frequency(145.0), Some(5e9));
        assert!((map.frequency(365.0).unwrap_or(0.0) - 7e9).abs() < 1.0);
        assert_eq!(map.frequency(101.0), None);
        assert!((map.x(7e9) - 365.0).abs() < 1e-3);
        assert_eq!(crate::nearest(&[5.0, 6.0, 7.0], 6.4), Some(6.0));
        assert_eq!(crate::nearest(&[], 6.4), None);
        // A sweep's frequencies with as many decimals as their step needs.
        assert_eq!(frequency_decimals(&[6.0, 7.0, 8.0]), 1);
        assert_eq!(frequency_decimals(&[6.0, 6.025, 6.05]), 2);
        assert_eq!(frequency_decimals(&[9.0]), 3);
        assert_eq!(frequency_decimals(&[1.0, 1.0 + 1e-12]), 6);
    }
}
