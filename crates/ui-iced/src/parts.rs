//! The window's small shared parts, each one look wherever it is: a panel's heading, a note in
//! a colour, an action, a chip (a choice that is on or off), the × that closes or takes away,
//! what a page says while it has nothing to show, and a shortcut's keys as key caps.

use emcraft_ui_core::lang::{Text, tr};
use iced::widget::{Column, button, container, text, tooltip};
use iced::{Color, Element, Widget};

use crate::app::Message;
use crate::look::{self, Tokens};

/// A heading of a panel's part: small, dim, in capitals.
pub fn heading<'a>(t: Tokens, title: Text) -> Element<'a, Message> {
    text(tr(title).to_uppercase()).size(t.font_small).color(t.text_dim).boxed()
}

/// A short note in `ink`.
pub fn note<'a>(t: Tokens, s: impl Into<String>, ink: Color) -> Element<'a, Message> {
    text(s.into()).size(t.font_small).color(ink).boxed()
}

/// A button that does something: the primary one filled; none to send, off.
pub fn action<'a>(t: Tokens, label: impl Into<String>, primary: bool, on: Option<Message>) -> Element<'a, Message> {
    button(text(label.into()).size(t.font_button)).style(look::button(t, primary)).on_press_maybe(on).boxed()
}

/// A choice that is on or off.
pub fn chip<'a>(t: Tokens, label: impl Into<String>, on: bool, choose: Option<Message>) -> Element<'a, Message> {
    button(text(label.into()).size(t.font_button)).style(look::toggle(t, on)).on_press_maybe(choose).boxed()
}

/// `content` with `said` on a card under it while the pointer is over it.
pub fn hint<'a>(t: Tokens, content: Element<'a, Message>, said: Element<'a, Message>) -> Element<'a, Message> {
    let card = container(said).padding(t.space_6).style(look::ground(t));
    tooltip(content, card, tooltip::Position::Bottom).gap(t.space_4).boxed()
}

/// `content` with a short text on hover.
pub fn hint_text<'a>(t: Tokens, content: Element<'a, Message>, said: impl Into<String>) -> Element<'a, Message> {
    hint(t, content, text(said.into()).size(t.font_small).boxed())
}

/// The × that closes or takes away what it stands by, `what` said on hover.
pub fn close<'a>(t: Tokens, what: Text, message: Message) -> Element<'a, Message> {
    let x = button(text("×").size(t.font_button)).style(look::quiet(t)).padding([0.0, t.space_4]).on_press(message);
    hint_text(t, x.boxed(), tr(what))
}

/// What a page says while it has nothing to show, and what makes it (if anything does).
pub fn empty<'a>(t: Tokens, said: impl Into<String>, make: Option<Element<'a, Message>>) -> Element<'a, Message> {
    let said = text(said.into()).size(t.font_body).color(t.text_dim).boxed();
    container(Column::with_children(std::iter::once(said).chain(make)).spacing(t.space_12)).padding(t.space_16).boxed()
}

/// The keys that do `what` ([`emcraft_ui_core::shortcuts::keys`]), each on a key cap; none
/// when no key does it.
pub fn keys<'a>(t: Tokens, what: &str) -> Option<Element<'a, Message>> {
    let caps = emcraft_ui_core::shortcuts::keys(what)?
        .into_iter()
        .map(|k| container(text(k).size(t.font_small - 1.0).color(t.text_dim)).padding([0.0, t.space_4]).style(look::keycap(t)).boxed());
    Some(iced::widget::Row::with_children(caps).spacing(t.space_2).align_y(iced::Alignment::Center).boxed())
}
