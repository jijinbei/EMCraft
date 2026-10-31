//! The S-parameter chart against the exact curves.

use super::*;
use crate::lang::args;

pub(super) struct SParameters;

impl ChartView for SParameters {
    type Key = SharedChart;
    const HANDLE: Fixed = Fixed::ChartSParameters;
    const CHART: emcraft_engine::views::ChartId = emcraft_engine::views::ChartId::SParameters;
    const HEIGHT: Height = OVER_TABLE;
    /// The band to refine, under the chart.
    const BELOW: f32 = 40.0;

    /// How S is shown — in dB, as a magnitude or a phase — as one small menu.
    fn controls(app: &EmcraftApp, st: &mut Parts, ui: &mut egui::Ui, _: &mut Actions) -> Option<SharedChart> {
        ui.horizontal(|ui| {
            let r = egui::ComboBox::from_id_salt("s_display").width(110.0).selected_text(shown(st.views.sdisplay)).show_ui(ui, |ui| {
                for d in view::SDisplay::ALL {
                    if ui.named(egui::Button::selectable(st.views.sdisplay == d, shown(d)), Handle::SParameters(d.id())).clicked() {
                        st.views.sdisplay = d;
                    }
                }
            });
            scenario::publish_target(Fixed::SDisplay, r.response.rect);
        });
        // A kept solve let go is drawn no more.
        let kept: Vec<u32> = app.session.archive().iter().map(|a| a.id).collect();
        st.views.compare.retain(|id| kept.contains(id));
        Some(SharedChart::SParameters(st.views.sdisplay, st.views.compare.clone()))
    }

    /// The history (once there are two solves to compare), the table of S and the ports'
    /// reference planes, packed round the chart.
    fn blocks(app: &EmcraftApp, _: &Parts, ui: &egui::Ui) -> Vec<egui::Vec2> {
        parts(app).map(|p| p.wants(app, ui)).collect()
    }

    fn block(app: &EmcraftApp, st: &mut Parts, ui: &mut egui::Ui, k: usize, out: &mut Actions) {
        match parts(app).nth(k) {
            Some(Part::History) => history(app, &mut st.views.compare, ui, out),
            Some(Part::Table) => table(app, ui),
            Some(Part::Planes) => {
                reference_planes(app, &mut st.views.deembed, ui, out);
                crate::ui_kit::caption(ui, "Click: the fields there · drag: a band to solve more finely");
            }
            None => {}
        }
    }

    fn chart(app: &EmcraftApp, key: &SharedChart) -> Result<emcraft_render::chart::Chart, String> {
        let (display, compare) = match key {
            SharedChart::SParameters(d, c) => (*d, c.as_slice()),
            _ => (view::SDisplay::Db, &[][..]),
        };
        view::sparameter_chart_compared(&app.session, display, compare).map_err(|e| e.to_string())
    }

    /// The field view's frequency on the chart, the band to refine, the ports' reference
    /// planes and the table of S.
    fn below(app: &EmcraftApp, st: &mut Parts, ui: &mut egui::Ui, chart: &egui::Response, size: egui::Vec2, out: &mut Actions) {
        if let Some(layout) = st.views.layout {
            let map = ChartMap { rect: chart.rect, size, layout, unit: app.session.display().frequency };
            frequency_marker(&mut st.field, ui, chart, &map, &app.frequencies(), out);
            refine_band(app, &mut st.views.refine, ui, chart, &map, out);
        }
    }
}

/// What is packed round the S-parameter chart.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Part {
    History,
    Table,
    Planes,
}

/// The parts the session has, in the order they go: the history once there are two solves to
/// compare, or one while nothing is solved now (a project just opened).
fn parts(app: &EmcraftApp) -> impl Iterator<Item = Part> {
    let kept = app.session.archive().len();
    let history = kept >= 2 || (kept == 1 && app.session.results().sweep.is_none());
    history.then_some(Part::History).into_iter().chain([Part::Table, Part::Planes])
}

