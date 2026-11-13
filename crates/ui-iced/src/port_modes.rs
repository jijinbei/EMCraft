//! The port modes page: each port's modes at the band's frequency as `solve.port_modes` found
//! them — whether each propagates, its β, εeff, impedance and attenuation — or, before they are
//! solved, what they need and the button that solves them.

use emcraft_engine::{PortModeList, PortModeRow};
use emcraft_ui_core::lang::{args, tr, trf};
use iced::widget::{Column, button, column, scrollable, table, text};
use iced::{Element, Widget};

use crate::app::{App, Message};
use crate::look;

/// A number of `digits` significant figures, or a dash.
fn number(v: Option<f64>, digits: usize) -> String {
    v.filter(|v| v.is_finite()).map_or_else(|| "–".to_owned(), |v| format!("{v:.digits$}"))
}

/// One port's modes as a table.
fn port<'a>(app: &'a App, p: &'a PortModeList, lu: &'a str) -> Element<'a, Message> {
    let t = app.look.tokens;
    let head = |s: String| text(s).size(t.font_body).color(t.form_dim);
    let cell = move |s: String| text(s).size(t.font_body).color(t.text).boxed();
    let columns = [
        table::column(head(tr(t!("Mode")).to_owned()), move |m: &PortModeRow| cell(m.name.clone())),
        table::column(head(tr(t!("Propagates")).to_owned()), move |m: &PortModeRow| {
            cell(tr(if m.propagating { t!("yes") } else { t!("no") }).to_owned())
        }),
        table::column(head(format!("β (rad/{lu})")), move |m: &PortModeRow| cell(number(m.beta, 5))),
        table::column(head("εeff".to_owned()), move |m: &PortModeRow| cell(number(m.eps_eff, 5))),
        table::column(head("Z (Ω)".to_owned()), move |m: &PortModeRow| cell(number(m.impedance, 3))),
        table::column(head(trf(t!("Attenuation (dB/{unit})"), args!(unit = lu))), move |m: &PortModeRow| cell(number(m.attenuation_db, 3))),
    ];
    column![text(trf(t!("Port {name}"), args!(name = p.name))).size(t.font_body), table(columns, &p.modes)].spacing(t.space_6).boxed()
}

/// The port modes page.
pub fn view(app: &App) -> Element<'_, Message> {
    let t = app.look.tokens;
    let Some(reply) = app.replies.port_modes.as_ref() else {
        let solve = button(text(tr(t!("Solve port modes…"))).size(t.font_button)).style(look::button(t, true));
        let solve = solve.on_press_maybe(app.job.is_none().then(|| Message::Command("solve.port_modes".into())));
        return crate::parts::empty(t, tr(t!("No port modes yet: they need a meshed model with ports.")), Some(solve.boxed()));
    };
    let fu = app.session.display().frequency;
    let title = trf(t!("Port modes at {f} {unit}"), args!(f = emcraft_ui_core::number::frequency(reply.frequency), unit = fu.symbol()));
    let ports = reply.ports.iter().map(|p| port(app, p, &reply.length_unit));
    scrollable(Column::with_children(std::iter::once(text(title).size(t.font_heading).boxed()).chain(ports)).spacing(t.space_16).padding(t.space_16))
        .boxed()
}
