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
        /// The model's H-plane and E-plane outlines.
        Outline = "outline" => "Outline",
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

impl View {
    /// What the card says while the view has nothing to show (`None` for the views that say it
    /// themselves, with a button that makes it).
    fn empty(self) -> Option<&'static str> {
        match self {
            View::Model | View::Outline | View::PortModes | View::Compare => None,
            View::SParameters => Some("No S-parameters yet: SIMULATE › Frequency sweep."),
            View::Tdr => Some("No sweep yet: SIMULATE › Frequency sweep (a harmonic one, f = k Δf, for low pass)."),
            View::Field => Some("No fields yet: solve a sweep, eigenmodes or an open problem first."),
            View::FarField => Some("No far field yet: SIMULATE › Radiation (an open model with sources or lumped ports)."),
            View::Eigenmodes => Some("No eigenmodes yet: SIMULATE › Eigenmodes."),
            View::Study => Some("No study yet: define a variable, then SIMULATE › Sweep or Optimize."),
            View::Dispersion => Some("No dispersion diagram yet: BOUNDARIES › Periodic boundaries, then SIMULATE › Dispersion."),
        }
    }

    /// Whether the view has something to show: the model always, results once solved.
    pub fn available(self, app: &EmcraftApp) -> bool {
        let r = app.session.results();
        match self {
            View::Model | View::Outline => true,
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
        !matches!(self, View::Model | View::Outline)
    }

    /// What the card says instead of the view while it has nothing to show, for the views that
    /// leave it to the card.
    pub fn missing(self, app: &EmcraftApp) -> Option<&'static str> {
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
