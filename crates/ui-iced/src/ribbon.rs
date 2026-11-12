//! The ribbon under the window's bar: its tabs and each tab's panels of commands as the shared
//! rules list them ([`emcraft_ui_core::ribbon`]), those of other kinds of analysis left out, each
//! command its glyph over its name, its tip what it does with its id. A command with parameters
//! opens its dialog; one without runs.

use emcraft_ui_core::lang::lookup;
use emcraft_ui_core::ribbon::{self, RibbonTab, TABS};
use iced::widget::{Column, button, column, container, row, text};
use iced::{Alignment, Element, Fill, Widget};

use crate::app::{App, Message};
use crate::look;

/// The ribbon: the tabs, then the panels of the one shown.
pub fn view(app: &App) -> Element<'_, Message> {
    let t = app.look.tokens;
    let kind = app.session.model().analysis();
    let shown = |id: &&&str| ribbon::fits(kind, id);
    let tabs = ribbon::tabs(kind).into_iter().map(|tab| {
        let on = tab == app.tab;
        button(text(lookup(tab.label()).to_owned()).size(t.font_small)).style(look::row(t, on)).padding([3, 10]).on_press(Message::Tab(tab)).boxed()
    });
    let idle = app.job.is_none();
    let panels = TABS
        .iter()
        .filter(|(tab, _)| *tab == app.tab)
        .flat_map(|(_, panels)| panels.iter())
        .filter(|(name, _)| *name != ribbon::STAGES)
        .filter_map(|(name, ids)| {
            let buttons: Vec<Element<Message>> = ids
                .iter()
                .filter(shown)
                .map(|id| {
                    let label = emcraft_ui_core::commands::button_label(id).to_owned();
                    let glyph = emcraft_ui_core::commands::glyph(id);
                    let face = column![crate::icons::view(glyph, 22.0, t.icon, t.accent), text(label).size(t.font_small)]
                        .spacing(2)
                        .align_x(Alignment::Center);
                    let b =
                        button(face).style(look::button(t, false)).padding([4, 8]).on_press_maybe(idle.then(|| Message::Command((*id).to_owned())));
                    tip(t, b.boxed(), id)
                })
                .collect();
            (!buttons.is_empty()).then(|| {
                column![row(buttons).spacing(6).wrap().vertical_spacing(6), text(lookup(name).to_owned()).size(t.font_small).color(t.text_dim)]
                    .spacing(4)
                    .boxed()
            })
        });
    let body = row(panels).spacing(24).align_y(Alignment::Start).wrap().vertical_spacing(10);
    let head = row(tabs).spacing(2).push(iced::widget::Space::new().width(Fill).boxed()).push(search_field(app)).align_y(Alignment::Center);
    container(Column::with_children([head.boxed(), body.boxed()].into_iter().chain(results(app))).spacing(6))
        .padding([6, 14])
        .width(Fill)
        .style(look::ground(t))
        .boxed()
}

/// The search field's widget id: `/` focuses it.
pub const SEARCH: &str = "emcraft-search";

/// The command search's field at the tab row's right.
fn search_field(app: &App) -> Element<'_, Message> {
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

/// The commands the query matches, under the ribbon while it has one: each its glyph, its name
/// and its id; a click (or Enter, the first) opens it as its ribbon button would. A query naming
/// a command with its parameters (`command.id {json}`) runs it as typed.
fn results(app: &App) -> Option<Element<'_, Message>> {
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
    Some(Column::with_children(direct.into_iter().chain(none).chain(rows)).spacing(2).width(520).boxed())
}

/// `content` with its command's tip on hover: what it does, first sentence, and its id.
fn tip<'a>(t: look::Tokens, content: Element<'a, Message>, id: &str) -> Element<'a, Message> {
    let doc = emcraft_ui_core::lang::command_doc(id);
    let first = doc.split_inclusive(". ").next().unwrap_or(doc).trim();
    let said = column![text(first.to_owned()).size(t.font_small).width(360), text(id.to_owned()).size(t.font_small).color(t.text_dim)]
        .spacing(2)
        .width(iced::Length::Shrink);
    let card = container(said).padding(8).style(look::ground(t));
    iced::widget::tooltip(content, card, iced::widget::tooltip::Position::Bottom).gap(4).boxed()
}

/// The tab a ribbon opens on: the system's for a system, else the model's.
pub fn first_tab(app: &App) -> RibbonTab {
    if app.session.model().analysis() == emcraft_engine::workflow::Analysis::System { RibbonTab::System } else { RibbonTab::Model }
}
