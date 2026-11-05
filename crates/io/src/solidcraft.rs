//! SolidCraft designs (`.sc`, `.solidcraft`), opened through SolidCraft's exchange snapshot
//! (`solidcraft-exchange`, the one SolidCraft crate EMCraft depends on): each body a surface in
//! metres whose triangles carry their B-rep face, and each face's exact surface from the design
//! itself, not fitted.

use std::sync::{Arc, Mutex, PoisonError};

use emcraft_model::{FaceSurface, Point3, Vec3};
use solidcraft_exchange as sx;

use crate::{Error, MAX_COORD, MAX_TRIANGLES, Result, Surface};

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

/// The design last opened, by its bytes: a template's card redrawn as its parameters change
/// reads the same design again and again, and a kept design recomputes only the features the
/// changed parameters reach. One design is kept.
static LAST: Mutex<Option<(Vec<u8>, Arc<sx::Design>)>> = Mutex::new(None);

/// The design of `bytes`, the one kept when it is the same file.
fn design(bytes: &[u8]) -> Result<Arc<sx::Design>> {
    let mut last = LAST.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some((kept, design)) = last.as_ref()
        && kept.as_slice() == bytes
    {
        return Ok(Arc::clone(design));
    }
    let design = Arc::new(sx::open(bytes).map_err(failed)?);
    *last = Some((bytes.to_vec(), Arc::clone(&design)));
    Ok(design)
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
    Ok(design(bytes)?
        .parameters()
        .into_iter()
        .map(|p| {
            let value = match p.quantity() {
                Some(sx::Quantity::Length(l)) => Some(l.0),
                Some(sx::Quantity::Angle(a)) => Some(a.to_degrees()),
                Some(sx::Quantity::Number(n)) => Some(n),
                _ => None,
            };
            Parameter { name: p.name.0, value, unit: p.display_unit }
        })
        .collect())
}

/// The design's bodies with `overrides` (parameter name, expression as the design takes it:
/// `3 mm`, `w / 2`) at chord tolerance `tolerance_mm`.
pub fn read(bytes: &[u8], overrides: &[(String, String)], tolerance_mm: Option<f64>) -> Result<Vec<Body>> {
    let mut options = sx::Options::default().with_overrides(overrides.iter().map(|(n, e)| (n.as_str(), e.as_str())));
    if let Some(t) = tolerance_mm {
        options = options.with_tolerance(sx::Tolerance::Absolute(t));
    }
    let snapshot = design(bytes)?.snapshot(&options).map_err(failed)?;
    let triangles = snapshot.triangle_count();
    if triangles > MAX_TRIANGLES {
        return Err(Error::Invalid(format!("the design's bodies have {triangles} triangles; at most {MAX_TRIANGLES} are read")));
    }
    snapshot.bodies.iter().map(body).collect()
}

fn point(p: sx::Point) -> Point3 {
    let [x, y, z] = p.to_metres();
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
    let (triangles, tags) = b.tagged_triangles();
    let surface = Surface { name: b.name.to_string(), positions, triangles, tags };
    let surfaces = b
        .faces
        .iter()
        .map(|f| match f.surface {
            sx::Surface::Plane { point: p, normal } => Some(FaceSurface::Plane { normal: vector(normal), point: point(p) }),
            sx::Surface::Cylinder { origin, axis, radius } => {
                Some(FaceSurface::Cylinder { origin: point(origin), axis: vector(axis), radius: radius.to_metres() })
            }
            sx::Surface::Sphere { center, radius } => Some(FaceSurface::Sphere { center: point(center), radius: radius.to_metres() }),
            sx::Surface::Cone { apex, axis, half_angle } => {
                Some(FaceSurface::Cone { apex: point(apex), axis: vector(axis), half_angle: half_angle.0 })
            }
            // Tori and free-form patches EMCraft has no exact form for: meshed from their triangles.
            _ => None,
        })
        .collect();
    Ok(Body { surface, face_names: b.faces.iter().map(|f| f.name.to_string()).collect(), surfaces })
}

#[cfg(test)]
mod tests {
    use super::*;

    const POST: &[u8] = include_bytes!("../../../tests/oracle/design-post/post.solidcraft");

    /// The same file is opened once and its design kept; another file replaces it; the kept
    /// design gives what a fresh one gives.
    #[test]
    fn the_last_design_is_kept_by_its_bytes() {
        let a = design(POST).unwrap();
        assert!(Arc::ptr_eq(&a, &design(POST).unwrap()), "the same bytes reuse the design");
        let fresh: Vec<Body> = {
            let d = sx::open(POST).unwrap();
            let s = d.snapshot(&sx::Options::default().with_override("d", "4 mm")).unwrap();
            s.bodies.iter().map(body).collect::<Result<_>>().unwrap()
        };
        let overrides = [("d".to_string(), "4 mm".to_string())];
        let _ = read(POST, &[], None).unwrap();
        assert_eq!(read(POST, &overrides, None).unwrap(), fresh, "a kept design gives what a fresh one does");
        let mut other = POST.to_vec();
        other.push(b' ');
        if let Ok(b) = design(&other) {
            assert!(!Arc::ptr_eq(&a, &b), "other bytes open their own design");
        }
    }
}