impl Part {
    /// The size it wants (points): the table's measured, the others' from what they show.
    fn wants(self, app: &EmcraftApp, ui: &egui::Ui) -> egui::Vec2 {
        match self {
            // A heading, a row per kept solve, a caption.
            Part::History => vec2(300.0, 30.0 + 34.0 * app.session.archive().len() as f32 + 34.0),
            Part::Table => STable::of(app, ui).map_or(vec2(300.0, 26.0), |t| t.size(ui)),
            Part::Planes => vec2(80.0 + 110.0 * app.session.model().ports.len() as f32, 46.0),
        }
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

    /// The plot area's top to bottom on screen.
    fn rows(&self) -> egui::Rangef {
        let [_, y0, _, ph] = self.layout.plane;
        let sy = self.rect.height() / self.size.y.max(1.0);
        egui::Rangef::new(self.rect.top() + y0 as f32 * sy, self.rect.top() + (y0 + ph) as f32 * sy)
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
        let top = map.rows().min;
        if r.rect.x_range().contains(x) {
            let painter = ui.painter_at(r.rect);
            painter.vline(x, map.rows(), Stroke::new(1.5, t.accent));
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
    let band = Rect::from_x_y_ranges(map.x(a)..=map.x(b), map.rows()).intersect(r.rect);
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

/// The table of S as drawn: its caption (a terminal port's entries named), headers, rows, each
/// column's width (its widest text, measured in its font) and whether its rows are marked.
struct STable {
    caption: Option<String>,
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
    widths: Vec<f32>,
    marks: bool,
}

/// The table's font: monospace, so the numbers line up.
fn table_font() -> FontId {
    FontId::monospace(Tokens::get().font_body)
}

/// The padding of each of the table's cells (points).
const CELL_PAD: f32 = 14.0;

impl STable {
    /// The sweep's first column of S (S11, S21, and on with more ports or terminals) at each
    /// frequency, each column at its own decimals; `None` without a sweep.
    fn of(app: &EmcraftApp, ui: &egui::Ui) -> Option<STable> {
        let sw = app.session.results().sweep.as_ref()?;
        let fu = app.session.display().frequency;
        let entries = sw.entries();
        let n = entries.len().clamp(2, TABLE_ENTRIES);
        let caption = entries.iter().any(|e| e.terminal.is_some()).then(|| {
            let names: Vec<String> = entries.iter().enumerate().map(|(i, e)| format!("{} = {}", i + 1, e.name)).collect();
            trf("Entries of S: {entries}", args!(entries = names.join(", ")))
        });
        let freqs: Vec<f64> = sw.results.iter().map(|r| fu.from_si(r.frequency)).collect();
        let fd = frequency_decimals(&freqs);
        let headers: Vec<String> = std::iter::once(format!("f ({})", fu.symbol()))
            .chain((1..=n).flat_map(|p| [format!("|S{p}1| (dB)"), format!("arg S{p}1 (°)")]))
            .collect();
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
        let widths = super::column_widths(ui, &table_font(), &headers, &rows).into_iter().map(|w| w + CELL_PAD).collect();
        Some(STable { caption, headers, rows, widths, marks: marked(sw) })
    }

    /// The size it takes (points): its columns side by side (no gaps between them: each cell
    /// carries its padding) and room for a scroll bar; a header, a row per frequency and the
    /// caption.
    fn size(&self, ui: &egui::Ui) -> egui::Vec2 {
        let marks = if self.marks { MARK } else { 0.0 };
        let caption = if self.caption.is_some() { 20.0 } else { 0.0 };
        vec2(marks + super::grid_width(ui, &self.widths, 0.0), caption + 26.0 + 22.0 * self.rows.len() as f32)
    }
}

/// The sweep's first column of S at each frequency, each column's numbers right-aligned under
/// its header.
fn table(app: &EmcraftApp, ui: &mut egui::Ui) {
    let Some(sw) = app.session.results().sweep.as_ref() else { return };
    let Some(STable { caption, headers, rows, widths, marks }) = STable::of(app, ui) else { return };
    if let Some(c) = caption {
        crate::ui_kit::caption(ui, Raw(&c));
    }
    let font = table_font();
    let cell = |ui: &mut egui::Ui, w: f32, text: egui::RichText| {
        ui.allocate_ui_with_layout(vec2(w, 18.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.set_min_width(w);
            ui.label(text);
        });
    };
    // A fast sweep's rows solved in full, marked as the chart marks them.
    let columns = headers.len() + usize::from(marks);
    egui::Grid::new("s_grid").striped(true).num_columns(columns).spacing(vec2(0.0, 2.0)).show(ui, |ui| {
        super::dim_if_stale(ui);
        if marks {
            let r = full_mark(ui, true);
            scenario::publish_target(Fixed::SFullMark, r.rect);
        }
        for (h, w) in headers.iter().zip(&widths) {
            cell(ui, *w, egui::RichText::new(h).font(font.clone()).strong());
        }
        ui.end_row();
        for (row, r) in rows.iter().zip(&sw.results) {
            if marks {
                full_mark(ui, r.full);
            }
            for (c, w) in row.iter().zip(&widths) {
                cell(ui, *w, egui::RichText::new(c).font(font.clone()));
            }
            ui.end_row();
        }
    });
}

/// The width of the table's column of marks (points).
const MARK: f32 = 14.0;

/// Whether the table marks its rows solved in full: a fast sweep's, some solved in its basis.
fn marked(sw: &emcraft_engine::Sweep) -> bool {
    sw.results.iter().any(|r| !r.full)
}

/// A cell of the column of marks: a small diamond, as on the chart, where `full`, its tip saying
/// what it means.
fn full_mark(ui: &mut egui::Ui, full: bool) -> egui::Response {
    let t = &Tokens::get();
    let (rect, r) = ui.allocate_exact_size(vec2(MARK, 18.0), Sense::hover());
    if !full {
        return r;
    }
    let c = rect.center();
    let d = 3.5;
    let corners = vec![c + vec2(0.0, -d), c + vec2(d, 0.0), c + vec2(0.0, d), c + vec2(-d, 0.0)];
    ui.painter().add(egui::Shape::convex_polygon(corners, t.text, Stroke::NONE));
    r.hint("Solved in full; the rest by the fast sweep")
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

/// The analysis history as rows, the newest first: a swatch in the colour its curves take when
/// drawn over the chart, its number, what changed since the one before and its deepest |S11|;
/// a click draws it over the chart (again: takes it off), the session's own marked ●; its menu
/// restores it, pins it (★) or lets it go.
fn history(app: &EmcraftApp, compare: &mut Vec<u32>, ui: &mut egui::Ui, out: &mut Actions) {
    let t = &Tokens::get();
    let rows = emcraft_engine::archive::rows(&app.session);
    crate::ui_kit::section(ui, "History");
    let busy = app.busy();
    let palette = crate::theme::palette();
    let font = FontId::proportional(t.font_small);
    for r in rows.iter().rev() {
        let Some(id) = r["id"].as_u64().and_then(|n| u32::try_from(n).ok()) else { continue };
        let current = r["current"] == json!(true);
        let pinned = r["pinned"] == json!(true);
        let drawn = compare.iter().position(|c| *c == id);
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::click());
        let resp = ui.hit(rect, Handle::HistoryRun(id), Sense::click()).union(resp);
        let p = ui.painter();
        let fill = if drawn.is_some() {
            t.accent_soft
        } else if resp.hovered() {
            t.hover
        } else {
            Color32::TRANSPARENT
        };
        p.rect_filled(rect, t.radius_sm, fill);
        swatch(p, pos2(rect.left() + 12.0, rect.center().y), current, drawn, &palette);
        let ink = if current { t.text } else { t.text_dim };
        let star = if pinned { " ★" } else { "" };
        p.text(pos2(rect.left() + 26.0, rect.top() + 9.0), Align2::LEFT_CENTER, format!("#{id}{star}"), FontId::proportional(t.font_body), ink);
        let changes: Vec<&str> = r["changes"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        let what = if changes.is_empty() { tr("as it was").to_owned() } else { changes.join(", ") };
        let line = p.with_clip_rect(rect.shrink2(vec2(4.0, 0.0)));
        line.text(pos2(rect.left() + 26.0, rect.bottom() - 9.0), Align2::LEFT_CENTER, &what, font.clone(), t.text_dim);
        if let Some(db) = r["best_s11_db"].as_f64() {
            p.text(pos2(rect.right() - 6.0, rect.top() + 9.0), Align2::RIGHT_CENTER, format!("{db:.1} dB"), font.clone(), ink);
        }
        let stored = app.session.archive().iter().any(|a| a.id == id && !a.in_memory());
        let resp = resp.hint(Raw(&history_tip(app, r, stored)));
        if resp.clicked() && !current {
            match drawn {
                Some(k) => {
                    compare.remove(k);
                }
                None => compare.push(id),
            }
        }
        resp.context_menu(|ui| history_menu(ui, id, pinned, !busy, out));
    }
    crate::ui_kit::caption(ui, "Click: draw it over the chart · right-click: restore, pin, delete");
}

/// A kept solve's swatch centred on `c`: its two curves' colours when drawn (`drawn`: its place
/// among those drawn), the current one's ●, else a ring.
fn swatch(p: &egui::Painter, c: egui::Pos2, current: bool, drawn: Option<usize>, palette: &emcraft_render::Palette) {
    let t = &Tokens::get();
    match (current, drawn) {
        (true, _) => {
            p.circle_filled(c, 5.0, t.accent);
        }
        (false, Some(k)) => {
            let [a, b] = emcraft_engine::view::compare_pens(k).map(|pen| crate::theme::color32(palette.pen(pen)));
            p.line_segment([c + vec2(-6.0, -3.0), c + vec2(6.0, -3.0)], Stroke::new(2.5, a));
            p.line_segment([c + vec2(-6.0, 3.0), c + vec2(6.0, 3.0)], Stroke::new(2.5, b));
        }
        (false, None) => {
            p.circle_stroke(c, 4.5, Stroke::new(1.2, t.text_dim));
        }
    }
}

/// A kept solve's tip: its label, its frequencies, its deepest |S11| and where, and for one read
/// back from the project file (`stored`) that restoring it solves it again.
fn history_tip(app: &EmcraftApp, r: &Value, stored: bool) -> String {
    let mut tip = trf("{n} frequencies", args!(n = r["points"].as_u64().unwrap_or(0)));
    if let Some(label) = r["label"].as_str() {
        tip = format!("{label}\n{tip}");
    }
    if stored {
        tip = format!("{tip}\n{}", tr("Saved: restore solves it again"));
    }
    let (Some(db), Some(at)) = (r["best_s11_db"].as_f64(), r["best_at"].as_f64()) else { return tip };
    let unit = app.session.display().frequency.symbol();
    let deepest = trf("deepest |S11| {db} dB at {f} {unit}", args!(db = format!("{db:.1}"), f = format!("{at:.4}"), unit = unit));
    format!("{tip}\n{deepest}")
}

/// A kept solve's menu: restore, pin or unpin, delete.
fn history_menu(ui: &mut egui::Ui, id: u32, pinned: bool, enabled: bool, out: &mut Actions) {
    let items = [
        ("Restore", Fixed::HistoryRestore, "results.history.restore", json!({"id": id})),
        (if pinned { "Unpin" } else { "Pin" }, Fixed::HistoryPin, "results.history.pin", json!({"id": id, "pinned": !pinned})),
        ("Delete", Fixed::HistoryDelete, "results.history.delete", json!({"id": id})),
    ];
    for (label, handle, command, params) in items {
        if ui.named_enabled(enabled, egui::Button::new(tr(label)), handle.into()).clicked() {
            out.push(Action::run(command, params));
            ui.close();
        }
    }
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
