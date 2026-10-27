//! EMCraft's solver-neutral electromagnetic model: materials, the waveguide geometry, ports, the
//! tetrahedral mesh and the model check. Everything is stored in SI ([`units`]); the model knows
//! no file format, no solver and no display unit.
//!
//! The first geometry is a rectangular waveguide along z: a cross-section of width `a` (x) and
//! height `b` (y), and sections along z each filled with one material. Its side walls are
//! perfect electric conductors; its two end faces carry the ports.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod geom;
pub mod mesh;
pub mod units;

pub use geom::{Aabb, CVec3, Point3, Vec3};
pub use mesh::{Boundary, BoundaryFace, Buckets, Grid, Mesh};
pub use num_complex::Complex64;
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

/// A linear, isotropic material, lossless unless it has a loss tangent or dispersion.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Material {
    pub name: String,
    /// Relative permittivity εr; with dispersion, its value at infinite frequency ε∞.
    pub eps_r: f64,
    /// Relative permeability μr.
    pub mu_r: f64,
    /// Dielectric loss tangent tan δ: the permittivity is εr (1 − j tan δ) (time dependence
    /// e^{jωt}).
    #[serde(default)]
    pub loss_tangent: f64,
    /// Dispersion: poles whose susceptibilities add to the permittivity ([`Material::eps_at`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dispersion: Vec<Pole>,
}

/// The most poles one material may carry.
pub const MAX_POLES: usize = 16;

/// One pole of a dispersive permittivity: a susceptibility χ(ω) added to ε∞. The forms are the
/// standard ones (A. Taflove, S. C. Hagness, *Computational Electrodynamics: The
/// Finite-Difference Time-Domain Method*, 3rd ed., Artech House 2005, §9.2 and §9.4), written
/// for the time dependence e^{jωt}, so that a lossy medium has ε = ε′ − jε″ with ε″ > 0.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Pole {
    /// Debye relaxation: χ = Δε / (1 + jωτ), τ the relaxation time (s).
    Debye { delta_eps: f64, tau: f64 },
    /// Lorentz resonance: χ = Δε ω₀² / (ω₀² − ω² + jωγ), ω₀ = 2π `frequency`, γ = 2π
    /// `damping` (both in Hz).
    Lorentz { delta_eps: f64, frequency: f64, damping: f64 },
    /// Drude (free carriers, a Lorentz pole with ω₀ = 0): χ = −ω_p² / (ω² − jωγ), ω_p = 2π
    /// `plasma_frequency`, γ = 2π `damping` (both in Hz).
    Drude { plasma_frequency: f64, damping: f64 },
}

impl Pole {
    /// Checks the pole: finite parameters, all > 0 (an undamped pole is singular at its
    /// resonance, an undamped Drude pole at zero frequency).
    pub fn checked(self) -> Result<Pole> {
        match self {
            Pole::Debye { delta_eps, tau } => {
                positive("Debye pole Δε", delta_eps)?;
                positive("Debye relaxation time", tau)?;
            }
            Pole::Lorentz { delta_eps, frequency, damping } => {
                positive("Lorentz pole Δε", delta_eps)?;
                positive("Lorentz resonance frequency", frequency)?;
                positive("Lorentz damping", damping)?;
            }
            Pole::Drude { plasma_frequency, damping } => {
                positive("Drude plasma frequency", plasma_frequency)?;
                positive("Drude damping", damping)?;
            }
        }
        Ok(self)
    }

    /// The pole as a second-order one, χ = a / (ω₀² − ω² + jωγ) in rad/s, or `None` for Debye.
    pub fn second_order(self) -> Option<(f64, f64, f64)> {
        let w = |f: f64| 2.0 * std::f64::consts::PI * f;
        match self {
            Pole::Debye { .. } => None,
            Pole::Lorentz { delta_eps, frequency, damping } => Some((delta_eps * w(frequency) * w(frequency), w(frequency), w(damping))),
            Pole::Drude { plasma_frequency, damping } => Some((w(plasma_frequency) * w(plasma_frequency), 0.0, w(damping))),
        }
    }

    /// The susceptibility χ(ω) at `frequency` (Hz).
    pub fn susceptibility(self, frequency: f64) -> Complex64 {
        let omega = 2.0 * std::f64::consts::PI * frequency;
        match self {
            Pole::Debye { delta_eps, tau } => Complex64::new(delta_eps, 0.0) / Complex64::new(1.0, omega * tau),
            p => match p.second_order() {
                Some((a, w0, g)) => Complex64::new(a, 0.0) / Complex64::new(w0 * w0 - omega * omega, omega * g),
                None => Complex64::new(0.0, 0.0),
            },
        }
    }
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
            dispersion: Vec::new(),
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

    /// The same material with dispersion poles (each checked, at most [`MAX_POLES`]).
    pub fn with_dispersion(mut self, poles: Vec<Pole>) -> Result<Material> {
        if poles.len() > MAX_POLES {
            return Err(invalid("dispersion", format!("holds at most {MAX_POLES} poles, got {}", poles.len())));
        }
        self.dispersion = poles.into_iter().map(Pole::checked).collect::<Result<_>>()?;
        Ok(self)
    }

    /// Whether the permittivity depends on the frequency.
    pub fn is_dispersive(&self) -> bool {
        !self.dispersion.is_empty()
    }

    /// The refractive index that sets the wavelength at `frequency` (Hz): √(εr μr), with
    /// dispersion √(|εr(ω)| μr).
    pub fn index_at(&self, frequency: f64) -> f64 {
        if self.is_dispersive() { (self.eps_at(frequency).norm() * self.mu_r).sqrt() } else { (self.eps_r * self.mu_r).sqrt() }
    }

