//! The command search at the window's bar (`/` focuses it) and the commands it finds under the
//! bar; a command's tip (what it does, with its id), which the tree's ＋ lists show too.

use iced::widget::{Column, button, column, container, row, text};
use iced::{Alignment, Element, Fill, Widget};

use crate::app::{App, Message};
use crate::look;

/// The search field's widget id: `/` focuses it.
pub const SEARCH: &str = "emcraft-search";

/// The command search's field at the bar's right.
pub fn field(app: &App) -> Element<'_, Message> {
    let t = app.look.tokens;
    iced::widget::text_input(emcraft_ui_core::lang::tr(t!("Search commands (/)")), &app.search)
        .id(SEARCH)
        .size(t.font_small)
        .width(240)
        .padding([3, 8])
        .style(look::field(t))
        .on_input(Message::Search)
        .on_submit(Message::SearchPick(None))
        .boxed()
}

/// The commands the query matches, under the bar while it has one: each its glyph, its name and
/// its id; a click (or Enter, the first) opens its dialog, or runs one without parameters. A query naming
/// a command with its parameters (`command.id {json}`) runs it as typed.
pub fn results(app: &App) -> Option<Element<'_, Message>> {
    let t = app.look.tokens;
    let q = app.search.trim();
    if q.is_empty() {
        return None;
    }
    let direct = emcraft_ui_core::search::direct(q).map(|r| match r {
        Ok((id, _)) => text(format!("↵ {id} …")).size(t.font_small).color(t.accent).boxed(),
        Err(e) => text(e).size(t.font_small).color(t.error).boxed(),
    });
    let list = emcraft_ui_core::search::matches(q);
    let none = (list.is_empty() && direct.is_none())
        .then(|| text(emcraft_ui_core::lang::tr(t!("No command matches."))).size(t.font_small).color(t.text_dim).boxed());
    let rows = list.into_iter().map(|c| {
        let face = row![
            crate::icons::view(emcraft_ui_core::commands::glyph(c.id), 18.0, t.icon, t.accent),
            text(emcraft_ui_core::lang::command_label(c.id).to_owned()).size(t.font_body),
            text(c.id).size(t.font_small).color(t.text_dim)
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        button(face).style(look::row(t, false)).width(Fill).on_press_maybe(app.job.is_none().then_some(Message::SearchPick(Some(c.id)))).boxed()
    });
    let list = Column::with_children(direct.into_iter().chain(none).chain(rows)).spacing(2).width(520);
    Some(container(list).padding([6, 14]).width(Fill).style(look::ground(t)).boxed())
}

/// `content` with its command's tip on hover: what it does, first sentence, and its id.
pub fn tip<'a>(t: look::Tokens, content: Element<'a, Message>, id: &str) -> Element<'a, Message> {
    let doc = emcraft_ui_core::lang::command_doc(id);
    let first = doc.split_inclusive(". ").next().unwrap_or(doc).trim();
    let said = column![text(first.to_owned()).size(t.font_small).width(360), text(id.to_owned()).size(t.font_small).color(t.text_dim)]
        .spacing(2)
        .width(iced::Length::Shrink);
    let card = container(said).padding(8).style(look::ground(t));
    iced::widget::tooltip(content, card, iced::widget::tooltip::Position::Bottom).gap(4).boxed()
}
