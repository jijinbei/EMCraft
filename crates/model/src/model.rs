//! The model: everything a solve needs, changed only through setters that check what they are
//! given. The port, open-boundary and symmetry setters live with their types ([`crate::port`],
//! [`crate::open`], [`crate::symmetry`]).
//!
//! The fields are public to read: the mesher, the solvers, the renderers and the UI read them
//! throughout. The engine changes the model only through the setters; tests and the solvers'
//! own fixtures may build a model field by field.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::analysis::Analysis;
use crate::band::Band;
use crate::error::{ModelError, Result, invalid};
use crate::geom::{Aabb, Point3};
use crate::material::Material;
use crate::mesh::Mesh;
use crate::open::{LumpedPort, Open, Source, inside_ball};
use crate::periodic::Periodic;
use crate::port::Port;
use crate::quantity::{Conductivity, Metres};
use crate::shape::{Combine, Feature, Inclusion, Solid};
use crate::symmetry::Symmetry;
use crate::waveguide::Waveguide;

/// The thickest wall a model may have (m).
pub const MAX_WALL_THICKNESS: f64 = 1.0;

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
    pub wall_conductivity: Option<Conductivity>,
    /// The walls' thickness (m): drawn round the guide, and with a conductivity the layer the
    /// surface impedance sees through (never meshed); infinitely thick when `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wall_thickness: Option<Metres>,
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
    /// The kind of analysis chosen for the job; `None`: inferred from the model
    /// ([`Model::analysis`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analysis: Option<Analysis>,
    /// Symmetry planes (at most one across x and one across y): the model is meshed and solved
    /// on the side below them only.
    #[serde(default)]
    pub symmetry: Vec<Symmetry>,
    /// Periodic boundaries: the model is the unit cell of an infinite array.
    #[serde(default)]
    pub periodic: Option<Periodic>,
    /// The mesh of the waveguide (made by `emcraft-mesh`), shared: the copies of the model a
    /// session keeps to undo to share it rather than copy it.
    #[serde(skip)]
    pub mesh: Option<Arc<Mesh>>,
}

/// What keeps a model from being solved ([`Model::check`]); its text says what to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Issue {
    /// A symmetry plane the model does not allow (why).
    Symmetry(String),
    /// Periodic boundaries the model does not allow (why).
    Periodic(String),
    NoGeometry,
    NoPorts,
    NoMesh,
}

impl std::fmt::Display for Issue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Issue::Symmetry(why) => write!(f, "symmetry: {why} (clear it with model.symmetry, clear true)"),
            Issue::Periodic(why) => write!(f, "periodic: {why} (clear it with model.periodic, clear true)"),
            Issue::NoGeometry => f.write_str("no geometry: create one with geometry.waveguide or geometry.solid"),
            Issue::NoPorts => f.write_str("no ports: create one with model.port.create"),
            Issue::NoMesh => f.write_str("no mesh: mesh the geometry with mesh.generate"),
        }
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
            wall_thickness: None,
            open: None,
            sources: Vec::new(),
            lumped: Vec::new(),
            band: None,
            analysis: None,
            symmetry: Vec::new(),
            periodic: None,
            mesh: None,
        }
    }
}

impl Model {
    pub fn new() -> Model {
        Model::default()
    }

    /// The material named `name` and its index.
    pub fn material(&self, name: &str) -> Result<(usize, &Material)> {
        self.materials.iter().enumerate().find(|(_, m)| m.name == name).ok_or_else(|| ModelError::Unknown { kind: "material", name: name.into() })
    }

    /// Adds a material under a name no other has.
    pub fn add_material(&mut self, m: Material) -> Result<()> {
        if self.materials.iter().any(|x| x.name == m.name) {
            return Err(ModelError::Duplicate { kind: "material", name: m.name });
        }
        self.materials.push(m);
        Ok(())
    }

    /// Sets the waveguide geometry after checking it; the mesh is dropped (it no longer fits).
    pub fn set_waveguide(&mut self, w: Waveguide) -> Result<()> {
        w.check(|name| self.material(name).map(|_| ()))?;
        self.waveguide = Some(w);
        self.solid = None;
        self.drop_what_fits_the_old_geometry();
        Ok(())
    }

    /// Sets a primitive body as the geometry (replacing a waveguide); the mesh is dropped.
    pub fn set_solid(&mut self, s: Solid) -> Result<()> {
        s.check_shape()?;
        self.material(&s.material)?;
        self.solid = Some(s);
        self.waveguide = None;
        self.drop_what_fits_the_old_geometry();
        Ok(())
    }

    /// Clears what was placed in the geometry just replaced: inclusions, features, the open
    /// boundary, sources, lumped ports and the mesh.
    fn drop_what_fits_the_old_geometry(&mut self) {
        self.inclusions.clear();
        self.features.clear();
        self.open = None;
        self.sources.clear();
        self.lumped.clear();
        self.mesh = None;
    }

