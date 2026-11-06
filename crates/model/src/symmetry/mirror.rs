//! Symmetry planes: the half of a symmetric model cut away and replaced by a wall.

use serde::{Deserialize, Serialize};

use crate::error::{Result, invalid};
use crate::geom::{Aabb, Point3};
use crate::model::Model;
use crate::shape::{Axis, Shape, Solid};
use crate::waveguide::{Strip, Via, Waveguide};

emcraft_macros::named_enum! {
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
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum SymmetryKind {
        /// A perfect electric conductor on the plane: tangential E = 0.
        Electric = "electric",
        /// A perfect magnetic conductor on the plane: tangential H = 0.
        Magnetic = "magnetic",
    }
}

emcraft_macros::named_serde!(SymmetryKind);

/// A plane of symmetry `axis` = `at` (m) through the model: only the side below it is meshed and
/// solved, the plane closed by the wall of `kind`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Symmetry {
    pub axis: Axis,
    pub at: f64,
    pub kind: SymmetryKind,
}

/// The point group that mirror planes across distinct axes make (Schoenflies' names): none C1,
/// one Cs, two C2v (with the two-fold axis where they meet), three D2h. Each solved plane halves
/// the model; the group's other parities are the other walls.
///
/// ```
/// use emcraft_model::symmetry::mirror_group;
///
/// assert_eq!([0, 1, 2, 3].map(mirror_group), ["C1", "Cs", "C2v", "D2h"]);
/// ```
pub fn mirror_group(planes: usize) -> &'static str {
    match planes {
        0 => "C1",
        1 => "Cs",
        2 => "C2v",
        _ => "D2h",
    }
}

/// A plane the model is symmetric about ([`Model::find_symmetry`]): its axis, position (m) and
/// the wall its ports ask for (`None`: no port decides).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Found {
    pub axis: Axis,
    pub at: f64,
    pub kind: Option<SymmetryKind>,
}

/// A point mirrored onto the side of the symmetry planes that is kept, with the signs the
/// image's field components take there (±1 per x, y, z component of E and of H).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Image {
    pub point: Point3,
    pub e: [f64; 3],
    pub h: [f64; 3],
}

impl Image {
    /// The point itself, every sign +1.
    pub fn of(point: Point3) -> Image {
        Image { point, e: [1.0; 3], h: [1.0; 3] }
    }

    /// This image mirrored again by `plane`, the signs multiplied.
    pub fn through(self, plane: Symmetry) -> Image {
        let next = plane.image(self.point);
        Image { point: next.point, e: [0, 1, 2].map(|k| self.e[k] * next.e[k]), h: [0, 1, 2].map(|k| self.h[k] * next.h[k]) }
    }
}

impl Symmetry {
    /// Mirrors a point from the side cut away onto the side kept (the point itself when it is
    /// kept), with the signs the image's E and H components take: an electric wall flips E's
    /// tangential components and H's normal one, a magnetic wall E's normal component and H's
    /// tangential ones (H being a pseudovector).
    pub fn image(self, p: Point3) -> Image {
        let d = self.axis.index();
        if p[d] <= self.at {
            return Image::of(p);
        }
        let point = p.with(d, 2.0 * self.at - p[d]);
        let normal = [0, 1, 2].map(|k| if k == d { -1.0 } else { 1.0 });
        let tangential = [0, 1, 2].map(|k| if k == d { 1.0 } else { -1.0 });
        match self.kind {
            SymmetryKind::Electric => Image { point, e: tangential, h: normal },
            SymmetryKind::Magnetic => Image { point, e: normal, h: tangential },
        }
    }

    /// A point mirrored by every plane in turn into the part they keep.
    pub fn image_through(planes: &[Symmetry], p: Point3) -> Image {
        planes.iter().fold(Image::of(p), |image, plane| image.through(*plane))
    }

    /// Checks a set of planes on its own: at most one across x and one across y, at finite
    /// positions.
    fn check_planes(planes: &[Symmetry]) -> std::result::Result<(), String> {
        if planes.len() > 2 {
            return Err(format!("at most two planes (one across x, one across y), got {}", planes.len()));
        }
        if let [a, b] = planes
            && a.axis == b.axis
        {
            return Err("two planes across the same axis".into());
        }
        if planes.iter().any(|p| !p.at.is_finite()) {
            return Err("the plane's position must be finite".into());
        }
        Ok(())
    }

