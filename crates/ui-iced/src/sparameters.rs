//! The S-parameters page: the sweep's chart as the engine draws it ([`crate::charts`]), its
//! words in the window's language.

use emcraft_ui_core::lang::tr;
use emcraft_ui_core::replies::Replies;
use emcraft_ui_core::view::View;
use iced::widget::{column, image, scrollable, text};
use iced::{Element, Fill, Widget};

use crate::app::{App, Message};

/// The S-parameters page, or why there is nothing to show yet.
pub fn view(app: &App) -> Element<'_, Message> {
    let t = app.look.tokens;
    let picture: Element<Message> = match app.charts.sparameters.as_ref().map(|p| &p.shown) {
        Some(Ok(h)) => image(h.clone()).width(Fill).boxed(),
        Some(Err(e)) => {
            let missing = View::SParameters.missing(&app.session, &Replies::default());
            text(missing.map_or_else(|| e.clone(), |m| tr(m).to_owned())).size(t.font_heading).color(t.text_dim).boxed()
        }
        None => text("").boxed(),
    };
    scrollable(column![picture].padding(16)).boxed()
}
