//! The browser: the model and its results as a tree beside the 3D view ([`emcraft_ui_core::browser`]
//! reads it from the session) — each section's rows, a row's name and its detail dimmer, what is
//! selected lit. A click selects what a row names (its part in the 3D view too) and shows the
//! page it stands for or runs its command.

use emcraft_ui_core::lang::lookup;
use iced::widget::{Column, button, column, container, scrollable, text};
use iced::{Element, Fill, Widget};

use crate::app::{App, Message};
use crate::look;

/// The browser's width (points).
const WIDTH: f32 = 270.0;

/// The browser's card.
pub fn view(app: &App) -> Element<'_, Message> {
    let t = app.look.tokens;
    let sections = app.tree.iter().flat_map(|(_, tree)| tree).filter(|(_, rows)| !rows.is_empty()).map(|(section, rows)| {
        let head = text(lookup(section).to_owned()).size(t.font_small).color(t.text_dim);
        let rows = rows.iter().map(|r| {
            let selected = app.row_selected(r);
            let name = text(r.shown.clone()).size(t.font_body).color(if selected { t.selected_ink } else { t.text });
            let detail = (!r.detail.is_empty()).then(|| text(r.detail.clone()).size(t.font_small).color(t.text_dim).boxed());
            let line = Column::with_children(std::iter::once(name.boxed()).chain(detail)).spacing(1);
            let indent = container(line).padding([0.0, 12.0 * f32::from(r.depth)]);
            button(indent).width(Fill).padding([3, 8]).style(look::row(t, selected)).on_press(Message::Row(r.id.clone())).boxed()
        });
        column![head, Column::with_children(rows).spacing(1)].spacing(4).boxed()
    });
    let list = Column::with_children(sections).spacing(12).padding(10).width(Fill);
    container(scrollable(list)).width(WIDTH).height(Fill).style(look::ground(t)).boxed()
}