    /// Checks the guide is symmetric about the plane (x or y): its walls, every section's
    /// aperture, its layers, strips and vias.
    fn check_guide(&self, w: &Waveguide) -> std::result::Result<(), String> {
        let (span, name) = match self.axis {
            Axis::X => (w.width.get(), "x = a/2"),
            Axis::Y => (w.height.get(), "y = b/2"),
            Axis::Z => return Err("a waveguide's symmetry plane must be x = const or y = const (its ports lie on z planes)".into()),
        };
        let tol = 1e-9 * span;
        let mid = span / 2.0;
        if (self.at - mid).abs() > tol {
            return Err(format!("the guide's walls are symmetric only about its middle, {name} = {mid} m; the plane is at {} m", self.at));
        }
        for i in 0..w.sections.len() {
            let aperture = if self.axis == Axis::X { w.aperture(i) } else { w.aperture_y(i) };
            if let Some(a) = aperture
                && (a.start + a.end() - span).abs() > tol
            {
                return Err(format!("section {}'s aperture is not centred on the plane: the guide is not symmetric about it", i + 1));
            }
        }
        if self.axis == Axis::Y && !w.layers.is_empty() {
            return Err("not supported yet: a plane across y in a layered guide (its layers from y = 0 are not symmetric about it)".into());
        }
        // Strips and vias: each mirrored by one (itself when it straddles the plane; a strip on
        // a y plane, or a post through the whole height about y = b/2, is its own image).
        let t = 1e-9 * w.width.max(w.height.get()).max(w.length());
        if let Some(k) = unmirrored(&w.strips, self.axis, span, t) {
            return Err(format!("strip {} has no mirror image about the plane: the guide is not symmetric about it", k + 1));
        }
        if let Some(k) = unmirrored(&w.vias, self.axis, span, t) {
            return Err(format!("via {} has no mirror image about the plane: the guide is not symmetric about it", k + 1));
        }
        Ok(())
    }

    /// A box mirrored about the plane.
    fn mirrored(self, b: Aabb) -> Aabb {
        let d = self.axis.index();
        let (lo, hi) = (2.0 * self.at - b.max[d], 2.0 * self.at - b.min[d]);
        Aabb::new(b.min.with(d, lo), b.max.with(d, hi))
    }

    /// Checks the solid is symmetric about the plane: its body, every feature and inclusion
    /// (each mirrored by one of the same kind), and that the plane cuts every port's face through
    /// its middle, so each port keeps the half of its modes of the plane's parity. Tolerance
    /// `tol` (m).
    fn check_solid(&self, m: &Model, solid: &Solid, tol: f64) -> std::result::Result<(), String> {
        let d = self.axis.index();
        let same = |a: Aabb, b: Aabb| (0..3).all(|k| (a.min[k] - b.min[k]).abs() <= tol && (a.max[k] - b.max[k]).abs() <= tol);
        let body = solid.shape.bounds();
        if !same(self.mirrored(body), body) || !self.surface_mirrors(&solid.shape, |q| q, tol) {
            return Err("the solid is not symmetric about the plane".into());
        }
        let unmirrored = |boxes: Vec<(Aabb, bool)>| {
            boxes.iter().position(|(b, _)| {
                let image = self.mirrored(*b);
                !boxes.iter().any(|(o, _)| same(*o, image))
            })
        };
        // Features and inclusions: each mirrored by one of the same combination, shape and axis
        // (compared through their boxes; a body is its own image when it straddles the plane).
        for (i, f) in m.features.iter().enumerate() {
            let image = self.mirrored(f.bounds());
            let mirrored = m.features.iter().any(|o| o.combine == f.combine && o.axis == f.axis && same(o.bounds(), image));
            if !mirrored || !self.surface_mirrors(&f.shape, |q| f.at + f.axis.turn(q.to_vec()), tol) {
                return Err(format!("feature {} has no mirror image about the plane: the solid is not symmetric about it", i + 1));
            }
        }
        if let Some(k) = unmirrored(m.inclusions.iter().map(|inc| (inc.bounds(), inc.material.is_none())).collect()) {
            return Err(format!("inclusion {} has no mirror image about the plane: the solid is not symmetric about it", k + 1));
        }
        if m.domain.open.is_some() || !m.lumped.is_empty() || !m.sources.is_empty() {
            return Err("not supported yet: a symmetry plane in an open problem, or with lumped ports or sources".into());
        }
        for p in &m.ports {
            let Some(face) = &p.face else {
                return Err(format!("port {} lies on an end of the solid: a symmetry plane cuts only ports on faces (axis and face)", p.name));
            };
            let (u, v, _) = crate::port::Frame::new(face.axis).axes();
            let (lo, side) = match d {
                _ if d == u => (face.u0, face.a.get()),
                _ if d == v => (face.v0, face.b.get()),
                _ => return Err(format!("port {}'s face is parallel to the plane: the plane must cut every port through its middle", p.name)),
            };
            if (lo + side / 2.0 - self.at).abs() > tol {
                return Err(format!("the plane does not cut port {}'s face through its middle", p.name));
            }
        }
        Ok(())
    }

