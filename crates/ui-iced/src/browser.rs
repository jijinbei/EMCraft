//! The browser: the model and its results as a tree at the window's left, the workflow's steps its
//! nodes ([`emcraft_ui_core::outline`] over [`emcraft_ui_core::browser`]'s rows; docs/adr/0009):
//! the kind of analysis at its head, each step's node its status's mark, number and name (a
//! click opens its panel), ▶ for what it runs and ＋ for what can be added to it (a list under
//! it: the commands that were the ribbon's, the 3D view's port tools), each node and section folding and unfolding (an empty one stays
//! folded), each row its glyph — a port its role's mark,
//! ⇄ the input whose reflection is S11, ➜ an output — its name and its detail dimmer (symbols
//! set as Unicode mathematics), a port's role in a pill of its colour, an eye that shows or hides
//! what it names (`view.visibility`), and ↻ on the results of a model changed since. A click
//! selects what a row names (its part in the 3D view too), takes the workflow to the step its
//! command belongs to, and shows the page it stands for or runs its command.

use emcraft_engine::workflow::Step;
use emcraft_ui_core::browser::{Act, Row};
use emcraft_ui_core::files::FileAction;
use emcraft_ui_core::glyph::Glyph;
use emcraft_ui_core::lang::{lookup, shown, tr};
use emcraft_ui_core::outline::{Add, Head, Mark, Part};
use emcraft_ui_core::steps::{Solver, kinds};
use emcraft_ui_core::system::gesture::Command;
use iced::widget::{Column, Space, button, container, pick_list, row, scrollable, text};
use iced::{Alignment, Color, Element, Fill, Widget};

use crate::app::{App, Message};
use crate::look::{self, Tokens};

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
    let words = Column::with_children(std::iter::once(name.boxed()).chain(detail)).spacing(t.space_2).width(Fill);
    let pill = r.role.as_ref().map(|role| {
        let colour = role_colour(app, role.input);
        container(text(emcraft_render::math::to_unicode(&role.text())).size(t.font_small).color(t.text))
            .padding([t.space_2, t.space_6])
            .style(look::pill(colour))
            .boxed()
    });
    let eye = r.eye.as_ref().map(|object| {
        let visible = app.session.views().visible(object);
        let params = serde_json::json!({"kind": object.kind(), "name": object.name(), "visible": !visible});
        let face = crate::icons::eye(visible, if visible { t.text } else { t.text_dim });
        button(face)
            .style(look::quiet(t))
            .padding(t.space_2)
            .on_press_maybe(app.job.is_none().then(|| Message::Run(emcraft_ui_core::system::gesture::Command { id: "view.visibility", params })))
            .boxed()
    });
    let stale_mark = stale.then(|| text("↻").size(t.font_body).color(t.info).boxed());
    // A design of the user's opens in SolidCraft.
    let open = match &r.act {
        Act::Design(path) => {
            let b = button(text("↗").size(t.font_body).color(t.accent))
                .style(look::quiet(t))
                .padding([0.0, t.space_4])
                .on_press(Message::OpenDesign(path.clone()));
            Some(crate::parts::hint_text(t, b.boxed(), tr(t!("Open in SolidCraft"))))
        }
        _ => None,
    };
    let content =
        row([container(mark).padding([0.0, 12.0 * f32::from(r.depth)]).boxed(), words.boxed()].into_iter().chain(pill).chain(stale_mark).chain(open))
            .spacing(t.space_6)
            .align_y(Alignment::Center);
    let pick = button(content).width(Fill).padding([t.space_4, t.space_6]).style(look::row(t, selected)).on_press(Message::Row(r.id.clone()));
    row(std::iter::once(pick.boxed()).chain(eye)).spacing(t.space_2).align_y(Alignment::Center).boxed()
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
        .padding([t.space_2, t.space_4])
        .on_press_maybe((count > 0).then(|| Message::Fold(part.key().to_owned())));
    let face = row![
        text(symbol).size(t.font_body).color(ink).width(16),
        text(format!("{} {}", step.index(), shown(step))).size(t.font_body).color(name_ink),
        Space::new().width(Fill),
        text(if count > 0 { count.to_string() } else { String::new() }).size(t.font_small).color(t.text_dim)
    ]
    .spacing(t.space_6)
    .align_y(Alignment::Center);
    let pick = button(face).width(Fill).padding([t.space_4, t.space_6]).style(look::row(t, open)).on_press(Message::Step((!open).then_some(step)));
    row([fold.boxed(), pick.boxed()].into_iter().chain(ends(app, t, part))).spacing(t.space_2).align_y(Alignment::Center).boxed()
}

