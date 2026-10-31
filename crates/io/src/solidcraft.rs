//! SolidCraft designs (`.sc`, `.solidcraft`), opened through SolidCraft's exchange snapshot
//! (`solidcraft-exchange`, the one SolidCraft crate EMCraft depends on): each body a surface in
//! metres whose triangles carry their B-rep face, and each face's exact surface from the design
//! itself, not fitted.

use emcraft_model::{FaceSurface, Point3, Vec3};
use solidcraft_exchange as sx;

use crate::{Error, MAX_COORD, MAX_TRIANGLES, Result, Surface};

/// Metres per millimetre: the snapshot's lengths are millimetres.
const MM: f64 = 1e-3;

/// A body of a design: its surface, each face's persistent name and exact surface (by tag).
#[derive(Clone, Debug, PartialEq)]
pub struct Body {
    pub surface: Surface,
    pub face_names: Vec<String>,
    pub surfaces: Vec<Option<FaceSurface>>,
}

/// A design's parameter: name, value in its display unit, the unit.
#[derive(Clone, Debug, PartialEq)]
pub struct Parameter {
    pub name: String,
    pub value: Option<f64>,
    pub unit: String,
}

/// Whether a file is a SolidCraft design, by its extension.
pub fn is_design(name: &str) -> bool {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    matches!(ext.as_str(), "sc" | "solidcraft")
}

fn failed(e: sx::Error) -> Error {
    Error::Invalid(format!("SolidCraft design: {e}"))
}

/// The design's parameters as it stores them.
///
/// ```
/// let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/oracle/design-post/post.solidcraft");
/// let bytes = std::fs::read(path).unwrap_or_default();
/// let p = emcraft_io::solidcraft::parameters(&bytes).unwrap_or_default();
/// // The post's diameter, 3 mm as saved.
/// assert!(p.iter().any(|q| q.name == "d" && q.value == Some(3.0) && q.unit == "mm"));
/// assert!(emcraft_io::solidcraft::parameters(b"not a design").is_err());
/// assert!(emcraft_io::solidcraft::is_design("post.solidcraft") && !emcraft_io::solidcraft::is_design("post.3mf"));
/// ```
pub fn parameters(bytes: &[u8]) -> Result<Vec<Parameter>> {
    let design = sx::open(bytes).map_err(failed)?;
    Ok(design
        .parameters()
        .into_iter()
        .map(|p| {
            let value = match p.value {
                sx::ParameterValue::Quantity(sx::Quantity::Length(l)) => Some(l.0),
                sx::ParameterValue::Quantity(sx::Quantity::Angle(a)) => Some(a.to_degrees()),
                sx::ParameterValue::Quantity(sx::Quantity::Number(n)) => Some(n),
                _ => None,
            };
            Parameter { name: p.name.0, value, unit: p.display_unit }
        })
        .collect())
}

/// The design's bodies with `overrides` (parameter name, expression as the design takes it:
/// `3 mm`, `w / 2`) at chord tolerance `tolerance_mm`.
pub fn read(bytes: &[u8], overrides: &[(String, String)], tolerance_mm: Option<f64>) -> Result<Vec<Body>> {
    let design = sx::open(bytes).map_err(failed)?;
    let mut options = sx::Options::default();
    for (name, expression) in overrides {
        options = options.with_override(name.as_str(), expression.as_str());
    }
    if let Some(t) = tolerance_mm {
        options = options.with_tolerance(sx::Tolerance::Absolute(t));
    }
    let snapshot = design.snapshot(&options).map_err(failed)?;
    let triangles: usize = snapshot.bodies.iter().flat_map(|b| &b.faces).map(|f| f.triangles.len()).sum();
    if triangles > MAX_TRIANGLES {
        return Err(Error::Invalid(format!("the design's bodies have {triangles} triangles; at most {MAX_TRIANGLES} are read")));
    }
    snapshot.bodies.iter().map(body).collect()
}

fn point(p: sx::Point) -> Point3 {
    let [x, y, z] = p.0.map(|c| c * MM);
    Point3::new(x, y, z)
}

fn vector(d: sx::Direction) -> Vec3 {
    let [x, y, z] = d.get();
    Vec3::new(x, y, z)
}

fn body(b: &sx::Body) -> Result<Body> {
    let positions: Vec<Point3> = b.positions.iter().map(|p| point(*p)).collect();
    if positions.iter().flat_map(|p| p.to_array()).any(|c| !c.is_finite() || c.abs() > MAX_COORD) {
        return Err(Error::Invalid(format!("body `{}`: a coordinate is out of range", b.name)));
    }
    let mut surface = Surface { name: b.name.to_string(), positions, ..Surface::default() };
    for f in &b.faces {
        for t in &f.triangles {
            surface.triangles.push(t.map(|v| v.0));
            surface.tags.push(f.index.0);
        }
    }
    let surfaces = b
        .faces
        .iter()
        .map(|f| match f.surface {
            sx::Surface::Plane { point: p, normal } => Some(FaceSurface::Plane { normal: vector(normal), point: point(p) }),
            sx::Surface::Cylinder { origin, axis, radius } => {
                Some(FaceSurface::Cylinder { origin: point(origin), axis: vector(axis), radius: radius.0 * MM })
            }
            sx::Surface::Sphere { center, radius } => Some(FaceSurface::Sphere { center: point(center), radius: radius.0 * MM }),
            sx::Surface::Cone { apex, axis, half_angle } => {
                Some(FaceSurface::Cone { apex: point(apex), axis: vector(axis), half_angle: half_angle.0 })
            }
            // Tori and free-form patches EMCraft has no exact form for: meshed from their triangles.
            _ => None,
        })
        .collect();
    Ok(Body { surface, face_names: b.faces.iter().map(|f| f.name.to_string()).collect(), surfaces })
}