    /// Whether an imported surface's vertices, placed by `place`, mirror onto its vertices (other
    /// shapes pass: their boxes decide).
    fn surface_mirrors(&self, shape: &Shape, place: impl Fn(Point3) -> Point3, tol: f64) -> bool {
        let Shape::Surface { positions, .. } = shape else { return true };
        let d = self.axis.index();
        let key = |p: Point3| p.to_array().map(|c| (c / tol).round() as i64);
        let placed: Vec<Point3> = positions.iter().map(|p| place(*p)).collect();
        let keys: std::collections::BTreeSet<[i64; 3]> = placed.iter().map(|p| key(*p)).collect();
        placed.iter().all(|p| {
            let image = key(p.with(d, 2.0 * self.at - p[d]));
            // A vertex rounded across a key's edge: its neighbours count too.
            (-1..=1).any(|o| {
                keys.contains(&{
                    let mut k = image;
                    k[d] += o;
                    k
                })
            })
        })
    }
}

/// A piece of a planar circuit that a symmetric guide must hold the mirror image of.
trait Mirror: Copy {
    fn mirrored(self, axis: Axis, span: f64) -> Self;
    fn near(&self, o: &Self, tol: f64) -> bool;
}

impl Mirror for Strip {
    fn mirrored(self, axis: Axis, span: f64) -> Strip {
        Strip::mirrored(self, axis, span)
    }

    fn near(&self, o: &Strip, tol: f64) -> bool {
        Strip::near(self, o, tol)
    }
}

impl Mirror for Via {
    fn mirrored(self, axis: Axis, span: f64) -> Via {
        Via::mirrored(self, axis, span)
    }

    fn near(&self, o: &Via, tol: f64) -> bool {
        Via::near(self, o, tol)
    }
}

/// The first piece whose mirror image about `axis` = `span` / 2 is not among them.
fn unmirrored<T: Mirror>(pieces: &[T], axis: Axis, span: f64, tol: f64) -> Option<usize> {
    pieces.iter().position(|p| {
        let image = p.mirrored(axis, span);
        !pieces.iter().any(|o| o.near(&image, tol))
    })
}

impl Model {
    /// Sets the symmetry planes (none clears them) after checking the geometry is symmetric
    /// about each; the mesh is dropped.
    pub fn set_symmetry(&mut self, planes: Vec<Symmetry>) -> Result<()> {
        if let Some(why) = self.symmetry_issue_of(&planes) {
            return Err(invalid("symmetry", why));
        }
        self.domain.symmetry = planes;
        self.domain.every_wall = false;
        self.mesh = None;
        Ok(())
    }

    /// Sets planes no port decides the walls of, for an eigenmode search to solve every
    /// combination of their walls ([`Model::wall_combinations`]).
    pub fn set_symmetry_every_wall(&mut self, planes: Vec<Symmetry>) -> Result<()> {
        self.set_symmetry(planes)?;
        self.domain.every_wall = !self.domain.symmetry.is_empty();
        Ok(())
    }

    /// The walls of every combination an eigenmode search solves: each plane electric or
    /// magnetic when [`crate::domain::Domain::every_wall`], else the planes' own (one combination).
    pub fn wall_combinations(&self) -> Vec<Vec<SymmetryKind>> {
        let own: Vec<SymmetryKind> = self.domain.symmetry.iter().map(|q| q.kind).collect();
        if !self.domain.every_wall {
            return vec![own];
        }
        let n = self.domain.symmetry.len().min(2);
        (0..1usize << n)
            .map(|bits| (0..n).map(|k| if bits >> k & 1 == 0 { SymmetryKind::Magnetic } else { SymmetryKind::Electric }).collect())
            .collect()
    }

