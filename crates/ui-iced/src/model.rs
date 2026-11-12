//! The model page: a bar of what the 3D view puts forward, its fit and standard views, and the
//! commands that mesh and solve the model; under it the 3D view ([`crate::view3d`]) and its
//! labels and legend. A solve runs on the worker ([`crate::run`]); its progress shows in the window's bar.

use emcraft_engine::view::{Component, Plane};
use emcraft_render::view3d::StandardView;
use emcraft_ui_core::handle::AXES;
use emcraft_ui_core::lang::{args, lookup, tr, trf};
use emcraft_ui_core::scene3d::{Cut, Overlay};
use iced::widget::{Space, canvas, column, container, image, pick_list, row, shader, slider, stack, text};
use iced::{Alignment, Element, Fill, Widget};

use crate::app::{App, Message};
use crate::look::{self, Tokens};
use crate::view3d::labels::Labels;
use crate::view3d::{Nav, field, shader::Viewer};

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
    // The field once there is a sweep to show it from.
    let solved = app.session.results().sweep.is_some();
    let offered = OVERLAYS.iter().copied().chain(solved.then_some((Overlay::Field, t!("Field"))));
    let overlays = offered.map(|(o, label)| tool(t, tr(label), o == overlay, Some(Message::View3d(Nav::Overlay(o)))));
    let section = app.view3d.section;
    let cut = app.view3d.bounds().map(|b| Cut::middle(0, b));
    let sectioning = tool(t, tr(t!("Section view")), section.is_some(), Some(Message::View3d(Nav::Section(section.map_or(cut, |_| None)))));
    let views = std::iter::once(tool(t, tr(t!("Fit")), false, Some(Message::View3d(Nav::Fit))))
        .chain(std::iter::once(sectioning))
        .chain(VIEWS.iter().map(|(v, label)| tool(t, tr(*label), false, Some(Message::View3d(Nav::Standard(*v))))));
    let idle = app.job.is_none();
    let commands = COMMANDS.iter().map(|(id, label)| tool(t, tr(*label), *id == "solve.run", idle.then(|| Message::Start((*id).to_owned()))));
    let gap = || Space::new().width(16).boxed();
    row(overlays.chain(std::iter::once(gap())).chain(views).chain(std::iter::once(gap())).chain(commands))
        .spacing(6)
        .align_y(Alignment::Center)
        .boxed()
}

/// The field overlay's choices: the plane, the component, the solved frequency and where the
/// plane lies across the model.
fn field_bar(app: &App) -> Element<'_, Message> {
    let t = app.look.tokens;
    let v = &app.view3d.field;
    let nav = |n: Nav| Message::View3d(n);
    let fu = app.session.display().frequency;
    let label = |s: &str| text(s.to_owned()).size(t.font_body).color(t.form_dim);
    let planes = pick_list(Some(v.plane), Plane::ALL, |p| lookup(p.label()).to_owned()).on_select(move |p| nav(Nav::Plane(p)));
    let components = pick_list(Some(v.component), Component::ALL, |c| c.label().to_owned()).on_select(move |c| nav(Nav::Component(c)));
    let solved: Vec<f64> = app.session.results().sweep.iter().flat_map(|s| s.results.iter().map(|r| r.frequency)).collect();
    let nearest = solved.iter().copied().min_by(|a, b| (a - v.frequency).abs().total_cmp(&(b - v.frequency).abs()));
    let frequencies =
        pick_list(nearest, solved, move |f| format!("{:.4} {}", fu.from_si(*f), fu.symbol())).on_select(move |f| nav(Nav::Frequency(f)));
    let span = app.view3d.bounds().map_or(0.0, |(lo, hi)| {
        let k = field::normal(v.plane);
        hi[k] - lo[k]
    });
    let at = v.at.unwrap_or(span / 2.0).clamp(0.0, span);
    let place = slider(0.0..=span.max(f64::MIN_POSITIVE), at, move |x| nav(Nav::At(x))).step(span / 200.0).width(200);
    let l = app.session.display().length;
    let shown_at = text(format!("{:.3} {}", l.from_si(at), l.symbol())).size(t.font_body).color(t.text_dim);
    // The phase ωt in degrees, and its playing.
    let playing = app.playing.is_some();
    let play = tool(t, tr(if playing { t!("Pause") } else { t!("Play") }), playing, Some(Message::Play(!playing)));
    let degrees = v.phase.to_degrees();
    let phase = slider(0.0..=360.0, degrees, move |d: f64| nav(Nav::Phase(d.to_radians()))).step(1.0).width(160);
    let shown_phase = text(format!("ωt {degrees:.0}°")).size(t.font_body).color(t.text_dim);
    let (e, h) = (v.show_e, v.show_h);
    let e_arrows = tool(t, tr(t!("E arrows")), e, Some(nav(Nav::Arrows(!e, h))));
    let h_arrows = tool(t, tr(t!("H arrows")), h, Some(nav(Nav::Arrows(e, !h))));
    row![
        label(tr(t!("Plane"))),
        planes.style(look::choice(t)),
        label(tr(t!("Component"))),
        components.style(look::choice(t)),
        label(tr(t!("Frequency"))),
        frequencies.style(look::choice(t)),
        place,
        shown_at,
        Space::new().width(16),
        play,
        phase,
        shown_phase,
        Space::new().width(16),
        e_arrows,
        h_arrows
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .boxed()
}

