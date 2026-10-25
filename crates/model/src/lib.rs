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
    /// A closed triangulated surface (imported): positions (m), outward triangles and a face
    /// tag per triangle.
    Surface { name: String, positions: Vec<[f64; 3]>, triangles: Vec<[u32; 3]>, tags: Vec<u32> },
}

/// Most triangles an imported surface may have.
pub const MAX_SURFACE_TRIANGLES: usize = 2_000_000;

impl Shape {
    /// The bounding box (lo, hi).
    pub fn bounds(&self) -> ([f64; 3], [f64; 3]) {
        match *self {
            Shape::Box { size } => ([0.0; 3], size),
            Shape::Cylinder { radius: r, length } => ([-r, -r, 0.0], [r, r, length]),
            Shape::Sphere { radius: r } => ([-r; 3], [r; 3]),
            Shape::Surface { ref positions, .. } => {
                let mut lo = [f64::INFINITY; 3];
                let mut hi = [f64::NEG_INFINITY; 3];
                for p in positions {
                    for d in 0..3 {
                        lo[d] = lo[d].min(p[d]);
                        hi[d] = hi[d].max(p[d]);
                    }
                }
                if positions.is_empty() { ([0.0; 3], [0.0; 3]) } else { (lo, hi) }
            }
        }
    }

    /// An imported surface's triangles on its lowest and highest z planes, the faces a port can
    /// close (empty for the primitives, whose ends the mesher knows).
    pub fn end_triangles(&self) -> [Vec<usize>; 2] {
        let Shape::Surface { positions, triangles, .. } = self else { return [Vec::new(), Vec::new()] };
        let (lo, hi) = self.bounds();
        let tol = 1e-9 * (0..3).map(|d| hi[d] - lo[d]).fold(0.0, f64::max);
        let on = |t: &[u32; 3], z: f64| t.iter().all(|v| positions.get(*v as usize).is_some_and(|p| (p[2] - z).abs() <= tol));
        [lo[2], hi[2]].map(|z| (0..triangles.len()).filter(|&i| on(&triangles[i], z)).collect())
    }

    pub fn volume(&self) -> f64 {
        match *self {
            Shape::Box { size } => size[0] * size[1] * size[2],
            Shape::Cylinder { radius, length } => std::f64::consts::PI * radius * radius * length,
            Shape::Sphere { radius } => 4.0 / 3.0 * std::f64::consts::PI * radius.powi(3),
            Shape::Surface { ref positions, ref triangles, .. } => triangles
                .iter()
                .map(|t| {
                    let [a, b, c] = t.map(|v| positions.get(v as usize).copied().unwrap_or([0.0; 3]));
                    (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0])) / 6.0
                })
                .sum(),
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

/// A body inside the solid: a dielectric region (`material`) or a conductor (`None`), placed
/// with its own origin at `at`. It must lie strictly inside the solid and apart from the others.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Inclusion {
    pub shape: Shape,
    pub at: [f64; 3],
    /// The filling; `None` for a perfect conductor (a hole in the mesh).
    pub material: Option<String>,
}

impl Inclusion {
    pub fn bounds(&self) -> ([f64; 3], [f64; 3]) {
        let (lo, hi) = self.shape.bounds();
        ([0, 1, 2].map(|d| lo[d] + self.at[d]), [0, 1, 2].map(|d| hi[d] + self.at[d]))
    }
}

impl Solid {
    /// The shape's dimensions are finite and positive.
    pub fn check_shape(&self) -> Result<()> {
        match self.shape {
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
            Shape::Surface { ref positions, ref triangles, ref tags, .. } => {
                if triangles.len() < 4 || triangles.len() > MAX_SURFACE_TRIANGLES {
                    return Err(invalid("surface", format!("has {} triangles; it needs 4 to {MAX_SURFACE_TRIANGLES}", triangles.len())));
                }
                if tags.len() != triangles.len() {
                    return Err(invalid("surface", "needs a face tag per triangle"));
                }
                if !positions.iter().flatten().all(|c| c.is_finite()) {
                    return Err(invalid("surface", "has a coordinate that is not a finite number"));
                }
                // Closed and oriented: every edge held once each way.
                let mut half = std::collections::HashMap::new();
                for t in triangles {
                    if t.iter().any(|v| *v as usize >= positions.len()) || t[0] == t[1] || t[1] == t[2] || t[0] == t[2] {
                        return Err(invalid("surface", "has a triangle with a missing or repeated corner"));
                    }
                    for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                        *half.entry((a, b)).or_insert(0u32) += 1;
                    }
                }
                if let Some(((a, b), _)) = half.iter().find(|((a, b), n)| **n != 1 || half.get(&(*b, *a)) != Some(&1)) {
                    return Err(invalid(
                        "surface",
                        format!(
                            "is not closed and consistently oriented (its edge between vertices {a} and {b}); a solid needs a watertight surface"
                        ),
                    ));
                }
                if !(self.shape.volume() > 0.0) {
                    return Err(invalid("surface", "encloses no volume (its triangles may face inwards)"));
                }
            }
        }
        Ok(())
    }
}

