//! Periodic boundaries: the model as the unit cell of an infinite array (a frequency-selective
//! surface, a metasurface, a phased array's element, a slow-wave structure).
//!
//! Opposite faces of the cell are tied by Floquet's theorem: a field of a lattice periodic with
//! period L along an axis, excited with the phase ψ per period, satisfies
//! E(r + L ê) = e^{−jψ} E(r) (time dependence e^{jωt}; F. Floquet 1883; J.-M. Jin, *The Finite
//! Element Method in Electromagnetics*, 3rd ed., Wiley 2014, its chapter on periodic structures). Across x and y the phases
//! come from the incident plane wave: ψ_x = k_x a, ψ_y = k_y b with
//! k_t = k₁ sin θ (cos φ, sin φ), k₁ the wavenumber of the medium at the first port. Along z
//! (a guide periodic along its axis, for its dispersion diagram) the phase is swept.

use serde::{Deserialize, Serialize};

use crate::error::{Result, invalid};
use crate::model::Model;

emcraft_macros::named_enum! {
    /// The polarization of the Floquet ports' excited and read mode, relative to the plane of
    /// incidence (z and k_t).
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum Polarization {
        /// E normal to the plane of incidence (s, transverse electric to z).
        #[default]
        Te = "te",
        /// H normal to the plane of incidence (p, transverse magnetic to z).
        Tm = "tm",
    }
}

emcraft_macros::named_serde!(Polarization);

/// The model's periodic boundaries.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Periodic {
    /// The faces x = 0 and x = a are tied.
    pub x: bool,
    /// The faces y = 0 and y = b are tied.
    pub y: bool,
    /// The faces z = 0 and z = length are tied (the guide's ports are then left out: a periodic
    /// guide's eigenmodes, `solve.dispersion`).
    #[serde(default)]
    pub z: bool,
    /// The incident wave's angle from the z axis (rad), 0 ≤ θ < π/2.
    #[serde(default)]
    pub theta: f64,
    /// Its azimuth from the x axis (rad).
    #[serde(default)]
    pub phi: f64,
    /// The polarization the Floquet ports excite and read first.
    #[serde(default)]
    pub polarization: Polarization,
}

impl Periodic {
    /// Checks the settings on their own: some axis tied, finite angles, θ in [0, 90°).
    pub fn check(&self) -> std::result::Result<(), String> {
        if !(self.x || self.y || self.z) {
            return Err("no axis is periodic".into());
        }
        if !(self.theta.is_finite() && self.phi.is_finite()) {
            return Err("the incidence angles must be finite".into());
        }
        if !(0.0..std::f64::consts::FRAC_PI_2 * (1.0 - 1e-9)).contains(&self.theta) {
            return Err(format!("θ must be at least 0 and below 90°, got {}°", self.theta.to_degrees()));
        }
        Ok(())
    }

    /// The axes tied, as component indices.
    pub fn axes(&self) -> Vec<usize> {
        [self.x, self.y, self.z].iter().enumerate().filter(|(_, on)| **on).map(|(d, _)| d).collect()
    }
}

impl Model {
    /// Sets the periodic boundaries (none clears them) after checking them against the
    /// geometry. The mesh stays: the structured mesh's opposite faces already match.
    pub fn set_periodic(&mut self, periodic: Option<Periodic>) -> Result<()> {
        if let Some(why) = periodic.and_then(|p| self.periodic_issue_of(&p)) {
            return Err(invalid("periodic boundaries", why));
        }
        self.periodic = periodic;
        Ok(())
    }

    /// Why the model's periodic boundaries cannot hold, if they cannot.
    pub fn periodic_issue(&self) -> Option<String> {
        self.periodic.as_ref().and_then(|p| self.periodic_issue_of(p))
    }

    fn periodic_issue_of(&self, p: &Periodic) -> Option<String> {
        let issue = || -> std::result::Result<(), String> {
            p.check()?;
            if self.waveguide.is_none() {
                return Err(if self.solid.is_some() {
                    "not supported yet: periodic boundaries on a solid (the unstructured mesher does not make matching opposite faces; use geometry.waveguide, whose structured mesh does)".into()
                } else {
                    "needs a waveguide (geometry.waveguide) as the unit cell".into()
                });
            }
            if !self.symmetry.is_empty() {
                return Err("not supported yet: symmetry planes with periodic boundaries".into());
            }
            if self.open.is_some() {
                return Err("not supported yet: an open boundary with periodic boundaries (the Floquet ports terminate the cell)".into());
            }
            if !self.lumped.is_empty() || !self.sources.is_empty() {
                return Err("not supported yet: lumped ports or sources in a periodic cell".into());
            }
            if p.z && !self.ports.is_empty() {
                return Err("a cell periodic along z has no ports (its faces z = 0 and z = length are tied): remove them, or leave z free".into());
            }
            Ok(())
        };
        issue().err()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{End, Port, Section, Waveguide};

    fn cell() -> Model {
        let mut m = Model::new();
        m.set_waveguide(Waveguide {
            width: 0.01,
            height: 0.01,
            sections: vec![Section::new(0.01, "vacuum")],
            layers: Vec::new(),
            strips: Vec::new(),
            vias: Vec::new(),
        })
        .unwrap();
        m
    }

    fn tied(x: bool, y: bool, z: bool) -> Periodic {
        Periodic { x, y, z, theta: 0.0, phi: 0.0, polarization: Polarization::Te }
    }

    #[test]
    fn periodic_boundaries_are_checked() {
        let mut m = cell();
        m.set_periodic(Some(tied(true, true, false))).unwrap();
        assert_eq!(m.periodic.map(|p| p.axes()), Some(vec![0, 1]));
        for bad in [
            tied(false, false, false),
            Periodic { theta: f64::NAN, ..tied(true, true, false) },
            Periodic { theta: std::f64::consts::FRAC_PI_2, ..tied(true, true, false) },
            Periodic { theta: -0.1, ..tied(true, true, false) },
        ] {
            assert!(m.set_periodic(Some(bad)).is_err(), "{bad:?}");
        }
        assert_eq!(m.periodic, Some(tied(true, true, false)), "a refused change keeps the old");
        m.add_port(Port::new("p1", End::Start)).unwrap();
        assert!(m.set_periodic(Some(tied(false, false, true))).unwrap_err().to_string().contains("no ports"));
        m.set_periodic(None).unwrap();
        assert!(Model::new().set_periodic(Some(tied(true, false, false))).is_err(), "no geometry");
    }
}