    /// The model with its symmetry planes' walls `kinds` (in their order), its mesh's faces on
    /// each plane tagged so: the same mesh, solved with other walls.
    ///
    /// ```
    /// use emcraft_model::{Model, Solid, Shape, Symmetry, SymmetryKind, Axis, BoxSize, Vec3};
    /// let mut m = Model::new();
    /// m.set_solid(Solid { shape: Shape::Box { size: BoxSize::new(Vec3::new(0.02, 0.01, 0.03))? }, material: "vacuum".into() })?;
    /// m.set_symmetry_every_wall(vec![Symmetry { axis: Axis::X, at: 0.01, kind: SymmetryKind::Magnetic }])?;
    /// assert_eq!(m.wall_combinations().len(), 2);
    /// let e = m.with_walls(&[SymmetryKind::Electric]);
    /// assert_eq!(e.domain.symmetry[0].kind, SymmetryKind::Electric);
    /// # Ok::<(), emcraft_model::ModelError>(())
    /// ```
    pub fn with_walls(&self, kinds: &[SymmetryKind]) -> Model {
        let mut m = self.clone();
        for (q, k) in m.domain.symmetry.iter_mut().zip(kinds) {
            q.kind = *k;
        }
        m.domain.every_wall = false;
        if let Some(mesh) = &self.mesh {
            let mut mesh = (**mesh).clone();
            let tol = 1e-9 * self.bounds().map_or(1.0, |b| (b.max - b.min).norm());
            for f in &mut mesh.boundary {
                if !matches!(f.tag, crate::mesh::Boundary::Symmetry(_)) {
                    continue;
                }
                let on = |q: &Symmetry| f.nodes.iter().all(|n| mesh.nodes.get(*n as usize).is_some_and(|p| (p[q.axis.index()] - q.at).abs() <= tol));
                if let Some(q) = m.domain.symmetry.iter().find(|q| on(q)) {
                    f.tag = crate::mesh::Boundary::Symmetry(q.kind);
                }
            }
            m.mesh = Some(std::sync::Arc::new(mesh));
        }
        m
    }

    /// Why the model is not symmetric about its symmetry planes, if it is not (the geometry may
    /// have changed since they were set).
    pub fn symmetry_issue(&self) -> Option<String> {
        if self.domain.every_wall && !self.ports.is_empty() {
            return Some(
                "its planes were set to try every wall in an eigenmode search; with ports, search again (model.symmetry.auto) for the walls they ask"
                    .into(),
            );
        }
        self.symmetry_issue_of(&self.domain.symmetry)
    }

    /// The fraction of the model the symmetry planes keep: ½ per plane.
    pub fn symmetry_fraction(&self) -> f64 {
        0.5f64.powi(self.domain.symmetry.len().min(2) as i32)
    }

