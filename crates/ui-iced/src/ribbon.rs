//! The ribbon under the window's bar: its tabs and each tab's panels of commands as the shared
//! rules list them ([`emcraft_ui_core::ribbon`]), those of other kinds of analysis left out. A
//! command with parameters opens its dialog; one without runs.

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
    let panels =
        TABS.iter().filter(|(tab, _)| *tab == app.tab).flat_map(|(_, panels)| panels.iter()).filter(|(name, _)| *name != ribbon::STAGES).filter_map(
            |(name, ids)| {
                let buttons: Vec<Element<Message>> = ids
                    .iter()
                    .filter(shown)
                    .map(|id| {
                        let label = emcraft_ui_core::commands::button_label(id).to_owned();
                        button(text(label).size(t.font_button))
                            .style(look::button(t, false))
                            .on_press_maybe(idle.then(|| Message::Command((*id).to_owned())))
                            .boxed()
                    })
                    .collect();
                (!buttons.is_empty()).then(|| {
                    column![row(buttons).spacing(6).wrap().vertical_spacing(6), text(lookup(name).to_owned()).size(t.font_small).color(t.text_dim)]
                        .spacing(4)
                        .boxed()
                })
            },
        );
    let body = row(panels).spacing(24).align_y(Alignment::Start).wrap().vertical_spacing(10);
    container(Column::with_children([row(tabs).spacing(2).boxed(), body.boxed()]).spacing(6))
        .padding([6, 14])
        .width(Fill)
        .style(look::ground(t))
        .boxed()
}

/// The tab a ribbon opens on: the system's for a system, else the model's.
pub fn first_tab(app: &App) -> RibbonTab {
    if app.session.model().analysis() == emcraft_engine::workflow::Analysis::System { RibbonTab::System } else { RibbonTab::Model }
}
