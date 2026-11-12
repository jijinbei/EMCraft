//! A command's dialog over the window: each parameter's field as the shared rules make it
//! ([`emcraft_ui_core::dialogs`]: its kind, its initial value, how its text parses), its
//! error under it, and OK (named for what the command does) or Cancel. OK runs the command — on
//! the worker when it is long — and a refusal comes back under the field it names; OK on the
//! model's own values unchanged runs nothing.

use emcraft_ui_core::dialogs::{self, Dialog, Field, Kind};
use emcraft_ui_core::lang::{self, args, tr, trf};
use emcraft_ui_core::param_labels::{param_label, value_label};
use iced::widget::{Column, Space, button, center, checkbox, container, opaque, pick_list, row, scrollable, text, text_input};
use iced::{Alignment, Color, Element, Fill, Widget};

use crate::app::{App, Message};
use crate::look::{self, Tokens};

/// What changes the open dialog: a field typed, ticked, chosen; a table's row added or taken
/// away; OK; Cancel.
#[derive(Clone, Debug)]
pub enum Edit {
    Text(usize, String),
    On(usize, bool),
    Part(usize, usize, String),
    Cell(usize, usize, usize, String),
    AddRow(usize),
    RemoveRow(usize, usize),
    Ok,
    Cancel,
}

/// The dialog's width (points).
const WIDTH: f32 = 560.0;

/// `d` after `edit` to one of its fields (OK and Cancel are the window's).
pub fn edited(d: Dialog, edit: &Edit) -> Dialog {
    let mut fields = d.fields;
    if let Some(f) = field_of(edit).and_then(|i| fields.get_mut(i)) {
        f.error = None;
        match edit {
            Edit::Text(_, s) => f.text = dialogs::bounded(s),
            Edit::On(_, on) => f.on = *on,
            Edit::Part(_, k, s) => {
                if let Some(p) = f.parts.get_mut(*k) {
                    *p = dialogs::bounded(s);
                }
            }
            Edit::Cell(_, r, c, s) => {
                if let Some(cell) = f.rows.get_mut(*r).and_then(|row| row.get_mut(*c)) {
                    *cell = dialogs::bounded(s);
                }
            }
            Edit::AddRow(_) => {
                let width = match &f.kind {
                    Kind::Table(cols) => cols.len(),
                    _ => 0,
                };
                f.rows.push(vec![String::new(); width]);
            }
            Edit::RemoveRow(_, r) if *r < f.rows.len() => {
                f.rows.remove(*r);
            }
            _ => {}
        }
    }
    Dialog { fields, ..d }
}

/// The field an edit changes.
fn field_of(edit: &Edit) -> Option<usize> {
    match edit {
        Edit::Text(i, _) | Edit::On(i, _) | Edit::Part(i, ..) | Edit::Cell(i, ..) | Edit::AddRow(i) | Edit::RemoveRow(i, _) => Some(*i),
        Edit::Ok | Edit::Cancel => None,
    }
}

fn send(edit: Edit) -> Message {
    Message::Dialog(edit)
}

/// A text field sending `edit` with what is typed; Enter is OK.
fn input<'a>(t: Tokens, value: &'a str, edit: impl Fn(String) -> Edit + 'a) -> Element<'a, Message> {
    text_input("", value).size(t.font_body).padding([3, 6]).style(look::field(t)).on_input(move |s| send(edit(s))).on_submit(send(Edit::Ok)).boxed()
}

/// A choice's word in the window's language.
fn word(v: &str) -> String {
    value_label(v).map_or_else(|| v.to_owned(), |l| lang::lookup(l).to_owned())
}

