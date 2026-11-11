//! What the viewport shows ([`emcraft_ui_core::view`]), and when each view has something to show
//! in this window: from the session and the replies the window keeps.

pub use emcraft_ui_core::view::{Place, View};

use crate::EmcraftApp;

/// A view as this window has it: stale, available, or missing with why.
pub trait ViewExt: Sized {
    /// Whether what the view shows is stale: its own kinds of result, or (a view of no one
    /// kind) any.
    fn stale(self, app: &EmcraftApp) -> bool;
    /// Whether the view has something to show: the model always, results once solved.
    fn available(self, app: &EmcraftApp) -> bool;
    /// What the card says instead of the view while it has nothing to show, for the views that
    /// leave it to the card.
    fn missing(self, app: &EmcraftApp) -> Option<crate::lang::Text>;
}

impl ViewExt for View {
    fn stale(self, app: &EmcraftApp) -> bool {
        let r = app.session.results();
        let mut own = emcraft_engine::ResultKind::ALL.into_iter().filter(|k| View::of(*k) == self).peekable();
        if own.peek().is_none() {
            return r.any_stale();
        }
        own.any(|k| r.is_stale(k))
    }

    fn available(self, app: &EmcraftApp) -> bool {
        let r = app.session.results();
        match self {
            View::Model => !matches!(app.session.model().analysis(), crate::steps::Solver::Pcb | crate::steps::Solver::System),
            // The PCB diagnosis' own stages; a board solved by finite elements is the model's.
            View::Pcb => app.session.model().analysis() == crate::steps::Solver::Pcb,
            // A project just opened: its kept sweeps, drawn from the history.
            View::SParameters => {
                r.sweep.is_some()
                    || r.time_domain.is_some()
                    || r.revolution.is_some()
                    || app.session.archive().iter().any(|a| a.kept.sweep.is_some())
                    || !app.session.views().measured.is_empty()
            }
            View::Tdr => r.sweep.is_some() || r.time_domain.is_some(),
            View::Pulse => r.sweep.is_some() || r.time_domain.is_some() || r.revolution.is_some(),
            View::Field => r.sweep.is_some() || r.eigen.is_some() || r.radiation.is_some(),
            View::PortModes => app.replies.port_modes.is_some() || (app.session.model().mesh.is_some() && !app.session.model().ports.is_empty()),
            View::FarField => r.radiation.is_some() || r.td_radiation.is_some(),
            View::Compare => r.sweep.is_some(),
            View::Eigenmodes => r.eigen.is_some(),
            View::Study => r.study.is_some(),
            View::Dispersion => r.dispersion.is_some(),
            View::Analyses => r.sweep.is_some() || app.replies.analysis.is_some(),
            View::System => app.session.model().analysis() == crate::steps::Solver::System || r.system.is_some(),
        }
    }

    fn missing(self, app: &EmcraftApp) -> Option<crate::lang::Text> {
        if self.available(app) { None } else { self.empty() }
    }
}
