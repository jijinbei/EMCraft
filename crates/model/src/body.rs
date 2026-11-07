//! The model's bodies: each a shape placed in the region with what it is made of, in the order
//! they were placed — the one list the window, the commands and the files read
//! ([`crate::Model::bodies`]).
//!
//! How the region takes a body is its [`Kind`]: combined with the region's shape (a metal body
//! the region loses, one of the region's own material it gains, a clip it is cut to), meshed
//! apart (a dielectric, or a conductor lying inside), or an imported part's walls round the
//! region or the region itself. The mesher reads the combined bodies as the solid's features
//! and the apart ones as its inclusions ([`MeshView`]), each list in the bodies' order, so a
//! model meshes as it always has.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::geom::Point3;
use crate::shape::{Axis, Combine, Feature, Inclusion, Shape};
use crate::waveguide::Waveguide;

/// What a body is made of.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fill {
    /// A perfect conductor.
    Metal,
    /// The region's own material (whatever fills the region).
    Region,
    /// Nothing of its own: the region is cut to it (an intersection).
    Clip,
    /// A material of the model's, by name.
    Material(String),
}

/// How the region takes a body.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Combined with the region's shape, after the bodies combined before it: metal subtracted,
    /// the region's material added, a clip intersected.
    Combined,
    /// Meshed apart and joined to the region: a dielectric, or a conductor lying inside.
    Apart,
    /// An imported part's walls round the region (the region is the air they enclose).
    Walls,
    /// An imported part's body that is the region itself.
    Interior,
    /// The region itself, a box extruded along the axis (a waveguide): the slabs placed in it
    /// fill it.
    Extruded(crate::Axis),
    /// A slab of the extruded region: the region between two planes normal to the axis,
    /// filled over the box's cross-section (a waveguide's section along z, with its aperture;
    /// a layer across y).
    Slab(crate::Axis),
    /// Placed in the extruded region: the cells it holds take its fill (a via); a sheet lies
    /// on their faces (a strip).
    Cells,
}

/// A body.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Body {
    pub shape: Shape,
    /// Where its own origin lies (m).
    pub at: Point3,
    /// Its z axis turned onto this one.
    #[serde(default)]
    pub axis: Axis,
    pub fill: Fill,
    pub kind: Kind,
    /// An imported body's own name (others are named by their place: `body 1`, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl Body {
    /// The feature the mesher combines with the region, for a combined body.
    pub fn feature(&self) -> Option<Feature> {
        let combine = match (&self.kind, &self.fill) {
            (Kind::Combined, Fill::Metal) => Combine::Subtract,
            (Kind::Combined, Fill::Region) => Combine::Add,
            (Kind::Combined, Fill::Clip) => Combine::Intersect,
            _ => return None,
        };
        Some(Feature { combine, shape: self.shape.clone(), at: self.at, axis: self.axis })
    }

    /// The inclusion the mesher meshes apart, for a body meshed apart.
    pub fn inclusion(&self) -> Option<Inclusion> {
        let material = match (&self.kind, &self.fill) {
            (Kind::Apart, Fill::Metal) => None,
            (Kind::Apart, Fill::Material(m)) => Some(m.clone()),
            _ => return None,
        };
        Some(Inclusion { shape: self.shape.clone(), at: self.at, material })
    }

    /// Whether it is an imported part's walls or region (not in the region's mesh).
    pub fn is_part(&self) -> bool {
        matches!(self.kind, Kind::Walls | Kind::Interior)
    }

    /// Whether it belongs to an extruded region (a waveguide's box, slabs and cells).
    pub fn is_extruded(&self) -> bool {
        matches!(self.kind, Kind::Extruded(_) | Kind::Slab(_) | Kind::Cells)
    }

    /// The material's name when it is one of the model's.
    pub fn material(&self) -> Option<&str> {
        match &self.fill {
            Fill::Material(m) => Some(m),
            _ => None,
        }
    }
}

impl From<Feature> for Body {
    fn from(f: Feature) -> Body {
        let fill = match f.combine {
            Combine::Subtract => Fill::Metal,
            Combine::Add => Fill::Region,
            Combine::Intersect => Fill::Clip,
        };
        Body { shape: f.shape, at: f.at, axis: f.axis, fill, kind: Kind::Combined, name: None }
    }
}

