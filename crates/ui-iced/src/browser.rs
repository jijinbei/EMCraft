//! The browser: the model and its results as a tree at the window's left, the workflow's steps its
//! nodes ([`emcraft_ui_core::outline`] over [`emcraft_ui_core::browser`]'s rows; docs/adr/0009):
//! the kind of analysis at its head, each step's node its status's mark, number and name (a
//! click opens its panel), each node and section folding and unfolding (an empty one stays
//! folded), each row its glyph — a port its role's mark,
//! ⇄ the input whose reflection is S11, ➜ an output — its name and its detail dimmer (symbols
//! set as Unicode mathematics), a port's role in a pill of its colour, an eye that shows or hides
//! what it names (`view.visibility`), and ↻ on the results of a model changed since. A click
//! selects what a row names (its part in the 3D view too), takes the workflow to the step its
//! command belongs to, and shows the page it stands for or runs its command.

use emcraft_engine::workflow::Step;
use emcraft_ui_core::browser::{Act, Row};
use emcraft_ui_core::lang::{lookup, shown, tr};
use emcraft_ui_core::outline::{Head, Mark, Part};
use emcraft_ui_core::steps::{Solver, kinds};
use emcraft_ui_core::system::gesture::Command;
use iced::widget::{Column, Space, button, column, container, pick_list, row, scrollable, text};
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

/// The kind of analysis at the tree's head: the job's, marked "auto" while the model infers it,
/// "Choose the analysis" while a blank project has none; another chosen runs `project.analysis`.
fn kind(app: &App) -> Element<'_, Message> {
    let t = app.look.tokens;
    let shown = emcraft_ui_core::outline::shown_kind(app.session.model());
    let auto = shown.is_some_and(|(_, auto)| auto);
    let name = move |k: &Solver| {
        let n = tr(kinds::card(*k).name);
        if auto && shown.is_some_and(|(s, _)| s == *k) { format!("{n} ({})", tr(t!("auto"))) } else { n.to_owned() }
    };
    let list = pick_list(shown.map(|(k, _)| k), Solver::ALL, name)
        .placeholder(tr(t!("Choose the analysis")))
        .text_size(t.font_button)
        .width(Fill)
        .style(look::choice(t));
    let choose = |k: Solver| Message::Run(Command { id: "project.analysis", params: serde_json::json!({"kind": k.id()}) });
    if app.job.is_none() { list.on_select(choose).boxed() } else { list.boxed() }
}

/// A step's node: its fold, its status's mark, its number and name, how many rows it has; a
/// click opens its panel (again: closes it).
fn step_head<'a>(app: &App, t: Tokens, step: Step, part: &Part, folded: bool) -> Element<'a, Message> {
    let states = app.statuses();
    let mark = states.get(step.index()).map(|s| emcraft_ui_core::outline::mark(s, app.running(), app.session.model()));
    let (symbol, ink) = match mark {
        Some(Mark::Running) => ("◌", t.accent),
        Some(Mark::Skipped) => ("⊖", t.text_dim),
        Some(Mark::Unneeded(status) | Mark::Plain(status)) => crate::steps::mark(t, status),
        None => ("", t.text_dim),
    };
    let open = app.workflow.step == Some(step);
    let name_ink = match (open, mark.is_some_and(Mark::dim)) {
        (true, _) => t.selected_ink,
        (false, true) => t.text_dim,
        (false, false) => t.text,
    };
    let count = part.count();
    // A node with no rows has nothing to fold: its place kept, no arrow.
    let arrow = match (count, folded) {
        (0, _) => " ",
        (_, true) => "▸",
        (_, false) => "▾",
    };
    let fold = button(text(arrow).size(t.font_small).color(t.text_dim).width(10))
        .style(look::quiet(t))
        .padding([2, 4])
        .on_press_maybe((count > 0).then(|| Message::Fold(part.key().to_owned())));
    let face = row![
        text(symbol).size(t.font_body).color(ink).width(16),
        text(format!("{} {}", step.index(), shown(step))).size(t.font_body).color(name_ink),
        Space::new().width(Fill),
        text(if count > 0 { count.to_string() } else { String::new() }).size(t.font_small).color(t.text_dim)
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    let pick = button(face).width(Fill).padding([3, 6]).style(look::row(t, open)).on_press(Message::Step((!open).then_some(step)));
    row![fold, pick].spacing(0).align_y(Alignment::Center).boxed()
}

/// A section's heading of its own (the variables, a system's): its fold, name and count.
fn section_head<'a>(t: Tokens, part: &Part, name: &str, folded: bool) -> Element<'a, Message> {
    let count = part.count();
    let ink = if count == 0 { t.text_dim } else { t.text };
    let head = row![
        text(if folded { "▸" } else { "▾" }).size(t.font_small).color(t.text_dim),
        text(lookup(name).to_owned()).size(t.font_small).color(ink),
        Space::new().width(Fill),
        text(count.to_string()).size(t.font_small).color(t.text_dim)
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    button(head).style(look::quiet(t)).width(Fill).padding([2, 4]).on_press_maybe((count > 0).then(|| Message::Fold(part.key().to_owned()))).boxed()
}

/// The browser: the kind of analysis, then the tree — a node a step (its rows under it, the
/// geometry's bodies and materials each under their heading), the sections of their own above.
pub fn view(app: &App) -> Element<'_, Message> {
    let t = app.look.tokens;
    let stale = app.statuses().get(Step::Results.index()).is_some_and(|s| s.status == emcraft_engine::workflow::Status::Stale);
    let parts = app.tree.iter().flat_map(|(_, tree)| tree).map(|part| {
        let folded = part.count() == 0 || app.folded.iter().any(|f| f == part.key());
        let head = match part.head {
            Head::Step(step) => step_head(app, t, step, part, folded),
            Head::Section(name) => section_head(t, part, name, folded),
        };
        let headed = part.groups.len() > 1;
        let groups = part.groups.iter().filter(|(_, rows)| !rows.is_empty()).map(|(name, rows)| {
            let heading = headed.then(|| container(text(lookup(name).to_owned()).size(t.font_small).color(t.text_dim)).padding([2, 26]).boxed());
            let lines = rows.iter().map(|r| line(app, t, r, stale && *name == "Results"));
            Column::with_children(heading.into_iter().chain(lines)).spacing(1).boxed()
        });
        let body = (!folded).then(|| Column::with_children(groups).spacing(4).boxed());
        column![head].push(body.unwrap_or_else(|| Space::new().boxed())).spacing(2).boxed()
    });
    let list = Column::with_children(std::iter::once(kind(app)).chain(parts))
        .spacing(6)
        .padding(iced::Padding { top: 8.0, right: 18.0, bottom: 8.0, left: 8.0 })
        .width(Fill);
    // The scroll bar beside the rows, not over their eyes.
    container(scrollable(list)).width(WIDTH).height(Fill).style(look::ground(t)).boxed()
}
