//! The settings page: the theme, the language, the units new projects start with, and the 3D
//! view's mouse mapping and drawing — each a choice that changes the window at once and is kept
//! in the preferences both windows share ([`emcraft_ui_core::prefs::Prefs::changed`]).

use emcraft_engine::theme::Base;
use emcraft_model::units::{Frequency, Length};
use emcraft_ui_core::action::Pref;
use emcraft_ui_core::lang::{self, Text, lookup, tr};
use emcraft_ui_core::navigation::{NAV_PRESETS, TOUCHPAD};
use emcraft_ui_core::prefs::GpuLook;
use iced::widget::{Column, button, column, container, pick_list, row, scrollable, text, text_input};

/// The settings' column's width (points): a line of text stays readable.
const READING_WIDTH: f32 = 900.0;
use iced::{Alignment, Element, Fill, Widget};

use crate::app::{App, Message};
use crate::look::{self, Tokens};

/// A choice among others: lit when it is the one kept.
/// A row of choices under its title.
fn titled<'a>(t: Tokens, title: Text, items: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    row![text(tr(title)).size(t.font_body).width(t.form_label_width)].extend(items).spacing(t.space_6).align_y(Alignment::Center).wrap().boxed()
}

fn choice<'a>(t: Tokens, label: &str, on: bool, pref: Pref) -> Element<'a, Message> {
    button(text(label.to_owned()).size(t.font_button)).style(look::toggle(t, on)).on_press(Message::Pref(pref)).boxed()
}

/// A section: its heading, what it is about, its choices.
fn section<'a>(t: Tokens, title: Text, about: &str, body: Element<'a, Message>) -> Element<'a, Message> {
    column![text(tr(title)).size(t.font_heading), text(about.to_owned()).size(t.font_small).color(t.text_dim), body].spacing(t.space_6).boxed()
}

/// A theme's name as its button reads: a built-in one capitalised, in the window's language.
fn theme_label(name: &str) -> String {
    match name.chars().next() {
        Some(first) if Base::parse(name).is_some() => lookup(&first.to_uppercase().chain(name.chars().skip(1)).collect::<String>()).to_owned(),
        _ => name.to_owned(),
    }
}

/// The settings page.
/// The font the language is shown with, or why it is not; a font file to choose (committed on
/// Enter), and the fonts looked for again.
fn font(app: &App) -> Element<'_, Message> {
    use emcraft_ui_core::fonts::State;
    use emcraft_ui_core::lang::{args, trf};
    let t = app.look.tokens;
    let standing = match &app.font {
        State::Found(family) => Some(text(trf(t!("Font: {family}"), args!(family = family))).size(t.font_small).color(t.text_dim)),
        State::Missing(n) => {
            let wanted = app.prefs.language();
            let name = lang::find(wanted).map_or(wanted, |l| l.name.as_str());
            let said = trf(
                t!(
                    "No font shows {language} ({n} characters missing): the window stays in English. Install one (Linux: fonts-noto-cjk) or choose a font file."
                ),
                args!(language = name, n = n),
            );
            Some(text(said).size(t.font_small).color(t.error))
        }
        State::NotNeeded => None,
    };
    let file = app.drafts_font.clone().unwrap_or_else(|| app.prefs.font_file.clone().unwrap_or_default());
    let committed = Message::Pref(Pref::FontFile(Some(file.trim().to_owned()).filter(|f| !f.is_empty())));
    let typed = text_input(tr(t!("the system's fonts")), file)
        .size(t.font_body)
        .width(320)
        .padding([t.space_4, t.space_6])
        .style(look::field(t))
        .on_input(Message::FontFile)
        .on_submit(committed);
    let row = row![
        text(tr(t!("Font file"))).size(t.font_small).color(t.text_dim),
        typed,
        button(text(tr(t!("Look again"))).size(t.font_button)).style(look::button(t, false)).on_press(Message::Pref(Pref::SearchFonts))
    ]
    .spacing(t.space_8)
    .align_y(Alignment::Center);
    Column::with_children(standing.map(Widget::boxed).into_iter().chain(std::iter::once(row.boxed()))).spacing(t.space_4).boxed()
}

/// The crash reports' buttons: the card of them, and a report of the session as it is.
/// The SolidCraft program designs open in, set as it is typed.
fn solidcraft(app: &App) -> Element<'_, Message> {
    let t = app.look.tokens;
    let typed = text_input("solidcraft", &app.prefs.solidcraft)
        .size(t.font_body)
        .width(420)
        .padding([t.space_4, t.space_6])
        .style(look::field(t))
        .on_input(|s| Message::Pref(Pref::SolidCraft(s)));
    row![text(tr(t!("SolidCraft program"))).size(t.font_small).color(t.text_dim), typed].spacing(t.space_8).align_y(Alignment::Center).boxed()
}

fn reports(app: &App) -> Element<'_, Message> {
    use crate::record::ReportAct;
    let t = app.look.tokens;
    let on = app.crashes.dir.is_some();
    let b = |label, a| {
        button(text(tr(label)).size(t.font_button)).style(look::button(t, false)).on_press_maybe(on.then_some(Message::Reports(a))).boxed()
    };
    row([b(t!("Crash reports…"), ReportAct::Show(true)), b(t!("Report a bug…"), ReportAct::Bug)]).spacing(t.space_8).boxed()
}