    /// The complex relative permittivity at `frequency` (Hz): εr (1 − j tan δ) + Σ χ_k(ω)
    /// (time dependence e^{jωt}, so ε = ε′ − jε″).
    pub fn eps_at(&self, frequency: f64) -> Complex64 {
        let base = Complex64::new(self.eps_r, -self.eps_r * self.loss_tangent);
        self.dispersion.iter().fold(base, |e, p| e + p.susceptibility(frequency))
    }

    /// Checks every field again (a model read from a file has not been through the
    /// constructors).
    pub fn checked(&self) -> Result<()> {
        let again = Material::new(&self.name, self.eps_r, self.mu_r)?.with_loss_tangent(self.loss_tangent)?;
        again.with_dispersion(self.dispersion.clone()).map(|_| ())
    }

    pub fn vacuum() -> Material {
        Material { name: "vacuum".into(), eps_r: 1.0, mu_r: 1.0, loss_tangent: 0.0, dispersion: Vec::new() }
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
///
/// Planar circuits in a shielding box are guides too: dielectric `layers` fill it from y = 0 up
/// (a substrate, its whole width and length), and conducting `strips` of no thickness lie on
/// planes y = const (a microstrip on the substrate, a stripline between two layers).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Waveguide {
    /// Broad wall width a (m), along x: the widest aperture.
    pub width: f64,
    /// Narrow wall height b (m), along y.
    pub height: f64,
    pub sections: Vec<Section>,
    /// Layers from y = 0 up; above the last, each section's own material.
    #[serde(default)]
    pub layers: Vec<Layer>,
    #[serde(default)]
    pub strips: Vec<Strip>,
}

/// A dielectric layer across the whole guide.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    /// Thickness along y (m).
    pub thickness: f64,
    pub material: String,
}

/// A perfectly conducting strip of no thickness: [x0, x0 + width] × [z0, z1] on the plane y.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Strip {
    pub y: f64,
    pub x0: f64,
    pub width: f64,
    pub z0: f64,
    pub z1: f64,
}

/// An interval along one axis: from `start`, `width` long (m).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Span {
    pub start: f64,
    pub width: f64,
}

impl Span {
    pub const fn new(start: f64, width: f64) -> Span {
        Span { start, width }
    }

    pub fn end(self) -> f64 {
        self.start + self.width
    }

    /// Whether `v` lies strictly inside.
    pub fn contains(self, v: f64) -> bool {
        v > self.start && v < self.end()
    }

    /// Whether `o` lies inside, within `tol`.
    pub fn holds(self, o: Span, tol: f64) -> bool {
        self.start <= o.start + tol && o.end() <= self.end() + tol
    }

    /// Whether the two are the same within `tol`.
    pub fn same(self, o: Span, tol: f64) -> bool {
        (self.start - o.start).abs() <= tol && (self.width - o.width).abs() <= tol
    }
}

impl Waveguide {
    pub fn length(&self) -> f64 {
        self.sections.iter().map(|s| s.length).sum()
    }

    /// Section i's aperture across x.
    pub fn aperture(&self, i: usize) -> Option<Span> {
        let s = self.sections.get(i)?;
        let w = s.width.unwrap_or(self.width);
        Some(Span::new(s.offset.unwrap_or((self.width - w) / 2.0), w))
    }

    /// Section i's aperture across y.
    pub fn aperture_y(&self, i: usize) -> Option<Span> {
        let s = self.sections.get(i)?;
        let h = s.height.unwrap_or(self.height);
        Some(Span::new(s.offset_y.unwrap_or((self.height - h) / 2.0), h))
    }

    /// The whole width, [0, a].
    pub fn across_x(&self) -> Span {
        Span::new(0.0, self.width)
    }

    /// The whole height, [0, b].
    pub fn across_y(&self) -> Span {
        Span::new(0.0, self.height)
    }

    /// Whether every section has the full width (no H-plane change).
    pub fn full_width(&self) -> bool {
        (0..self.sections.len()).all(|i| self.aperture(i).is_some_and(|a| a.same(self.across_x(), 1e-12 * self.width)))
    }

    /// Whether every section has the full height (no E-plane change).
    pub fn full_height(&self) -> bool {
        (0..self.sections.len()).all(|i| self.aperture_y(i).is_some_and(|a| a.same(self.across_y(), 1e-12 * self.height)))
    }

    /// Whether every section has the full cross-section.
    pub fn uniform(&self) -> bool {
        self.full_width() && self.full_height()
    }

    /// Whether the guide is a plain one: no layers, no strips.
    pub fn plain(&self) -> bool {
        self.layers.is_empty() && self.strips.is_empty()
    }

    /// The layer holding height y (the tops of the layers, bottom up), `None` above them.
    pub fn layer_at(&self, y: f64) -> Option<&Layer> {
        let mut top = 0.0;
        self.layers.iter().find(|l| {
            top += l.thickness;
            y < top
        })
    }

    /// The planes y = const between layers.
    pub fn layer_tops(&self) -> Vec<f64> {
        self.layers
            .iter()
            .scan(0.0, |top, l| {
                *top += l.thickness;
                Some(*top)
            })
            .collect()
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
    Box { size: Vec3 },
    /// Axis z through the origin, from z = 0 to `length`.
    Cylinder { radius: f64, length: f64 },
    /// Centred on the origin.
    Sphere { radius: f64 },
    /// A closed triangulated surface (imported): positions (m), outward triangles and a face
    /// tag per triangle.
    Surface { name: String, positions: Vec<Point3>, triangles: Vec<[u32; 3]>, tags: Vec<u32> },
}

/// Most triangles an imported surface may have.
pub const MAX_SURFACE_TRIANGLES: usize = 2_000_000;

impl Shape {
    /// The bounding box.
    pub fn bounds(&self) -> Aabb {
        match *self {
            Shape::Box { size } => Aabb::new(Point3::ORIGIN, size.to_point()),
            Shape::Cylinder { radius: r, length } => Aabb::new(Point3::new(-r, -r, 0.0), Point3::new(r, r, length)),
            Shape::Sphere { radius: r } => Aabb::new(Point3::new(-r, -r, -r), Point3::new(r, r, r)),
            Shape::Surface { ref positions, .. } => Aabb::around(positions.iter().copied()).unwrap_or_default(),
        }
    }

