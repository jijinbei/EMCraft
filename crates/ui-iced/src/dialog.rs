//! A command's dialog — in the properties panel at the page's right on the project's pages (an
//! item's form follows the selection, docs/adr/0009-model-tree-window.md), over the window
//! elsewhere: each parameter's field as the shared rules make it
//! ([`emcraft_ui_core::dialogs`]: its kind, its initial value, how its text parses), its
//! error under it, and OK (named for what the command does) or Cancel. OK runs the command — on
//! the worker when it is long — and a refusal comes back under the field it names; OK on the
//! model's own values unchanged runs nothing.

use emcraft_ui_core::dialogs::{self, Dialog, Field, Kind};
use emcraft_ui_core::lang::{self, args, tr, trf};
use emcraft_ui_core::param_labels::{param_label, value_label};
use iced::widget::{Column, Space, button, center, checkbox, container, opaque, pick_list, row, scrollable, text, text_input};
use iced::{Alignment, Element, Fill, Widget};

use crate::app::{App, Message};
use crate::look::{self, Tokens};

/// What changes the open dialog: a field typed, ticked, chosen; a table's row added or taken
/// away; its options or its description unfolded or folded; OK; Cancel.
#[derive(Clone, Debug)]
pub enum Edit {
    Text(usize, String),
    On(usize, bool),
    Part(usize, usize, String),
    Cell(usize, usize, usize, String),
    AddRow(usize),
    RemoveRow(usize, usize),
    Options,
    Help,
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
    match edit {
        Edit::Options => Dialog { fields, options: !d.options, ..d },
        Edit::Help => Dialog { fields, help: !d.help, ..d },
        _ => Dialog { fields, ..d },
    }
}

/// The field an edit changes.
fn field_of(edit: &Edit) -> Option<usize> {
    match edit {
        Edit::Text(i, _) | Edit::On(i, _) | Edit::Part(i, ..) | Edit::Cell(i, ..) | Edit::AddRow(i) | Edit::RemoveRow(i, _) => Some(*i),
        Edit::Options | Edit::Help | Edit::Ok | Edit::Cancel => None,
    }
}

fn send(edit: Edit) -> Message {
    Message::Dialog(edit)
}

/// A text field sending `edit` with what is typed; Enter is OK.
/// A vector's part's field's width (points): a number of a few digits.
const VECTOR_PART: f32 = 72.0;

fn input<'a>(t: Tokens, value: &'a str, edit: impl Fn(String) -> Edit + 'a) -> Element<'a, Message> {
    text_input("", value)
        .size(t.font_body)
        .padding([t.space_4, t.space_6])
        .style(look::field(t))
        .on_input(move |s| send(edit(s)))
        .on_submit(send(Edit::Ok))
        .boxed()
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
            pick_list(Some(f.text.clone()), options, shown)
                .on_select(move |v| send(Edit::Text(i, v)))
                .text_size(t.font_button)
                .style(look::choice(t))
                .boxed()
        }
        Kind::Vector(names) => {
            let parts = names.iter().enumerate().map(|(k, n)| {
                let value = f.parts.get(k).map_or("", String::as_str);
                let field = container(input(t, value, move |s| Edit::Part(i, k, s))).width(VECTOR_PART);
                row![text(lang::lookup(n).to_owned()).size(t.font_small).color(t.text_dim), field]
                    .spacing(t.space_4)
                    .align_y(Alignment::Center)
                    .boxed()
            });
            // As many on a line as the panel holds.
            row(parts).spacing(t.space_8).wrap().vertical_spacing(t.space_6).boxed()
        }
        Kind::Table(cols) => {
            let head = row(cols.iter().map(|c| {
                let name = if c.required { format!("{}*", c.name) } else { c.name.to_owned() };
                text(name).size(t.font_small).color(t.text_dim).width(Fill).boxed()
            }))
            .push(Space::new().width(24).boxed())
            .spacing(t.space_4);
            let rows = f.rows.iter().enumerate().map(|(r, cells)| {
                let cells = cells.iter().enumerate().map(|(c, v)| container(input(t, v, move |s| Edit::Cell(i, r, c, s))).width(Fill).boxed());
                let x = crate::parts::close(t, t!("Remove"), send(Edit::RemoveRow(i, r)));
                row(cells).push(x).spacing(t.space_4).align_y(Alignment::Center).boxed()
            });
            let add = button(text(trf(t!("Add {row}"), args!(row = f.name.trim_end_matches('s')))).size(t.font_small))
                .style(look::button(t, false))
                .on_press(send(Edit::AddRow(i)));
            Column::with_children(std::iter::once(head.boxed()).chain(rows).chain(std::iter::once(add.boxed()))).spacing(t.space_4).boxed()
        }
        // Its id names it (`dialog-<parameter>`), for scenarios and agents to find.
        Kind::Number | Kind::Integer | Kind::Text | Kind::Numbers | Kind::Json => text_input("", &f.text)
            .id(format!("dialog-{}", f.name))
            .size(t.font_body)
            .padding([t.space_4, t.space_6])
            .style(look::field(t))
            .on_input(move |s| send(Edit::Text(i, s)))
            .on_submit(send(Edit::Ok))
            .boxed(),
    }
}