pub fn view(app: &App) -> Element<'_, Message> {
    let t = app.look.tokens;
    let p = &app.prefs;
    let themes = row(app.themes.list.iter().map(|th| choice(t, &theme_label(&th.name), th.name == p.theme, Pref::Theme(th.name.clone()))))
        .spacing(t.space_6)
        .wrap();
    let current = p.language();
    let languages: Vec<(String, String)> = lang::languages().iter().map(|l| (l.id.to_owned(), l.name.clone())).collect();
    let language = pick_list(languages.iter().find(|(id, _)| id == current).cloned(), languages, |(_, name)| name.clone())
        .on_select(|(id, _)| Message::Pref(Pref::Language(id)))
        .text_size(t.font_button)
        .style(look::choice(t));
    let lengths = [(Length::Millimetre, "mm"), (Length::Metre, "m")].map(|(u, s)| choice(t, s, p.length == u, Pref::Length(u)));
    let frequencies = [(Frequency::Gigahertz, "GHz"), (Frequency::Megahertz, "MHz"), (Frequency::Hertz, "Hz")]
        .map(|(u, s)| choice(t, s, p.frequency == u, Pref::Frequency(u)));
    let units = column![
        row![text(tr(t!("Length"))).size(t.font_body).width(t.form_label_width)].extend(lengths).spacing(t.space_6).align_y(Alignment::Center),
        row![text(tr(t!("Frequency"))).size(t.font_body).width(t.form_label_width)].extend(frequencies).spacing(t.space_6).align_y(Alignment::Center),
    ]
    .spacing(t.space_6);
    let nav = p.nav();
    let mappings = row(NAV_PRESETS.iter().map(|m| {
        let label = if m.id == TOUCHPAD { tr(t!("Touchpad")) } else { m.label };
        choice(t, label, m.id == nav.id, Pref::Navigation(m.id.to_owned()))
    }))
    .spacing(t.space_6)
    .wrap();
    // What an orbit turns about, and the orientation control in the view's corner.
    use emcraft_ui_core::navigation::{Navigator, OrbitAround};
    let around = OrbitAround::ALL.iter().map(|o| choice(t, lookup(o.label()), *o == p.orbit_around(), Pref::OrbitAround(*o))).collect();
    let navigators = Navigator::ALL.iter().map(|n| choice(t, lookup(n.label()), *n == p.navigator(), Pref::Navigator(*n))).collect();
    let mouse = column![
        mappings,
        text(tr(t!(
            "Every mapping: Ctrl+left drag orbits, Shift snaps an orbit, the wheel zooms to the cursor, arrows turn, Ctrl+1–7 standard views."
        )))
        .size(t.font_small)
        .color(t.text_dim),
        titled(t, t!("Orbit around"), around),
        titled(t, t!("Navigator"), navigators),
    ]
    .spacing(t.space_8);
    let l = p.gpu_look;
    let switches: [(Text, bool, fn(GpuLook) -> GpuLook); 4] = [
        (t!("Edges"), l.edges, |l| GpuLook { edges: !l.edges, ..l }),
        (t!("Shadows"), l.ao, |l| GpuLook { ao: !l.ao, ..l }),
        (t!("Outline"), l.outline, |l| GpuLook { outline: !l.outline, ..l }),
        (t!("GPU field"), l.field, |l| GpuLook { field: !l.field, ..l }),
    ];
    let drawing = row(switches.map(|(label, on, flip)| choice(t, tr(label), on, Pref::GpuLook(flip(l))))).spacing(t.space_6).wrap();
    let sections = [
        section(t, t!("Theme"), tr(t!("Window, 3D view and pictures; it changes at once.")), themes.boxed()),
        section(
            t,
            t!("Language"),
            tr(t!("The language of the window. It changes at once.")),
            column![language, font(app)].spacing(t.space_6).boxed(),
        ),
        section(
            t,
            t!("New projects"),
            tr(t!(
                "The units new projects start with: what dialogs, tables and pictures show and commands read. An open project changes its own with Units in step 1."
            )),
            units.boxed(),
        ),
        section(t, t!("Mouse"), tr(nav.help), mouse.boxed()),
        section(t, t!("Drawing"), tr(t!("What the 3D view adds to the plain picture, each its own switch.")), drawing.boxed()),
        section(t, t!("SolidCraft"), tr(t!("The program designs open in (empty: solidcraft on the PATH).")), solidcraft(app)),
        section(t, t!("Crash reports"), tr(t!("Kept on this computer only. Copy one to report it; nothing is sent.")), reports(app)),
    ];
    // The settings at most a reading width wide; the scroll bar at the window's edge.
    let page = Column::with_children(sections).spacing(t.space_24).padding(t.space_24).width(READING_WIDTH);
    scrollable(container(page).width(Fill)).width(Fill).height(Fill).boxed()
}
