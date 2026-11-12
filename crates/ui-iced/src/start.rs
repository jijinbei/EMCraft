//! The start page: New project and Open, the recent projects, and the templates by group, each a
//! card with its model drawn from the corner ([`emcraft_ui_core::start`]) — drawn off the window's
//! thread, one task a template, again in a new theme. A card opens its template.

use std::collections::BTreeMap;

use emcraft_engine::templates::{Group, TEMPLATES};
use emcraft_ui_core::files::{FileAction, Then};
use emcraft_ui_core::lang::{sample_label, tr};
use emcraft_ui_core::start::{TemplatePicture, group_title, template_picture, title_and_rest};
use emcraft_ui_core::theme::Theme;
use iced::widget::image::Handle;
use iced::widget::{Column, Space, button, column, container, image, row, scrollable, text};
use iced::{Element, Fill, Task, Widget};

use crate::app::{App, Message};
use crate::look;

/// A card's picture's size in points, and in pixels (twice that).
const CARD: [f32; 2] = [240.0, 150.0];
const PIXELS: [usize; 2] = [480, 300];

/// The templates' pictures: the theme they were drawn in, and each one's (or why it could not
/// be drawn).
pub type Thumbs = (String, BTreeMap<&'static str, Result<Handle, String>>);

/// Template `name` drawn in `theme`, on a thread of its own: its pixels, or why not.
pub(crate) fn drawn(name: &'static str, theme: Theme) -> Result<(u32, u32, Vec<u8>), String> {
    drawn_with(name, &serde_json::Value::Null, theme, PIXELS)
}

/// Template `name` built with `values` (its parameters' changed) and drawn in `theme` at
/// `pixels`: its pixels, or why not.
pub(crate) fn drawn_with(name: &str, values: &serde_json::Value, theme: Theme, pixels: [usize; 2]) -> Result<(u32, u32, Vec<u8>), String> {
    let canvas = match template_picture(name, values, pixels, &theme)? {
        TemplatePicture::Board(c) => c,
        TemplatePicture::Scene { scene, camera, ground } => {
            emcraft_gpu3d::scene_picture(&scene, &camera, [pixels[0] as u32, pixels[1] as u32], ground).map_err(|e| e.to_string())?
        }
    };
    Ok((canvas.w as u32, canvas.h as u32, canvas.rgba))
}

impl App {
    /// The templates' pictures drawn for the theme the window is in (none when they are): a task
    /// a template, each sending its picture back.
    pub(crate) fn draw_thumbs(&mut self) -> Task<Message> {
        let theme = (*self.look.theme).clone();
        if self.thumbs.0 == theme.name {
            return Task::none();
        }
        self.thumbs = (theme.name.clone(), BTreeMap::new());
        Task::batch(TEMPLATES.iter().map(|s| {
            let (name, theme) = (s.name, theme.clone());
            let work = async move {
                let (tx, rx) = iced::futures::channel::oneshot::channel();
                std::thread::spawn(move || {
                    let _ = tx.send(drawn(name, theme));
                });
                rx.await.unwrap_or_else(|_| Err("the drawing stopped".into()))
            };
            Task::perform(work, move |r| Message::Thumb(name, r))
        }))
    }
}

/// A template's card: its picture (a gap while drawing), its name and the line under it.
fn card<'a>(app: &'a App, name: &'static str) -> Element<'a, Message> {
    let t = app.look.tokens;
    let picture: Element<Message> = match app.thumbs.1.get(name) {
        Some(Ok(h)) => image(h.clone()).width(CARD[0]).height(CARD[1]).boxed(),
        _ => Space::new().width(CARD[0]).height(CARD[1]).boxed(),
    };
    let (title, rest) = title_and_rest(sample_label(name));
    let rest = if rest.is_empty() { name } else { rest };
    let body = column![picture, text(title.to_owned()).size(t.font_body), text(rest.to_owned()).size(t.font_small).color(t.text_dim)]
        .spacing(4)
        .width(CARD[0]);
    // Its card opens: its parameters and its model, before it is built.
    let open = Message::Template(crate::template_dialog::TemplateAct::Open(Some(name)));
    button(body).padding(8).style(look::row(t, false)).on_press(open).boxed()
}

/// The start page.
pub fn view(app: &App) -> Element<'_, Message> {
    let t = app.look.tokens;
    let big =
        |label, action| button(text(tr(label)).size(t.font_heading)).padding([10, 18]).style(look::button(t, false)).on_press(Message::File(action));
    let head = column![
        row![text("EMCraft").size(30), text(tr(t!("Start"))).size(30).color(t.text_dim)].spacing(12),
        row![
            big(t!("New project"), FileAction::Request(Then::New)),
            big(t!("Open…"), FileAction::Request(Then::Open)),
            big(
                if app.purpose == Some(emcraft_ui_core::steps::Solver::Pcb) { t!("Import PCB…") } else { t!("Import a SolidCraft design…") },
                FileAction::Request(Then::Import)
            )
        ]
        .spacing(10),
    ]
    .spacing(12);
    let recent = (!app.prefs.recent.is_empty()).then(|| {
        let paths = app.prefs.recent.iter().take(8).map(|p| {
            button(text(p.clone()).size(t.font_body))
                .style(look::row(t, false))
                .on_press(Message::File(FileAction::Request(Then::OpenPath(p.clone()))))
                .boxed()
        });
        column![text(tr(t!("Recent projects"))).size(t.font_heading), Column::with_children(paths).spacing(2)].spacing(6).boxed()
    });
    // What the job is for: a kind chosen shows its templates (again: every one), a blank
    // project for it beside them.
    let kinds = emcraft_ui_core::steps::Solver::ALL.into_iter().map(|k| {
        let on = app.purpose == Some(k);
        let face = row![
            crate::icons::view(emcraft_ui_core::steps::kinds::card(k).glyph, 22.0, t.icon, t.accent),
            column![
                text(tr(emcraft_ui_core::steps::kinds::card(k).name)).size(t.font_body),
                text(tr(emcraft_ui_core::steps::kinds::card(k).plain)).size(t.font_small).color(t.text_dim)
            ]
            .spacing(2)
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);
        button(face).padding(8).width(260).style(look::row(t, on)).on_press(Message::Purpose((!on).then_some(k))).boxed()
    });
    let purposes = column![text(tr(t!("Analysis"))).size(t.font_heading), row(kinds).spacing(10).wrap().vertical_spacing(10)].spacing(8);
    let shown = |s: &&emcraft_engine::templates::Template| app.purpose.is_none_or(|k| s.kinds.contains(&k));
    let groups = Group::ALL.into_iter().filter_map(|g| {
        let cards: Vec<Element<Message>> = TEMPLATES.iter().filter(|s| s.group == g).filter(shown).map(|s| card(app, s.name)).collect();
        (!cards.is_empty()).then(|| {
            column![text(tr(group_title(g))).size(t.font_body).color(t.form_dim), row(cards).spacing(10).wrap().vertical_spacing(10)]
                .spacing(6)
                .boxed()
        })
    });
    let blank = app.purpose.map(|_| {
        button(text(tr(t!("Blank"))).size(t.font_button))
            .style(look::button(t, false))
            .on_press(Message::File(FileAction::Request(Then::New)))
            .boxed()
    });
    let heading =
        row(std::iter::once(text(tr(t!("Templates"))).size(t.font_heading).boxed()).chain(blank)).spacing(12).align_y(iced::Alignment::Center);
    let templates = column![heading, Column::with_children(groups).spacing(18)].spacing(10);
    let page = Column::with_children([head.boxed()].into_iter().chain(recent).chain([purposes.boxed(), templates.boxed()])).spacing(24).padding(28);
    container(scrollable(page)).width(Fill).height(Fill).boxed()
}
