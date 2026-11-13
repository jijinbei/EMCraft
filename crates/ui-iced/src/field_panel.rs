//! The field view's side column on the model page, beside the 3D view while it shows the field
//!: the power that reaches each port at the field's frequency,
//! the project's probes with the field coloured there (+ adds one at the pinned point, else the
//! plane's middle; ✕ takes one away), the fields at the pinned point (a click on the plane pins
//! it), and the picture and the animation saved through their dialogs (`view.field`,
//! `view.animation`). The rules are the shared crate's ([`emcraft_ui_core::results::fields`]).

use emcraft_model::geom::Point3;
use emcraft_ui_core::lang::{Text, tr};
use emcraft_ui_core::results::fields;
use iced::widget::{Column, button, container, row, scrollable, text};
use iced::{Alignment, Element, Fill, Widget};
use serde_json::json;

use crate::app::{App, Message};
use crate::look::{self, Tokens};

fn heading<'a>(t: Tokens, title: Text) -> Element<'a, Message> {
    text(tr(title).to_uppercase()).size(t.font_small).color(t.text_dim).boxed()
}

fn line<'a>(t: Tokens, s: impl Into<String>) -> Element<'a, Message> {
    text(s.into()).size(t.font_small).color(t.text).boxed()
}

fn action<'a>(t: Tokens, label: impl Into<String>, on: Option<Message>) -> Element<'a, Message> {
    button(text(label.into()).size(t.font_button)).style(look::button(t, false)).on_press_maybe(on).boxed()
}

fn run(id: &'static str, params: serde_json::Value) -> Message {
    Message::Run(emcraft_ui_core::system::gesture::Command { id, params })
}

impl App {
    /// The point on the plane a probe is added at (display units): the pinned one, else the
    /// plane's middle.
    pub(crate) fn probe_point(&self) -> Option<[f64; 3]> {
        let samples = self.view3d.sampled.as_ref()?.samples();
        let unit = self.session.display().length;
        let q = self.pinned.or_else(|| {
            let (x, y) = samples.axes();
            Some([0.5 * (x.min + x.max), 0.5 * (y.min + y.max)])
        })?;
        Some(samples.model_point(q, unit).to_array().map(|v| unit.from_si(v)))
    }

    /// The fields at the probes, for the field view as it stands.
    fn probe_values(&self) -> Vec<Option<(emcraft_model::CVec3, emcraft_model::CVec3)>> {
        let points: Vec<Point3> = self.session.views().probes.iter().map(emcraft_engine::views::Probe::point).collect();
        emcraft_engine::view::fields_at(&self.session, &self.view3d.field, &points).map(|p| p.values).unwrap_or_default()
    }
}

/// Whether the column shows: the model's page with its field.
pub fn shown(app: &App) -> bool {
    app.page == emcraft_ui_core::view::View::Model && app.view3d.overlay == emcraft_ui_core::scene3d::Overlay::Field
}

/// The column, while the field is shown.
pub fn view(app: &App, width: f32) -> Option<Element<'_, Message>> {
    if !shown(app) {
        return None;
    }
    let t = app.look.tokens;
    let s = &app.session;
    let v = &app.view3d.field;
    let idle = app.job.is_none();
    // The power at the field's frequency (a sweep's, not a mode's).
    let power = (v.mode.is_none() && s.results().sweep.is_some()).then(|| match emcraft_engine::power::at(s, v.frequency, v.excitation) {
        Ok(report) => {
            let (table, rest) = fields::power(&report);
            Column::with_children([heading(t, t!("POWER DISTRIBUTION")), crate::table::view(t, &table), line(t, rest)]).spacing(4).boxed()
        }
        Err(e) => Column::with_children([heading(t, t!("POWER DISTRIBUTION")), line(t, e.to_string())]).spacing(4).boxed(),
    });
    // The probes.
    let add = app.probe_point().map(|at| action(t, "+", idle.then(|| run("view.probe.add", json!({"at": at})))));
    let head = row(std::iter::once(heading(t, t!("PROBES"))).chain(add)).spacing(8).align_y(Alignment::Center).boxed();
    let values = app.probe_values();
    let probes = s.views().probes.iter().enumerate().map(|(i, p)| {
        let said = fields::probe_line(&p.name, values.get(i).copied().flatten(), v.component);
        let delete = action(t, "✕", idle.then(|| run("view.probe.delete", json!({"name": p.name}))));
        row![text(said).size(t.font_small).color(t.text).width(Fill), delete].spacing(6).align_y(Alignment::Center).boxed()
    });
    // The pinned point's fields.
    let lu = s.display().length.symbol();
    let pinned = app.pinned.and_then(|q| app.view3d.sampled.as_ref()?.samples().probe(q)).map(|p| {
        let axes = app.view3d.sampled.as_ref().map_or(("", ""), |s| {
            let (x, y) = s.samples().axes();
            (x.label.split(' ').next().unwrap_or(""), y.label.split(' ').next().unwrap_or(""))
        });
        fields::readout(&p, axes, lu, v.component, v.phase, true).into_iter().map(|l| line(t, l)).collect::<Vec<_>>()
    });
    let pinned = pinned.unwrap_or_else(|| {
        vec![text(tr(t!("click the picture to pin a point (right click lets it go)"))).size(t.font_small).color(t.text_dim).boxed()]
    });
    // The picture and the animation, through their dialogs.
    let at = v.at.map(|m| s.display().length.from_si(m));
    let picture = fields::save_params(v, at, s, "field.png", false);
    let animation = fields::save_params(v, at, s, "field-animation.png", true);
    let save = row![
        action(t, tr(t!("Picture…")), idle.then(|| Message::Open("view.field", picture))),
        action(t, tr(t!("Animation…")), idle.then(|| Message::Open("view.animation", animation)))
    ]
    .spacing(6)
    .boxed();
    let parts = power
        .into_iter()
        .chain(std::iter::once(head))
        .chain(probes)
        .chain(std::iter::once(heading(t, t!("PINNED"))))
        .chain(pinned)
        .chain([heading(t, t!("SAVE")), save]);
    Some(container(scrollable(Column::with_children(parts).spacing(8).padding(10))).width(width).height(Fill).style(look::ground(t)).boxed())
}