/// A coordinate axis.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Axis {
    X,
    Y,
    #[default]
    Z,
}

impl Axis {
    pub fn index(self) -> usize {
        match self {
            Axis::X => 0,
            Axis::Y => 1,
            Axis::Z => 2,
        }
    }

    /// A shape's own coordinates turned so its z axis lies along this one (a cyclic turn of
    /// the axes, keeping handedness): x → (z, x, y), y → (y, z, x).
    pub fn turn(self, p: [f64; 3]) -> [f64; 3] {
        match self {
            Axis::X => [p[2], p[0], p[1]],
            Axis::Y => [p[1], p[2], p[0]],
            Axis::Z => p,
        }
    }
}

/// How a feature combines with the solid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Combine {
    /// The union: the body grows (a stub, a probe's housing).
    Add,
    /// The difference: the body loses the feature's volume, which becomes conductor (a post, a
    /// screw, a notch).
    Subtract,
    /// The intersection: only what both hold remains.
    Intersect,
}

/// A primitive combined with the solid's shape: its own origin at `at`, its z axis turned onto
/// `axis`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Feature {
    pub combine: Combine,
    pub shape: Shape,
    pub at: [f64; 3],
    #[serde(default)]
    pub axis: Axis,
}

impl Feature {
    /// The bounding box (lo, hi).
    pub fn bounds(&self) -> ([f64; 3], [f64; 3]) {
        let (lo, hi) = self.shape.bounds();
        let (a, b) = (self.axis.turn(lo), self.axis.turn(hi));
        ([0, 1, 2].map(|d| a[d].min(b[d]) + self.at[d]), [0, 1, 2].map(|d| a[d].max(b[d]) + self.at[d]))
    }
}

/// An open boundary: a perfectly matched layer of this thickness (m) lining every face of a box
/// solid, the conducting walls behind it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Open {
    pub thickness: f64,
}

/// What drives an open problem.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Source {
    /// A short current element: `current` (A, peak) along the unit `direction` over `length`
    /// (m) at `at` — the Hertzian dipole of moment I·l.
    Dipole { at: [f64; 3], direction: [f64; 3], length: f64, current: f64 },
    /// A plane wave of 1 V/m travelling along the unit `direction`, its electric field along the
    /// unit `polarization` (perpendicular to it); the problem is solved for the field the
    /// dielectric inclusions scatter.
    PlaneWave { direction: [f64; 3], polarization: [f64; 3] },
}

/// A lumped (discrete) port: a gap from `from` to `to` (m) across which a source of `voltage`
/// (V, peak, complex as [re, im]) in series with `impedance` (Ω) drives current from `from`
/// towards `to` — the feed of an antenna, its terminals the conductors at either end.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LumpedPort {
    pub name: String,
    pub from: [f64; 3],
    pub to: [f64; 3],
    pub impedance: f64,
    #[serde(default = "unit_voltage")]
    pub voltage: [f64; 2],
    /// The current spread evenly round a band of this radius (m) about the gap's line — the
    /// surface of the wire a centre-fed antenna's gap cuts — instead of along the line itself.
    #[serde(default)]
    pub radius: f64,
}

fn unit_voltage() -> [f64; 2] {
    [1.0, 0.0]
}

fn unit(name: &str, v: [f64; 3]) -> Result<[f64; 3]> {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if !(l.is_finite() && l > 0.0) {
        return Err(invalid(name, "must be a nonzero finite vector"));
    }
    Ok(v.map(|c| c / l))
}

