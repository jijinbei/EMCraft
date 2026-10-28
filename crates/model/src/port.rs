//! Waveguide ports: the faces matched to a guide's modes, where S-parameters are read.

use serde::{Deserialize, Serialize};

use crate::error::{ModelError, Result, all_finite, invalid, named, positive};
use crate::material::Material;
use crate::model::Model;
use crate::waveguide::Span;

/// The cross-section of a port: the mode functions it carries.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PortShape {
    /// [x0, x0 + a] × [y0, y0 + b].
    Rect { x0: f64, y0: f64, a: f64, b: f64 },
    /// A circle of radius r about (cx, cy).
    Circle { cx: f64, cy: f64, r: f64 },
}

/// Which end of the waveguide a port closes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum End {
    /// z = 0, outward normal −z.
    #[serde(rename = "start")]
    Start,
    /// z = length, outward normal +z.
    #[serde(rename = "end")]
    End,
}

impl End {
    /// The outward normal's z component.
    pub fn normal(self) -> f64 {
        match self {
            End::Start => -1.0,
            End::End => 1.0,
        }
    }
}

/// The most modes a port may carry.
pub const MAX_PORT_MODES: u32 = 32;

/// A waveguide port: the end face, matched to the guide's modes. Mode 1 (TE10) is the one
/// excited and the one S-parameters are read in; the others absorb whatever else reaches the
/// port.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Port {
    pub name: String,
    pub end: End,
    /// Modes in the port's boundary condition (the lowest cut-off first).
    pub modes: u32,
    /// A port on a rectangle of a z plane inside the geometry instead of at one of its ends (a
    /// guide's closed end inside a body, feeding an antenna); `end` then says which side the
    /// guide lies (start: above the plane, its outward normal −z; end: below it).
    #[serde(default)]
    pub face: Option<PortFace>,
    /// Where the modes come from: the closed forms where they apply (a whole rectangle or
    /// circle of one material) or the numerical cross-section solver; any other cross-section —
    /// a coaxial line, a ridge, a polygon — gets the numerical ones in any case.
    #[serde(default)]
    pub mode_solver: ModeSolver,
    /// The impedance (Ω) the S-parameters are referred to at this port, for a line with a
    /// characteristic impedance (coaxial, strip); its own mode's when `None`.
    #[serde(default)]
    pub reference: Option<f64>,
    /// De-embedding: how far (m) the reference plane S is read at lies inside the model from
    /// the port's face, along the line (negative: outside it, a line added).
    #[serde(default)]
    pub deembed: f64,
    /// Whether a cross-section holding several conductors (coupled strips) is one terminal per
    /// conductor, its S-parameters read per strip, or one modal port (its first mode only).
    #[serde(default)]
    pub terminals: Terminals,
}

/// How a port whose cross-section holds several conductors besides the ground is read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Terminals {
    /// One terminal per conductor when there are two or more (named `<port>:<k>`, the conductors
    /// from the lowest x, then y), each referred to the port's reference impedance (50 Ω when
    /// none is set); a port with one conductor or none is read in its first mode.
    #[default]
    Auto,
    /// The first mode only, whatever the conductors (the even mode of two like strips).
    Modal,
}

/// The most conductors a terminal port may hold.
pub const MAX_TERMINALS: usize = 8;

/// The reference impedance (Ω) of a terminal port's terminals when the port sets none.
pub const DEFAULT_TERMINAL_IMPEDANCE: f64 = 50.0;

/// Where a port's modes come from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModeSolver {
    /// The closed forms where they apply, the numerical solver elsewhere.
    #[default]
    Auto,
    /// The numerical cross-section solver everywhere.
    Numeric,
}

/// A port's rectangle: [x0, x0 + a] × [y0, y0 + b] at height z (m).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PortFace {
    pub z: f64,
    pub x0: f64,
    pub y0: f64,
    pub a: f64,
    pub b: f64,
}

impl PortFace {
    /// Checks the rectangle: finite, with positive sides.
    pub fn check(&self) -> Result<()> {
        all_finite("port face", &[self.z, self.x0, self.y0])?;
        positive("port face width", self.a)?;
        positive("port face height", self.b)?;
        Ok(())
    }

