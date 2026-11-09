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
use crate::body::{BodiesMut, Body, MeshView, ViewCache};
use crate::error::{ModelError, Result, invalid};
use crate::geom::{Aabb, Point3};
use crate::material::Material;
use crate::mesh::Mesh;
use crate::open::{LumpedPort, Source, inside_ball};
use crate::port::Port;
use crate::quantity::{Conductivity, Metres};
use crate::shape::{Combine, Feature, Inclusion, Shape, Solid};
use crate::waveguide::Waveguide;

/// The thickest wall a model may have (m).
pub const MAX_WALL_THICKNESS: f64 = 1.0;

/// The roughest walls (m rms): a tenth of a millimetre, far rougher than any machined or plated
/// surface, where the factor has long reached 2.
pub const MAX_WALL_ROUGHNESS: f64 = 1e-4;

/// The model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub materials: Vec<Material>,
    /// The region solved and what bounds it (crate::domain).
    #[serde(default)]
    pub domain: crate::domain::Domain,
    /// The bodies, in the order they were placed ([`crate::body`]): read with
    /// [`Model::bodies`], changed with [`Model::bodies_mut`] or the setters.
    #[serde(default)]
    bodies: Vec<Body>,
    /// The mesher's view of the bodies (features and inclusions), made when first read.
    #[serde(skip)]
    view: ViewCache,
    pub ports: Vec<Port>,
    /// What drives an open problem.
    #[serde(default)]
    pub sources: Vec<Source>,
    /// Lumped ports (antenna feeds).
    #[serde(default)]
    pub lumped: Vec<LumpedPort>,
    /// Lumped circuit elements: passive loads across gaps.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub elements: Vec<crate::element::LumpedElement>,
    /// The job: the kind of analysis and the run's confirmed settings.
    #[serde(default)]
    pub job: crate::job::Job,
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
    /// A rotational symmetry the model does not allow (why).
    Rotation(String),
    NoGeometry,
    NoPorts,
    NoMesh,
}

impl std::fmt::Display for Issue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Issue::Symmetry(why) => write!(f, "symmetry: {why} (clear it with model.symmetry, clear true)"),
            Issue::Periodic(why) => write!(f, "periodic: {why} (clear it with model.periodic, clear true)"),
            Issue::Rotation(why) => write!(f, "rotation: {why} (clear it with model.rotation, clear true)"),
            Issue::NoGeometry => f.write_str("no geometry: create one with geometry.waveguide or geometry.solid"),
            Issue::NoPorts => f.write_str("no ports: create one with model.port.create or model.port.lumped"),
            Issue::NoMesh => f.write_str("no mesh: mesh the geometry with mesh.generate"),
        }
    }
}

impl Default for Model {
    /// An empty model with one material, vacuum.
    fn default() -> Model {
        Model {
            materials: vec![Material::vacuum()],
            domain: crate::domain::Domain::default(),
            bodies: Vec::new(),
            view: ViewCache::default(),
            ports: Vec::new(),
            sources: Vec::new(),
            lumped: Vec::new(),
            elements: Vec::new(),
            job: crate::job::Job::Open,
            mesh: None,
        }
    }
}

impl Model {
    /// The bodies, in the order they were placed.
    pub fn bodies(&self) -> &[Body] {
        &self.bodies
    }