    /// Whether a point in the shape's own frame lies inside it (an imported surface by its winding
    /// number).
    pub fn contains(&self, p: Point3) -> bool {
        match *self {
            Shape::Box { size } => (0..3).all(|d| p[d] > 0.0 && p[d] < size[d]),
            Shape::Cylinder { radius, length } => p.z > 0.0 && p.z < length && p.x * p.x + p.y * p.y < radius * radius,
            Shape::Sphere { radius } => p.to_vec().norm_sqr() < radius * radius,
            Shape::Surface { ref positions, ref triangles, .. } => {
                let corner = |v: u32| positions.get(v as usize).copied().unwrap_or(Point3::ORIGIN) - p;
                let total: f64 = triangles.iter().map(|t| solid_angle(t.map(corner))).sum();
                total / (4.0 * std::f64::consts::PI) > 0.5
            }
        }
    }

    /// An imported surface's triangles on its lowest and highest z planes, the faces a port can
    /// close (empty for the primitives, whose ends the mesher knows).
    pub fn end_triangles(&self) -> [Vec<usize>; 2] {
        let Shape::Surface { positions, triangles, .. } = self else { return [Vec::new(), Vec::new()] };
        let Aabb { min: lo, max: hi } = self.bounds();
        let tol = 1e-9 * (0..3).map(|d| hi[d] - lo[d]).fold(0.0, f64::max);
        let on = |t: &[u32; 3], z: f64| t.iter().all(|v| positions.get(*v as usize).is_some_and(|p| (p[2] - z).abs() <= tol));
        [lo[2], hi[2]].map(|z| (0..triangles.len()).filter(|&i| on(&triangles[i], z)).collect())
    }

