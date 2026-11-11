//! Under the canvas: the chart chosen (drawn by the engine) and the budget at its frequency.

use emcraft_engine::view::SystemChart;
use emcraft_ui_core::lang::tr;
use emcraft_ui_core::system::budget;
use iced::widget::{Column, button, column, image, row, space, table, text};
use iced::{Element, Fill, Widget};

use crate::app::{App, Message};

/// The chart's size in pixels.
const CHART: [usize; 2] = [1100, 320];

/// A chart choice's name.
fn label(c: SystemChart) -> emcraft_ui_core::lang::Text {
    match c {
        SystemChart::Along => t!("Along the chain"),
        SystemChart::Figure => t!("Noise figure"),
        SystemChart::Gain => t!("Gain"),
    }
}

/// The chart chosen and its choices, once the chain is read.
pub(super) fn chart(app: &App) -> Element<'_, Message> {
    if app.session.results().system.is_none() {
        return space().boxed();
    }
    let choices = SystemChart::ALL.into_iter().map(|c| {
        let style = if c == app.chart { button::primary } else { button::secondary };
        button(text(tr(label(c))).size(13)).style(style).on_press(Message::Chart(c)).boxed()
    });
    let at = super::budget_at(app).map(|s| text(s).size(13).boxed());
    let picture: Element<Message> = match emcraft_engine::view::system_chart(&app.session, app.chart) {
        Ok(c) => {
            let canvas = emcraft_render::chart::draw(&c, CHART[0], CHART[1], &emcraft_render::Palette::default());
            image(image::Handle::from_rgba(canvas.w as u32, canvas.h as u32, canvas.rgba)).width(Fill).boxed()
        }
        Err(e) => text(e.to_string()).boxed(),
    };
    column![row(choices.chain(at)).spacing(6), picture].spacing(6).boxed()
}

/// The budget, a row a stage, and what the receiver reads under it.
pub(super) fn sheet(app: &App) -> Element<'_, Message> {
    let Some((p, rows)) = app.session.results().system.as_deref().and_then(budget::table) else { return space().boxed() };
    let columns = budget::HEADERS
        .into_iter()
        .enumerate()
        .map(|(k, h)| table::column(text(tr(h)).size(13), move |cells: Vec<String>| text(cells.get(k).cloned().unwrap_or_default()).size(13)));
    let notes = budget::summary(&app.session, p)
        .into_iter()
        .map(|(line, dim)| text(line).size(13).style(if dim { text::secondary } else { text::default }).boxed());
    Column::with_children(std::iter::once(table(columns, rows).boxed()).chain(notes)).spacing(6).boxed()
}