    /// The planes the model is symmetric about (each axis' middle that [`Model::set_symmetry`]
    /// would take), with the wall its ports' first modes ask for: a plane across a rectangular
    /// port's broad side keeps TE10 with a magnetic wall (its E tangential and even), one across
    /// the narrow side with an electric wall (its E normal to it). `kind` is `None` where no port
    /// decides it (an eigenmode search: either wall keeps a family of modes) and the plane is left
    /// out where the ports ask for different walls or one's first mode is not TE10.
    ///
    /// ```
    /// use emcraft_model::{Axis, End, Metres, Model, Port, Section, SymmetryKind, Waveguide};
    ///
    /// let mut m = Model::new();
    /// let (width, height) = (Metres::new(0.02286)?, Metres::new(0.01016)?); // m
    /// let sections = vec![Section::new(Metres::new(0.03)?, "vacuum")];
    /// m.set_waveguide(Waveguide { width, height, sections, layers: vec![], strips: vec![], vias: vec![] })?;
    /// // No port: both middles, either wall.
    /// let found = m.find_symmetry();
    /// assert_eq!(found.iter().map(|f| (f.axis, f.kind)).collect::<Vec<_>>(), [(Axis::X, None), (Axis::Y, None)]);
    /// assert!((found[0].at - 0.01143).abs() < 1e-15);
    /// // Ports at the ends: TE10 asks for a magnetic wall across the broad side, an electric one
    /// // across the narrow side.
    /// m.add_port(Port::new("1", End::Start))?;
    /// m.add_port(Port::new("2", End::End))?;
    /// let kinds: Vec<_> = m.find_symmetry().iter().map(|f| f.kind).collect();
    /// assert_eq!(kinds, [Some(SymmetryKind::Magnetic), Some(SymmetryKind::Electric)]);
    /// # Ok::<(), emcraft_model::ModelError>(())
    /// ```
    pub fn find_symmetry(&self) -> Vec<Found> {
        let Some(b) = self.bounds() else { return Vec::new() };
        let mut out = Vec::new();
        for axis in Axis::ALL {
            let at = 0.5 * (b.min[axis.index()] + b.max[axis.index()]);
            let plane = Symmetry { axis, at, kind: SymmetryKind::Magnetic };
            if self.symmetry_issue_of(&[plane]).is_some() {
                continue;
            }
            let walls: Vec<Option<SymmetryKind>> = self.ports.iter().map(|p| self.wall_for(p, axis)).collect();
            // No port decides (none, or square and round ones, whose first modes the plane's
            // wall chooses between): found, the wall left to choose. Ports asking for different
            // walls: not this plane.
            let kind = match walls.first() {
                None => None,
                Some(_) if walls.iter().all(Option::is_none) => None,
                Some(&first) if first.is_some() && walls.iter().all(|w| *w == first) => first,
                Some(_) => continue,
            };
            out.push(Found { axis, at, kind });
        }
        out
    }

    /// The wall a plane across `axis` needs to keep port `p`'s TE10: `None` when its face is not
    /// a rectangle (a circle) or the plane does not cross it. A square port's TE10 is the one
    /// along its first side (u: its modes come in the order of its sides), as it excites it.
    fn wall_for(&self, p: &crate::Port, axis: Axis) -> Option<SymmetryKind> {
        let crate::PortShape::Rect { a, b, .. } = self.port_shape_of(p).ok()? else { return None };
        let (u, v, _) = p.frame().axes();
        let square = (a - b).abs() <= 1e-9 * a.max(b);
        let (broad, narrow) = if a > b || square { (u, v) } else { (v, u) };
        match axis.index() {
            d if d == broad => Some(SymmetryKind::Magnetic),
            d if d == narrow => Some(SymmetryKind::Electric),
            _ => None,
        }
    }

    fn symmetry_issue_of(&self, planes: &[Symmetry]) -> Option<String> {
        if planes.is_empty() {
            return None;
        }
        let symmetric = || {
            Symmetry::check_planes(planes)?;
            match (&self.waveguide, &self.domain.solid) {
                (Some(w), _) => planes.iter().try_for_each(|p| p.check_guide(w)),
                (None, Some(solid)) => {
                    let tol = 1e-9 * self.bounds().map_or(1.0, |b| (b.max - b.min).norm());
                    planes.iter().try_for_each(|p| p.check_solid(self, solid, tol))
                }
                (None, None) => Err("needs a waveguide or a solid to cut".into()),
            }
        };
        symmetric().err()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Section, Shape, Solid, Vec3};

    /// A length (m), checked.
    fn metres(v: f64) -> crate::Metres {
        crate::Metres::new(v).unwrap()
    }

    /// A box's sides (m), checked.
    fn box_size(v: crate::Vec3) -> crate::BoxSize {
        crate::BoxSize::new(v).unwrap()
    }

    fn plane(axis: Axis, at: f64, kind: SymmetryKind) -> Symmetry {
        Symmetry { axis, at, kind }
    }

