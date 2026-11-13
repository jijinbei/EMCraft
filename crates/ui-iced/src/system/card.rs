//! A node's card: its header (a stage's pressed to move it, with a mark that deletes it), its
//! rows of settings edited in place, the chain's reading at its output. What it holds is the
//! shared crate's ([`emcraft_ui_core::system::card`]); drawing it is one loop.

use emcraft_ui_core::lang::tr;
use emcraft_ui_core::system::card::{self as shared, Edit, Row};
use emcraft_ui_core::system::gesture::Command;
use emcraft_ui_core::system::layout::{HEAD_H, Kind, NodeBox, ROW_H};
use iced::widget::{Column, column, container, mouse_area, pick_list, row, text, text_input};
use iced::{Alignment, Element, Fill, Widget, mouse};
use serde_json::json;

use crate::app::{App, Field, Message};
use crate::look;

/// A row's label column (points).
const LABEL_W: f32 = 56.0;

/// The card of `node`.
pub(super) fn card<'a>(app: &'a App, node: &NodeBox) -> Element<'a, Message> {
    let rows = shared::rows(&app.session, node).into_iter().map(|r| line(app, node, r));
    let t = app.look.tokens;
    let reading = shared::reading(&app.session, node).map(|r| text(r).size(t.font_small).color(t.accent).boxed());
    let off = (!node.on_chain).then(|| text(tr(t!("off the chain"))).size(t.font_small - 1.0).color(t.text_dim).boxed());
    let body = Column::with_children(rows.chain(reading).chain(off)).padding([t.space_4, t.space_8]);
    let card = column![header(app, node), body];
    container(card).width(node.rect.width()).height(node.rect.height()).style(look::card(t, node.on_chain)).boxed()
}

/// The header: a stage's pressed to move it, with a mark that deletes it.
fn header<'a>(app: &App, node: &NodeBox) -> Element<'a, Message> {
    let t = app.look.tokens;
    let title = text(shared::title(&app.session, node)).size(t.font_small).width(Fill);
    let content: Element<Message> = match node.kind {
        Kind::Stage(_) => {
            let delete = Command { id: "system.stage.delete", params: json!({"stage": node.name}) };
            let grab = mouse_area(title).on_press(Message::Grab(node.name.clone())).interaction(mouse::Interaction::Grab);
            row![grab, crate::parts::close(t, t!("Delete"), Message::Run(delete))].align_y(Alignment::Center).boxed()
        }
        Kind::Source | Kind::Receiver => title.boxed(),
    };
    container(content)
        .padding([t.space_4, t.space_8])
        .height(HEAD_H)
        .width(Fill)
        .align_y(Alignment::Center)
        .style(look::header(t, node.on_chain))
        .boxed()
}

/// A row: its label, its value edited in place (a number committed on Enter, a name chosen) and
/// its unit.
fn line<'a>(app: &App, node: &NodeBox, r: Row) -> Element<'a, Message> {
    let field = Field { node: node.name.clone(), key: r.key };
    let t = app.look.tokens;
    let label = text(r.label).size(t.font_small).color(t.form_dim).width(LABEL_W).wrapping(text::Wrapping::None);
    let edit: Element<Message> = match r.value {
        Edit::Number { value, unit, .. } => {
            let typed = field.clone();
            let input = text_input("", app.draft(&field, value))
                .size(t.font_small)
                .padding([t.space_2, t.space_6])
                .style(look::field(t))
                .on_input(move |s| Message::Typed(typed.clone(), s))
                .on_submit(Message::Commit(field));
            row![input, text(unit).size(t.font_small).color(t.text_dim).width(34)].spacing(t.space_4).align_y(Alignment::Center).boxed()
        }
        Edit::Choice { options, current } => {
            let now = options.iter().find(|(v, _)| *v == current).cloned();
            pick_list(now, options, |(_, shown): &(String, String)| shown.clone())
                .on_select(move |(v, _)| Message::Chose(field.clone(), v))
                .text_size(t.font_small)
                .padding([t.space_2, t.space_6])
                .style(look::choice(t))
                .width(Fill)
                .boxed()
        }
    };
    row![label, edit].spacing(t.space_4).align_y(Alignment::Center).height(ROW_H).boxed()
}
