//! What the viewport shows: the views, their ids (the control channel's and the scenarios'
//! `view:<id>` handles), their tab labels, and when each has something to show.

use crate::EmcraftApp;

emcraft_macros::named_enum! {
    /// What the viewport shows: its id (the control channel's and the scenarios' `view:<id>`
    /// handles) and its tab's label.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum View {
        /// The model in 3D.
        Model = "model" => "Model 3D",
        Pcb = "pcb" => "PCB route",
        SParameters = "sparameters" => "S-parameters",
        /// Time-domain reflectometry of a port.
        Tdr = "tdr" => "TDR",
        Field = "field" => "Field",
        PortModes = "port_modes" => "Port modes",
        FarField = "far_field" => "Far field",
        Compare = "compare" => "Compare with theory",
        Eigenmodes = "eigenmodes" => "Eigenmodes",
        /// The last parameter study: a sweep's curves or an optimization's progress.
        Study = "study" => "Study",
        /// A periodic cell's dispersion diagram.
        Dispersion = "dispersion" => "Dispersion",
    }
}
emcraft_macros::named_serde!(View);

/// Where the window is: the PCB diagnosis' own window, or a view of the workflow with its
/// step's panel open (or none). The step panel never shows over the PCB window, and a view of
/// the workflow is never the PCB's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Pcb,
    Workflow { view: View, step: Option<crate::steps::Step> },
}

impl Default for Place {
    fn default() -> Place {
        Place::Workflow { view: View::Model, step: None }
    }
}

impl Place {
    /// The view shown.
    pub const fn view(self) -> View {
        match self {
            Place::Pcb => View::Pcb,
            Place::Workflow { view, .. } => view,
        }
    }

    /// The step whose panel is open.
    pub const fn step(self) -> Option<crate::steps::Step> {
        match self {
            Place::Pcb => None,
            Place::Workflow { step, .. } => step,
        }
    }

    /// Showing `view`: the PCB's is its own window, without the step panel; another keeps the
    /// step open.
    ///
    /// ```
    /// use emcraft_ui_egui::view::{Place, View};
    /// use emcraft_ui_egui::steps::Step;
    ///
    /// let at = Place::default().with_step(Some(Step::Mesh));
    /// assert_eq!(at.show(View::Field).step(), Some(Step::Mesh));
    /// assert_eq!(at.show(View::Pcb), Place::Pcb);
    /// assert_eq!(Place::Pcb.with_step(Some(Step::Mesh)).view(), View::Model);
    /// ```
    pub const fn show(self, view: View) -> Place {
        match view {
            View::Pcb => Place::Pcb,
            view => Place::Workflow { view, step: self.step() },
        }
    }

    /// Step `step`'s panel open (`None`: closed); the view kept, the model's coming from the
    /// PCB window.
    pub const fn with_step(self, step: Option<crate::steps::Step>) -> Place {
        let view = match self {
            Place::Pcb => View::Model,
            Place::Workflow { view, .. } => view,
        };
        Place::Workflow { view, step }
    }
}

impl View {
    /// The view's glyph, on its tab.
    pub const fn glyph(self) -> crate::glyphs::Glyph {
        use crate::glyphs::Glyph;
        match self {
            View::Model => Glyph::Box,
            View::Pcb => Glyph::Part,
            View::SParameters => Glyph::Chart,
            View::Tdr => Glyph::Time,
            View::Field => Glyph::FieldLines,
            View::PortModes => Glyph::PortModes,
            View::FarField => Glyph::Pattern,
            View::Compare => Glyph::Compare,
            View::Eigenmodes => Glyph::Eigen,
            View::Study => Glyph::ParamSweep,
            View::Dispersion => Glyph::Dispersion,
        }
    }

    /// The view shown with no step open (the PCB's: its window).
    pub const fn place(self) -> Place {
        Place::Workflow { view: View::Model, step: None }.show(self)
    }

