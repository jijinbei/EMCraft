//! The model page: a bar of what the 3D view puts forward, its fit and standard views, and the
//! commands that mesh and solve the model; under it the 3D view ([`crate::view3d`]) and its
//! labels and legend. A solve runs on the worker ([`crate::run`]); its progress shows in the window's bar.

use emcraft_render::view3d::StandardView;
use emcraft_ui_core::lang::{args, tr, trf};
use emcraft_ui_core::scene3d::Overlay;
use iced::widget::{Space, canvas, column, container, row, shader, stack, text};
use iced::{Alignment, Element, Fill, Widget};

use crate::app::{App, Message};
use crate::look::{self, Tokens};
use crate::view3d::labels::Labels;
use crate::view3d::{Nav, shader::Viewer};

/// What the view can put forward, and each one's name.
const OVERLAYS: [(Overlay, emcraft_ui_core::lang::Text); 5] = [
    (Overlay::Plain, t!("Geometry")),
    (Overlay::Materials, t!("Materials")),
    (Overlay::Ports, t!("Ports")),
    (Overlay::Boundaries, t!("Boundaries")),
    (Overlay::Mesh, t!("Mesh")),
];

/// The standard views offered, and each one's name.
const VIEWS: [(StandardView, emcraft_ui_core::lang::Text); 4] =
    [(StandardView::Iso, t!("Iso")), (StandardView::Front, t!("Front")), (StandardView::Top, t!("Top")), (StandardView::Right, t!("Right"))];

/// The commands the bar runs, and each one's name: the mesh made, the confirmed job run.
const COMMANDS: [(&str, emcraft_ui_core::lang::Text); 2] = [("mesh.generate", t!("Generate mesh")), ("solve.run", t!("Run"))];

/// A button of the bar: `label`, lit when `on`, sending `message` (none: greyed).
fn tool<'a>(t: Tokens, label: &str, on: bool, message: Option<Message>) -> Element<'a, Message> {
    iced::widget::button(text(label.to_owned()).size(t.font_button)).style(look::button(t, on)).on_press_maybe(message).boxed()
}

/// The bar over the view.
fn bar(app: &App) -> Element<'_, Message> {
    let t = app.look.tokens;
    let overlay = app.view3d.overlay;
    let overlays = OVERLAYS.iter().map(|(o, label)| tool(t, tr(*label), *o == overlay, Some(Message::View3d(Nav::Overlay(*o)))));
    let views = std::iter::once(tool(t, tr(t!("Fit")), false, Some(Message::View3d(Nav::Fit))))
        .chain(VIEWS.iter().map(|(v, label)| tool(t, tr(*label), false, Some(Message::View3d(Nav::Standard(*v))))));
    let idle = app.job.is_none();
    let commands = COMMANDS.iter().map(|(id, label)| tool(t, tr(*label), *id == "solve.run", idle.then(|| Message::Start((*id).to_owned()))));
    let gap = || Space::new().width(16).boxed();
    row(overlays.chain(std::iter::once(gap())).chain(views).chain(std::iter::once(gap())).chain(commands))
        .spacing(6)
        .align_y(Alignment::Center)
        .boxed()
}

/// The legend over the view's corner: a swatch and a name each.
fn legend(app: &App) -> Element<'_, Message> {
    let t = app.look.tokens;
    if app.view3d.shown.as_ref().is_none_or(|s| s.legend.is_empty()) {
        return Space::new().boxed();
    }
    let entries = app.view3d.shown.iter().flat_map(|s| &s.legend).map(|(name, c)| {
        let swatch = container(Space::new().width(12).height(12))
            .style(move |_: &iced::Theme| container::Style { background: Some(look::rgb(*c).into()), ..container::Style::default() });
        row![swatch, text(name.clone()).size(t.font_small).color(t.text)].spacing(6).align_y(Alignment::Center).boxed()
    });
    let list = column(entries).spacing(4);
    container(container(list).padding(8).style(look::ground(t))).padding(12).boxed()
}

/// What is selected, on a card at the top of the view: its name and detail, or how many parts.
fn card(app: &App) -> Element<'_, Message> {
    let t = app.look.tokens;
    let rows = app.picked.as_ref().map_or(&[][..], |(_, rows)| rows.as_slice());
    let line = match (rows, app.selection.len()) {
        (_, 0) => return Space::new().boxed(),
        ([r], 1) if r.detail.is_empty() => r.shown.clone(),
        ([r], 1) => format!("{}  ·  {}", r.shown, r.detail),
        (_, n) => trf(t!("{n} selected"), args!(n = n)),
    };
    let card = container(text(line).size(t.font_body).color(t.text)).padding([6, 12]).style(look::ground(t));
    container(card).padding(12).center_x(Fill).boxed()
}

/// The model page.
pub fn view(app: &App) -> Element<'_, Message> {
    let t = app.look.tokens;
    let picture: Element<Message> = match app.view3d.shown.as_ref().filter(|s| s.bounds.is_some()) {
        Some(s) => {
            let viewer = Viewer { shot: app.shot(s), nav: app.nav };
            let labels = Labels { labels: &s.labels, camera: app.view3d.camera, tokens: t };
            stack![shader(viewer).width(Fill).height(Fill), canvas(labels).width(Fill).height(Fill), legend(app), card(app)].boxed()
        }
        None => container(text(tr(t!("No geometry yet: open a sample or start at step 1, Geometry."))).size(t.font_heading).color(t.text_dim))
            .center(Fill)
            .boxed(),
    };
    column![bar(app), picture].spacing(8).padding(12).height(Fill).boxed()
}