    /// The bodies to change in place (the mesher's view follows).
    pub fn bodies_mut(&mut self) -> BodiesMut<'_> {
        BodiesMut { bodies: &mut self.bodies, view: &mut self.view }
    }

    /// The waveguide the bodies make, when they make one ([`Waveguide::of_bodies`]).
    pub fn waveguide(&self) -> Option<&Waveguide> {
        self.view.guide(&self.bodies)
    }

    /// The printed circuit board among the bodies, if there is one.
    pub fn pcb(&self) -> Option<&crate::pcb::Board> {
        self.bodies.iter().find_map(|b| match &b.shape {
            Shape::Board(board) => Some(&**board),
            _ => None,
        })
    }

    /// Sets the board (first among the bodies, in place of the last one), or takes it out.
    pub fn set_pcb(&mut self, board: Option<crate::pcb::Board>) {
        let is_board = |b: &Body| b.kind == crate::body::Kind::Board;
        if board.is_none() && !self.bodies.iter().any(is_board) {
            return;
        }
        let mut bodies = self.bodies_mut();
        bodies.retain(|b| !is_board(b));
        if let Some(board) = board {
            let body = Body {
                shape: Shape::Board(Box::new(board)),
                at: Point3::ORIGIN,
                axis: crate::Axis::Z,
                fill: crate::body::Fill::Metal,
                kind: crate::body::Kind::Board,
                name: None,
            };
            bodies.insert(0, body);
        }
    }

    /// Takes the waveguide's bodies out (the region is another's).
    pub fn clear_waveguide(&mut self) {
        if self.bodies.iter().any(Body::is_extruded) {
            self.bodies_mut().retain(|b| !b.is_extruded());
        }
    }

    /// The bodies as the mesher reads them.
    pub fn mesh_view(&self) -> &MeshView {
        self.view.get(&self.bodies)
    }

    /// The bodies combined with the region's shape, as features, in their order.
    pub fn features(&self) -> &[Feature] {
        &self.mesh_view().features
    }

    /// The bodies meshed apart, as inclusions, in their order.
    pub fn inclusions(&self) -> &[Inclusion] {
        &self.mesh_view().inclusions
    }

    /// An imported part's walls and region body, with where each lies among the bodies.
    pub fn parts(&self) -> impl Iterator<Item = (usize, &Body)> {
        self.bodies.iter().enumerate().filter(|(_, b)| b.is_part())
    }

    /// Sets an imported part's walls and region body in place of the last part's (first among
    /// the bodies).
    pub fn set_parts(&mut self, parts: Vec<Body>) {
        let mut bodies = self.bodies_mut();
        bodies.retain(|b| !b.is_part());
        bodies.splice(0..0, parts);
    }

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

    /// Sets the waveguide geometry after checking it, as its bodies ([`Waveguide::bodies`]) in
    /// place of the model's; the mesh is dropped (it no longer fits).
    pub fn set_waveguide(&mut self, w: Waveguide) -> Result<()> {
        w.check(|name| self.material(name).map(|_| ()))?;
        let bodies = w.bodies()?;
        self.domain.solid = None;
        self.drop_what_fits_the_old_geometry();
        *self.bodies_mut() = bodies;
        Ok(())
    }

    /// Sets a primitive body as the geometry (replacing a waveguide); the mesh is dropped.
    pub fn set_solid(&mut self, s: Solid) -> Result<()> {
        s.check_shape()?;
        self.material(&s.material)?;
        self.domain.solid = Some(s);
        self.clear_waveguide();
        // A region set is pinned (ADR 0005).
        self.domain.region = crate::region::Region::Pinned;
        self.drop_what_fits_the_old_geometry();
        Ok(())
    }

    /// Clears what was placed in the geometry just replaced: inclusions, features, the open
    /// boundary, sources, lumped ports and the mesh.
    fn drop_what_fits_the_old_geometry(&mut self) {
        self.set_pcb(None);
        self.bodies_mut().clear();
        self.domain.open = None;
        self.sources.clear();
        self.lumped.clear();
        self.mesh = None;
    }

    /// Adds a body inside the solid; it must lie within the solid's bounding box, apart from the
    /// other inclusions' (the mesher checks the surfaces do not meet).
    pub fn add_inclusion(&mut self, inc: Inclusion) -> Result<()> {
        let solid = self.domain.solid.as_ref().ok_or_else(|| invalid("inclusion", "needs a solid (geometry.solid) to lie in"))?;
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
        if self.inclusions().iter().any(|other| other.bounds().overlaps_inside(bounds)) {
            return Err(invalid("inclusion", "not supported yet: inclusions whose bounding boxes overlap"));
        }
        self.bodies_mut().push(inc.into());
        self.mesh = None;
        Ok(())
    }

    /// Combines a primitive with the solid (after the features already there). Its box must
    /// meet the solid's; the mesher checks the surfaces meet cleanly.
    /// Takes feature `i` out of the model; the mesh is dropped (it no longer fits).
    pub fn remove_feature(&mut self, i: usize) -> Option<Feature> {
        let k = *self.mesh_view().feature_body.get(i)?;
        self.mesh = None;
        let b = self.bodies_mut().remove(k);
        b.feature()
    }

    /// Takes inclusion `i` out of the model; the mesh is dropped (it no longer fits).
    pub fn remove_inclusion(&mut self, i: usize) -> Option<Inclusion> {
        let k = *self.mesh_view().inclusion_body.get(i)?;
        self.mesh = None;
        let b = self.bodies_mut().remove(k);
        b.inclusion()
    }

    /// Sets the job.
    pub fn set_job(&mut self, job: crate::job::Job) {
        self.job = job;
    }

    /// The job's kind of analysis: the one chosen, else the one the model calls for
    /// ([`Analysis::infer`]).
    pub fn analysis(&self) -> Analysis {
        self.job.chosen().unwrap_or_else(|| Analysis::infer(self))
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
        let solid = self.domain.solid.as_ref().ok_or_else(|| invalid("feature", "needs a solid (geometry.solid) to combine with"))?;
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
        self.bodies_mut().push(f.into());
        self.mesh = None;
        Ok(())
    }

    /// Sets the walls' conductivity (S/m, checked by its type), or perfect conductors with
    /// `None`.
    pub fn set_wall_conductivity(&mut self, sigma: Option<Conductivity>) {
        self.domain.wall_conductivity = sigma;
    }

    /// Sets the walls' thickness (m, at most [`MAX_WALL_THICKNESS`]), or infinitely thick walls
    /// with `None`.
    pub fn set_wall_thickness(&mut self, thickness: Option<Metres>) -> Result<()> {
        if let Some(t) = thickness
            && t.get() > MAX_WALL_THICKNESS
        {
            return Err(invalid("wall thickness", format!("must be at most {MAX_WALL_THICKNESS} m, got {t}")));
        }
        self.domain.wall_thickness = thickness;
        Ok(())
    }

    /// Sets the rms height of the walls' roughness (m, at most [`MAX_WALL_ROUGHNESS`]), or smooth
    /// walls with `None`.
    pub fn set_wall_roughness(&mut self, roughness: Option<Metres>) -> Result<()> {
        if let Some(h) = roughness
            && h.get() > MAX_WALL_ROUGHNESS
        {
            return Err(invalid("wall roughness", format!("must be at most {MAX_WALL_ROUGHNESS} m, got {h}")));
        }
        self.domain.wall_roughness = roughness;
        Ok(())
    }

    /// The corners of everything that radiates or scatters: inclusions, features, dipoles and
    /// the ends of lumped ports (an open problem's Huygens box encloses them).
    pub fn radiators(&self) -> impl Iterator<Item = Point3> + '_ {
        self.inclusions()
            .iter()
            .flat_map(|i| i.bounds().corners())
            .chain(self.features().iter().flat_map(|f| f.bounds().corners()))
            .chain(self.sources.iter().filter_map(Source::position))
            .chain(self.lumped.iter().flat_map(|l| [l.from, l.to]))
    }

    /// The model moved by `v` (m): its region (a box primitive becomes the [`Shape::block`] it
    /// spans moved), bodies, sources, lumped ports and port faces; its mesh dropped. Refused for
    /// what is tied to the origin or the region's own frame — symmetry planes, a rotation,
    /// periodic boundaries, a waveguide or a board — and for a cylinder or sphere region, which
    /// stand on the origin.
    ///
    /// ```
    /// use emcraft_model::{BoxSize, Model, Point3, Shape, Solid, Vec3};
    ///
    /// let mut m = Model::new();
    /// m.set_solid(Solid { shape: Shape::Box { size: BoxSize::new(Vec3::new(0.02, 0.02, 0.02))? }, material: "vacuum".into() })?;
    /// let t = m.translated(Vec3::new(-0.01, -0.01, 0.0))?;
    /// let b = t.domain.solid.as_ref().and_then(|s| s.shape.box_bounds()).unwrap();
    /// assert_eq!((b.min.x, b.max.x), (-0.01, 0.01));
    /// # Ok::<(), emcraft_model::ModelError>(())
    /// ```
    pub fn translated(&self, v: crate::geom::Vec3) -> Result<Model> {
        if !self.domain.symmetry.is_empty()
            || self.domain.rotation.is_some()
            || self.domain.periodic.is_some()
            || self.waveguide().is_some()
            || self.pcb().is_some()
        {
            return Err(invalid("move", "not supported for symmetry planes, a rotation, periodic boundaries, a waveguide or a board"));
        }
        let mut m = self.clone();
        m.mesh = None;
        let shift = |p: Point3| p + v;
        if let Some(solid) = m.domain.solid.as_mut() {
            solid.shape = match &solid.shape {
                Shape::Box { size } => Shape::block(shift(Point3::ORIGIN), shift(size.to_point()))?,
                Shape::Surface { name, positions, triangles, tags, surfaces } => Shape::Surface {
                    name: name.clone(),
                    positions: positions.iter().copied().map(shift).collect(),
                    triangles: triangles.clone(),
                    tags: tags.clone(),
                    surfaces: surfaces.iter().map(|f| f.clone().map(|f| f.moved(v))).collect(),
                },
                _ if v == crate::geom::Vec3::ZERO => solid.shape.clone(),
                _ => return Err(invalid("move", "a cylinder or sphere region stands on the origin")),
            };
        }
        // The bodies in the region move with it (an imported part's walls are drawn where
        // they were read).
        m.bodies_mut().iter_mut().filter(|b| !b.is_part()).for_each(|b| b.at = shift(b.at));
        for src in &mut m.sources {
            if let Source::Dipole { at, .. } = src {
                *at = shift(*at);
            }
        }
        for l in &mut m.lumped {
            (l.from, l.to) = (shift(l.from), shift(l.to));
        }
        for p in &mut m.ports {
            if let Some(f) = p.face.as_mut() {
                let crate::port::FrameAxes { u, v: w, .. } = crate::port::Frame::new(f.axis).axes();
                f.at += v[f.axis.index()];
                f.u0 += v[u];
                f.v0 += v[w];
            }
        }
        Ok(m)
    }

    /// What fills a point of a solid model: `None` outside the body or inside a conductor, else
    /// the material's index (a dielectric inclusion's, or the solid's).
    pub fn material_at(&self, p: Point3) -> Option<usize> {
        let s = self.domain.solid.as_ref()?;
        let inside = self.features().iter().fold(s.shape.contains(p), |inside, f| f.combine.apply(inside, f.contains(p)));
        if !inside {
            return None;
        }
        let name = match self.inclusions().iter().find(|inc| inc.contains(p)) {
            Some(inc) => inc.material.as_ref()?,
            None => &s.material,
        };
        self.material(name).ok().map(|(i, _)| i)
    }

    /// The bounding box of the geometry.
    pub fn bounds(&self) -> Option<Aabb> {
        if let Some(w) = self.waveguide() {
            return Some(Aabb::new(Point3::ORIGIN, Point3::new(w.width.get(), w.height.get(), w.length())));
        }
        let s = self.domain.solid.as_ref()?;
        Some(self.features().iter().filter(|f| f.combine == Combine::Add).map(Feature::bounds).fold(s.shape.bounds(), Aabb::union))
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
        if let Some(why) = self.rotation_issue() {
            issues.push(Issue::Rotation(why));
        }
        if self.waveguide().is_none() && self.domain.solid.is_none() {
            issues.push(Issue::NoGeometry);
        }
        if self.ports.is_empty() && self.lumped.is_empty() {
            issues.push(Issue::NoPorts);
        }
        if self.mesh.is_none() {
            issues.push(Issue::NoMesh);
        }
        issues
    }
}

