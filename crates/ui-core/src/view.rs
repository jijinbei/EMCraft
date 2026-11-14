//! What the viewport shows: the views, their ids (the control channel's and the scenarios'
//! `view:<id>` handles), their tab labels, where each shows (the PCB's own window, else with a
//! step's panel) and what each says while it has nothing to show.

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
        /// A chirped pulse through the line: its compression.
        Pulse = "pulse" => "Pulse",
        Field = "field" => "Field",
        PortModes = "port_modes" => "Port modes",
        FarField = "far_field" => "Far field",
        Eigenmodes = "eigenmodes" => "Eigenmodes",
        /// The last parameter study: a sweep's curves or an optimization's progress.
        Study = "study" => "Study",
        /// A periodic cell's dispersion diagram.
        Dispersion = "dispersion" => "Dispersion",
        /// The analyses read from a sweep: material, noise, active reflection, power, temperature.
        Analyses = "analyses" => "Analyses",
        /// A system's chain, its block diagram and budget.
        System = "system" => "System",
        /// A particle run: its energies, currents, phase space and probes.
        Particles = "particles" => "Particles",
    }
}
emcraft_macros::named_serde!(View);

/// Where the window is: the PCB diagnosis' own window, or a view of the workflow with its
/// step's panel open (or none). The step panel never shows over the PCB window, and a view of
/// the workflow is never the PCB's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Pcb,
    Workflow { view: View, step: Option<emcraft_engine::workflow::Step> },
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
    pub const fn step(self) -> Option<emcraft_engine::workflow::Step> {
        match self {
            Place::Pcb => None,
            Place::Workflow { step, .. } => step,
        }
    }

    /// Showing `view`: the PCB's is its own window, without the step panel; another keeps the
    /// step open.
    ///
    /// ```
    /// use emcraft_ui_core::view::{Place, View};
    /// use emcraft_engine::workflow::Step;
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
    pub const fn with_step(self, step: Option<emcraft_engine::workflow::Step>) -> Place {
        let view = match self {
            Place::Pcb => View::Model,
            Place::Workflow { view, .. } => view,
        };
        Place::Workflow { view, step }
    }
}

impl View {
    /// The view's glyph, on its tab.
    pub const fn glyph(self) -> crate::glyph::Glyph {
        use crate::glyph::Glyph;
        match self {
            View::Model => Glyph::Box,
            View::Pcb => Glyph::Part,
            View::SParameters | View::Analyses => Glyph::Chart,
            View::Tdr | View::Pulse => Glyph::Time,
            View::Field => Glyph::FieldLines,
            View::PortModes => Glyph::PortModes,
            View::FarField => Glyph::Pattern,
            View::Eigenmodes => Glyph::Eigen,
            View::Study => Glyph::ParamSweep,
            View::Dispersion => Glyph::Dispersion,
            View::System => Glyph::Table,
            View::Particles => Glyph::Time,
        }
    }

    /// The view shown with no step open (the PCB's: its window).
    pub const fn place(self) -> Place {
        Place::Workflow { view: View::Model, step: None }.show(self)
    }

    /// What the card says while the view has nothing to show (`None` for the views that say it
    /// themselves, with a button that makes it).
    pub fn empty(self) -> Option<crate::lang::Text> {
        match self {
            View::Model | View::Pcb | View::PortModes => None,
            View::SParameters => Some(t!("No S-parameters yet: Run (step 6).")),
            View::Tdr => Some(t!("No sweep yet: Run (step 6) (a harmonic sweep, f = k Δf, for low pass).")),
            View::Pulse => Some(t!("No sweep yet: Run (step 6), or a Bloch wave.")),
            View::Field => Some(t!("No fields yet: solve a sweep, eigenmodes or an open problem first.")),
            View::FarField => Some(t!("No far field yet: Run (step 6) with sources or lumped ports.")),
            View::Eigenmodes => Some(t!("No eigenmodes yet: Run (step 6).")),
            View::Study => Some(t!("No study yet: define a variable, then Sweep or Optimize (step 5).")),
            View::Dispersion => Some(t!("No dispersion diagram yet: periodic boundaries (step 4), then Dispersion (step 5).")),
            View::Particles => Some(t!("No particle run yet: Run (step 6).")),
            View::Analyses | View::System => None,
        }
    }

