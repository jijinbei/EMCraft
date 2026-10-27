//! Open problems: the boundary that lets the field radiate (a perfectly matched layer), the
//! sources that drive it and the lumped ports that feed antennas.

use serde::{Deserialize, Serialize};

use crate::error::{ModelError, Result, invalid, named, positive};
use crate::geom::{Aabb, Point3, Vec3};
use crate::model::Model;
use crate::shape::{Shape, Solid};

/// An open boundary: a perfectly matched layer of this thickness (m) lining every face of a box
/// solid, or the outer shell of a sphere solid, the conducting walls behind it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Open {
    pub thickness: f64,
    /// The reflection the layer is designed for at normal incidence (its absorption profile's
    /// strength); `None` for the default, 1e-6.
    #[serde(default)]
    pub reflection: Option<f64>,
    /// The layer's shape: Cartesian slabs on a box's faces (the default, and what models saved
    /// before there was a choice hold), or a spherical shell stretched along the radius.
    #[serde(default)]
    pub shape: OpenShape,
}

/// The shape of a perfectly matched layer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OpenShape {
    /// Slabs lining every face of a box solid, stretched along x, y and z.
    #[default]
    Box,
    /// The outer shell R − d < r < R of a sphere solid of radius R (centred on the origin),
    /// stretched along r only.
    Sphere,
}

/// A spherical perfectly matched layer: the shell `inner` < r < `outer` (m) about the origin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SphericalLayer {
    /// The radius the layer starts at, R − d.
    pub inner: f64,
    /// The sphere solid's radius R, where the layer ends on the conducting wall.
    pub outer: f64,
}

impl Open {
    /// Checks the layer on its own: a positive thickness, a reflection between 0 and 1.
    pub fn check(&self) -> Result<()> {
        positive("layer thickness", self.thickness)?;
        if let Some(r) = self.reflection
            && !(r > 0.0 && r < 1.0)
        {
            return Err(invalid("layer reflection", "must be between 0 and 1"));
        }
        Ok(())
    }
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

/// The vector made a unit one, refused when it has no direction.
fn unit(name: &str, v: Vec3) -> Result<Vec3> {
    v.normalized().ok_or_else(|| invalid(name, "must be a nonzero finite vector"))
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
                if k.dot(e).abs() > 1e-9 {
                    return Err(invalid("plane wave", "its polarization must be perpendicular to its direction"));
                }
                Source::PlaneWave { direction: k, polarization: e }
            }
        })
    }

    /// Where the source sits, for a localized one (a dipole).
    pub fn position(&self) -> Option<Point3> {
        match self {
            Source::Dipole { at, .. } => Some(*at),
            Source::PlaneWave { .. } => None,
        }
    }
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

impl LumpedPort {
    /// Checks the port's own settings: a positive impedance, a finite radius ≥ 0, finite ends
    /// and voltage, and a gap between two different points.
    pub fn check(&self) -> Result<()> {
        positive("port impedance", self.impedance)?;
        if !(self.radius.is_finite() && self.radius >= 0.0) {
            return Err(invalid("lumped port radius", "must be a finite number ≥ 0"));
        }
        if !(self.from.is_finite() && self.to.is_finite() && self.voltage.iter().all(|v| v.is_finite())) {
            return Err(invalid("lumped port", "its ends and voltage must be finite"));
        }
        if !(self.from.distance(self.to) > 0.0) {
            return Err(invalid("lumped port", "its two ends must differ"));
        }
        Ok(())
    }
}

/// Whether a box lies strictly inside the ball of this radius about the origin (its farthest
/// corner does).
pub(crate) fn inside_ball(b: &Aabb, radius: f64) -> bool {
    b.min.zip_map(b.max, |lo, hi| lo.abs().max(hi.abs())).to_vec().norm() < radius
}