/// Field `i`'s editor.
fn editor<'a>(t: Tokens, i: usize, f: &'a Field) -> Element<'a, Message> {
    match &f.kind {
        Kind::Bool => checkbox(f.on).on_toggle(move |on| send(Edit::On(i, on))).boxed(),
        Kind::Choice(list) => {
            // Empty is the command's default.
            let options: Vec<String> = std::iter::once(String::new()).chain(list.iter().cloned()).collect();
            let shown = |v: &String| if v.is_empty() { tr(t!("default")).to_owned() } else { word(v) };
            pick_list(Some(f.text.clone()), options, shown).on_select(move |v| send(Edit::Text(i, v))).style(look::choice(t)).boxed()
        }
        Kind::Vector(names) => {
            let parts = names.iter().enumerate().map(|(k, n)| {
                let value = f.parts.get(k).map_or("", String::as_str);
                row![text(*n).size(t.font_small).color(t.text_dim), input(t, value, move |s| Edit::Part(i, k, s))]
                    .spacing(4)
                    .align_y(Alignment::Center)
                    .boxed()
            });
            row(parts).spacing(8).boxed()
        }
        Kind::Table(cols) => {
            let head = row(cols.iter().map(|c| {
                let name = if c.required { format!("{}*", c.name) } else { c.name.to_owned() };
                text(name).size(t.font_small).color(t.text_dim).width(Fill).boxed()
            }))
            .push(Space::new().width(24).boxed())
            .spacing(4);
            let rows = f.rows.iter().enumerate().map(|(r, cells)| {
                let cells = cells.iter().enumerate().map(|(c, v)| container(input(t, v, move |s| Edit::Cell(i, r, c, s))).width(Fill).boxed());
                let x = button(text("×").size(t.font_small)).style(look::quiet(t)).on_press(send(Edit::RemoveRow(i, r)));
                row(cells).push(x.boxed()).spacing(4).align_y(Alignment::Center).boxed()
            });
            let add = button(text(trf(t!("Add {row}"), args!(row = f.name.trim_end_matches('s')))).size(t.font_small))
                .style(look::button(t, false))
                .on_press(send(Edit::AddRow(i)));
            Column::with_children(std::iter::once(head.boxed()).chain(rows).chain(std::iter::once(add.boxed()))).spacing(4).boxed()
        }
        Kind::Number | Kind::Integer | Kind::Text | Kind::Numbers | Kind::Json => input(t, &f.text, move |s| Edit::Text(i, s)),
    }
}

/// The open dialog `d` over the window: its title, its fields with their units and errors, the
/// error the engine gave, OK and Cancel.
pub fn view<'a>(app: &'a App, d: &'a Dialog) -> Element<'a, Message> {
    let t = app.look.tokens;
    let id = d.command.id;
    let fields = d.fields.iter().enumerate().map(|(i, f)| {
        let name = format!("{}{}", lang::lookup(param_label(id, f.name)), if f.required { " *" } else { "" });
        let unit = dialogs::unit(f.quantity, &app.session);
        let head = row![text(name).size(t.font_body).color(t.form_dim), text(unit).size(t.font_small).color(t.text_dim)].spacing(6);
        let error = f.error.as_ref().map(|e| text(e.clone()).size(t.font_small).color(t.error).boxed());
        Column::with_children([head.boxed(), editor(t, i, f)].into_iter().chain(error)).spacing(3).boxed()
    });
    let busy = app.job.is_some();
    let ok =
        button(text(tr(dialogs::ok_label(id))).size(t.font_button)).style(look::button(t, true)).on_press_maybe((!busy).then_some(send(Edit::Ok)));
    let cancel = button(text(tr(t!("Cancel"))).size(t.font_button)).style(look::button(t, false)).on_press(send(Edit::Cancel));
    let error = d.error.as_ref().map(|e| text(e.clone()).size(t.font_body).color(t.error).boxed());
    let title = text(lang::command_label(id).to_owned()).size(t.font_heading).boxed();
    let list = scrollable(Column::with_children(fields).spacing(10).padding([0, 8])).height(iced::Length::Shrink).boxed();
    let footer = row![Space::new().width(Fill), cancel, ok].spacing(8).boxed();
    let body = Column::with_children([title, list].into_iter().chain(error).chain(std::iter::once(footer))).spacing(12);
    let card = container(body).padding(18).width(WIDTH).style(look::ground(t));
    let veil = container(center(card))
        .width(Fill)
        .height(Fill)
        .style(|_: &iced::Theme| container::Style { background: Some(Color { a: 0.35, ..Color::BLACK }.into()), ..container::Style::default() });
    opaque(veil).boxed()
}