impl From<Inclusion> for Body {
    fn from(inc: Inclusion) -> Body {
        let fill = inc.material.map_or(Fill::Metal, Fill::Material);
        Body { shape: inc.shape, at: inc.at, axis: Axis::Z, fill, kind: Kind::Apart, name: None }
    }
}

/// The bodies as the mesher reads them: the combined ones as features and the apart ones as
/// inclusions, each in the bodies' order, with where each lies among the bodies.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshView {
    pub features: Vec<Feature>,
    pub inclusions: Vec<Inclusion>,
    /// The body each feature is.
    pub feature_body: Vec<usize>,
    /// The body each inclusion is.
    pub inclusion_body: Vec<usize>,
}

impl MeshView {
    /// The view of `bodies`.
    ///
    /// ```
    /// use emcraft_model::body::{Body, MeshView};
    /// use emcraft_model::{Combine, Feature, Inclusion, Point3, Shape, Metres, Axis};
    ///
    /// let ball = Shape::Sphere { radius: Metres::new(0.002)? };
    /// let post = Body::from(Feature { combine: Combine::Subtract, shape: ball.clone(), at: Point3::ORIGIN, axis: Axis::Z });
    /// let bead = Body::from(Inclusion { shape: ball, at: Point3::new(0.01, 0.0, 0.0), material: Some("ptfe".into()) });
    /// let v = MeshView::of(&[bead, post]);
    /// assert_eq!((v.features.len(), v.inclusions.len()), (1, 1));
    /// assert_eq!((v.feature_body[0], v.inclusion_body[0]), (1, 0));
    /// # Ok::<(), emcraft_model::ModelError>(())
    /// ```
    pub fn of(bodies: &[Body]) -> MeshView {
        let mut v = MeshView::default();
        for (i, b) in bodies.iter().enumerate() {
            if let Some(f) = b.feature() {
                v.features.push(f);
                v.feature_body.push(i);
            } else if let Some(inc) = b.inclusion() {
                v.inclusions.push(inc);
                v.inclusion_body.push(i);
            }
        }
        v
    }
}

/// The views of the bodies (the mesher's, the waveguide's), each made when first read after the
/// bodies changed. Copies of a model share nothing; two models compare by their bodies alone.
#[derive(Debug, Default)]
pub(crate) struct ViewCache {
    mesh: OnceLock<MeshView>,
    guide: OnceLock<Option<Waveguide>>,
}

impl ViewCache {
    pub(crate) fn get(&self, bodies: &[Body]) -> &MeshView {
        self.mesh.get_or_init(|| MeshView::of(bodies))
    }

    pub(crate) fn guide(&self, bodies: &[Body]) -> Option<&Waveguide> {
        self.guide.get_or_init(|| Waveguide::of_bodies(bodies)).as_ref()
    }

    pub(crate) fn clear(&mut self) {
        *self = ViewCache::default();
    }
}

/// A copy of what a cell holds.
fn copied<T: Clone>(cell: &OnceLock<T>) -> OnceLock<T> {
    cell.get().cloned().map_or_else(OnceLock::new, OnceLock::from)
}

impl Clone for ViewCache {
    fn clone(&self) -> ViewCache {
        ViewCache { mesh: copied(&self.mesh), guide: copied(&self.guide) }
    }
}

impl PartialEq for ViewCache {
    fn eq(&self, _: &ViewCache) -> bool {
        true
    }
}

/// The bodies, changed in place; the mesher's view is made again after.
pub struct BodiesMut<'a> {
    pub(crate) bodies: &'a mut Vec<Body>,
    pub(crate) view: &'a mut ViewCache,
}

impl std::ops::Deref for BodiesMut<'_> {
    type Target = Vec<Body>;
    fn deref(&self) -> &Vec<Body> {
        self.bodies
    }
}

impl std::ops::DerefMut for BodiesMut<'_> {
    fn deref_mut(&mut self) -> &mut Vec<Body> {
        self.bodies
    }
}

impl Drop for BodiesMut<'_> {
    fn drop(&mut self) {
        self.view.clear();
    }
}