    pub fn volume(&self) -> f64 {
        match *self {
            Shape::Box { size } => size.x * size.y * size.z,
            Shape::Cylinder { radius, length } => std::f64::consts::PI * radius * radius * length,
            Shape::Sphere { radius } => 4.0 / 3.0 * std::f64::consts::PI * radius.powi(3),
            Shape::Surface { ref positions, ref triangles, .. } => triangles
                .iter()
                .map(|t| {
                    let [a, b, c] = t.map(|v| positions.get(v as usize).copied().unwrap_or(Point3::ORIGIN).to_vec());
                    geom::triple(a, b, c) / 6.0
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
    pub at: Point3,
    /// The filling; `None` for a perfect conductor (a hole in the mesh).
    pub material: Option<String>,
}

impl Inclusion {
    pub fn contains(&self, p: Point3) -> bool {
        self.shape.contains((p - self.at).to_point())
    }

    pub fn bounds(&self) -> Aabb {
        let b = self.shape.bounds();
        Aabb::new(b.min + self.at.to_vec(), b.max + self.at.to_vec())
    }
}

impl Solid {
    /// The shape's dimensions are finite and positive.
    pub fn check_shape(&self) -> Result<()> {
        match self.shape {
            Shape::Box { size } => {
                for (k, v) in size.iter().enumerate() {
                    positive(&format!("box size {}", ["x", "y", "z"][k]), v)?;
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
                if !positions.iter().all(|c| c.is_finite()) {
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
    pub fn turn(self, p: Vec3) -> Vec3 {
        match self {
            Axis::X => Vec3::new(p.z, p.x, p.y),
            Axis::Y => Vec3::new(p.y, p.z, p.x),
            Axis::Z => p,
        }
    }

    /// The inverse of [`Axis::turn`]: global coordinates into the shape's own.
    pub fn unturn(self, p: Vec3) -> Vec3 {
        match self {
            Axis::X => Vec3::new(p.y, p.z, p.x),
            Axis::Y => Vec3::new(p.z, p.x, p.y),
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
    pub at: Point3,
    #[serde(default)]
    pub axis: Axis,
}

impl Feature {
    /// Whether a point lies inside the feature's body.
    pub fn contains(&self, p: Point3) -> bool {
        self.shape.contains(self.axis.unturn(p - self.at).to_point())
    }

    /// The bounding box.
    pub fn bounds(&self) -> Aabb {
        let own = self.shape.bounds();
        let (a, b) = (self.axis.turn(own.min.to_vec()), self.axis.turn(own.max.to_vec()));
        Aabb::new(self.at + a.min(b), self.at + a.max(b))
    }
}

/// What a symmetry plane is: the wall that stands in for the half of the model it cuts away.
///
/// A model symmetric about a plane carries fields of two parities about it, and each parity is
/// the field of the half model closed on the plane by a wall (J.-M. Jin, *The Finite Element
/// Method in Electromagnetics*, 3rd ed., Wiley 2014: ch. 1 on the boundary conditions, ch. 8 on
/// their essential and natural imposition with edge elements; R. F. Harrington, *Time-Harmonic
/// Electromagnetic Fields*, McGraw-Hill 1961, ch. 3 on duality and images):
/// - an electric wall (perfect electric conductor): tangential E = 0 on the plane — the field
///   whose tangential E is odd and normal E even about it (TE20 about x = a/2 in a rectangular
///   guide; TE10 about y = b/2, its E being normal to that plane);
/// - a magnetic wall (perfect magnetic conductor): tangential H = 0, so normal E = 0 — the field
///   whose tangential E is even and normal E odd (TE10 about x = a/2).
///
/// With edge elements the electric wall is the essential condition (the tangential unknowns on
/// the plane are eliminated, as on the conducting walls) and the magnetic wall the natural one
/// (the unknowns are kept and no surface term is added): n̂ × (μr⁻¹ ∇ × E) = 0 holds weakly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SymmetryKind {
    /// A perfect electric conductor on the plane: tangential E = 0.
    Electric,
    /// A perfect magnetic conductor on the plane: tangential H = 0.
    Magnetic,
}

/// A plane of symmetry `axis` = `at` (m) through the model: only the side below it is meshed and
/// solved, the plane closed by the wall of `kind`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Symmetry {
    pub axis: Axis,
    pub at: f64,
    pub kind: SymmetryKind,
}

impl Symmetry {
    /// Mirrors a point from the side cut away onto the side kept (the point itself when it is
    /// kept), with the signs the image's E and H components take: an electric wall flips E's
    /// tangential components and H's normal one, a magnetic wall E's normal component and H's
    /// tangential ones (H being a pseudovector).
    pub fn image(self, p: Point3) -> (Point3, [f64; 3], [f64; 3]) {
        let d = self.axis.index();
        if p[d] <= self.at {
            return (p, [1.0; 3], [1.0; 3]);
        }
        let q = p.with(d, 2.0 * self.at - p[d]);
        let normal = |k: usize| if k == d { -1.0 } else { 1.0 };
        let tangential = |k: usize| if k == d { 1.0 } else { -1.0 };
        match self.kind {
            SymmetryKind::Electric => (q, [0, 1, 2].map(tangential), [0, 1, 2].map(normal)),
            SymmetryKind::Magnetic => (q, [0, 1, 2].map(normal), [0, 1, 2].map(tangential)),
        }
    }
}

/// An open boundary: a perfectly matched layer of this thickness (m) lining every face of a box
/// solid, the conducting walls behind it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Open {
    pub thickness: f64,
    /// The reflection the layer is designed for at normal incidence (its absorption profile's
    /// strength); `None` for the default, 1e-6.
    #[serde(default)]
    pub reflection: Option<f64>,
}

/// What drives an open problem.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Source {
    /// A short current element: `current` (A, peak) along the unit `direction` over `length`
    /// (m) at `at` — the Hertzian dipole of moment I·l.
    Dipole { at: Point3, direction: Vec3, length: f64, current: f64 },
    /// A plane wave of 1 V/m travelling along the unit `direction`, its electric field along the
    /// unit `polarization` (perpendicular to it); the problem is solved for the field the
    /// dielectric inclusions scatter.
    PlaneWave { direction: Vec3, polarization: Vec3 },
}

/// A lumped (discrete) port: a gap from `from` to `to` (m) across which a source of `voltage`
/// (V, peak, complex as [re, im]) in series with `impedance` (Ω) drives current from `from`
/// towards `to` — the feed of an antenna, its terminals the conductors at either end.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LumpedPort {
    pub name: String,
    pub from: Point3,
    pub to: Point3,
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

fn unit(name: &str, v: Vec3) -> Result<Vec3> {
    v.normalized().ok_or_else(|| invalid(name, "must be a nonzero finite vector"))
}

/// The solid angle a triangle subtends at the origin, its corners given relative to it (A. Van
/// Oosterom, J. Strackee, IEEE Trans. Biomed. Eng. 30(2), 1983).
pub fn solid_angle([a, b, c]: [Vec3; 3]) -> f64 {
    let (la, lb, lc) = (a.norm(), b.norm(), c.norm());
    2.0 * geom::triple(a, b, c).atan2(la * lb * lc + a.dot(b) * lc + b.dot(c) * la + c.dot(a) * lb)
}

impl Source {
    /// The source with its directions made unit vectors, checked.
    pub fn checked(self) -> Result<Source> {
        Ok(match self {
            Source::Dipole { at, direction, length, current } => {
                if !at.is_finite() {
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
}

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

impl Port {
    /// A port at `end` with four modes, the closed forms' where they apply, its S-parameters
    /// in its own mode's power waves.
    pub fn new(name: impl Into<String>, end: End) -> Port {
        Port { name: name.into(), end, modes: 4, face: None, mode_solver: ModeSolver::Auto, reference: None, deembed: 0.0 }
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

    /// The port's S-parameters referred to `ohms`.
    pub fn referred_to(self, ohms: f64) -> Port {
        Port { reference: Some(ohms), ..self }
    }
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
    /// The job's frequency band (Hz): the default for meshing by wavelength, sweeps and port
    /// previews.
    #[serde(default)]
    pub band: Option<Band>,
    /// Symmetry planes (at most one across x and one across y): the model is meshed and solved
    /// on the side below them only.
    #[serde(default)]
    pub symmetry: Vec<Symmetry>,
    /// The mesh of the waveguide (made by `emcraft-mesh`).
    #[serde(skip)]
    pub mesh: Option<Mesh>,
}

/// A frequency band [start, stop] (Hz).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Band {
    pub start: f64,
    pub stop: f64,
}

impl Band {
    /// The band, checked: finite, 0 < start ≤ stop.
    pub fn new(start: f64, stop: f64) -> Result<Band> {
        if !(start.is_finite() && stop.is_finite() && start > 0.0 && stop >= start) {
            return Err(invalid("frequency band", format!("must be finite with 0 < start ≤ stop, got {start} to {stop}")));
        }
        Ok(Band { start, stop })
    }
}

impl Default for Model {
    /// An empty model with one material, vacuum.
    fn default() -> Model {
        Model {
            materials: vec![Material::vacuum()],
            waveguide: None,
            solid: None,
            inclusions: Vec::new(),
            features: Vec::new(),
            ports: Vec::new(),
            wall_conductivity: None,
            open: None,
            sources: Vec::new(),
            lumped: Vec::new(),
            band: None,
            symmetry: Vec::new(),
            mesh: None,
        }
    }
}

impl Model {
    pub fn new() -> Model {
        Model::default()
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
            if !w.across_x().holds(w.aperture(i).unwrap_or(w.across_x()), tol) {
                return Err(invalid(format!("section {}", i + 1), "its aperture must lie within the guide's width"));
            }
            let toly = 1e-9 * w.height;
            if !w.across_y().holds(w.aperture_y(i).unwrap_or(w.across_y()), toly) {
                return Err(invalid(format!("section {}", i + 1), "its aperture must lie within the guide's height"));
            }
        }
        let toly = 1e-9 * w.height;
        for (i, l) in w.layers.iter().enumerate() {
            positive(&format!("thickness of layer {}", i + 1), l.thickness)?;
            self.material(&l.material)?;
        }
        if w.layers.iter().map(|l| l.thickness).sum::<f64>() > w.height + toly {
            return Err(invalid("layers", "are thicker than the guide is high"));
        }
        let length = w.length();
        for (i, st) in w.strips.iter().enumerate() {
            let what = || format!("strip {}", i + 1);
            if ![st.y, st.x0, st.width, st.z0, st.z1].iter().all(|v| v.is_finite()) {
                return Err(invalid(what(), "must be finite"));
            }
            positive(&format!("width of strip {}", i + 1), st.width)?;
            if !(st.y > toly && st.y < w.height - toly) {
                return Err(invalid(what(), "must lie inside the guide (0 < y < height)"));
            }
            if st.x0 < -tol || st.x0 + st.width > w.width + tol || !(st.z0 >= -1e-9 * length && st.z1 <= length * (1.0 + 1e-9) && st.z1 > st.z0) {
                return Err(invalid(what(), "must lie within the guide's width and length, z0 < z1"));
            }
            if !w.uniform() {
                return Err(invalid(what(), "not supported yet: strips in a guide whose aperture changes"));
            }
        }
        if !w.layers.is_empty() && !w.uniform() {
            return Err(invalid("layers", "not supported yet: layers in a guide whose aperture changes"));
        }
        // Neighbouring apertures must nest: one rectangle holds the other (steps, irises).
        let toly = 1e-9 * w.height;
        for i in 1..w.sections.len() {
            let rect = |k: usize| w.aperture(k).zip(w.aperture_y(k));
            if let (Some(p), Some(q)) = (rect(i - 1), rect(i)) {
                let holds = |(ax, ay): (Span, Span), (bx, by): (Span, Span)| ax.holds(bx, tol) && ay.holds(by, toly);
                if !holds(p, q) && !holds(q, p) {
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
        if !inc.at.is_finite() {
            return Err(invalid("inclusion", "its position must be finite"));
        }
        if let Some(m) = &inc.material {
            self.material(m)?;
        }
        let Aabb { min: lo, max: hi } = inc.bounds();
        let Aabb { min: slo, max: shi } = solid.shape.bounds();
        if (0..3).any(|d| lo[d] <= slo[d] || hi[d] >= shi[d]) {
            return Err(invalid("inclusion", "must lie strictly inside the solid's bounding box"));
        }
        for other in &self.inclusions {
            let Aabb { min: olo, max: ohi } = other.bounds();
            if (0..3).all(|d| lo[d] < ohi[d] && olo[d] < hi[d]) {
                return Err(invalid("inclusion", "not supported yet: inclusions whose bounding boxes overlap"));
            }
        }
        self.inclusions.push(inc);
        self.mesh = None;
        Ok(())
    }

    /// The corners of everything that radiates or scatters: inclusions, features, dipoles and
    /// the ends of lumped ports (an open problem's Huygens box encloses them).
    pub fn radiators(&self) -> impl Iterator<Item = Point3> + '_ {
        self.inclusions
            .iter()
            .flat_map(|i| i.bounds().corners())
            .chain(self.features.iter().flat_map(|f| f.bounds().corners()))
            .chain(self.sources.iter().filter_map(|s| match s {
                Source::Dipole { at, .. } => Some(*at),
                Source::PlaneWave { .. } => None,
            }))
            .chain(self.lumped.iter().flat_map(|l| [l.from, l.to]))
    }

    /// What fills a point of a solid model: `None` outside the body or inside a conductor, else
    /// the material's index (a dielectric inclusion's, or the solid's).
    pub fn material_at(&self, p: Point3) -> Option<usize> {
        let s = self.solid.as_ref()?;
        let mut inside = s.shape.contains(p);
        for f in &self.features {
            match f.combine {
                Combine::Add => inside |= f.contains(p),
                Combine::Subtract => inside &= !f.contains(p),
                Combine::Intersect => inside &= f.contains(p),
            }
        }
        if !inside {
            return None;
        }
        for inc in &self.inclusions {
            if inc.contains(p) {
                return match &inc.material {
                    Some(m) => self.material(m).ok().map(|(i, _)| i),
                    None => None,
                };
            }
        }
        self.material(&s.material).ok().map(|(i, _)| i)
    }

    /// Combines a primitive with the solid (after the features already there). Its box must
    /// meet the solid's; the mesher checks the surfaces meet cleanly.
    pub fn add_feature(&mut self, f: Feature) -> Result<()> {
        let solid = self.solid.as_ref().ok_or_else(|| invalid("feature", "needs a solid (geometry.solid) to combine with"))?;
        Solid { shape: f.shape.clone(), material: solid.material.clone() }.check_shape()?;
        if !f.at.is_finite() {
            return Err(invalid("feature", "its position must be finite"));
        }
        if !f.bounds().overlaps(self.bounds().unwrap_or_else(|| solid.shape.bounds())) {
            return Err(invalid("feature", "must meet the solid (their bounding boxes are apart)"));
        }
        self.features.push(f);
        self.mesh = None;
        Ok(())
    }

    /// The bounding box of the geometry.
    pub fn bounds(&self) -> Option<Aabb> {
        if let Some(w) = &self.waveguide {
            return Some(Aabb::new(Point3::ORIGIN, Point3::new(w.width, w.height, w.length())));
        }
        let s = self.solid.as_ref()?;
        Some(self.features.iter().filter(|f| f.combine == Combine::Add).map(Feature::bounds).fold(s.shape.bounds(), Aabb::union))
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
            let Span { start: x0, width: a } = w.aperture(i).ok_or_else(|| invalid("waveguide", "has no sections"))?;
            let Span { start: y0, width: b } = w.aperture_y(i).ok_or_else(|| invalid("waveguide", "has no sections"))?;
            return Ok(PortShape::Rect { x0, y0, a, b });
        }
        if let Some(s) = &self.solid {
            // A port needs the end face whole: no feature may reach it.
            let hi = s.shape.bounds().max;
            let plane = match end {
                End::Start => 0.0,
                End::End => hi[2],
            };
            let tol = 1e-9 * hi[2].abs().max(1e-300);
            if self.features.iter().any(|f| {
                let b = f.bounds();
                b.min.z <= plane + tol && b.max.z >= plane - tol
            }) {
                return Err(invalid("port", "a feature reaches the end face; a port needs the guide's whole cross-section there"));
            }
        }
        match self.solid.as_ref().map(|s| &s.shape) {
            Some(Shape::Box { size }) => Ok(PortShape::Rect { x0: 0.0, y0: 0.0, a: size.x, b: size.y }),
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
            if let Some(r) = o.reflection
                && !(r > 0.0 && r < 1.0)
            {
                return Err(invalid("layer reflection", "must be between 0 and 1"));
            }
            if size.iter().any(|s| 2.0 * o.thickness >= s) {
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
            && let Some(Aabb { min: lo, max: hi }) = self.bounds()
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
        if !(p.from.is_finite() && p.to.is_finite() && p.voltage.iter().all(|v| v.is_finite())) {
            return Err(invalid("lumped port", "its ends and voltage must be finite"));
        }
        let gap = p.from.distance(p.to);
        if !(gap > 0.0) {
            return Err(invalid("lumped port", "its two ends must differ"));
        }
        if let Some(Aabb { min: lo, max: hi }) = self.bounds()
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
        if let Some(r) = p.reference {
            positive(&format!("reference impedance of port `{}`", p.name), r)?;
        }
        if !p.deembed.is_finite() {
            return Err(invalid(format!("de-embedding of port `{}`", p.name), "must be finite"));
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

    /// Sets the symmetry planes (none clears them) after checking the geometry is symmetric
    /// about each; the mesh is dropped.
    pub fn set_symmetry(&mut self, planes: Vec<Symmetry>) -> Result<()> {
        if let Some(why) = self.symmetry_issue_of(&planes) {
            return Err(invalid("symmetry", why));
        }
        self.symmetry = planes;
        self.mesh = None;
        Ok(())
    }

    /// Why the model is not symmetric about its symmetry planes, if it is not (the geometry may
    /// have changed since they were set).
    pub fn symmetry_issue(&self) -> Option<String> {
        self.symmetry_issue_of(&self.symmetry)
    }

    /// The fraction of the model the symmetry planes keep: ½ per plane.
    pub fn symmetry_fraction(&self) -> f64 {
        0.5f64.powi(self.symmetry.len().min(2) as i32)
    }

    fn symmetry_issue_of(&self, planes: &[Symmetry]) -> Option<String> {
        if planes.is_empty() {
            return None;
        }
        if planes.len() > 2 {
            return Some(format!("at most two planes (one across x, one across y), got {}", planes.len()));
        }
        if planes.iter().any(|p| p.axis == Axis::Z) {
            return Some("a symmetry plane must be x = const or y = const (the ports lie on z planes)".into());
        }
        if planes.len() == 2 && planes[0].axis == planes[1].axis {
            return Some("two planes across the same axis".into());
        }
        if planes.iter().any(|p| !p.at.is_finite()) {
            return Some("the plane's position must be finite".into());
        }
        let Some(w) = &self.waveguide else {
            return Some(if self.solid.is_some() {
                "not supported yet: symmetry planes in a solid (they need the structured waveguide mesh, geometry.waveguide)".into()
            } else {
                "needs a waveguide (geometry.waveguide) to cut".into()
            });
        };
        for p in planes {
            let d = p.axis.index();
            let (span, name) = if d == 0 { (w.width, "x = a/2") } else { (w.height, "y = b/2") };
            let tol = 1e-9 * span;
            let mid = span / 2.0;
            if (p.at - mid).abs() > tol {
                return Some(format!("the guide's walls are symmetric only about its middle, {name} = {mid} m; the plane is at {} m", p.at));
            }
            for i in 0..w.sections.len() {
                let ap = if d == 0 { w.aperture(i) } else { w.aperture_y(i) };
                if let Some(a) = ap
                    && (a.start + a.end() - span).abs() > tol
                {
                    return Some(format!("section {}'s aperture is not centred on the plane: the guide is not symmetric about it", i + 1));
                }
            }
            if d == 1 && !w.layers.is_empty() {
                return Some("not supported yet: a plane across y in a layered guide (its layers from y = 0 are not symmetric about it)".into());
            }
            // Strips: each mirrored by one (itself when it straddles an x plane).
            let mirror = |s: &Strip| if d == 0 { Strip { x0: span - s.x0 - s.width, ..*s } } else { Strip { y: span - s.y, ..*s } };
            let t = 1e-9 * w.width.max(w.height).max(w.length());
            let same = |a: &Strip, b: &Strip| {
                (a.y - b.y).abs() <= t
                    && (a.x0 - b.x0).abs() <= t
                    && (a.width - b.width).abs() <= t
                    && (a.z0 - b.z0).abs() <= t
                    && (a.z1 - b.z1).abs() <= t
            };
            for (k, s) in w.strips.iter().enumerate() {
                if d == 1 && (s.y - mid).abs() <= tol {
                    return Some(format!("not supported yet: strip {} lies on the symmetry plane", k + 1));
                }
                let m = mirror(s);
                if !w.strips.iter().any(|o| same(o, &m)) {
                    return Some(format!("strip {} has no mirror image about the plane: the guide is not symmetric about it", k + 1));
                }
            }
        }
        None
    }

    /// Everything a frequency-domain solve needs, each problem named.
    pub fn check(&self) -> Vec<String> {
        let mut issues = Vec::new();
        if let Some(why) = self.symmetry_issue() {
            issues.push(format!("symmetry: {why} (clear it with model.symmetry, clear true)"));
        }
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
        Waveguide { width: 0.02286, height: 0.01016, sections: vec![Section::new(0.01, "vacuum")], layers: Vec::new(), strips: Vec::new() }
    }

    #[test]
    fn points_are_classified() {
        let mut m = Model::new();
        m.add_material(Material::new("ceramic", 4.0, 1.0).unwrap()).unwrap();
        m.set_solid(Solid { shape: Shape::Box { size: Vec3::new(0.04, 0.04, 0.04) }, material: "vacuum".into() }).unwrap();
        m.add_inclusion(Inclusion { shape: Shape::Sphere { radius: 0.005 }, at: Point3::new(0.02, 0.02, 0.02), material: Some("ceramic".into()) })
            .unwrap();
        m.add_inclusion(Inclusion { shape: Shape::Cylinder { radius: 0.001, length: 0.01 }, at: Point3::new(0.03, 0.03, 0.01), material: None })
            .unwrap();
        m.add_feature(Feature {
            combine: Combine::Subtract,
            shape: Shape::Cylinder { radius: 0.002, length: 0.04 },
            at: Point3::new(0.005, 0.0, 0.005),
            axis: Axis::Y,
        })
        .unwrap();
        assert_eq!(m.material_at(Point3::new(0.02, 0.02, 0.022)), Some(1));
        assert_eq!(m.material_at(Point3::new(0.01, 0.03, 0.03)), Some(0));
        assert_eq!(m.material_at(Point3::new(0.0305, 0.03, 0.015)), None, "the conducting rod");
        assert_eq!(m.material_at(Point3::new(0.006, 0.02, 0.006)), None, "the post along y");
        assert_eq!(m.material_at(Point3::new(0.05, 0.02, 0.02)), None, "outside");
        // A turned box: size [x, y, z] along y becomes [z, x, y].
        let f =
            Feature { combine: Combine::Add, shape: Shape::Box { size: Vec3::new(1.0, 2.0, 3.0) }, at: Point3::new(0.0, 0.0, 0.0), axis: Axis::Y };
        let b = f.bounds();
        assert!(f.contains(b.center()) && !f.contains(b.center().with(0, b.max.x + 0.1)));
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
    fn dispersion_is_checked_and_evaluated() {
        use std::f64::consts::PI;
        let x = || Material::new("x", 2.0, 1.0).unwrap();
        for bad in [
            Pole::Debye { delta_eps: f64::NAN, tau: 1e-11 },
            Pole::Debye { delta_eps: 1.0, tau: 0.0 },
            Pole::Debye { delta_eps: -1.0, tau: 1e-11 },
            Pole::Lorentz { delta_eps: 1.0, frequency: f64::INFINITY, damping: 1e9 },
            Pole::Lorentz { delta_eps: 1.0, frequency: 1e10, damping: -1.0 },
            Pole::Drude { plasma_frequency: 1e10, damping: 0.0 },
        ] {
            assert!(x().with_dispersion(vec![bad]).is_err(), "{bad:?}");
        }
        assert!(x().with_dispersion(vec![Pole::Debye { delta_eps: 1.0, tau: 1e-11 }; MAX_POLES + 1]).is_err());
        // Debye: ε(0) = ε∞ + Δε, ε(∞) = ε∞, at ωτ = 1 the loss peak ε″ = Δε/2.
        let tau = 1e-11;
        let d = x().with_dispersion(vec![Pole::Debye { delta_eps: 3.0, tau }]).unwrap();
        let e = d.eps_at(1.0 / (2.0 * PI * tau));
        assert!((e.re - 3.5).abs() < 1e-12 && (e.im + 1.5).abs() < 1e-12, "{e}");
        assert!((d.eps_at(1e-3).re - 5.0).abs() < 1e-9);
        // Lorentz: static ε∞ + Δε; above the resonance below ε∞; lossy (ε″ > 0).
        let l = x().with_dispersion(vec![Pole::Lorentz { delta_eps: 2.0, frequency: 1e10, damping: 1e9 }]).unwrap();
        assert!((l.eps_at(1.0).re - 4.0).abs() < 1e-9);
        assert!(l.eps_at(1e10).im < 0.0 && l.eps_at(3e10).re < 2.0);
        // Drude: χ = −ωp²/(ω² − jωγ).
        let (fp, g) = (1e10, 1e9);
        let dr = x().with_dispersion(vec![Pole::Drude { plasma_frequency: fp, damping: g }]).unwrap();
        let w = 2.0 * PI * 2e10;
        let expect = Complex64::new(2.0, 0.0) - (2.0 * PI * fp).powi(2) / Complex64::new(w * w, -w * 2.0 * PI * g);
        assert!((dr.eps_at(2e10) - expect).norm() < 1e-12);
        assert!(dr.eps_at(2e10).im < 0.0);
        dr.checked().unwrap();
        // Old models (no dispersion) still read; new ones round-trip.
        let old: Material = serde_json::from_str(r#"{"name":"a","eps_r":2.0,"mu_r":1.0}"#).unwrap();
        assert!(!old.is_dispersive());
        let back: Material = serde_json::from_str(&serde_json::to_string(&l).unwrap()).unwrap();
        assert_eq!(back, l);
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
        m.add_port(Port::new("1", End::Start).with_modes(4)).unwrap();
        assert!(m.add_port(Port::new("2", End::Start).with_modes(4)).is_err());
        assert!(m.add_port(Port::new("2", End::End).with_modes(0)).is_err());
        assert!(m.add_port(Port::new("2", End::End).with_modes(1000)).is_err());
        m.add_port(Port::new("2", End::End).with_modes(1)).unwrap();
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
        assert_eq!(m.bounds(), Some(Aabb::new(Point3::new(-0.01, -0.01, 0.0), Point3::new(0.01, 0.01, 0.03))));
        assert_eq!(m.port_shape(End::End).unwrap(), PortShape::Circle { cx: 0.0, cy: 0.0, r: 0.01 });
        m.set_solid(Solid { shape: Shape::Sphere { radius: 0.01 }, material: "vacuum".into() }).unwrap();
        assert!(m.port_shape(End::Start).is_err());
        assert!((Shape::Sphere { radius: 1.0 }.volume() - 4.18879).abs() < 1e-5);
    }

    #[test]
    fn symmetry_planes_need_a_symmetric_guide() {
        let mut m = Model::new();
        let plane = |axis, at, kind| Symmetry { axis, at, kind };
        assert!(m.set_symmetry(vec![plane(Axis::X, 0.01, SymmetryKind::Magnetic)]).is_err(), "no geometry");
        let sec = |w: Option<f64>, o: Option<f64>| Section { length: 0.01, material: "vacuum".into(), width: w, offset: o, ..Default::default() };
        let guide = |s: Vec<Section>| Waveguide { width: 0.02, height: 0.01, sections: s, layers: Vec::new(), strips: Vec::new() };
        m.set_waveguide(guide(vec![sec(None, None), sec(Some(0.01), None), sec(None, None)])).unwrap();
        m.set_symmetry(vec![plane(Axis::X, 0.01, SymmetryKind::Magnetic), plane(Axis::Y, 0.005, SymmetryKind::Electric)]).unwrap();
        assert_eq!(m.symmetry_fraction(), 0.25);
        assert!(m.check().iter().all(|i| !i.starts_with("symmetry")));
        for bad in [
            vec![plane(Axis::X, 0.009, SymmetryKind::Magnetic)],
            vec![plane(Axis::Z, 0.01, SymmetryKind::Magnetic)],
            vec![plane(Axis::X, f64::NAN, SymmetryKind::Magnetic)],
            vec![plane(Axis::X, 0.01, SymmetryKind::Magnetic), plane(Axis::X, 0.01, SymmetryKind::Electric)],
            vec![plane(Axis::X, 0.01, SymmetryKind::Magnetic); 3],
        ] {
            assert!(m.set_symmetry(bad).is_err());
        }
        assert_eq!(m.symmetry.len(), 2, "a refused change keeps the planes");
        // An off-centre aperture breaks the symmetry; the check says so.
        m.set_waveguide(guide(vec![sec(None, None), sec(Some(0.01), Some(0.0))])).unwrap();
        assert!(m.check().iter().any(|i| i.starts_with("symmetry")));
        m.set_symmetry(Vec::new()).unwrap();
        // Strips must mirror; a solid is refused.
        let mut w = guide(vec![sec(None, None)]);
        w.strips = vec![Strip { y: 0.002, x0: 0.002, width: 0.004, z0: 0.0, z1: 0.01 }];
        m.set_waveguide(w.clone()).unwrap();
        assert!(m.set_symmetry(vec![plane(Axis::X, 0.01, SymmetryKind::Magnetic)]).is_err());
        w.strips.push(Strip { y: 0.002, x0: 0.014, width: 0.004, z0: 0.0, z1: 0.01 });
        m.set_waveguide(w).unwrap();
        m.set_symmetry(vec![plane(Axis::X, 0.01, SymmetryKind::Electric)]).unwrap();
        m.set_solid(Solid { shape: Shape::Box { size: Vec3::new(0.02, 0.01, 0.01) }, material: "vacuum".into() }).unwrap();
        assert!(m.check().iter().any(|i| i.contains("not supported yet")));
        // Images: a magnetic wall flips E's normal component, H's tangential ones.
        let (q, e, h) = plane(Axis::X, 0.01, SymmetryKind::Magnetic).image(Point3::new(0.015, 0.0, 0.0));
        assert!((q.x - 0.005).abs() < 1e-15 && e == [-1.0, 1.0, 1.0] && h == [1.0, -1.0, -1.0]);
        let (_, e, h) = plane(Axis::Y, 0.005, SymmetryKind::Electric).image(Point3::new(0.0, 0.008, 0.0));
        assert!(e == [-1.0, 1.0, -1.0] && h == [1.0, -1.0, 1.0]);
    }

    #[test]
    fn apertures_must_fit_and_nest() {
        let mut m = Model::new();
        let sec = |w: Option<f64>, o: Option<f64>| Section { length: 0.01, material: "vacuum".into(), width: w, offset: o, ..Default::default() };
        let guide = |s: Vec<Section>| Waveguide { width: 0.02, height: 0.01, sections: s, layers: Vec::new(), strips: Vec::new() };
        m.set_waveguide(guide(vec![sec(None, None), sec(Some(0.01), None), sec(None, None)])).unwrap();
        let w = m.waveguide.as_ref().unwrap();
        assert_eq!(w.aperture(1), Some(Span::new(0.005, 0.01)));
        assert!(!w.uniform());
        assert!(m.set_waveguide(guide(vec![sec(Some(0.03), None)])).is_err());
        assert!(m.set_waveguide(guide(vec![sec(Some(0.01), Some(0.0)), sec(Some(0.01), Some(0.008))])).is_err());
        assert!(m.set_waveguide(guide(vec![sec(Some(0.01), Some(f64::NAN))])).is_err());
        m.set_waveguide(guide(vec![sec(Some(0.012), Some(0.0)), sec(Some(0.008), Some(0.002))])).unwrap();
    }
}
