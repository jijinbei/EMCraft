//! A result page that is one of the engine's charts — S-parameters, TDR, the pulse, the far field,
//! dispersion, a study, the analyses read from a sweep ([`emcraft_ui_core::view::View::chart`])
//! — drawn by the GPU
//! ([`crate::charts`]), its words in the window's language; or why there is nothing to show yet.

use emcraft_ui_core::analyses::{BUTTONS, summary};
use emcraft_ui_core::lang::tr;
use emcraft_ui_core::view::View;
use iced::widget::{Column, button, canvas, row, scrollable, text};
use iced::{Element, Fill, Widget};

use crate::app::{App, Message};
use crate::chart::Plot;
use crate::look;

/// The chart's height (points).
const CHART_H: f32 = 520.0;

/// Result page `page`.
pub fn view(app: &App, page: View) -> Element<'_, Message> {
    let t = app.look.tokens;
    let picture: Element<Message> = match app.charts.page.as_ref().map(|p| &p.shown) {
        Some(Ok(c)) => canvas(Plot { chart: c, palette: &app.look.theme.palette }).width(Fill).height(CHART_H).boxed(),
        Some(Err(e)) => {
            let missing = page.missing(&app.session, &app.replies);
            text(missing.map_or_else(|| e.clone(), |m| tr(m).to_owned())).size(t.font_heading).color(t.text_dim).boxed()
        }
        None => text("").boxed(),
    };
    // The analyses: a button each (its dialog), the chart of the last, a line on its numbers.
    let (above, below) = if page == View::Analyses { analyses(app) } else { (None, None) };
    scrollable(Column::with_children(above.into_iter().chain(std::iter::once(picture)).chain(below)).spacing(10).padding(16)).boxed()
}

/// The analyses' buttons, the one shown lit, and a line on its numbers.
fn analyses(app: &App) -> (Option<Element<'_, Message>>, Option<Element<'_, Message>>) {
    let t = app.look.tokens;
    let shown = app.replies.analysis.as_ref();
    let idle = app.job.is_none();
    let buttons = BUTTONS.iter().map(|(id, label, _)| {
        let on = shown.is_some_and(|a| a.command == *id);
        button(text(tr(*label)).size(t.font_button))
            .style(look::button(t, on))
            .on_press_maybe(idle.then(|| Message::Command((*id).to_owned())))
            .boxed()
    });
    let line = shown.and_then(|a| summary(&app.session, a)).map(|l| text(l).size(t.font_body).color(t.text).boxed());
    (Some(row(buttons).spacing(6).wrap().boxed()), line)
}
