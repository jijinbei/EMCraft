//! EMCraft's solver-neutral electromagnetic model: materials, the waveguide geometry, ports, the
//! tetrahedral mesh and the model check. Everything is stored in SI ([`units`]); the model knows
//! no file format, no solver and no display unit.
//!
//! The first geometry is a rectangular waveguide along z: a cross-section of width `a` (x) and
//! height `b` (y), and sections along z each filled with one material. Its side walls are
//! perfect electric conductors; its two end faces carry the ports.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod mesh;
pub mod units;

pub use mesh::{Boundary, BoundaryFace, Grid, Mesh};
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ModelError {
    #[error("invalid {what}: {why}")]
    Invalid { what: String, why: String },
    #[error("{kind} `{name}` already exists")]
    Duplicate { kind: &'static str, name: String },
    #[error("unknown {kind} `{name}`")]
    Unknown { kind: &'static str, name: String },
}

pub type Result<T> = std::result::Result<T, ModelError>;

fn invalid(what: impl Into<String>, why: impl Into<String>) -> ModelError {
    ModelError::Invalid { what: what.into(), why: why.into() }
}

/// A finite number > 0.
pub fn positive(what: &str, v: f64) -> Result<f64> {
    if v.is_finite() && v > 0.0 { Ok(v) } else { Err(invalid(what, format!("must be a finite number > 0, got {v}"))) }
}

/// A linear, isotropic, lossless material.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Material {
    pub name: String,
    /// Relative permittivity εr.
    pub eps_r: f64,
    /// Relative permeability μr.
    pub mu_r: f64,
}

impl Material {
    pub fn new(name: &str, eps_r: f64, mu_r: f64) -> Result<Material> {
        if name.trim().is_empty() {
            return Err(invalid("material name", "must not be empty"));
        }
        Ok(Material { name: name.to_owned(), eps_r: positive("relative permittivity", eps_r)?, mu_r: positive("relative permeability", mu_r)? })
    }

    pub fn vacuum() -> Material {
        Material { name: "vacuum".into(), eps_r: 1.0, mu_r: 1.0 }
    }
}

/// A length of the waveguide filled with one material.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Section {
    /// Length along z (m).
    pub length: f64,
    /// The material's name.
    pub material: String,
}

/// A rectangular waveguide along z: x ∈ [0, a], y ∈ [0, b], z from 0 through its sections.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Waveguide {
    /// Broad wall width a (m), along x.
    pub width: f64,
    /// Narrow wall height b (m), along y.
    pub height: f64,
    pub sections: Vec<Section>,
}

impl Waveguide {
    pub fn length(&self) -> f64 {
        self.sections.iter().map(|s| s.length).sum()
    }

    /// The z coordinates of the section boundaries, from 0 to the length.
    pub fn interfaces(&self) -> Vec<f64> {
        let mut z = vec![0.0];
        let mut at = 0.0;
        for s in &self.sections {
            at += s.length;
            z.push(at);
        }
        z
    }
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
}

/// The model.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub materials: Vec<Material>,
    pub waveguide: Option<Waveguide>,
    pub ports: Vec<Port>,
    /// The mesh of the waveguide (made by `emcraft-mesh`).
    #[serde(skip)]
    pub mesh: Option<Mesh>,
}

impl Model {
    pub fn new() -> Model {
        Model { materials: vec![Material::vacuum()], ..Model::default() }
    }

    pub fn material(&self, name: &str) -> Result<(usize, &Material)> {
        self.materials.iter().enumerate().find(|(_, m)| m.name == name).ok_or_else(|| ModelError::Unknown { kind: "material", name: name.into() })
    }

    pub fn add_material(&mut self, m: Material) -> Result<()> {
        if self.materials.iter().any(|x| x.name == m.name) {
            return Err(ModelError::Duplicate { kind: "material", name: m.name });
        }
        self.materials.push(m);
        Ok(())
    }

    /// Sets the waveguide geometry after checking it; the mesh is dropped (it no longer fits).
    pub fn set_waveguide(&mut self, w: Waveguide) -> Result<()> {
        positive("waveguide width", w.width)?;
        positive("waveguide height", w.height)?;
        if w.sections.is_empty() {
            return Err(invalid("waveguide", "needs at least one section"));
        }
        for (i, s) in w.sections.iter().enumerate() {
            positive(&format!("length of section {}", i + 1), s.length)?;
            self.material(&s.material)?;
        }
        self.waveguide = Some(w);
        self.mesh = None;
        Ok(())
    }

