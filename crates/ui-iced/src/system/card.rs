//! A node's card: its header (a stage's pressed to move it, with a mark that deletes it), its
//! rows of settings edited in place, the chain's reading at its output. What it holds is the
//! shared crate's ([`emcraft_ui_core::system::card`]); drawing it is one loop.

use emcraft_ui_core::lang::tr;
use emcraft_ui_core::system::card::{self as shared, Edit, Row};
use emcraft_ui_core::system::gesture::Command;
use emcraft_ui_core::system::layout::{HEAD_H, Kind, NodeBox, ROW_H};
use iced::widget::{Column, button, column, container, mouse_area, pick_list, row, text, text_input};
use iced::{Alignment, Element, Fill, Widget, mouse};
use serde_json::json;

use crate::app::{App, Field, Message};

/// The card of `node`.
pub(super) fn card<'a>(app: &'a App, node: &NodeBox) -> Element<'a, Message> {
    let rows = shared::rows(&app.session, node).into_iter().map(|r| line(app, node, r));
    let reading = shared::reading(&app.session, node).map(|r| text(r).size(12).style(text::primary).boxed());
    let off = (!node.on_chain).then(|| text(tr(t!("off the chain"))).size(11).style(text::secondary).boxed());
    let body = Column::with_children(rows.chain(reading).chain(off)).padding([4, 10]);
    let style = if node.on_chain { container::bordered_box } else { container::dark };
    container(column![header(app, node), body]).width(node.rect.width()).height(node.rect.height()).style(style).boxed()
}

/// The header: a stage's pressed to move it, with a mark that deletes it.
fn header<'a>(app: &App, node: &NodeBox) -> Element<'a, Message> {
    let title = text(shared::title(&app.session, node)).size(12).width(Fill);
    let content: Element<Message> = match node.kind {
        Kind::Stage(_) => {
            let delete = Command { id: "system.stage.delete", params: json!({"stage": node.name}) };
            let grab = mouse_area(title).on_press(Message::Grab(node.name.clone())).interaction(mouse::Interaction::Grab);
            row![grab, button(text("×").size(11)).padding([0, 4]).style(button::text).on_press(Message::Run(delete))]
                .align_y(Alignment::Center)
                .boxed()
        }
        Kind::Source | Kind::Receiver => title.boxed(),
    };
    container(content).padding([4, 10]).height(HEAD_H).width(Fill).style(container::secondary).boxed()
}

/// A row: its label, its value edited in place (a number committed on Enter, a name chosen) and
/// its unit.
fn line<'a>(app: &App, node: &NodeBox, r: Row) -> Element<'a, Message> {
    let field = Field { node: node.name.clone(), key: r.key };
    let label = text(r.label).size(12).width(44);
    let edit: Element<Message> = match r.value {
        Edit::Number { value, unit, .. } => {
            let typed = field.clone();
            let input = text_input("", app.draft(&field, value))
                .size(12)
                .on_input(move |t| Message::Typed(typed.clone(), t))
                .on_submit(Message::Commit(field));
            row![input, text(unit).size(12).width(34)].spacing(4).align_y(Alignment::Center).boxed()
        }
        Edit::Choice { options, current } => {
            let now = options.iter().find(|(v, _)| *v == current).cloned();
            pick_list(now, options, |(_, shown): &(String, String)| shown.clone())
                .on_select(move |(v, _)| Message::Chose(field.clone(), v))
                .text_size(12)
                .width(Fill)
                .boxed()
        }
    };
    row![label, edit].spacing(4).align_y(Alignment::Center).height(ROW_H).boxed()
}
