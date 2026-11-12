//! A result page that is one of the engine's charts — S-parameters, TDR, the pulse, the far field,
//! dispersion, a study ([`emcraft_ui_core::view::View::chart`]) — drawn by the GPU
//! ([`crate::charts`]), its words in the window's language; or why there is nothing to show yet.

use emcraft_ui_core::lang::tr;
use emcraft_ui_core::view::View;
use iced::widget::{canvas, column, scrollable, text};
use iced::{Element, Fill, Widget};

use crate::app::{App, Message};
use crate::chart::Plot;

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
    scrollable(column![picture].padding(16)).boxed()
}