    #[test]
    fn symmetry_planes_need_a_symmetric_guide() {
        let mut m = Model::new();
        assert!(m.set_symmetry(vec![plane(Axis::X, 0.01, SymmetryKind::Magnetic)]).is_err(), "no geometry");
        let sec = |w: Option<f64>, o: Option<f64>| Section {
            length: metres(0.01),
            material: "vacuum".into(),
            width: w.map(metres),
            offset: o,
            height: None,
            offset_y: None,
        };
        let guide = |s: Vec<Section>| Waveguide {
            width: metres(0.02),
            height: metres(0.01),
            sections: s,
            layers: Vec::new(),
            strips: Vec::new(),
            vias: Vec::new(),
        };
        m.set_waveguide(guide(vec![sec(None, None), sec(Some(0.01), None), sec(None, None)])).unwrap();
        m.set_symmetry(vec![plane(Axis::X, 0.01, SymmetryKind::Magnetic), plane(Axis::Y, 0.005, SymmetryKind::Electric)]).unwrap();
        assert_eq!(m.symmetry_fraction(), 0.25);
        assert!(m.check().iter().all(|i| !matches!(i, crate::Issue::Symmetry(_))));
        for bad in [
            vec![plane(Axis::X, 0.009, SymmetryKind::Magnetic)],
            vec![plane(Axis::Z, 0.01, SymmetryKind::Magnetic)],
            vec![plane(Axis::X, f64::NAN, SymmetryKind::Magnetic)],
            vec![plane(Axis::X, 0.01, SymmetryKind::Magnetic), plane(Axis::X, 0.01, SymmetryKind::Electric)],
            vec![plane(Axis::X, 0.01, SymmetryKind::Magnetic); 3],
        ] {
            assert!(m.set_symmetry(bad).is_err());
        }
        assert_eq!(m.domain.symmetry.len(), 2, "a refused change keeps the planes");
        // An off-centre aperture breaks the symmetry; the check says so.
        m.set_waveguide(guide(vec![sec(None, None), sec(Some(0.01), Some(0.0))])).unwrap();
        assert!(m.check().iter().any(|i| matches!(i, crate::Issue::Symmetry(_))));
        m.set_symmetry(Vec::new()).unwrap();
        // Strips must mirror; a solid is refused.
        let mut w = guide(vec![sec(None, None)]);
        w.strips = vec![Strip { y: 0.002, x0: 0.002, width: metres(0.004), z0: 0.0, z1: 0.01 }];
        m.set_waveguide(w.clone()).unwrap();
        assert!(m.set_symmetry(vec![plane(Axis::X, 0.01, SymmetryKind::Magnetic)]).is_err());
        w.strips.push(Strip { y: 0.002, x0: 0.014, width: metres(0.004), z0: 0.0, z1: 0.01 });
        m.set_waveguide(w).unwrap();
        m.set_symmetry(vec![plane(Axis::X, 0.01, SymmetryKind::Electric)]).unwrap();
    }

    /// Regression: a square guide along y (5 × 5 mm, 100 mm long, ports on its ends) found no
    /// plane: a square port was taken to decide no wall, and the plane was dropped. Its TE10 is
    /// the one along its first side (x), E along z: a magnetic wall across x, an electric one
    /// across z, a quarter of the model.
    #[test]
    fn a_square_ports_te10_decides_the_walls() {
        let mut m = Model::new();
        m.set_solid(Solid { shape: Shape::Box { size: box_size(Vec3::new(0.005, 0.1, 0.005)) }, material: "vacuum".into() }).unwrap();
        for (name, at, end) in [("1", 0.0, crate::End::Start), ("2", 0.1, crate::End::End)] {
            let face = crate::PortFace { axis: Axis::Y, at, u0: 0.0, v0: 0.0, a: metres(0.005), b: metres(0.005) };
            m.add_port(crate::Port { face: Some(face), ..crate::Port::new(name, end) }).unwrap();
        }
        let found: Vec<(Axis, Option<SymmetryKind>)> = m.find_symmetry().iter().map(|f| (f.axis, f.kind)).collect();
        assert_eq!(found, [(Axis::X, Some(SymmetryKind::Magnetic)), (Axis::Z, Some(SymmetryKind::Electric))]);
    }