    /// Whether what the view shows of `s` is stale: its own kinds of result, or (a view of no
    /// one kind) any.
    pub fn stale(self, s: &emcraft_engine::Session) -> bool {
        let r = s.results();
        let mut own = emcraft_engine::ResultKind::ALL.into_iter().filter(|k| View::of(*k) == self).peekable();
        if own.peek().is_none() {
            return r.any_stale();
        }
        own.any(|k| r.is_stale(k))
    }

    /// Whether the view has something to show of `s` with `replies` kept: the model always,
    /// results once solved.
    pub fn available(self, s: &emcraft_engine::Session, replies: &crate::replies::Replies) -> bool {
        use emcraft_engine::workflow::Analysis as Solver;
        let r = s.results();
        let analysis = s.model().analysis();
        match self {
            View::Model => !matches!(analysis, Solver::Pcb | Solver::System),
            // The PCB diagnosis' own stages; a board solved by finite elements is the model's.
            View::Pcb => analysis == Solver::Pcb,
            // A project just opened: its kept sweeps, drawn from the history.
            View::SParameters => {
                r.sweep.is_some()
                    || r.time_domain.is_some()
                    || r.revolution.is_some()
                    || s.archive().iter().any(|a| a.kept.sweep.is_some())
                    || !s.views().measured.is_empty()
            }
            View::Tdr => r.sweep.is_some() || r.time_domain.is_some(),
            View::Pulse => r.sweep.is_some() || r.time_domain.is_some() || r.revolution.is_some(),
            View::Field => r.sweep.is_some() || r.eigen.is_some() || r.radiation.is_some(),
            View::PortModes => replies.port_modes.is_some() || (s.model().mesh.is_some() && !s.model().ports.is_empty()),
            View::FarField => r.radiation.is_some() || r.td_radiation.is_some(),
            View::Eigenmodes => r.eigen.is_some(),
            View::Study => r.study.is_some(),
            View::Dispersion => r.dispersion.is_some(),
            View::Analyses => r.sweep.is_some() || replies.analysis.is_some(),
            View::System => analysis == Solver::System || r.system.is_some(),
            View::Particles => r.particles.is_some(),
        }
    }

    /// What the card says instead of the view while it has nothing to show, for the views that
    /// leave it to the card.
    pub fn missing(self, s: &emcraft_engine::Session, replies: &crate::replies::Replies) -> Option<crate::lang::Text> {
        if self.available(s, replies) { None } else { self.empty() }
    }

    /// The view that shows a kind of result first.
    pub const fn of(kind: emcraft_engine::ResultKind) -> View {
        use emcraft_engine::ResultKind as R;
        match kind {
            R::Sweep | R::TimeDomain | R::Revolution => View::SParameters,
            R::Eigen => View::Eigenmodes,
            R::Radiation | R::TdRadiation => View::FarField,
            R::Dispersion => View::Dispersion,
            R::Study => View::Study,
            R::Bloch => View::Pulse,
            R::System => View::System,
            R::Particles => View::Particles,
        }
    }

    /// The engine's chart the view is (`emcraft_engine::views::chart_of`), for the views that are
    /// one chart.
    ///
    /// ```
    /// use emcraft_engine::views::ChartId;
    /// use emcraft_ui_core::view::View;
    /// assert_eq!(View::FarField.chart(), Some(ChartId::Pattern));
    /// assert_eq!(View::Model.chart(), None);
    /// ```
    pub const fn chart(self) -> Option<emcraft_engine::views::ChartId> {
        use emcraft_engine::views::ChartId as C;
        Some(match self {
            View::SParameters => C::SParameters,
            View::Tdr => C::Tdr,
            View::Pulse => C::Pulse,
            View::FarField => C::Pattern,
            View::Dispersion => C::Dispersion,
            View::Study => C::Study,
            View::Analyses => C::Analyses,
            _ => return None,
        })
    }

    /// Whether the view shows results (which go out of date when the model changes after its
    /// run), not the model.
    pub fn is_result(self) -> bool {
        !matches!(self, View::Model | View::Pcb)
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
        assert_eq!(View::COUNT, 14);
    }
}
