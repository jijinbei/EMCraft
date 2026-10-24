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

pub use mesh::{Boundary, BoundaryFace, Buckets, Grid, Mesh};
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
    /// Dielectric loss tangent tan δ: the permittivity is εr (1 − j tan δ) (time dependence
    /// e^{jωt}).
    #[serde(default)]
    pub loss_tangent: f64,
}

impl Material {
    pub fn new(name: &str, eps_r: f64, mu_r: f64) -> Result<Material> {
        if name.trim().is_empty() {
            return Err(invalid("material name", "must not be empty"));
        }
        Ok(Material {
            name: name.to_owned(),
            eps_r: positive("relative permittivity", eps_r)?,
            mu_r: positive("relative permeability", mu_r)?,
            loss_tangent: 0.0,
        })
    }

    /// The same material with a dielectric loss tangent (finite, ≥ 0).
    pub fn with_loss_tangent(mut self, tan_d: f64) -> Result<Material> {
        if !(tan_d.is_finite() && tan_d >= 0.0) {
            return Err(invalid("loss tangent", format!("must be a finite number ≥ 0, got {tan_d}")));
        }
        self.loss_tangent = tan_d;
        Ok(self)
    }

    pub fn vacuum() -> Material {
        Material { name: "vacuum".into(), eps_r: 1.0, mu_r: 1.0, loss_tangent: 0.0 }
    }
}

/// A length of the waveguide filled with one material, with its own aperture across x.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct Section {
    /// Length along z (m).
    pub length: f64,
    /// The material's name.
    pub material: String,
    /// The aperture's width along x (m); the guide's width when `None`.
    #[serde(default)]
    pub width: Option<f64>,
    /// Where the aperture starts along x (m); centred in the guide when `None`.
    #[serde(default)]
    pub offset: Option<f64>,
    /// The aperture's height along y (m); the guide's height when `None` (an E-plane step or a
    /// capacitive iris when smaller).
    #[serde(default)]
    pub height: Option<f64>,
    /// Where the aperture starts along y (m); centred when `None`.
    #[serde(default)]
    pub offset_y: Option<f64>,
}

impl Section {
    pub fn new(length: f64, material: &str) -> Section {
        Section { length, material: material.to_owned(), ..Default::default() }
    }
}

/// A rectangular waveguide along z: x ∈ [0, a], y ∈ [0, b], z from 0 through its sections. A
/// section may narrow the broad wall to an aperture of its own (an H-plane step, an inductive
/// iris); the narrow wall b is the same throughout.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Waveguide {
    /// Broad wall width a (m), along x: the widest aperture.
    pub width: f64,
    /// Narrow wall height b (m), along y.
    pub height: f64,
    pub sections: Vec<Section>,
}

impl Waveguide {
    pub fn length(&self) -> f64 {
        self.sections.iter().map(|s| s.length).sum()
    }

    /// Section i's aperture across x: (start, width) in metres.
    pub fn aperture(&self, i: usize) -> Option<(f64, f64)> {
        let s = self.sections.get(i)?;
        let w = s.width.unwrap_or(self.width);
        Some((s.offset.unwrap_or((self.width - w) / 2.0), w))
    }

    /// Section i's aperture across y: (start, height) in metres.
    pub fn aperture_y(&self, i: usize) -> Option<(f64, f64)> {
        let s = self.sections.get(i)?;
        let h = s.height.unwrap_or(self.height);
        Some((s.offset_y.unwrap_or((self.height - h) / 2.0), h))
    }

    /// Whether every section has the full width (no H-plane change).
    pub fn full_width(&self) -> bool {
        (0..self.sections.len())
            .all(|i| self.aperture(i).is_some_and(|(x0, w)| x0.abs() <= 1e-12 * self.width && (w - self.width).abs() <= 1e-12 * self.width))
    }

    /// Whether every section has the full height (no E-plane change).
    pub fn full_height(&self) -> bool {
        (0..self.sections.len())
            .all(|i| self.aperture_y(i).is_some_and(|(y0, h)| y0.abs() <= 1e-12 * self.height && (h - self.height).abs() <= 1e-12 * self.height))
    }

    /// Whether every section has the full cross-section.
    pub fn uniform(&self) -> bool {
        self.full_width() && self.full_height()
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

/// A primitive body, meshed with unstructured tetrahedra.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Shape {
    /// [0, x] × [0, y] × [0, z].
    Box { size: [f64; 3] },
    /// Axis z through the origin, from z = 0 to `length`.
    Cylinder { radius: f64, length: f64 },
    /// Centred on the origin.
    Sphere { radius: f64 },
}

impl Shape {
    /// The bounding box (lo, hi).
    pub fn bounds(&self) -> ([f64; 3], [f64; 3]) {
        match *self {
            Shape::Box { size } => ([0.0; 3], size),
            Shape::Cylinder { radius: r, length } => ([-r, -r, 0.0], [r, r, length]),
            Shape::Sphere { radius: r } => ([-r; 3], [r; 3]),
        }
    }