    /// The rectangle as a port's cross-section.
    pub fn shape(&self) -> PortShape {
        PortShape::Rect { x0: self.x0, y0: self.y0, a: self.a, b: self.b }
    }
}

impl Port {
    /// A port at `end` with four modes, the closed forms' where they apply, its S-parameters
    /// in its own mode's power waves.
    pub fn new(name: impl Into<String>, end: End) -> Port {
        Port {
            name: name.into(),
            end,
            modes: 4,
            face: None,
            mode_solver: ModeSolver::Auto,
            reference: None,
            deembed: 0.0,
            terminals: Terminals::Auto,
        }
    }

    /// The port with `modes` modes in its boundary condition.
    pub fn with_modes(self, modes: u32) -> Port {
        Port { modes, ..self }
    }

    /// The port on a rectangle inside the geometry instead of an end face.
    pub fn on_face(self, face: PortFace) -> Port {
        Port { face: Some(face), ..self }
    }

    pub fn with_mode_solver(self, mode_solver: ModeSolver) -> Port {
        Port { mode_solver, ..self }
    }

    /// The port's reference plane `metres` inside the model.
    pub fn deembedded(self, metres: f64) -> Port {
        Port { deembed: metres, ..self }
    }

    /// The port read in its first mode whatever its conductors, or per conductor.
    pub fn with_terminals(self, terminals: Terminals) -> Port {
        Port { terminals, ..self }
    }

    /// The port's S-parameters referred to `ohms`.
    pub fn referred_to(self, ohms: f64) -> Port {
        Port { reference: Some(ohms), ..self }
    }

    /// Checks the port's own settings: a positive reference impedance, a finite de-embedding
    /// and 1 to [`MAX_PORT_MODES`] modes.
    pub fn check_settings(&self) -> Result<()> {
        if self.name.contains(':') {
            return Err(invalid(format!("port `{}`", self.name), "a port's name must not hold `:` (terminals are named `<port>:<k>`)"));
        }
        if let Some(r) = self.reference {
            positive(&format!("reference impedance of port `{}`", self.name), r)?;
        }
        all_finite(format!("de-embedding of port `{}`", self.name), &[self.deembed])?;
        if self.modes == 0 || self.modes > MAX_PORT_MODES {
            return Err(invalid(format!("port `{}`", self.name), format!("modes must be 1 to {MAX_PORT_MODES}, got {}", self.modes)));
        }
        Ok(())
    }
}

impl Model {
    /// Whether a waveguide or lumped port is named `name`.
    pub(crate) fn has_port_named(&self, name: &str) -> bool {
        self.ports.iter().any(|x| x.name == name) || self.lumped.iter().any(|x| x.name == name)
    }

    /// Adds a waveguide port after checking it fits the model.
    pub fn add_port(&mut self, p: Port) -> Result<()> {
        named("port name", &p.name)?;
        if self.spherical_layer().is_some() {
            return Err(invalid("port", "not supported yet: waveguide ports with a spherical layer (use lumped ports or sources)"));
        }
        if self.open.is_some() && p.face.is_none() {
            return Err(invalid("port", "with an open boundary a port goes on a face inside the box (`face`), the box's ends being under the layer"));
        }
        if let Some(f) = &p.face {
            if self.solid.is_none() {
                return Err(invalid("port", "a port on a face needs a solid (geometry.solid)"));
            }
            f.check()?;
        }
        if self.has_port_named(&p.name) {
            return Err(ModelError::Duplicate { kind: "port", name: p.name });
        }
        if p.face.is_none() && self.ports.iter().any(|x| x.face.is_none() && x.end == p.end) {
            return Err(invalid(format!("port `{}`", p.name), "that end of the waveguide already has a port"));
        }
        p.check_settings()?;
        self.ports.push(p);
        Ok(())
    }

    /// The port named `name` and its index.
    pub fn port(&self, name: &str) -> Result<(usize, &Port)> {
        self.ports.iter().enumerate().find(|(_, p)| p.name == name).ok_or_else(|| ModelError::Unknown { kind: "port", name: name.into() })
    }