    #[test]
    fn a_solid_must_mirror_with_its_bodies_and_ports() {
        let mut m = Model::new();
        m.set_solid(Solid { shape: Shape::Box { size: box_size(Vec3::new(0.02, 0.01, 0.03)) }, material: "vacuum".into() }).unwrap();
        // A box about its middle, across any axis (z too: its ports lie on faces).
        m.set_symmetry(vec![plane(Axis::X, 0.01, SymmetryKind::Magnetic), plane(Axis::Z, 0.015, SymmetryKind::Electric)]).unwrap();
        assert_eq!(m.symmetry_fraction(), 0.25);
        assert!(m.set_symmetry(vec![plane(Axis::X, 0.012, SymmetryKind::Magnetic)]).is_err(), "off the middle");
        m.set_symmetry(Vec::new()).unwrap();
        // A port cut through its middle is kept; one off it, or on a face parallel to the plane, not.
        let face = |axis: Axis, at: f64, u0: f64, a: f64| crate::PortFace { axis, at, u0, v0: 0.0, a: metres(a), b: metres(0.01) };
        let port = |name: &str, f| crate::Port { face: Some(f), ..crate::Port::new(name, crate::End::Start) };
        m.add_port(port("1", face(Axis::Z, 0.0, 0.0, 0.02))).unwrap();
        m.set_symmetry(vec![plane(Axis::X, 0.01, SymmetryKind::Magnetic)]).unwrap();
        m.ports[0].face = Some(face(Axis::Z, 0.0, 0.0, 0.015));
        assert!(m.symmetry_issue().is_some_and(|why| why.contains("does not cut port 1")));
        m.ports[0].face = Some(face(Axis::X, 0.0, 0.0, 0.03));
        assert!(m.symmetry_issue().is_some_and(|why| why.contains("parallel to the plane")));
        m.ports.clear();
        // A post on one side needs its image on the other.
        let post = |x: f64| crate::Feature {
            combine: crate::Combine::Subtract,
            shape: Shape::Cylinder { radius: metres(0.001), length: metres(0.01) },
            at: Point3::new(x, 0.0, 0.015),
            axis: Axis::Y,
        };
        m.features.push(post(0.006));
        assert!(m.symmetry_issue().is_some_and(|why| why.starts_with("feature 1 has no mirror image")));
        m.features.push(post(0.014));
        assert_eq!(m.symmetry_issue(), None);
    }

    #[test]
    fn vias_must_mirror() {
        let mut m = Model::new();
        let via = Via { x0: 0.002, z0: 0.004, wx: metres(0.001), wz: metres(0.001), y0: 0.0, y1: 0.01 };
        let mut w = Waveguide {
            width: metres(0.02),
            height: metres(0.01),
            sections: vec![Section::new(metres(0.01), "vacuum")],
            layers: Vec::new(),
            strips: Vec::new(),
            vias: vec![via],
        };
        m.set_waveguide(w.clone()).unwrap();
        // Through the whole height it is its own image about y = b/2, not about x = a/2.
        m.set_symmetry(vec![plane(Axis::Y, 0.005, SymmetryKind::Electric)]).unwrap();
        let Some(why) = m.symmetry_issue_of(&[plane(Axis::X, 0.01, SymmetryKind::Magnetic)]) else { panic!("an unmirrored via passed") };
        assert!(why.starts_with("via 1 has no mirror image"), "{why}");
        w.vias.push(Via { x0: 0.017, ..via });
        m.set_waveguide(w).unwrap();
        m.set_symmetry(vec![plane(Axis::X, 0.01, SymmetryKind::Magnetic), plane(Axis::Y, 0.005, SymmetryKind::Electric)]).unwrap();
    }

    #[test]
    fn images_carry_the_walls_signs() {
        // A magnetic wall flips E's normal component, H's tangential ones.
        let Image { point, e, h } = plane(Axis::X, 0.01, SymmetryKind::Magnetic).image(Point3::new(0.015, 0.0, 0.0));
        assert!((point.x - 0.005).abs() < 1e-15 && e == [-1.0, 1.0, 1.0] && h == [1.0, -1.0, -1.0]);
        let Image { e, h, .. } = plane(Axis::Y, 0.005, SymmetryKind::Electric).image(Point3::new(0.0, 0.008, 0.0));
        assert!(e == [-1.0, 1.0, -1.0] && h == [1.0, -1.0, 1.0]);
        assert_eq!(plane(Axis::Y, 0.005, SymmetryKind::Electric).image(Point3::new(0.0, 0.002, 0.0)), Image::of(Point3::new(0.0, 0.002, 0.0)));
        // Through both planes the signs multiply.
        let planes = [plane(Axis::X, 0.01, SymmetryKind::Magnetic), plane(Axis::Y, 0.005, SymmetryKind::Electric)];
        let both = Symmetry::image_through(&planes, Point3::new(0.015, 0.008, 0.0));
        assert_eq!(both.point, Point3::new(2.0 * 0.01 - 0.015, 2.0 * 0.005 - 0.008, 0.0));
        assert!(both.e == [1.0, 1.0, -1.0] && both.h == [1.0, 1.0, -1.0]);
    }
}
