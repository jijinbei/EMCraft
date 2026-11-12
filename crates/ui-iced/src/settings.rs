//! The settings page: the theme, the language, the units new projects start with, and the 3D
//! view's mouse mapping and drawing — each a choice that changes the window at once and is kept
//! in the preferences both windows share ([`emcraft_ui_core::prefs::Prefs::changed`]).

use emcraft_engine::theme::Base;
use emcraft_model::units::{Frequency, Length};
use emcraft_ui_core::action::Pref;
use emcraft_ui_core::lang::{self, Text, lookup, tr};
use emcraft_ui_core::navigation::{NAV_PRESETS, TOUCHPAD};
use emcraft_ui_core::prefs::GpuLook;
use iced::widget::{Column, button, column, container, pick_list, row, scrollable, text};
use iced::{Alignment, Element, Fill, Widget};

use crate::app::{App, Message};
use crate::look::{self, Tokens};

/// A choice among others: lit when it is the one kept.
fn choice<'a>(t: Tokens, label: &str, on: bool, pref: Pref) -> Element<'a, Message> {
    button(text(label.to_owned()).size(t.font_button)).style(look::button(t, on)).on_press(Message::Pref(pref)).boxed()
}

/// A section: its heading, what it is about, its choices.
fn section<'a>(t: Tokens, title: Text, about: &str, body: Element<'a, Message>) -> Element<'a, Message> {
    column![text(tr(title)).size(t.font_heading), text(about.to_owned()).size(t.font_small).color(t.text_dim), body].spacing(6).boxed()
}

/// A theme's name as its button reads: a built-in one capitalised, in the window's language.
fn theme_label(name: &str) -> String {
    match name.chars().next() {
        Some(first) if Base::parse(name).is_some() => lookup(&first.to_uppercase().chain(name.chars().skip(1)).collect::<String>()).to_owned(),
        _ => name.to_owned(),
    }
}

/// The settings page.
pub fn view(app: &App) -> Element<'_, Message> {
    let t = app.look.tokens;
    let p = &app.prefs;
    let themes =
        row(app.themes.list.iter().map(|th| choice(t, &theme_label(&th.name), th.name == p.theme, Pref::Theme(th.name.clone())))).spacing(6).wrap();
    let current = p.language();
    let languages: Vec<(String, String)> = lang::languages().iter().map(|l| (l.id.to_owned(), l.name.clone())).collect();
    let language = pick_list(languages.iter().find(|(id, _)| id == current).cloned(), languages, |(_, name)| name.clone())
        .on_select(|(id, _)| Message::Pref(Pref::Language(id)))
        .style(look::choice(t));
    let lengths = [(Length::Millimetre, "mm"), (Length::Metre, "m")].map(|(u, s)| choice(t, s, p.length == u, Pref::Length(u)));
    let frequencies = [(Frequency::Gigahertz, "GHz"), (Frequency::Megahertz, "MHz"), (Frequency::Hertz, "Hz")]
        .map(|(u, s)| choice(t, s, p.frequency == u, Pref::Frequency(u)));
    let units = column![
        row![text(tr(t!("Length"))).size(t.font_body).width(100)].extend(lengths).spacing(6).align_y(Alignment::Center),
        row![text(tr(t!("Frequency"))).size(t.font_body).width(100)].extend(frequencies).spacing(6).align_y(Alignment::Center),
    ]
    .spacing(6);
    let nav = p.nav();
    let mappings = row(NAV_PRESETS.iter().map(|m| {
        let label = if m.id == TOUCHPAD { tr(t!("Touchpad")) } else { m.label };
        choice(t, label, m.id == nav.id, Pref::Navigation(m.id.to_owned()))
    }))
    .spacing(6)
    .wrap();
    let l = p.gpu_look;
    let switches: [(Text, bool, fn(GpuLook) -> GpuLook); 4] = [
        (t!("Edges"), l.edges, |l| GpuLook { edges: !l.edges, ..l }),
        (t!("Shadows"), l.ao, |l| GpuLook { ao: !l.ao, ..l }),
        (t!("Outline"), l.outline, |l| GpuLook { outline: !l.outline, ..l }),
        (t!("GPU field"), l.field, |l| GpuLook { field: !l.field, ..l }),
    ];
    let drawing = row(switches.map(|(label, on, flip)| choice(t, tr(label), on, Pref::GpuLook(flip(l))))).spacing(6).wrap();
    let sections = [
        section(t, t!("Theme"), tr(t!("The colours of the window, the 3D view and the pictures. It changes at once.")), themes.boxed()),
        section(t, t!("Language"), tr(t!("The language of the window. It changes at once.")), language.boxed()),
        section(
            t,
            t!("New projects"),
            tr(t!(
                "The units new projects start with: what dialogs, tables and pictures show and commands read. An open project changes its own with Units in step 1."
            )),
            units.boxed(),
        ),
        section(t, t!("Mouse"), tr(nav.help), mappings.boxed()),
        section(t, t!("Drawing"), tr(t!("What the 3D view adds to the plain picture, each its own switch.")), drawing.boxed()),
    ];
    let page = Column::with_children(sections).spacing(24).padding(24).width(900);
    container(scrollable(page)).width(Fill).height(Fill).boxed()
}