/// The section view's choices: the axis it cuts across, where along it, and which side is kept.
fn section_bar(app: &App, cut: Cut) -> Element<'_, Message> {
    let t = app.look.tokens;
    let set = |c: Cut| Message::View3d(Nav::Section(Some(c)));
    let Some((lo, hi)) = app.view3d.bounds() else { return Space::new().boxed() };
    let axes = AXES.iter().enumerate().map(|(k, name)| {
        let label = trf(t!("Cut across {axis}"), args!(axis = name));
        tool(t, &label, k == cut.axis, Some(set(Cut { axis: k, ..Cut::middle(k, (lo, hi)) })))
    });
    let k = cut.axis.min(2);
    let range = lo[k]..=hi[k].max(lo[k] + f64::MIN_POSITIVE);
    let place = slider(range, cut.at, move |at| set(Cut { at, ..cut })).step((hi[k] - lo[k]) / 200.0).width(240);
    let l = app.session.display().length;
    let shown = text(format!("{:.3} {}", l.from_si(cut.at), l.symbol())).size(t.font_body).color(t.text_dim);
    let flip = tool(t, tr(t!("Keep the other side")), false, Some(set(Cut { keep: cut.keep.flipped(), ..cut })));
    row(axes.chain([place.boxed(), shown.boxed(), flip])).spacing(8).align_y(Alignment::Center).boxed()
}

/// The field's colour bar: its quantity, its gradient and its ticks.
fn colour_bar(t: Tokens, bar: &field::Bar) -> Element<'_, Message> {
    let gradient = image(bar.gradient.clone()).width(BAR_W).height(12).content_fit(iced::ContentFit::Fill).filter_method(image::FilterMethod::Linear);
    let ticks = bar.ticks.iter().enumerate().flat_map(|(k, tick)| {
        let gap = (k > 0).then(|| Space::new().width(Fill).boxed());
        gap.into_iter().chain(std::iter::once(text(tick.clone()).size(t.font_small).color(t.text_dim).boxed()))
    });
    column![text(bar.title.clone()).size(t.font_body).color(t.text), gradient, row(ticks).width(BAR_W)].spacing(4).boxed()
}

/// The colour bar's width (points).
const BAR_W: f32 = 180.0;

/// The legend over the view's corner: the field's colour bar, then a swatch and a name each.
fn legend(app: &App) -> Element<'_, Message> {
    let t = app.look.tokens;
    let Some(s) = app.view3d.shown.as_ref().filter(|s| !s.legend.is_empty() || !s.arrow_legend.is_empty() || s.bar.is_some()) else {
        return Space::new().boxed();
    };
    // An arrow in its colour beside what its set shows.
    let arrows = s.arrow_legend.iter().map(|(name, c)| {
        let ink = look::rgb(*c);
        row![text("→").size(t.font_body).color(ink), text(name.clone()).size(t.font_small).color(t.text)]
            .spacing(6)
            .align_y(Alignment::Center)
            .boxed()
    });
    let entries = s.legend.iter().map(|(name, c)| {
        let swatch = container(Space::new().width(12).height(12))
            .style(move |_: &iced::Theme| container::Style { background: Some(look::rgb(*c).into()), ..container::Style::default() });
        row![swatch, text(name.clone()).size(t.font_small).color(t.text)].spacing(6).align_y(Alignment::Center).boxed()
    });
    let list = column(s.bar.iter().map(|b| colour_bar(t, b)).chain(arrows).chain(entries)).spacing(4);
    container(container(list).padding(8).style(look::ground(t))).padding(12).boxed()
}

/// What is selected, on a card at the top of the view: its name and detail, or how many parts.
fn card(app: &App) -> Element<'_, Message> {
    let t = app.look.tokens;
    let rows = app.picked.as_ref().map_or(&[][..], |(_, rows)| rows.as_slice());
    let line = match (rows, app.selection.len()) {
        (_, 0) if app.view3d.overlay == Overlay::Field => match &app.view3d.field_error {
            Some(e) => e.clone(),
            None => return Space::new().boxed(),
        },
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
    let field = (app.view3d.overlay == Overlay::Field).then(|| field_bar(app));
    let section = app.view3d.section.map(|c| section_bar(app, c));
    column(std::iter::once(bar(app)).chain(section).chain(field).chain(std::iter::once(picture))).spacing(8).padding(12).height(Fill).boxed()
}
