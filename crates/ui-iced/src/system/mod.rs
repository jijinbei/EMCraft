//! A system's node editor: the band over a canvas of nodes — the source, the stages and the
//! receiver — each card's settings typed in place and, once read, the chain's gain and noise
//! figure at its output; links dragged from an output to an input, taken away by the mark on
//! them; nodes moved by their headers. Under the canvas the chart and the budget, read again at
//! every change. What each holds comes from [`emcraft_ui_core::system`]; this lays it out.

mod board;
mod budget;
mod card;

use emcraft_ui_core::lang::{tr, trf};
use emcraft_ui_core::system::gesture::Command;
use emcraft_ui_core::system::{self as shared, band};
use iced::widget::{Column, button, column, row, scrollable, text, text_input};
use iced::{Alignment, Element, Widget};

use crate::app::{App, BAND, Field, Message};
use crate::look;

/// The window's System view.
pub fn view(app: &App) -> Element<'_, Message> {
    let chain = &app.session.model().system;
    let t = app.look.tokens;
    let notes =
        [app.error.as_deref(), app.unread.as_deref()].into_iter().flatten().map(|e| text(e.to_owned()).size(t.font_body).color(t.error).boxed());
    let head = [band_bar(app)].into_iter().chain(notes);
    let body: Vec<Element<Message>> =
        if chain.stages.is_empty() { vec![start(app)] } else { vec![adders(app), board::board(app), budget::chart(app), budget::sheet(app)] };
    scrollable(Column::with_children(head.chain(body)).spacing(10).padding(16)).boxed()
}

/// The command that adds a stage of `kind` at the end of the chain.
fn add(kind: &str) -> Message {
    Message::Run(Command { id: "system.stage.add", params: shared::starting(kind) })
}

/// The band over the canvas: start, stop, points and the budget's frequency, confirmed
/// (`project.setup`) as each is changed; the band the chain was read over until one is.
fn band_bar(app: &App) -> Element<'_, Message> {
    let fu = app.session.display().frequency.symbol();
    let now = band::shown(&app.session);
    let fields = [("start", t!("start"), fu), ("stop", t!("stop"), fu), ("points", t!("Frequency points"), ""), ("frequency", t!("budget at"), fu)];
    let cells = fields.into_iter().map(|(key, label, unit)| {
        let field = Field { node: BAND.to_owned(), key };
        let shown = app.draft(&field, band::field(now, key));
        let typed = field.clone();
        let t = app.look.tokens;
        let input = text_input("", shown).size(t.font_body).padding([3, 6]).width(80).style(look::field(t));
        row![
            text(tr(label)).size(t.font_body).color(t.form_dim),
            input.on_input(move |s| Message::Typed(typed.clone(), s)).on_submit(Message::Commit(field)),
            text(unit).size(t.font_body).color(t.text_dim),
        ]
        .spacing(4)
        .align_y(Alignment::Center)
        .boxed()
    });
    row(cells).spacing(16).boxed()
}

/// A chain with no stage yet: what a system is, and a button for each kind of stage.
fn start(app: &App) -> Element<'_, Message> {
    let t = app.look.tokens;
    let kinds = shared::KINDS.iter().map(|(kind, label, _)| {
        button(text(format!("+ {}", tr(*label))).size(t.font_heading)).padding([10, 18]).style(look::button(t, false)).on_press(add(kind)).boxed()
    });
    column![
        text(tr(t!("A chain from the antenna to the receiver"))).size(t.font_heading + 4.0),
        text(tr(t!("Add its stages in order: an LNA, a filter's S-parameters (a Touchstone file or a sweep), lines, more amplifiers.")))
            .size(t.font_body)
            .color(t.text_dim),
        row(kinds).spacing(8),
    ]
    .spacing(10)
    .align_x(Alignment::Center)
    .boxed()
}

/// A button for each kind of stage, adding it at the end of the chain.
fn adders(app: &App) -> Element<'_, Message> {
    let t = app.look.tokens;
    let kinds = shared::KINDS.iter().map(|(kind, label, _)| {
        button(text(format!("+ {}", tr(*label))).size(t.font_button)).style(look::button(t, false)).on_press(add(kind)).boxed()
    });
    row(kinds).spacing(6).boxed()
}

/// "budget at 1.09 GHz", under the chart's choices.
fn budget_at(app: &App) -> Option<String> {
    let r = app.session.results().system.as_deref()?;
    let p = r.nearest(r.at)?;
    let unit = app.session.display().frequency;
    Some(trf(t!("budget at {f} {unit}"), emcraft_ui_core::args!(f = unit.from_si(p.frequency), unit = unit.symbol())))
}