impl Model {
    /// Lines the box solid's faces with a perfectly matched layer of `thickness` (m) — or, for a
    /// spherical layer, makes the sphere solid's outer shell of that thickness the layer — or
    /// closes it again with `None`.
    pub fn set_open(&mut self, open: Option<Open>) -> Result<()> {
        if let Some(o) = open {
            o.check()?;
            match (o.shape, &self.solid) {
                (OpenShape::Box, Some(Solid { shape: Shape::Box { size }, .. })) => {
                    if size.iter().any(|s| 2.0 * o.thickness >= s) {
                        return Err(invalid("layer thickness", "must leave room inside the box (under half its smallest side)"));
                    }
                    if self.ports.iter().any(|p| p.face.is_none()) {
                        return Err(invalid(
                            "open boundary",
                            "ports at the box's ends would lie under the layer: put them on faces inside it (`face`)",
                        ));
                    }
                }
                (OpenShape::Sphere, Some(Solid { shape: Shape::Sphere { radius }, .. })) => {
                    if o.thickness >= 0.75 * radius {
                        return Err(invalid("layer thickness", "must leave room inside the sphere (under three quarters of its radius)"));
                    }
                    if !self.ports.is_empty() {
                        return Err(invalid(
                            "open boundary",
                            "not supported yet: waveguide ports with a spherical layer (use lumped ports or sources)",
                        ));
                    }
                    if !self.features.is_empty() {
                        return Err(invalid("open boundary", "not supported yet: features combined with a sphere lined by a spherical layer"));
                    }
                    if self.inclusions.iter().any(|i| !inside_ball(&i.bounds(), radius - o.thickness)) {
                        return Err(invalid("open boundary", "the inclusions must lie inside the spherical layer's inner sphere"));
                    }
                }
                (OpenShape::Box, _) => {
                    return Err(invalid(
                        "open boundary",
                        "not supported yet: a perfectly matched layer needs a box solid (geometry.solid, shape box) to line, or shape sphere for a sphere solid",
                    ));
                }
                (OpenShape::Sphere, _) => {
                    return Err(invalid("open boundary", "a spherical layer needs a sphere solid (geometry.solid, shape sphere) whose shell it is"));
                }
            }
        }
        self.open = open;
        Ok(())
    }

    /// The spherical layer, when the model is open with one.
    pub fn spherical_layer(&self) -> Option<SphericalLayer> {
        match (&self.open, &self.solid) {
            (Some(Open { shape: OpenShape::Sphere, thickness, .. }), Some(Solid { shape: Shape::Sphere { radius }, .. })) => {
                Some(SphericalLayer { inner: radius - thickness, outer: *radius })
            }
            _ => None,
        }
    }

    /// Adds a source for an open problem.
    pub fn add_source(&mut self, src: Source) -> Result<()> {
        let src = src.checked()?;
        if let Some(at) = src.position()
            && let Some(b) = self.bounds()
            && !b.holds_inside(at)
        {
            return Err(invalid("dipole", "must lie inside the geometry"));
        }
        self.sources.push(src);
        Ok(())
    }

    /// Adds a lumped port.
    pub fn add_lumped_port(&mut self, p: LumpedPort) -> Result<()> {
        named("port name", &p.name)?;
        if self.has_port_named(&p.name) {
            return Err(ModelError::Duplicate { kind: "port", name: p.name });
        }
        p.check()?;
        if let Some(b) = self.bounds()
            && !(b.holds_inside(p.from) && b.holds_inside(p.to))
        {
            return Err(invalid("lumped port", "must lie inside the geometry"));
        }
        self.lumped.push(p);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Axis, Combine, End, Feature, Inclusion, Port};

