//! The browser: the model and its results as a tree beside the 3D view ([`emcraft_ui_core::browser`]
//! reads it from the session): each section a heading with its count that
//! folds and unfolds (an empty one stays folded), each row its glyph — a port its role's mark,
//! ⇄ the input whose reflection is S11, ➜ an output — its name and its detail dimmer (symbols
//! set as Unicode mathematics), a port's role in a pill of its colour, an eye that shows or hides
//! what it names (`view.visibility`), and ↻ on the results of a model changed since. A click
//! selects what a row names (its part in the 3D view too), takes the workflow to the step its
//! command belongs to, and shows the page it stands for or runs its command.

use emcraft_ui_core::browser::{Act, Row};
use emcraft_ui_core::lang::lookup;
use iced::widget::{Column, Space, button, column, container, row, scrollable, text};
use iced::{Alignment, Color, Element, Fill, Widget};

use crate::app::{App, Message};
use crate::look::{self, Tokens};

/// The browser's width (points).
const WIDTH: f32 = 290.0;

/// The step a row's command belongs to.
pub fn step_of(r: &Row) -> Option<emcraft_engine::workflow::Step> {
    let command = match (&r.item, &r.act) {
        (Some(item), _) => item.edit,
        (None, Act::Edit(id) | Act::EditWith(id, _)) => id,
        _ => return None,
    };
    emcraft_engine::workflow::Step::of_command(command)
}

/// A port role's colour.
fn role_colour(app: &App, input: bool) -> Color {
    let sc = &app.look.theme.scene;
    look::rgb(if input { sc.port_input } else { sc.port_output })
}

/// A row: its mark, its name and detail, its role's pill, its eye, ↻ when out of date.
fn line<'a>(app: &'a App, t: Tokens, r: &'a Row, stale: bool) -> Element<'a, Message> {
    let selected = app.row_selected(r);
    let ink = if selected {
        t.selected_ink
    } else if stale {
        t.text_dim
    } else {
        t.text
    };
    let mark: Element<Message> = match &r.role {
        Some(role) => text(if role.input { "⇄" } else { "➜" }).size(t.font_body).color(role_colour(app, role.input)).width(18).boxed(),
        None => crate::icons::view(r.glyph, 16.0, t.icon, t.accent),
    };
    let name = text(emcraft_render::math::to_unicode(&emcraft_ui_core::math::texify(&r.shown))).size(t.font_body).color(ink);
    let detail = (!r.detail.is_empty()).then(|| text(emcraft_render::math::to_unicode(&r.detail_tex())).size(t.font_small).color(t.text_dim).boxed());
    let words = Column::with_children(std::iter::once(name.boxed()).chain(detail)).spacing(1).width(Fill);
    let pill = r.role.as_ref().map(|role| {
        let colour = role_colour(app, role.input);
        container(text(emcraft_render::math::to_unicode(&role.text())).size(t.font_small).color(t.text))
            .padding([1, 6])
            .style(move |_: &iced::Theme| container::Style {
                background: Some(Color { a: 0.2, ..colour }.into()),
                border: iced::Border { color: colour, width: 1.0, radius: 8.0.into() },
                ..container::Style::default()
            })
            .boxed()
    });
    let eye = r.eye.as_ref().map(|object| {
        let visible = app.session.views().visible(object);
        let params = serde_json::json!({"kind": object.kind(), "name": object.name(), "visible": !visible});
        let face = crate::icons::eye(visible, if visible { t.text } else { t.text_dim });
        button(face)
            .style(look::quiet(t))
            .padding(2)
            .on_press_maybe(app.job.is_none().then(|| Message::Run(emcraft_ui_core::system::gesture::Command { id: "view.visibility", params })))
            .boxed()
    });
    let stale_mark = stale.then(|| text("↻").size(t.font_body).color(t.info).boxed());
    let content = row([container(mark).padding([0.0, 12.0 * f32::from(r.depth)]).boxed(), words.boxed()].into_iter().chain(pill).chain(stale_mark))
        .spacing(6)
        .align_y(Alignment::Center);
    let pick = button(content).width(Fill).padding([3, 6]).style(look::row(t, selected)).on_press(Message::Row(r.id.clone()));
    row(std::iter::once(pick.boxed()).chain(eye)).spacing(2).align_y(Alignment::Center).boxed()
}

/// The browser's card.
pub fn view(app: &App) -> Element<'_, Message> {
    let t = app.look.tokens;
    let stale =
        app.statuses().get(emcraft_engine::workflow::Step::Results.index()).is_some_and(|s| s.status == emcraft_engine::workflow::Status::Stale);
    let sections = app.tree.iter().flat_map(|(_, tree)| tree).map(|(section, rows)| {
        let folded = rows.is_empty() || app.folded.iter().any(|f| f == section);
        let ink = if rows.is_empty() { t.text_dim } else { t.text };
        let head = row![
            text(if folded { "▸" } else { "▾" }).size(t.font_small).color(t.text_dim),
            text(lookup(section).to_owned()).size(t.font_small).color(ink),
            Space::new().width(Fill),
            text(rows.len().to_string()).size(t.font_small).color(t.text_dim)
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        let head = button(head)
            .style(look::quiet(t))
            .width(Fill)
            .padding([2, 4])
            .on_press_maybe((!rows.is_empty()).then(|| Message::Fold((*section).to_owned())));
        let rows = (!folded).then(|| Column::with_children(rows.iter().map(|r| line(app, t, r, stale && *section == "Results"))).spacing(1).boxed());
        column![head].push(rows.unwrap_or_else(|| Space::new().boxed())).spacing(2).boxed()
    });
    let list = Column::with_children(sections).spacing(8).padding(8).width(Fill);
    container(scrollable(list)).width(WIDTH).height(Fill).style(look::ground(t)).boxed()
}