    /// Adds a body inside the solid; it must lie within the solid's bounding box, apart from the
    /// other inclusions' (the mesher checks the surfaces do not meet).
    pub fn add_inclusion(&mut self, inc: Inclusion) -> Result<()> {
        let solid = self.solid.as_ref().ok_or_else(|| invalid("inclusion", "needs a solid (geometry.solid) to lie in"))?;
        inc.shape.check()?;
        if !inc.at.is_finite() {
            return Err(invalid("inclusion", "its position must be finite"));
        }
        if let Some(m) = &inc.material {
            self.material(m)?;
        }
        let bounds = inc.bounds();
        if !solid.shape.bounds().encloses_inside(bounds) {
            return Err(invalid("inclusion", "must lie strictly inside the solid's bounding box"));
        }
        if let Some(layer) = self.spherical_layer()
            && !inside_ball(&bounds, layer.inner)
        {
            return Err(invalid("inclusion", "must lie inside the spherical layer's inner sphere"));
        }
        if self.inclusions.iter().any(|other| other.bounds().overlaps_inside(bounds)) {
            return Err(invalid("inclusion", "not supported yet: inclusions whose bounding boxes overlap"));
        }
        self.inclusions.push(inc);
        self.mesh = None;
        Ok(())
    }

    /// Combines a primitive with the solid (after the features already there). Its box must
    /// meet the solid's; the mesher checks the surfaces meet cleanly.
    /// Takes feature `i` out of the model; the mesh is dropped (it no longer fits).
    pub fn remove_feature(&mut self, i: usize) -> Option<Feature> {
        (i < self.features.len()).then(|| {
            self.mesh = None;
            self.features.remove(i)
        })
    }

    /// Takes inclusion `i` out of the model; the mesh is dropped (it no longer fits).
    pub fn remove_inclusion(&mut self, i: usize) -> Option<Inclusion> {
        (i < self.inclusions.len()).then(|| {
            self.mesh = None;
            self.inclusions.remove(i)
        })
    }

    /// Sets the job's frequency band (already checked by [`Band::new`]).
    pub fn set_band(&mut self, band: Option<Band>) {
        self.band = band;
    }

    /// Sets the kind of analysis chosen for the job (`None`: inferred again).
    pub fn set_analysis(&mut self, analysis: Option<Analysis>) {
        self.analysis = analysis;
    }

    /// The job's kind of analysis: the one chosen, else the one the model calls for
    /// ([`Analysis::infer`]).
    pub fn analysis(&self) -> Analysis {
        self.analysis.unwrap_or_else(|| Analysis::infer(self))
    }

    /// Removes every source (the mesh stays: it does not depend on them).
    pub fn clear_sources(&mut self) {
        self.sources.clear();
    }

    /// Sets the mesh made of this model (`emcraft-mesh`).
    pub fn set_mesh(&mut self, mesh: Mesh) {
        self.mesh = Some(Arc::new(mesh));
    }

    pub fn add_feature(&mut self, f: Feature) -> Result<()> {
        let solid = self.solid.as_ref().ok_or_else(|| invalid("feature", "needs a solid (geometry.solid) to combine with"))?;
        f.shape.check()?;
        if !f.at.is_finite() {
            return Err(invalid("feature", "its position must be finite"));
        }
        if !f.bounds().overlaps(self.bounds().unwrap_or_else(|| solid.shape.bounds())) {
            return Err(invalid("feature", "must meet the solid (their bounding boxes are apart)"));
        }
        if self.spherical_layer().is_some() {
            return Err(invalid("feature", "not supported yet: features combined with a sphere lined by a spherical layer"));
        }
        self.features.push(f);
        self.mesh = None;
        Ok(())
    }

    /// Sets the walls' conductivity (S/m, checked by its type), or perfect conductors with
    /// `None`.
    pub fn set_wall_conductivity(&mut self, sigma: Option<Conductivity>) {
        self.wall_conductivity = sigma;
    }

    /// Sets the walls' thickness (m, at most [`MAX_WALL_THICKNESS`]), or infinitely thick walls
    /// with `None`.
    pub fn set_wall_thickness(&mut self, thickness: Option<Metres>) -> Result<()> {
        if let Some(t) = thickness
            && t.get() > MAX_WALL_THICKNESS
        {
            return Err(invalid("wall thickness", format!("must be at most {MAX_WALL_THICKNESS} m, got {t}")));
        }
        self.wall_thickness = thickness;
        Ok(())
    }

