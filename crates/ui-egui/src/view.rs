//! What the viewport shows: the views, their ids (the control channel's and the scenarios'
//! `view:<id>` handles), their tab labels, and when each has something to show.

use crate::EmcraftApp;

/// What the viewport shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    /// The model in 3D.
    Model,
    /// The model's H-plane and E-plane outlines.
    Outline,
    SParameters,
    /// Time-domain reflectometry of a port.
    Tdr,
    Field,
    PortModes,
    FarField,
    Compare,
    Eigenmodes,
    /// The last parameter study: a sweep's curves or an optimization's progress.
    Study,
    /// A periodic cell's dispersion diagram.
    Dispersion,
}

/// A view's id, tab label, and what the card says while the view has nothing to show (`None`
/// for the views that say it themselves, with a button that makes it).
struct Meta {
    view: View,
    id: &'static str,
    label: &'static str,
    empty: Option<&'static str>,
}

const fn meta(view: View, id: &'static str, label: &'static str, empty: Option<&'static str>) -> Meta {
    Meta { view, id, label, empty }
}

/// Every view in tab order (the enum's order).
const VIEWS: [Meta; 11] = [
    meta(View::Model, "model", "Model 3D", None),
    meta(View::Outline, "outline", "Outline", None),
    meta(View::SParameters, "sparameters", "S-parameters", Some("No S-parameters yet: SIMULATE › Frequency sweep.")),
    meta(View::Tdr, "tdr", "TDR", Some("No sweep yet: SIMULATE › Frequency sweep (a harmonic one, f = k Δf, for low pass).")),
    meta(View::Field, "field", "Field", Some("No fields yet: solve a sweep, eigenmodes or an open problem first.")),
    meta(View::PortModes, "port_modes", "Port modes", None),
    meta(View::FarField, "far_field", "Far field", Some("No far field yet: SIMULATE › Radiation (an open model with sources or lumped ports).")),
    meta(View::Compare, "compare", "Compare with theory", None),
    meta(View::Eigenmodes, "eigenmodes", "Eigenmodes", Some("No eigenmodes yet: SIMULATE › Eigenmodes.")),
    meta(View::Study, "study", "Study", Some("No study yet: define a variable, then SIMULATE › Sweep or Optimize.")),
    meta(View::Dispersion, "dispersion", "Dispersion", Some("No dispersion diagram yet: MATERIALS › Periodic, then SIMULATE › Dispersion.")),
];

impl View {
    pub const ALL: [View; 11] = {
        let mut all = [View::Model; 11];
        let mut i = 0;
        while i < VIEWS.len() {
            all[i] = VIEWS[i].view;
            i += 1;
        }
        all
    };

    fn meta(self) -> &'static Meta {
        // The table lists the views in the enum's order (checked by a test below).
        &VIEWS[self as usize]
    }

    pub fn id(self) -> &'static str {
        self.meta().id
    }

    pub fn label(self) -> &'static str {
        self.meta().label
    }

    pub fn parse(s: &str) -> Option<View> {
        View::ALL.into_iter().find(|v| v.id() == s)
    }

    /// Whether the view has something to show: the model always, results once solved.
    pub fn available(self, app: &EmcraftApp) -> bool {
        let r = &app.session.results;
        match self {
            View::Model | View::Outline => true,
            View::SParameters | View::Tdr => r.sweep.is_some() || r.time_domain.is_some(),
            View::Field => r.sweep.is_some() || r.eigen.is_some() || r.radiation.is_some(),
            View::PortModes => app.views.port_modes.is_some() || (app.session.model.mesh.is_some() && !app.session.model.ports.is_empty()),
            View::FarField => r.radiation.is_some() || r.td_radiation.is_some(),
            View::Compare => r.sweep.is_some(),
            View::Eigenmodes => r.eigen.is_some(),
            View::Study => r.study.is_some(),
            View::Dispersion => r.dispersion.is_some(),
        }
    }

    /// What the card says instead of the view while it has nothing to show, for the views that
    /// leave it to the card.
    pub fn missing(self, app: &EmcraftApp) -> Option<&'static str> {
        if self.available(app) { None } else { self.meta().empty }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_follows_the_enum() {
        for (i, v) in View::ALL.into_iter().enumerate() {
            assert_eq!(v as usize, i);
            assert_eq!(View::parse(v.id()), Some(v));
        }
        let mut ids: Vec<&str> = View::ALL.iter().map(|v| v.id()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), View::ALL.len());
        assert_eq!(View::Tdr.label(), "TDR");
        assert_eq!(View::PortModes.id(), "port_modes");
    }
}
