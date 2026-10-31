//! Symmetry planes: the half of a symmetric model cut away and replaced by a wall.

use serde::{Deserialize, Serialize};

use crate::error::{Result, invalid};
use crate::geom::Point3;
use crate::model::Model;
use crate::shape::Axis;
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
        if planes.iter().any(|p| p.axis == Axis::Z) {
            return Err("a symmetry plane must be x = const or y = const (the ports lie on z planes)".into());
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
            _ => (w.height.get(), "y = b/2"),
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
        let symmetric = || {
            Symmetry::check_planes(planes)?;
            let Some(w) = &self.waveguide else {
                return Err(if self.solid.is_some() {
                    "not supported yet: symmetry planes in a solid (they need the structured waveguide mesh, geometry.waveguide)".into()
                } else {
                    "needs a waveguide (geometry.waveguide) to cut".into()
                });
            };
            planes.iter().try_for_each(|p| p.check_guide(w))
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
        assert_eq!(m.symmetry.len(), 2, "a refused change keeps the planes");
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
        m.set_solid(Solid { shape: Shape::Box { size: box_size(Vec3::new(0.02, 0.01, 0.01)) }, material: "vacuum".into() }).unwrap();
        assert!(m.check().iter().any(|i| i.to_string().contains("not supported yet")));
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