/// Dialog `d`'s card inside `padding`: its title, its fields with their units and errors, the error the engine
/// gave, OK and Cancel (an item's form: Delete too).
fn body<'a>(app: &'a App, d: &'a Dialog, padding: iced::Padding) -> Element<'a, Message> {
    let t = app.look.tokens;
    let id = d.command.id;
    let shown = crate::import::shown(app, d);
    let fields = d.fields.iter().enumerate().filter(|(_, f)| shown(f.name)).map(|(i, f)| {
        let name = format!("{}{}", lang::lookup(param_label(id, f.name)), if f.required { " *" } else { "" });
        let unit = dialogs::unit(f.quantity, &app.session);
        let head = row![text(name).size(t.font_body).color(t.form_dim), text(unit).size(t.font_small).color(t.text_dim)].spacing(t.space_6);
        let error = f.error.as_ref().map(|e| text(e.clone()).size(t.font_small).color(t.error).boxed());
        Column::with_children([head.boxed(), editor(t, i, f)].into_iter().chain(error)).spacing(t.space_4).boxed()
    });
    let busy = app.job.is_some();
    // An item's form applies its changes and stays.
    let ok_label = if d.item.is_some() { t!("Apply") } else { dialogs::ok_label(id) };
    let ok = button(text(tr(ok_label)).size(t.font_button)).style(look::button(t, true)).on_press_maybe((!busy).then_some(send(Edit::Ok)));
    let cancel = button(text(tr(t!("Cancel"))).size(t.font_button)).style(look::button(t, false)).on_press(send(Edit::Cancel));
    // An item's form deletes it too.
    let deletable = d.item.is_some() && app.picked().iter().any(|r| r.item.as_ref().is_some_and(|i| !i.delete.is_empty()));
    let delete = deletable.then(|| {
        button(text(tr(t!("Delete"))).size(t.font_button)).style(look::button(t, false)).on_press_maybe((!busy).then_some(Message::Delete)).boxed()
    });
    let error = d.error.as_ref().map(|e| text(e.clone()).size(t.font_body).color(t.error).boxed());
    let title = text(lang::command_label(id).to_owned()).size(t.font_heading).boxed();
    let list = Column::with_children(fields).spacing(t.space_8).padding([0.0, t.space_8]).boxed();
    let delete = delete.map(|b| crate::search::busy(app, b));
    let ok = crate::search::busy(app, ok.boxed());
    let footer = row(delete.into_iter().chain([Space::new().width(Fill).boxed(), cancel.boxed(), ok])).spacing(t.space_8).boxed();
    // The import's head: the formats, Choose file…, what the file holds (crate::import).
    let head = crate::import::head(app, d);
    let (fold, help) = (crate::import::fold(app, d), crate::import::help(app, d));
    Column::with_children(std::iter::once(title).chain(head).chain(fold).chain([list]).chain(help).chain(error).chain(std::iter::once(footer)))
        .spacing(t.space_12)
        .padding(padding)
        .boxed()
}

/// The open dialog `d` over the window (a page that is not the project's: the start page).
pub fn view<'a>(app: &'a App, d: &'a Dialog) -> Element<'a, Message> {
    let t = app.look.tokens;
    let card =
        container(scrollable(body(app, d, iced::Padding::ZERO)).height(iced::Length::Shrink)).padding(t.space_16).width(WIDTH).style(look::ground(t));
    let veil = container(center(card)).width(Fill).height(Fill).style(look::veil);
    opaque(veil).boxed()
}

/// Dialog `d` in the properties panel at the page's right (docs/adr/0009-model-tree-window.md):
/// the model beside it stays in sight and in reach.
pub fn panel<'a>(app: &'a App, d: &'a Dialog, width: f32) -> Element<'a, Message> {
    let t = app.look.tokens;
    let list = body(app, d, iced::Padding { top: 12.0, right: 18.0, bottom: 12.0, left: 12.0 });
    container(scrollable(list).height(Fill)).width(width).height(Fill).style(look::ground(t)).boxed()
}