/// A node's buttons at its end: ▶ for what its step runs (the mesh, the run), ＋ for what can be
/// added to it.
fn ends<'a>(app: &App, t: Tokens, part: &Part) -> Vec<Element<'a, Message>> {
    let idle = app.job.is_none();
    let small = |label: &'static str, on: bool, message: Option<Message>| {
        // The accent's mark while it can be used and is off; else the style's ink (the
        // selection's when on, the dim one when it cannot be used).
        let mark = text(label).size(t.font_body);
        let mark = if message.is_some() && !on { mark.color(t.accent) } else { mark };
        button(mark).style(look::toggle(t, on)).padding([t.space_2, t.space_6]).on_press_maybe(message).boxed()
    };
    let play = match part.head {
        Head::Step(step) => {
            emcraft_ui_core::outline::play(step).map(|id| crate::search::command_tip(app, small("▶", false, idle.then(|| play_message(app, id))), id))
        }
        Head::Section(_) => None,
    };
    let open = app.adding == Some(part.key());
    let offers = !emcraft_ui_core::outline::adds(part.head, app.solver()).is_empty();
    let add = offers.then(|| small("＋", open, Some(Message::Add((!open).then_some(part.key())))));
    play.into_iter().chain(add).collect()
}

/// What a step's ▶ sends for command `id`: the confirmed run through Run's preflight, any other
/// command run with what its dialog would start with (the mesh: six elements per wavelength at
/// the band's top, as its form offers; with nothing it was refused for want of a size).
pub(crate) fn play_message(app: &App, id: &'static str) -> Message {
    if id == "solve.run" {
        Message::StepAct(crate::step_panel::Act::Run)
    } else {
        Message::Run(emcraft_ui_core::system::gesture::Command { id, params: emcraft_ui_core::dialogs::initial(id, &app.session) })
    }
}

/// What an entry of a ＋ list sends: a command's dialog (or its run), a tool turned on, a new
/// SolidCraft design.
pub(crate) fn add_message(a: Add) -> Message {
    match a {
        Add::Command(id) => Message::Command(id.to_owned()),
        Add::Tool(k) => Message::Tool(Some(k)),
        Add::NewDesign => Message::File(FileAction::NewDesign),
    }
}

/// A node's ＋ list, while it is shown: a new SolidCraft design (the geometry's), its commands (a
/// click opens its dialog in the properties panel, or runs it) and the 3D view's tools that add
/// to it.
fn add_list<'a>(app: &App, t: Tokens, part: &Part) -> Option<Element<'a, Message>> {
    if app.adding != Some(part.key()) {
        return None;
    }
    let idle = app.job.is_none();
    let entry = |glyph, label: String, message: Message| {
        let face =
            row![crate::icons::view(glyph, 16.0, t.icon, t.accent), text(label).size(t.font_body)].spacing(t.space_8).align_y(Alignment::Center);
        button(face).width(Fill).padding([t.space_4, t.space_8]).style(look::row(t, false)).on_press_maybe(idle.then_some(message))
    };
    let entries = emcraft_ui_core::outline::adds(part.head, app.solver()).into_iter().map(|a| match a {
        Add::Command(id) => {
            let b = entry(emcraft_ui_core::commands::glyph(id), emcraft_ui_core::commands::button_label(id).to_owned(), add_message(a));
            crate::search::command_tip(app, b.boxed(), id)
        }
        Add::Tool(k) => crate::search::busy(app, entry(k.glyph(), lookup(k.label()).to_owned(), add_message(a)).boxed()),
        Add::NewDesign => crate::search::busy(app, entry(Glyph::Import, tr(t!("New SolidCraft design")).to_owned(), add_message(a)).boxed()),
    });
    let list = Column::with_children(entries).spacing(t.space_2);
    Some(container(list).padding(iced::Padding { top: 2.0, right: 0.0, bottom: 4.0, left: 22.0 }).boxed())
}