    pub fn add_port(&mut self, p: Port) -> Result<()> {
        if p.name.trim().is_empty() {
            return Err(invalid("port name", "must not be empty"));
        }
        if self.ports.iter().any(|x| x.name == p.name) {
            return Err(ModelError::Duplicate { kind: "port", name: p.name });
        }
        if self.ports.iter().any(|x| x.end == p.end) {
            return Err(invalid(format!("port `{}`", p.name), "that end of the waveguide already has a port"));
        }
        if p.modes == 0 || p.modes > MAX_PORT_MODES {
            return Err(invalid(format!("port `{}`", p.name), format!("modes must be 1 to {MAX_PORT_MODES}, got {}", p.modes)));
        }
        self.ports.push(p);
        Ok(())
    }

    pub fn port(&self, name: &str) -> Result<(usize, &Port)> {
        self.ports.iter().enumerate().find(|(_, p)| p.name == name).ok_or_else(|| ModelError::Unknown { kind: "port", name: name.into() })
    }

    /// The material filling the waveguide at a port's end.
    pub fn port_material(&self, end: End) -> Result<&Material> {
        let w = self.waveguide.as_ref().ok_or_else(|| invalid("model", "has no waveguide"))?;
        let s = match end {
            End::Start => w.sections.first(),
            End::End => w.sections.last(),
        }
        .ok_or_else(|| invalid("waveguide", "has no sections"))?;
        Ok(self.material(&s.material)?.1)
    }

    /// Everything a frequency-domain solve needs, each problem named.
    pub fn check(&self) -> Vec<String> {
        let mut issues = Vec::new();
        if self.waveguide.is_none() {
            issues.push("no waveguide: create one with mesh.waveguide".to_owned());
        }
        if self.ports.is_empty() {
            issues.push("no ports: create one with model.port.create".to_owned());
        }
        if self.mesh.is_none() {
            issues.push("no mesh: mesh the waveguide with mesh.waveguide".to_owned());
        }
        issues
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guide() -> Waveguide {
        Waveguide { width: 0.02286, height: 0.01016, sections: vec![Section { length: 0.01, material: "vacuum".into() }] }
    }

    #[test]
    fn materials_are_checked() {
        let mut m = Model::new();
        assert!(Material::new("x", f64::NAN, 1.0).is_err());
        assert!(Material::new("x", 2.0, -1.0).is_err());
        assert!(Material::new(" ", 2.0, 1.0).is_err());
        m.add_material(Material::new("ptfe", 2.0, 1.0).unwrap()).unwrap();
        assert!(matches!(m.add_material(Material::new("ptfe", 3.0, 1.0).unwrap()), Err(ModelError::Duplicate { .. })));
        assert_eq!(m.material("ptfe").unwrap().0, 1);
    }

    #[test]
    fn waveguide_and_ports_are_checked() {
        let mut m = Model::new();
        let mut w = guide();
        w.width = f64::INFINITY;
        assert!(m.set_waveguide(w).is_err());
        let mut w = guide();
        w.sections[0].material = "nothing".into();
        assert!(matches!(m.set_waveguide(w), Err(ModelError::Unknown { .. })));
        m.set_waveguide(guide()).unwrap();
        assert!((m.waveguide.as_ref().unwrap().length() - 0.01).abs() < 1e-15);
        m.add_port(Port { name: "1".into(), end: End::Start, modes: 4 }).unwrap();
        assert!(m.add_port(Port { name: "2".into(), end: End::Start, modes: 4 }).is_err());
        assert!(m.add_port(Port { name: "2".into(), end: End::End, modes: 0 }).is_err());
        assert!(m.add_port(Port { name: "2".into(), end: End::End, modes: 1000 }).is_err());
        m.add_port(Port { name: "2".into(), end: End::End, modes: 1 }).unwrap();
        assert_eq!(m.port_material(End::End).unwrap().name, "vacuum");
        assert_eq!(m.check(), vec!["no mesh: mesh the waveguide with mesh.waveguide".to_owned()]);
    }
}