impl Source {
    /// The source with its directions made unit vectors, checked.
    pub fn checked(self) -> Result<Source> {
        Ok(match self {
            Source::Dipole { at, direction, length, current } => {
                if !at.iter().all(|v| v.is_finite()) {
                    return Err(invalid("dipole", "its position must be finite"));
                }
                positive("dipole length", length)?;
                if !current.is_finite() || current == 0.0 {
                    return Err(invalid("dipole current", "must be a finite nonzero number"));
                }
                Source::Dipole { at, direction: unit("dipole direction", direction)?, length, current }
            }
            Source::PlaneWave { direction, polarization } => {
                let (k, e) = (unit("plane-wave direction", direction)?, unit("plane-wave polarization", polarization)?);
                if (k[0] * e[0] + k[1] * e[1] + k[2] * e[2]).abs() > 1e-9 {
                    return Err(invalid("plane wave", "its polarization must be perpendicular to its direction"));
                }
                Source::PlaneWave { direction: k, polarization: e }
            }
        })
    }
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
    /// A port on a rectangle of a z plane inside the geometry instead of at one of its ends (a
    /// guide's closed end inside a body, feeding an antenna); `end` then says which side the
    /// guide lies (start: above the plane, its outward normal −z; end: below it).
    #[serde(default)]
    pub face: Option<PortFace>,
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

/// The model.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub materials: Vec<Material>,
    pub waveguide: Option<Waveguide>,
    /// A primitive body instead of a waveguide.
    #[serde(default)]
    pub solid: Option<Solid>,
    /// Bodies inside the solid (other materials, conductors).
    #[serde(default)]
    pub inclusions: Vec<Inclusion>,
    /// Primitives combined with the solid's shape, in order.
    #[serde(default)]
    pub features: Vec<Feature>,
    pub ports: Vec<Port>,
    /// The conductivity (S/m) of the guide's walls (and its irises and steps); perfectly
    /// conducting when `None`.
    #[serde(default)]
    pub wall_conductivity: Option<f64>,
    /// An open boundary (radiation into free space through a PML).
    #[serde(default)]
    pub open: Option<Open>,
    /// What drives an open problem.
    #[serde(default)]
    pub sources: Vec<Source>,
    /// Lumped ports (antenna feeds).
    #[serde(default)]
    pub lumped: Vec<LumpedPort>,
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
        self.inclusions.clear();
        self.features.clear();
        self.open = None;
        self.sources.clear();
        self.lumped.clear();
        self.mesh = None;
        Ok(())
    }

    /// Sets a primitive body as the geometry (replacing a waveguide); the mesh is dropped.
    pub fn set_solid(&mut self, s: Solid) -> Result<()> {
        s.check_shape()?;
        self.material(&s.material)?;
        self.solid = Some(s);
        self.waveguide = None;
        self.inclusions.clear();
        self.features.clear();
        self.open = None;
        self.sources.clear();
        self.lumped.clear();
        self.mesh = None;
        Ok(())
    }

    /// Adds a body inside the solid; it must lie within the solid's bounding box, apart from the
    /// other inclusions' (the mesher checks the surfaces do not meet).
    pub fn add_inclusion(&mut self, inc: Inclusion) -> Result<()> {
        let solid = self.solid.as_ref().ok_or_else(|| invalid("inclusion", "needs a solid (geometry.solid) to lie in"))?;
        Solid { shape: inc.shape.clone(), material: solid.material.clone() }.check_shape()?;
        if !inc.at.iter().all(|v| v.is_finite()) {
            return Err(invalid("inclusion", "its position must be finite"));
        }
        if let Some(m) = &inc.material {
            self.material(m)?;
        }
        let (lo, hi) = inc.bounds();
        let (slo, shi) = solid.shape.bounds();
        if (0..3).any(|d| lo[d] <= slo[d] || hi[d] >= shi[d]) {
            return Err(invalid("inclusion", "must lie strictly inside the solid's bounding box"));
        }
        for other in &self.inclusions {
            let (olo, ohi) = other.bounds();
            if (0..3).all(|d| lo[d] < ohi[d] && olo[d] < hi[d]) {
                return Err(invalid("inclusion", "not supported yet: inclusions whose bounding boxes overlap"));
            }
        }
        self.inclusions.push(inc);
        self.mesh = None;
        Ok(())
    }

    /// Combines a primitive with the solid (after the features already there). Its box must
    /// meet the solid's; the mesher checks the surfaces meet cleanly.
    pub fn add_feature(&mut self, f: Feature) -> Result<()> {
        let solid = self.solid.as_ref().ok_or_else(|| invalid("feature", "needs a solid (geometry.solid) to combine with"))?;
        Solid { shape: f.shape.clone(), material: solid.material.clone() }.check_shape()?;
        if !f.at.iter().all(|v| v.is_finite()) {
            return Err(invalid("feature", "its position must be finite"));
        }
        let (lo, hi) = f.bounds();
        let (slo, shi) = self.bounds().unwrap_or_else(|| solid.shape.bounds());
        if !(0..3).all(|d| lo[d] <= shi[d] && slo[d] <= hi[d]) {
            return Err(invalid("feature", "must meet the solid (their bounding boxes are apart)"));
        }
        self.features.push(f);
        self.mesh = None;
        Ok(())
    }

    /// The bounding box of the geometry.
    pub fn bounds(&self) -> Option<([f64; 3], [f64; 3])> {
        if let Some(w) = &self.waveguide {
            return Some(([0.0; 3], [w.width, w.height, w.length()]));
        }
        let s = self.solid.as_ref()?;
        let (mut lo, mut hi) = s.shape.bounds();
        for f in self.features.iter().filter(|f| f.combine == Combine::Add) {
            let (a, b) = f.bounds();
            for d in 0..3 {
                lo[d] = lo[d].min(a[d]);
                hi[d] = hi[d].max(b[d]);
            }
        }
        Some((lo, hi))
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
        if let Some(s) = &self.solid {
            // A port needs the end face whole: no feature may reach it.
            let (_, hi) = s.shape.bounds();
            let plane = match end {
                End::Start => 0.0,
                End::End => hi[2],
            };
            let tol = 1e-9 * hi[2].abs().max(1e-300);
            if self.features.iter().any(|f| {
                let (a, b) = f.bounds();
                a[2] <= plane + tol && b[2] >= plane - tol
            }) {
                return Err(invalid("port", "a feature reaches the end face; a port needs the guide's whole cross-section there"));
            }
        }
        match self.solid.as_ref().map(|s| &s.shape) {
            Some(Shape::Box { size }) => Ok(PortShape::Rect { x0: 0.0, y0: 0.0, a: size[0], b: size[1] }),
            Some(Shape::Cylinder { radius, .. }) => Ok(PortShape::Circle { cx: 0.0, cy: 0.0, r: *radius }),
            Some(Shape::Sphere { .. }) => Err(invalid("port", "a sphere has no flat end faces for ports")),
            Some(shape @ Shape::Surface { positions, triangles, .. }) => {
                // The end plane's triangles: a full rectangle, or a disc (a polygon round it).
                let tris = &shape.end_triangles()[match end {
                    End::Start => 0,
                    End::End => 1,
                }];
                if tris.is_empty() {
                    return Err(invalid("port", "the imported surface has no flat face on its end plane (its lowest or highest z)"));
                }
                let mut lo = [f64::INFINITY; 2];
                let mut hi = [f64::NEG_INFINITY; 2];
                let mut area = 0.0;
                for &i in tris {
                    let [a, b, c] = triangles[i].map(|v| positions[v as usize]);
                    area += 0.5 * ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs();
                    for p in [a, b, c] {
                        for d in 0..2 {
                            lo[d] = lo[d].min(p[d]);
                            hi[d] = hi[d].max(p[d]);
                        }
                    }
                }
                let (w, h) = (hi[0] - lo[0], hi[1] - lo[1]);
                if (area - w * h).abs() <= 1e-6 * w * h {
                    Ok(PortShape::Rect { x0: lo[0], y0: lo[1], a: w, b: h })
                } else if (w - h).abs() <= 1e-3 * w && (area / (std::f64::consts::PI * w * h / 4.0) - 1.0).abs() < 0.05 {
                    Ok(PortShape::Circle { cx: 0.5 * (lo[0] + hi[0]), cy: 0.5 * (lo[1] + hi[1]), r: 0.25 * (w + h) })
                } else {
                    Err(invalid("port", "the end face of the imported surface is neither a whole rectangle nor a disc"))
                }
            }
            None => Err(invalid("model", "has no geometry")),
        }
    }

    /// Lines the box solid's faces with a perfectly matched layer of `thickness` (m), or closes
    /// it again with `None`.
    pub fn set_open(&mut self, open: Option<Open>) -> Result<()> {
        if let Some(o) = open {
            let Some(Solid { shape: Shape::Box { size }, .. }) = &self.solid else {
                return Err(invalid(
                    "open boundary",
                    "not supported yet: a perfectly matched layer needs a box solid (geometry.solid, shape box) to line",
                ));
            };
            positive("layer thickness", o.thickness)?;
            if size.iter().any(|s| 2.0 * o.thickness >= *s) {
                return Err(invalid("layer thickness", "must leave room inside the box (under half its smallest side)"));
            }
            if self.ports.iter().any(|p| p.face.is_none()) {
                return Err(invalid("open boundary", "ports at the box's ends would lie under the layer: put them on faces inside it (`face`)"));
            }
        }
        self.open = open;
        Ok(())
    }

    /// Adds a source for an open problem.
    pub fn add_source(&mut self, src: Source) -> Result<()> {
        let src = src.checked()?;
        if let Source::Dipole { at, .. } = src
            && let Some((lo, hi)) = self.bounds()
            && (0..3).any(|d| at[d] <= lo[d] || at[d] >= hi[d])
        {
            return Err(invalid("dipole", "must lie inside the geometry"));
        }
        self.sources.push(src);
        Ok(())
    }

    /// Adds a lumped port.
    pub fn add_lumped_port(&mut self, p: LumpedPort) -> Result<()> {
        if p.name.trim().is_empty() {
            return Err(invalid("port name", "must not be empty"));
        }
        if self.lumped.iter().any(|x| x.name == p.name) || self.ports.iter().any(|x| x.name == p.name) {
            return Err(ModelError::Duplicate { kind: "port", name: p.name });
        }
        positive("port impedance", p.impedance)?;
        if !(p.radius.is_finite() && p.radius >= 0.0) {
            return Err(invalid("lumped port radius", "must be a finite number ≥ 0"));
        }
        if !p.from.iter().chain(&p.to).chain(&p.voltage).all(|v| v.is_finite()) {
            return Err(invalid("lumped port", "its ends and voltage must be finite"));
        }
        let gap = ((0..3).map(|d| (p.to[d] - p.from[d]).powi(2)).sum::<f64>()).sqrt();
        if !(gap > 0.0) {
            return Err(invalid("lumped port", "its two ends must differ"));
        }
        if let Some((lo, hi)) = self.bounds()
            && [p.from, p.to].iter().any(|q| (0..3).any(|d| q[d] <= lo[d] || q[d] >= hi[d]))
        {
            return Err(invalid("lumped port", "must lie inside the geometry"));
        }
        self.lumped.push(p);
        Ok(())
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
        if self.open.is_some() && p.face.is_none() {
            return Err(invalid("port", "with an open boundary a port goes on a face inside the box (`face`), the box's ends being under the layer"));
        }
        if let Some(f) = &p.face {
            if self.solid.is_none() {
                return Err(invalid("port", "a port on a face needs a solid (geometry.solid)"));
            }
            if ![f.z, f.x0, f.y0].iter().all(|v| v.is_finite()) {
                return Err(invalid("port face", "must be finite"));
            }
            positive("port face width", f.a)?;
            positive("port face height", f.b)?;
        }
        if self.ports.iter().any(|x| x.name == p.name) || self.lumped.iter().any(|x| x.name == p.name) {
            return Err(ModelError::Duplicate { kind: "port", name: p.name });
        }
        if p.face.is_none() && self.ports.iter().any(|x| x.face.is_none() && x.end == p.end) {
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

    /// A port's cross-section: its face's rectangle, or the geometry's end.
    pub fn port_shape_of(&self, p: &Port) -> Result<PortShape> {
        match p.face {
            Some(f) => Ok(PortShape::Rect { x0: f.x0, y0: f.y0, a: f.a, b: f.b }),
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
        m.add_port(Port { name: "1".into(), end: End::Start, modes: 4, face: None }).unwrap();
        assert!(m.add_port(Port { name: "2".into(), end: End::Start, modes: 4, face: None }).is_err());
        assert!(m.add_port(Port { name: "2".into(), end: End::End, modes: 0, face: None }).is_err());
        assert!(m.add_port(Port { name: "2".into(), end: End::End, modes: 1000, face: None }).is_err());
        m.add_port(Port { name: "2".into(), end: End::End, modes: 1, face: None }).unwrap();
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
