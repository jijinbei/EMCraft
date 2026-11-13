//! The eigenmodes page: each mode the solve found — its resonant frequency and Q (lossless: none)
//! — in a table, and a button that shows its field in the 3D view.

use emcraft_ui_core::lang::tr;
use emcraft_ui_core::view::View;
use iced::widget::{button, column, scrollable, table, text};
use iced::{Element, Widget};

use crate::app::{App, Message};
use crate::look;

/// The eigenmodes page, or why there is nothing to show yet.
pub fn view(app: &App) -> Element<'_, Message> {
    let t = app.look.tokens;
    let Some(eigen) = app.session.results().eigen.as_ref() else {
        let missing = View::Eigenmodes.missing(&app.session, &app.replies).map_or("", tr);
        return crate::parts::empty(t, missing, None);
    };
    let fu = app.session.display().frequency;
    let rows: Vec<(usize, f64, Option<f64>)> = eigen.modes.iter().enumerate().map(|(i, m)| (i, m.frequency, m.q())).collect();
    let cell = move |s: String| text(s).size(t.font_body).color(t.text);
    let columns = [
        table::column(text(tr(t!("Mode"))).size(t.font_body).color(t.form_dim), move |(i, _, _): (usize, f64, Option<f64>)| {
            cell(format!("{}", i + 1)).boxed()
        }),
        table::column(text(format!("f ({})", fu.symbol())).size(t.font_body).color(t.form_dim), move |(_, f, _): (usize, f64, Option<f64>)| {
            cell(format!("{:.6}", fu.from_si(f))).boxed()
        }),
        table::column(text("Q").size(t.font_body).color(t.form_dim), move |(_, _, q): (usize, f64, Option<f64>)| {
            cell(q.map_or_else(|| "–".to_owned(), |q| format!("{q:.1}"))).boxed()
        }),
        table::column(text(""), move |(i, _, _): (usize, f64, Option<f64>)| {
            button(text(tr(t!("Field"))).size(t.font_button)).style(look::button(t, false)).on_press(Message::Mode(i)).boxed()
        }),
    ];
    scrollable(column![table(columns, rows)].padding(t.space_16)).boxed()
}