    /// What the card says while the view has nothing to show (`None` for the views that say it
    /// themselves, with a button that makes it).
    fn empty(self) -> Option<crate::lang::Text> {
        match self {
            View::Model | View::Pcb | View::PortModes | View::Compare => None,
            View::SParameters => Some(t!("No S-parameters yet: Run (step 7).")),
            View::Tdr => Some(t!("No sweep yet: Run (step 7) (a harmonic sweep, f = k Δf, for low pass).")),
            View::Field => Some(t!("No fields yet: solve a sweep, eigenmodes or an open problem first.")),
            View::FarField => Some(t!("No far field yet: Run (step 7) on an open model with sources or lumped ports.")),
            View::Eigenmodes => Some(t!("No eigenmodes yet: Run (step 7).")),
            View::Study => Some(t!("No study yet: define a variable, then Sweep or Optimize (step 5).")),
            View::Dispersion => Some(t!("No dispersion diagram yet: periodic boundaries (step 4), then Dispersion (step 5).")),
        }
    }

    /// Whether what the view shows is stale: its own kinds of result, or (a view of no one
    /// kind) any.
    pub fn stale(self, app: &crate::EmcraftApp) -> bool {
        let r = app.session.results();
        let mut own = emcraft_engine::ResultKind::ALL.into_iter().filter(|k| View::of(*k) == self).peekable();
        if own.peek().is_none() {
            return r.any_stale();
        }
        own.any(|k| r.is_stale(k))
    }

    /// The view that shows a kind of result first.
    pub const fn of(kind: emcraft_engine::ResultKind) -> View {
        use emcraft_engine::ResultKind as R;
        match kind {
            R::Sweep | R::TimeDomain => View::SParameters,
            R::Eigen => View::Eigenmodes,
            R::Radiation | R::TdRadiation => View::FarField,
            R::Dispersion => View::Dispersion,
            R::Study => View::Study,
        }
    }

    /// Whether the view has something to show: the model always, results once solved.
    pub fn available(self, app: &EmcraftApp) -> bool {
        let r = app.session.results();
        match self {
            View::Model => app.session.model().analysis() != crate::steps::Solver::Pcb,
            // The PCB diagnosis' own stages; a board solved by finite elements is the model's.
            View::Pcb => app.session.model().analysis() == crate::steps::Solver::Pcb,
            // A project just opened: its kept sweeps, drawn from the history.
            View::SParameters => r.sweep.is_some() || r.time_domain.is_some() || app.session.archive().iter().any(|a| a.kept.sweep.is_some()),
            View::Tdr => r.sweep.is_some() || r.time_domain.is_some(),
            View::Field => r.sweep.is_some() || r.eigen.is_some() || r.radiation.is_some(),
            View::PortModes => app.replies.port_modes.is_some() || (app.session.model().mesh.is_some() && !app.session.model().ports.is_empty()),
            View::FarField => r.radiation.is_some() || r.td_radiation.is_some(),
            View::Compare => r.sweep.is_some(),
            View::Eigenmodes => r.eigen.is_some(),
            View::Study => r.study.is_some(),
            View::Dispersion => r.dispersion.is_some(),
        }
    }

    /// Whether the view shows results (which go out of date when the model changes after its
    /// run), not the model.
    pub fn is_result(self) -> bool {
        !matches!(self, View::Model | View::Pcb)
    }

    /// What the card says instead of the view while it has nothing to show, for the views that
    /// leave it to the card.
    pub fn missing(self, app: &EmcraftApp) -> Option<crate::lang::Text> {
        if self.available(app) { None } else { self.empty() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn views_read_their_ids_and_labels() {
        for v in View::ALL {
            assert_eq!(View::parse(v.id()), Some(v));
        }
        assert_eq!(View::Tdr.label(), "TDR");
        assert_eq!(View::PortModes.id(), "port_modes");
        assert_eq!(View::COUNT, 11);
    }
}