    /// The corners of everything that radiates or scatters: inclusions, features, dipoles and
    /// the ends of lumped ports (an open problem's Huygens box encloses them).
    pub fn radiators(&self) -> impl Iterator<Item = Point3> + '_ {
        self.inclusions
            .iter()
            .flat_map(|i| i.bounds().corners())
            .chain(self.features.iter().flat_map(|f| f.bounds().corners()))
            .chain(self.sources.iter().filter_map(Source::position))
            .chain(self.lumped.iter().flat_map(|l| [l.from, l.to]))
    }

    /// What fills a point of a solid model: `None` outside the body or inside a conductor, else
    /// the material's index (a dielectric inclusion's, or the solid's).
    pub fn material_at(&self, p: Point3) -> Option<usize> {
        let s = self.solid.as_ref()?;
        let inside = self.features.iter().fold(s.shape.contains(p), |inside, f| f.combine.apply(inside, f.contains(p)));
        if !inside {
            return None;
        }
        let name = match self.inclusions.iter().find(|inc| inc.contains(p)) {
            Some(inc) => inc.material.as_ref()?,
            None => &s.material,
        };
        self.material(name).ok().map(|(i, _)| i)
    }

    /// The bounding box of the geometry.
    pub fn bounds(&self) -> Option<Aabb> {
        if let Some(w) = &self.waveguide {
            return Some(Aabb::new(Point3::ORIGIN, Point3::new(w.width, w.height, w.length())));
        }
        let s = self.solid.as_ref()?;
        Some(self.features.iter().filter(|f| f.combine == Combine::Add).map(Feature::bounds).fold(s.shape.bounds(), Aabb::union))
    }

    /// Everything a frequency-domain solve needs, each problem named.
    pub fn check(&self) -> Vec<Issue> {
        let mut issues = Vec::new();
        if let Some(why) = self.symmetry_issue() {
            issues.push(Issue::Symmetry(why));
        }
        if let Some(why) = self.periodic_issue() {
            issues.push(Issue::Periodic(why));
        }
        if self.waveguide.is_none() && self.solid.is_none() {
            issues.push(Issue::NoGeometry);
        }
        if self.ports.is_empty() {
            issues.push(Issue::NoPorts);
        }
        if self.mesh.is_none() {
            issues.push(Issue::NoMesh);
        }
        issues
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Axis, End, PortShape, Section, Shape, Vec3};

    fn guide() -> Waveguide {
        Waveguide {
            width: 0.02286,
            height: 0.01016,
            sections: vec![Section::new(0.01, "vacuum")],
            layers: Vec::new(),
            strips: Vec::new(),
            vias: Vec::new(),
        }
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
        // Inclusions stay strictly inside the solid and apart from each other.
        let touching = Inclusion { shape: Shape::Sphere { radius: 0.005 }, at: Point3::new(0.035, 0.02, 0.02), material: None };
        assert!(m.add_inclusion(touching).is_err());
        let overlapping = Inclusion { shape: Shape::Sphere { radius: 0.002 }, at: Point3::new(0.024, 0.02, 0.02), material: None };
        assert!(m.add_inclusion(overlapping).is_err());
        assert_eq!(m.radiators().count(), 6, "two corners per inclusion and feature");
    }

    #[test]
    fn materials_are_unique() {
        let mut m = Model::new();
        m.add_material(Material::new("ptfe", 2.0, 1.0).unwrap()).unwrap();
        assert!(matches!(m.add_material(Material::new("ptfe", 3.0, 1.0).unwrap()), Err(ModelError::Duplicate { .. })));
        assert_eq!(m.material("ptfe").unwrap().0, 1);
        assert!(matches!(m.material("glass"), Err(ModelError::Unknown { .. })));
    }

    #[test]
    fn waveguides_are_checked() {
        let mut m = Model::new();
        let mut w = guide();
        w.width = f64::INFINITY;
        assert!(m.set_waveguide(w).is_err());
        let mut w = guide();
        w.sections[0].material = "nothing".into();
        assert!(matches!(m.set_waveguide(w), Err(ModelError::Unknown { .. })));
        m.set_waveguide(guide()).unwrap();
        assert!((m.waveguide.as_ref().unwrap().length() - 0.01).abs() < 1e-15);
        m.add_port(Port::new("1", End::Start)).unwrap();
        m.add_port(Port::new("2", End::End)).unwrap();
        assert_eq!(m.check(), vec![Issue::NoMesh]);
        assert_eq!(Issue::NoMesh.to_string(), "no mesh: mesh the geometry with mesh.generate");
        let mm = |t: f64| Some(Metres::new(t * 1e-3).unwrap());
        assert!(m.set_wall_thickness(mm(2000.0)).is_err(), "thicker than a metre");
        m.set_wall_thickness(mm(1.0)).unwrap();
        assert_eq!(m.wall_thickness, mm(1.0));
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
    }
}