    #[test]
    fn open_boundaries_match_their_solids() {
        let open = |thickness: f64, shape: OpenShape| Some(Open { thickness, reflection: None, shape });
        let mut m = Model::new();
        m.set_solid(Solid { shape: Shape::Sphere { radius: 0.03 }, material: "vacuum".into() }).unwrap();
        // A box layer needs a box; a spherical one a sphere with room inside.
        assert!(m.set_open(open(0.01, OpenShape::Box)).is_err());
        for d in [0.0, -0.01, f64::NAN, f64::INFINITY, 0.0225, 0.05] {
            assert!(m.set_open(open(d, OpenShape::Sphere)).is_err(), "{d}");
        }
        m.set_open(open(0.01, OpenShape::Sphere)).unwrap();
        assert_eq!(m.spherical_layer(), Some(SphericalLayer { inner: 0.03 - 0.01, outer: 0.03 }));
        // Inclusions inside the inner sphere only; no features or waveguide ports.
        let inc = |x: f64| Inclusion { shape: Shape::Sphere { radius: 0.004 }, at: Point3::new(x, 0.0, 0.0), material: None };
        assert!(m.add_inclusion(inc(0.018)).is_err());
        m.add_inclusion(inc(0.0)).unwrap();
        let f = Feature { combine: Combine::Add, shape: Shape::Box { size: Vec3::new(0.01, 0.01, 0.01) }, at: Point3::ORIGIN, axis: Axis::Z };
        assert!(m.add_feature(f).is_err());
        let port = Port::new("1", End::Start);
        assert!(m.add_port(port).is_err());
        // A box keeps its default layer; models saved before the shape existed read as boxes.
        m.set_solid(Solid { shape: Shape::Box { size: Vec3::new(0.04, 0.04, 0.04) }, material: "vacuum".into() }).unwrap();
        assert!(m.set_open(open(0.01, OpenShape::Sphere)).is_err());
        m.set_open(open(0.01, OpenShape::Box)).unwrap();
        assert!(m.set_open(Some(Open { thickness: 0.01, reflection: Some(1.0), shape: OpenShape::Box })).is_err());
        let old: Open = serde_json::from_str(r#"{"thickness": 0.01, "reflection": null}"#).unwrap();
        assert_eq!(old.shape, OpenShape::Box);
        assert_eq!(m.spherical_layer(), None);
    }

    #[test]
    fn sources_and_lumped_ports_lie_inside() {
        let mut m = Model::new();
        m.set_solid(Solid { shape: Shape::Box { size: Vec3::new(0.04, 0.04, 0.04) }, material: "vacuum".into() }).unwrap();
        let dipole = |x: f64| Source::Dipole { at: Point3::new(x, 0.02, 0.02), direction: Vec3::new(0.0, 0.0, 2.0), length: 0.001, current: 1.0 };
        assert!(m.add_source(dipole(0.04)).is_err(), "on the wall");
        m.add_source(dipole(0.02)).unwrap();
        assert!(matches!(m.sources[0], Source::Dipole { direction, .. } if direction == Vec3::new(0.0, 0.0, 1.0)));
        let skew = Source::PlaneWave { direction: Vec3::new(0.0, 0.0, 1.0), polarization: Vec3::new(0.0, 1.0, 1.0) };
        assert!(m.add_source(skew).is_err());
        let feed = LumpedPort {
            name: "feed".into(),
            from: Point3::new(0.02, 0.02, 0.019),
            to: Point3::new(0.02, 0.02, 0.021),
            impedance: 50.0,
            voltage: unit_voltage(),
            radius: 0.0,
        };
        for bad in [
            LumpedPort { name: " ".into(), ..feed.clone() },
            LumpedPort { impedance: 0.0, ..feed.clone() },
            LumpedPort { radius: -1.0, ..feed.clone() },
            LumpedPort { to: feed.from, ..feed.clone() },
            LumpedPort { to: Point3::new(0.02, 0.02, 0.05), ..feed.clone() },
        ] {
            assert!(m.add_lumped_port(bad).is_err());
        }
        m.add_lumped_port(feed.clone()).unwrap();
        assert!(matches!(m.add_lumped_port(feed), Err(ModelError::Duplicate { .. })));
        assert_eq!(m.radiators().count(), 3, "the dipole and the feed's two ends");
    }
}