/// A group's last row, dim: ＋ and what command `id` adds (its dialog opens in the properties
/// panel).
fn ghost<'a>(app: &App, t: Tokens, id: &'static str) -> Element<'a, Message> {
    let said = format!("＋ {}…", emcraft_ui_core::commands::button_label(id));
    let face = text(said).size(t.font_small).color(t.text_dim);
    let b = button(face)
        .width(Fill)
        .padding(iced::Padding { top: t.space_2, right: t.space_6, bottom: t.space_2, left: t.space_24 + t.space_6 })
        .style(look::row(t, false))
        .on_press_maybe(app.job.is_none().then(|| add_message(Add::Command(id))));
    crate::search::command_tip(app, b.boxed(), id)
}

/// A section's heading of its own (the variables, a system's): its fold, name and count.
fn section_head<'a>(app: &App, t: Tokens, part: &Part, name: &str, folded: bool) -> Element<'a, Message> {
    let count = part.count();
    let ink = if count == 0 { t.text_dim } else { t.text };
    let head = row![
        text(if folded { "▸" } else { "▾" }).size(t.font_small).color(t.text_dim),
        text(lookup(name).to_owned()).size(t.font_small).color(ink),
        Space::new().width(Fill),
        text(count.to_string()).size(t.font_small).color(t.text_dim)
    ]
    .spacing(t.space_6)
    .align_y(Alignment::Center);
    let head = button(head)
        .style(look::quiet(t))
        .width(Fill)
        .padding([t.space_2, t.space_4])
        .on_press_maybe((count > 0).then(|| Message::Fold(part.key().to_owned())));
    row(std::iter::once(head.boxed()).chain(ends(app, t, part))).spacing(t.space_2).align_y(Alignment::Center).boxed()
}

/// The browser: the kind of analysis, then the tree — a node a step (its rows under it, the
/// geometry's bodies and materials each under their heading), the sections of their own above.
pub fn view(app: &App, width: f32) -> Element<'_, Message> {
    let t = app.look.tokens;
    let stale = app.statuses().get(Step::Results.index()).is_some_and(|s| s.status == emcraft_engine::workflow::Status::Stale);
    let parts = app.tree.iter().flat_map(|(_, tree)| tree).map(|part| {
        let folded = part.count() > 0 && app.folded.iter().any(|f| f == part.key());
        let head = match part.head {
            Head::Step(step) => step_head(app, t, step, part, folded),
            Head::Section(name) => section_head(app, t, part, name, folded),
        };
        let headed = part.groups.len() > 1;
        let kind = app.solver();
        let shown = |(name, rows): &&(&str, Vec<Row>)| !rows.is_empty() || emcraft_ui_core::outline::ghost(name, kind).is_some();
        let groups = part.groups.iter().filter(shown).map(|(name, rows)| {
            let heading = headed
                .then(|| container(text(lookup(name).to_owned()).size(t.font_small).color(t.text_dim)).padding([t.space_2, t.space_24]).boxed());
            let lines = rows.iter().map(|r| line(app, t, r, stale && *name == "Results"));
            let more = emcraft_ui_core::outline::ghost(name, kind).map(|id| ghost(app, t, id));
            Column::with_children(heading.into_iter().chain(lines).chain(more)).spacing(t.space_2).boxed()
        });
        let body = (!folded).then(|| Column::with_children(groups).spacing(t.space_4).boxed());
        Column::with_children([head].into_iter().chain(add_list(app, t, part)).chain(body)).spacing(t.space_2).boxed()
    });
    let list = Column::with_children(std::iter::once(kind(app)).chain(parts))
        .spacing(t.space_6)
        .padding(iced::Padding { top: 8.0, right: 18.0, bottom: 8.0, left: 8.0 })
        .width(Fill);
    // The scroll bar beside the rows, not over their eyes.
    container(scrollable(list)).width(width).height(Fill).style(look::ground(t)).boxed()
}