/// Whether a region is pinned (left out of a saved model, as models before regions were).
pub(crate) fn is_pinned(r: &crate::region::Region) -> bool {
    *r == crate::region::Region::Pinned
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Axis, End, PortShape, Section, Shape, Vec3};

    /// A length (m), checked.
    fn metres(v: f64) -> crate::Metres {
        crate::Metres::new(v).unwrap()
    }

    /// A box's sides (m), checked.
    fn box_size(v: crate::Vec3) -> crate::BoxSize {
        crate::BoxSize::new(v).unwrap()
    }

    fn guide() -> Waveguide {
        Waveguide {
            width: metres(0.02286),
            height: metres(0.01016),
            sections: vec![Section::new(metres(0.01), "vacuum")],
            layers: Vec::new(),
            strips: Vec::new(),
            vias: Vec::new(),
        }
    }

    #[test]
    fn points_are_classified() {
        let mut m = Model::new();
        m.add_material(Material::new("ceramic", 4.0, 1.0).unwrap()).unwrap();
        m.set_solid(Solid { shape: Shape::Box { size: box_size(Vec3::new(0.04, 0.04, 0.04)) }, material: "vacuum".into() }).unwrap();
        m.add_inclusion(Inclusion {
            shape: Shape::Sphere { radius: metres(0.005) },
            at: Point3::new(0.02, 0.02, 0.02),
            material: Some("ceramic".into()),
        })
        .unwrap();
        m.add_inclusion(Inclusion {
            shape: Shape::Cylinder { radius: metres(0.001), length: metres(0.01) },
            at: Point3::new(0.03, 0.03, 0.01),
            material: None,
        })
        .unwrap();
        m.add_feature(Feature {
            combine: Combine::Subtract,
            shape: Shape::Cylinder { radius: metres(0.002), length: metres(0.04) },
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
        let touching = Inclusion { shape: Shape::Sphere { radius: metres(0.005) }, at: Point3::new(0.035, 0.02, 0.02), material: None };
        assert!(m.add_inclusion(touching).is_err());
        let overlapping = Inclusion { shape: Shape::Sphere { radius: metres(0.002) }, at: Point3::new(0.024, 0.02, 0.02), material: None };
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
        w.sections.clear();
        assert!(m.set_waveguide(w).is_err());
        let mut w = guide();
        w.sections[0].material = "nothing".into();
        assert!(matches!(m.set_waveguide(w), Err(ModelError::Unknown { .. })));
        m.set_waveguide(guide()).unwrap();
        approx::assert_abs_diff_eq!(m.waveguide().unwrap().length(), 0.01, epsilon = 1e-15);
        m.add_port(Port::new("1", End::Start)).unwrap();
        m.add_port(Port::new("2", End::End)).unwrap();
        assert_eq!(m.check(), vec![Issue::NoMesh]);
        assert_eq!(Issue::NoMesh.to_string(), "no mesh: mesh the geometry with mesh.generate");
        let mm = |t: f64| Some(Metres::new(t * 1e-3).unwrap());
        assert!(m.set_wall_thickness(mm(2000.0)).is_err(), "thicker than a metre");
        m.set_wall_thickness(mm(1.0)).unwrap();
        assert_eq!(m.domain.wall_thickness, mm(1.0));
    }

    #[test]
    fn solids_are_checked_and_replace_the_waveguide() {
        let mut m = Model::new();
        m.set_waveguide(guide()).unwrap();
        assert!(m.set_solid(Solid { shape: Shape::Sphere { radius: metres(0.01) }, material: "nothing".into() }).is_err());
        m.set_solid(Solid { shape: Shape::Cylinder { radius: metres(0.01), length: metres(0.03) }, material: "vacuum".into() }).unwrap();
        assert!(m.waveguide().is_none());
        assert_eq!(m.bounds(), Some(Aabb::new(Point3::new(-0.01, -0.01, 0.0), Point3::new(0.01, 0.01, 0.03))));
        assert_eq!(m.port_shape(End::End).unwrap(), PortShape::Circle { cx: 0.0, cy: 0.0, r: 0.01 });
        m.set_solid(Solid { shape: Shape::Sphere { radius: metres(0.01) }, material: "vacuum".into() }).unwrap();
        assert!(m.port_shape(End::Start).is_err());
    }
}
