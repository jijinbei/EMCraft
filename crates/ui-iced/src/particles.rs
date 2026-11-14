//! The particle run's page (docs/adr/0012-particles.md, stage B): what the run gave in a table,
//! then its charts — the energy in the field and the particles, each wall's current, the
//! particles' phase space, the probes' field (emcraft_ui_core::results::particles).

use emcraft_ui_core::lang::tr;
use emcraft_ui_core::view::View;
use iced::widget::{canvas, column, scrollable};
use iced::{Element, Fill, Widget};

use crate::app::{App, Message};
use crate::chart::Plot;

/// A chart's height (points).
const CHART_H: f32 = 260.0;

/// The page, or why there is nothing to show yet.
pub fn view(app: &App) -> Element<'_, Message> {
    let t = app.look.tokens;
    let Some(summary) = emcraft_ui_core::results::particles::summary(&app.session) else {
        let missing = View::Particles.missing(&app.session, &app.replies).map_or("", tr);
        return crate::parts::not_yet(app, View::Particles, missing);
    };
    let charts = emcraft_ui_core::results::particles::charts(&app.session)
        .into_iter()
        .map(|c| canvas(Plot { chart: std::borrow::Cow::Owned(c), palette: &app.look.theme.palette }).width(Fill).height(CHART_H).boxed());
    let parts = std::iter::once(crate::table::view(t, &summary)).chain(charts);
    scrollable(column(parts).spacing(t.space_16).padding(t.space_16)).boxed()
}