    pub fn volume(&self) -> f64 {
        match *self {
            Shape::Box { size } => size[0] * size[1] * size[2],
            Shape::Cylinder { radius, length } => std::f64::consts::PI * radius * radius * length,
            Shape::Sphere { radius } => 4.0 / 3.0 * std::f64::consts::PI * radius.powi(3),
        }
    }
}

/// A body of one material bounded by conducting walls (a cavity, or a guide whose end faces are
/// ports).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Solid {
    pub shape: Shape,
    pub material: String,
}

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
}

/// The model.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub materials: Vec<Material>,
    pub waveguide: Option<Waveguide>,
    /// A primitive body instead of a waveguide.
    #[serde(default)]
    pub solid: Option<Solid>,
    pub ports: Vec<Port>,
    /// The conductivity (S/m) of the guide's walls (and its irises and steps); perfectly
    /// conducting when `None`.
    #[serde(default)]
    pub wall_conductivity: Option<f64>,
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
        let tol = 1e-9 * w.width;
        for (i, s) in w.sections.iter().enumerate() {
            positive(&format!("length of section {}", i + 1), s.length)?;
            self.material(&s.material)?;
            if let Some(a) = s.width {
                positive(&format!("aperture width of section {}", i + 1), a)?;
            }
            if let Some(o) = s.offset
                && !o.is_finite()
            {
                return Err(invalid(format!("aperture offset of section {}", i + 1), "must be finite"));
            }
            if let Some(h) = s.height {
                positive(&format!("aperture height of section {}", i + 1), h)?;
            }
            if let Some(o) = s.offset_y
                && !o.is_finite()
            {
                return Err(invalid(format!("aperture y offset of section {}", i + 1), "must be finite"));
            }
            let (x0, a) = w.aperture(i).unwrap_or((0.0, w.width));
            if x0 < -tol || x0 + a > w.width + tol {
                return Err(invalid(format!("section {}", i + 1), "its aperture must lie within the guide's width"));
            }
            let toly = 1e-9 * w.height;
            let (y0, h) = w.aperture_y(i).unwrap_or((0.0, w.height));
            if y0 < -toly || y0 + h > w.height + toly {
                return Err(invalid(format!("section {}", i + 1), "its aperture must lie within the guide's height"));
            }
        }
        // Neighbouring apertures must nest: one rectangle holds the other (steps, irises).
        let toly = 1e-9 * w.height;
        for i in 1..w.sections.len() {
            let rect = |k: usize| w.aperture(k).zip(w.aperture_y(k));
            if let (Some(((x0, a0), (y0, b0))), Some(((x1, a1), (y1, b1)))) = (rect(i - 1), rect(i)) {
                let holds = |xa: f64, wa: f64, ya: f64, ha: f64, xb: f64, wb: f64, yb: f64, hb: f64| {
                    xa <= xb + tol && xb + wb <= xa + wa + tol && ya <= yb + toly && yb + hb <= ya + ha + toly
                };
                if !holds(x0, a0, y0, b0, x1, a1, y1, b1) && !holds(x1, a1, y1, b1, x0, a0, y0, b0) {
                    return Err(invalid(
                        format!("sections {} and {}", i, i + 1),
                        "not supported yet: apertures that overlap only in part (one must hold the other)",
                    ));
                }
            }
        }
        self.waveguide = Some(w);
        self.solid = None;
        self.mesh = None;
        Ok(())
    }

    /// Sets a primitive body as the geometry (replacing a waveguide); the mesh is dropped.
    pub fn set_solid(&mut self, s: Solid) -> Result<()> {
        match s.shape {
            Shape::Box { size } => {
                for (k, v) in size.iter().enumerate() {
                    positive(&format!("box size {}", ["x", "y", "z"][k]), *v)?;
                }
            }
            Shape::Cylinder { radius, length } => {
                positive("cylinder radius", radius)?;
                positive("cylinder length", length)?;
            }
            Shape::Sphere { radius } => {
                positive("sphere radius", radius)?;
            }
        }
        self.material(&s.material)?;
        self.solid = Some(s);
        self.waveguide = None;
        self.mesh = None;
        Ok(())
    }

    /// The bounding box of the geometry.
    pub fn bounds(&self) -> Option<([f64; 3], [f64; 3])> {
        if let Some(w) = &self.waveguide {
            return Some(([0.0; 3], [w.width, w.height, w.length()]));
        }
        self.solid.as_ref().map(|s| s.shape.bounds())
    }

    /// The cross-section of the port at an end: the waveguide's end section, or a box's or a
    /// cylinder's end face.
    pub fn port_shape(&self, end: End) -> Result<PortShape> {
        if let Some(w) = &self.waveguide {
            let i = match end {
                End::Start => 0,
                End::End => w.sections.len().saturating_sub(1),
            };
            let (x0, a) = w.aperture(i).ok_or_else(|| invalid("waveguide", "has no sections"))?;
            let (y0, b) = w.aperture_y(i).ok_or_else(|| invalid("waveguide", "has no sections"))?;
            return Ok(PortShape::Rect { x0, y0, a, b });
        }
        match self.solid.as_ref().map(|s| &s.shape) {
            Some(Shape::Box { size }) => Ok(PortShape::Rect { x0: 0.0, y0: 0.0, a: size[0], b: size[1] }),
            Some(Shape::Cylinder { radius, .. }) => Ok(PortShape::Circle { cx: 0.0, cy: 0.0, r: *radius }),
            Some(Shape::Sphere { .. }) => Err(invalid("port", "a sphere has no flat end faces for ports")),
            None => Err(invalid("model", "has no geometry")),
        }
    }

    /// Sets the walls' conductivity (S/m), or perfect conductors with `None`.
    pub fn set_wall_conductivity(&mut self, sigma: Option<f64>) -> Result<()> {
        if let Some(s) = sigma {
            positive("wall conductivity", s)?;
        }
        self.wall_conductivity = sigma;
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

    /// Everything a frequency-domain solve needs, each problem named.
    pub fn check(&self) -> Vec<String> {
        let mut issues = Vec::new();
        if self.waveguide.is_none() && self.solid.is_none() {
            issues.push("no geometry: create one with geometry.waveguide or geometry.solid".to_owned());
        }
        if self.ports.is_empty() {
            issues.push("no ports: create one with model.port.create".to_owned());
        }
        if self.mesh.is_none() {
            issues.push("no mesh: mesh the geometry with mesh.generate".to_owned());
        }
        issues
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guide() -> Waveguide {
        Waveguide { width: 0.02286, height: 0.01016, sections: vec![Section::new(0.01, "vacuum")] }
    }

    #[test]
    fn materials_are_checked() {
        let mut m = Model::new();
        assert!(Material::new("x", f64::NAN, 1.0).is_err());
        assert!(Material::new("x", 2.0, -1.0).is_err());
        assert!(Material::new(" ", 2.0, 1.0).is_err());
        assert!(Material::new("x", 2.0, 1.0).unwrap().with_loss_tangent(-0.1).is_err());
        assert!(Material::new("x", 2.0, 1.0).unwrap().with_loss_tangent(f64::NAN).is_err());
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
        assert_eq!(m.check(), vec!["no mesh: mesh the geometry with mesh.generate".to_owned()]);
    }

    #[test]
    fn solids_are_checked_and_replace_the_waveguide() {
        let mut m = Model::new();
        m.set_waveguide(guide()).unwrap();
        assert!(m.set_solid(Solid { shape: Shape::Cylinder { radius: -1.0, length: 1.0 }, material: "vacuum".into() }).is_err());
        assert!(m.set_solid(Solid { shape: Shape::Sphere { radius: 0.01 }, material: "nothing".into() }).is_err());
        m.set_solid(Solid { shape: Shape::Cylinder { radius: 0.01, length: 0.03 }, material: "vacuum".into() }).unwrap();
        assert!(m.waveguide.is_none());
        assert_eq!(m.bounds(), Some(([-0.01, -0.01, 0.0], [0.01, 0.01, 0.03])));
        assert_eq!(m.port_shape(End::End).unwrap(), PortShape::Circle { cx: 0.0, cy: 0.0, r: 0.01 });
        m.set_solid(Solid { shape: Shape::Sphere { radius: 0.01 }, material: "vacuum".into() }).unwrap();
        assert!(m.port_shape(End::Start).is_err());
        assert!((Shape::Sphere { radius: 1.0 }.volume() - 4.18879).abs() < 1e-5);
    }

    #[test]
    fn apertures_must_fit_and_nest() {
        let mut m = Model::new();
        let sec = |w: Option<f64>, o: Option<f64>| Section { length: 0.01, material: "vacuum".into(), width: w, offset: o, ..Default::default() };
        let guide = |s: Vec<Section>| Waveguide { width: 0.02, height: 0.01, sections: s };
        m.set_waveguide(guide(vec![sec(None, None), sec(Some(0.01), None), sec(None, None)])).unwrap();
        let w = m.waveguide.as_ref().unwrap();
        assert_eq!(w.aperture(1), Some((0.005, 0.01)));
        assert!(!w.uniform());
        assert!(m.set_waveguide(guide(vec![sec(Some(0.03), None)])).is_err());
        assert!(m.set_waveguide(guide(vec![sec(Some(0.01), Some(0.0)), sec(Some(0.01), Some(0.008))])).is_err());
        assert!(m.set_waveguide(guide(vec![sec(Some(0.01), Some(f64::NAN))])).is_err());
        m.set_waveguide(guide(vec![sec(Some(0.012), Some(0.0)), sec(Some(0.008), Some(0.002))])).unwrap();
    }
}
