//! The eigenmodes page: each mode the solve found — its resonant frequency and Q (lossless: none)
//! — in a table, and a button that shows its field in the 3D view; with losses, the modes'
//! resonances over the band above it (Q against frequency, each mode marked by its number).

use emcraft_render::Pen;
use emcraft_render::axes::Axis;
use emcraft_render::chart::{Chart, Legend, Series, Style};
use emcraft_ui_core::lang::tr;
use emcraft_ui_core::view::View;
use iced::widget::{button, canvas, column, scrollable, table, text};
use iced::{Element, Fill, Widget};

use crate::app::{App, Message};
use crate::chart::Plot;
use crate::look;

/// The resonances chart's height (points).
const CHART_H: f32 = 300.0;

/// A mode: its number from 0, frequency (Hz) and Q (none when lossless).
type Mode = (usize, f64, Option<f64>);

/// The modes' Q against their frequencies (display units `fu`), each marked by its number;
/// none while no mode has a Q (a lossless cavity).
fn resonances(modes: &[Mode], fu: emcraft_model::units::Frequency) -> Option<Chart> {
    let points: Vec<[f64; 2]> = modes.iter().filter_map(|(_, f, q)| Some([fu.from_si(*f), (*q)?])).filter(|p| p[1] > 0.0).collect();
    let (f_lo, f_hi) = points.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p[0]), b.max(p[0])));
    let (q_lo, q_hi) = points.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p[1]), b.max(p[1])));
    if points.is_empty() || !(f_lo.is_finite() && q_lo.is_finite()) {
        return None;
    }
    // A little room either side, and a decade's half above and below the Qs.
    let pad = 0.08 * (f_hi - f_lo).max(1e-3 * f_hi.abs().max(1.0));
    // Its name in the title: a Q's five digits leave the axis no room for it.
    let y = Axis { log: true, ..Axis::new("", q_lo / 2.0, q_hi * 2.0) };
    let x = Axis::new(&format!("frequency ({})", fu.symbol()), f_lo - pad, f_hi + pad);
    let mut chart = Chart::new("Q of each resonance", x, y, vec![Series::new("q", "Q", Pen::Series(1), Style::Markers, points)]);
    chart.marks = modes.iter().map(|(i, f, _)| (fu.from_si(*f), format!("{}", i + 1))).collect();
    chart.look.legend = Legend::None;
    emcraft_ui_core::chart_words::translate_chart(&mut chart);
    Some(chart)
}

/// The eigenmodes page, or why there is nothing to show yet.
pub fn view(app: &App) -> Element<'_, Message> {
    let t = app.look.tokens;
    let Some(eigen) = app.session.results().eigen.as_ref() else {
        let missing = View::Eigenmodes.missing(&app.session, &app.replies).map_or("", tr);
        return crate::parts::empty(t, missing, None);
    };
    let fu = app.session.display().frequency;
    let rows: Vec<Mode> = eigen.modes.iter().enumerate().map(|(i, m)| (i, m.frequency, m.q())).collect();
    let chart = resonances(&rows, fu)
        .map(|c| canvas(Plot { chart: std::borrow::Cow::Owned(c), palette: &app.look.theme.palette }).width(Fill).height(CHART_H).boxed());
    let cell = move |s: String| text(s).size(t.font_body).color(t.text);
    let head = move |s: String| text(s).size(t.font_body).color(t.form_dim);
    let columns = [
        table::column(head(tr(t!("Mode")).to_owned()), move |(i, _, _): Mode| cell(format!("{}", i + 1)).boxed()),
        table::column(head(format!("f ({})", fu.symbol())), move |(_, f, _): Mode| cell(format!("{:.4}", fu.from_si(f))).boxed()),
        table::column(head("Q".to_owned()), move |(_, _, q): Mode| cell(q.map_or_else(|| "–".to_owned(), |q| format!("{q:.0}"))).boxed()),
        table::column(text(""), move |(i, _, _): Mode| {
            button(text(tr(t!("Field"))).size(t.font_button)).style(look::button(t, false)).on_press(Message::Mode(i)).boxed()
        }),
    ];
    let parts = chart.into_iter().chain(std::iter::once(table(columns, rows).boxed()));
    scrollable(column(parts).spacing(t.space_16).padding(t.space_16)).boxed()
}
