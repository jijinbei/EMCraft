//! The region a model is solved in (docs/adr/0005-geometry-ux.md): pinned — the solid as set —
//! or automatic, the box round what the model holds grown by a margin and lined by a perfectly
//! matched layer, its filling the background.
//!
//! **The automatic region** holds the bodies (features and inclusions), the sources, the lumped
//! ports and the port faces. Round their bounding box it adds a `margin` of background and a
//! layer `thickness` thick, both a quarter of the wavelength at the band's top by default (a
//! quarter of what it holds before there is a band, 10 mm round a lone point), rounded up to
//! two significant digits. It is the block those make
//! ([`crate::Shape::block`]), lined by the box layer; it follows the model as it changes
//! ([`crate::Model::refresh_region`]) and keeps the mesh while it stays the same.

use serde::{Deserialize, Serialize};

use crate::error::{Result, invalid};
use crate::geom::Aabb;
use crate::open::{Open, OpenShape};
use crate::quantity::Metres;
use crate::shape::{Shape, Solid};

/// How a model's region is set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum Region {
    /// The solid as it was set (geometry.solid, an import, a template).
    #[default]
    Pinned,
    /// The box round what the model holds, with its margin and layer (each a quarter of the
    /// wavelength at the band's top when not given).
    Auto {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        margin: Option<Metres>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        thickness: Option<Metres>,
    },
}

/// The margin and layer (m) round a lone point before there is a band.
pub const PROVISIONAL_MARGIN: f64 = 0.01;

/// The material an automatic region is filled with when none is set.
pub const BACKGROUND: &str = "vacuum";

impl crate::Model {
    /// What an automatic region must hold: the bodies', sources', lumped ports' and port faces'
    /// bounding box; none when the model holds nothing yet.
    pub fn held(&self) -> Option<Aabb> {
        let corners = self
            .features()
            .iter()
            .flat_map(|f| f.bounds().corners())
            .chain(self.radiators())
            .chain(self.ports.iter().filter_map(|p| p.face.as_ref()).flat_map(crate::port::PortFace::corners));
        Aabb::around(corners)
    }

    /// The automatic region's box and layer thickness (m) for what the model holds now; none
    /// when the region is pinned or nothing is held.
    ///
    /// ```
    /// use emcraft_model::{Feature, Model, Point3, Region, Shape, Metres, Combine, Axis};
    ///
    /// let mut m = Model::new();
    /// m.domain.region = Region::Auto { margin: Some(Metres::new(0.01)?), thickness: Some(Metres::new(0.005)?) };
    /// m.bodies_mut().push(emcraft_model::Body::from(Feature { combine: Combine::Subtract, shape: Shape::Sphere { radius: Metres::new(0.002)? }, at: Point3::ORIGIN, axis: Axis::Z }));
    /// let (b, t) = m.automatic_region()?.unwrap();
    /// assert!((b.min.x + 0.017).abs() < 1e-12 && (b.max.z - 0.017).abs() < 1e-12 && t == 0.005);
    /// # Ok::<(), emcraft_model::ModelError>(())
    /// ```
    pub fn automatic_region(&self) -> Result<Option<(Aabb, f64)>> {
        self.automatic_region_with(None)
    }

    /// [`Self::automatic_region`] holding `extra` too (a body about to be placed).
    pub fn automatic_region_with(&self, extra: Option<Aabb>) -> Result<Option<(Aabb, f64)>> {
        let Region::Auto { margin, thickness } = self.domain.region else { return Ok(None) };
        let held = match (self.held(), extra) {
            (Some(a), Some(b)) => Some(a.union(b)),
            (a, b) => a.or(b),
        };
        let Some(held) = held else { return Ok(None) };
        let extent = (held.max - held.min).norm();
        // Before there is a band: a quarter of what it holds, 10 mm round a point.
        let provisional = if extent > 0.0 { 0.25 * extent } else { PROVISIONAL_MARGIN };
        let quarter = self.job.range().map_or(provisional, |b| crate::units::C0 / b.stop.get() / 4.0);
        // Given values as given; the defaults rounded up to two significant digits, so the box's
        // sides read as numbers.
        let m = margin.map_or_else(|| round_up(quarter), Metres::get);
        let t = thickness.map_or_else(|| round_up(quarter), Metres::get);
        if !(m > 0.0 && t > 0.0 && m.is_finite() && t.is_finite()) {
            return Err(invalid("region", "its margin and layer need a size: give them, or a band (project.setup)"));
        }
        Ok(Some((held.grown(m + t), t)))
    }

    /// Makes an automatic region the box [`Self::automatic_region`] gives (its filling the
    /// region's material, or the background), lined by its layer; whether it changed (the mesh
    /// is dropped then). A pinned region, or one holding nothing yet, is left as it is.
    pub fn refresh_region(&mut self) -> Result<bool> {
        self.refresh_region_with(None)
    }

    /// [`Self::refresh_region`] holding `extra` too.
    pub fn refresh_region_with(&mut self, extra: Option<Aabb>) -> Result<bool> {
        let Some((b, t)) = self.automatic_region_with(extra)? else { return Ok(false) };
        if self.ports.iter().any(|p| p.face.is_none()) {
            return Err(invalid(
                "region",
                "an automatic region's waveguide ports lie on faces inside it (model.port.create with `face`): its ends are the layer",
            ));
        }
        let shape = Shape::block(b.min, b.max)?;
        let material = self
            .domain
            .solid
            .as_ref()
            .map_or_else(|| self.domain.background.clone().unwrap_or_else(|| BACKGROUND.to_owned()), |s| s.material.clone());
        self.material(&material)?;
        let reflection = self.domain.open.and_then(|o| o.reflection);
        let open = Open { thickness: Metres::new(t)?, reflection, shape: OpenShape::Box };
        if self.domain.solid.as_ref().is_some_and(|s| s.shape == shape && s.material == material) && self.domain.open == Some(open) {
            return Ok(false);
        }
        self.domain.solid = Some(Solid { shape, material });
        self.set_parts(Vec::new());
        self.waveguide = None;
        self.mesh = None;
        self.domain.open = Some(open);
        Ok(true)
    }
}

/// `x` (> 0) rounded up to two significant digits: 7.4948 mm → 7.5 mm, 0.0433 → 0.044.
fn round_up(x: f64) -> f64 {
    if !(x > 0.0 && x.is_finite()) {
        return x;
    }
    let p = 10f64.powf(x.log10().floor() - 1.0);
    (x / p - 1e-9).ceil() * p
}