    /// The cross-section of the port at an end: the waveguide's end section, or a box's or a
    /// cylinder's end face.
    pub fn port_shape(&self, end: End) -> Result<PortShape> {
        if let Some(w) = &self.waveguide {
            if !w.plain() {
                return Err(invalid("port", "a layered guide or one with strips has no closed-form modes"));
            }
            let i = match end {
                End::Start => 0,
                End::End => w.sections.len().saturating_sub(1),
            };
            let no_sections = || invalid("waveguide", "has no sections");
            let Span { start: x0, width: a } = w.aperture(i).ok_or_else(no_sections)?;
            let Span { start: y0, width: b } = w.aperture_y(i).ok_or_else(no_sections)?;
            return Ok(PortShape::Rect { x0, y0, a, b });
        }
        let s = self.solid.as_ref().ok_or_else(|| invalid("model", "has no geometry"))?;
        // A port needs the end face whole: no feature may reach it.
        let hi = s.shape.bounds().max;
        let plane = match end {
            End::Start => 0.0,
            End::End => hi[2],
        };
        let tol = 1e-9 * hi[2].abs().max(1e-300);
        if self.features.iter().map(|f| f.bounds()).any(|b| b.min.z <= plane + tol && b.max.z >= plane - tol) {
            return Err(invalid("port", "a feature reaches the end face; a port needs the guide's whole cross-section there"));
        }
        s.shape.end_face(end)
    }

    /// A port's cross-section: its face's rectangle, or the geometry's end.
    pub fn port_shape_of(&self, p: &Port) -> Result<PortShape> {
        match p.face {
            Some(f) => Ok(f.shape()),
            None => self.port_shape(p.end),
        }
    }

    /// The material filling the waveguide at a port's end.
    pub fn port_material(&self, end: End) -> Result<&Material> {
        if self.waveguide.is_none()
            && let Some(s) = &self.solid
        {
            return Ok(self.material(&s.material)?.1);
        }
        let w = self.waveguide.as_ref().ok_or_else(|| invalid("model", "has no waveguide"))?;
        let s = match end {
            End::Start => w.sections.first(),
            End::End => w.sections.last(),
        }
        .ok_or_else(|| invalid("waveguide", "has no sections"))?;
        Ok(self.material(&s.material)?.1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Section, Waveguide};

    #[test]
    fn ports_are_checked() {
        let mut m = Model::new();
        m.set_waveguide(Waveguide {
            width: 0.02286,
            height: 0.01016,
            sections: vec![Section::new(0.01, "vacuum")],
            layers: Vec::new(),
            strips: Vec::new(),
            vias: Vec::new(),
        })
        .unwrap();
        m.add_port(Port::new("1", End::Start).with_modes(4)).unwrap();
        assert!(m.add_port(Port::new("2", End::Start).with_modes(4)).is_err());
        assert!(m.add_port(Port::new("2", End::End).with_modes(0)).is_err());
        assert!(m.add_port(Port::new("2", End::End).with_modes(1000)).is_err());
        assert!(m.add_port(Port::new("2", End::End).deembedded(f64::NAN)).is_err());
        assert!(m.add_port(Port::new("2", End::End).referred_to(-50.0)).is_err());
        assert!(matches!(m.add_port(Port::new("1", End::End)), Err(ModelError::Duplicate { .. })));
        assert!(m.add_port(Port::new(" ", End::End)).is_err());
        m.add_port(Port::new("2", End::End).with_modes(1)).unwrap();
        assert_eq!(m.port("2").unwrap().0, 1);
        assert_eq!(m.port_material(End::End).unwrap().name, "vacuum");
        assert_eq!(m.port_shape(End::End).unwrap(), PortShape::Rect { x0: 0.0, y0: 0.0, a: 0.02286, b: 0.01016 });
        let face = PortFace { z: 0.0, x0: 0.0, y0: 0.0, a: 0.01, b: f64::INFINITY };
        assert!(face.check().is_err());
        assert!(m.add_port(Port::new("3", End::End).on_face(PortFace { b: 0.01, ..face })).is_err(), "a face needs a solid");
    }
}
