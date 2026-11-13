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
static LAST: Kept = Kept::new();

/// One kept design and the bytes it was opened from.
struct Kept(Mutex<Option<(Vec<u8>, Arc<sx::Design>)>>);

impl Kept {
    const fn new() -> Self {
        Self(Mutex::new(None))
    }

    /// The design of `bytes`, the kept one when it is the same file.
    fn design(&self, bytes: &[u8]) -> Result<Arc<sx::Design>> {
        let mut last = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((kept, design)) = last.as_ref()
            && kept.as_slice() == bytes
        {
            return Ok(Arc::clone(design));
        }
        let design = Arc::new(sx::open(bytes).map_err(failed)?);
        *last = Some((bytes.to_vec(), Arc::clone(&design)));
        Ok(design)
    }
}

/// The design of `bytes`, the one kept when it is the same file.
fn design(bytes: &[u8]) -> Result<Arc<sx::Design>> {
    LAST.design(bytes)
}

/// The design's parameters as it stores them: those made in its parameters table, not the
/// names SolidCraft gives its sketches' dimensions and its features' inputs (`d1`, `d2`…,
/// which follow those).
///
/// ```
/// let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/oracle/design-post/post.solidcraft");
/// let bytes = std::fs::read(path).unwrap_or_default();
/// let p = emcraft_io::solidcraft::parameters(&bytes).unwrap_or_default();
/// // The post's diameter, 3 mm as saved; its four parameters and none of the dimensions'.
/// assert!(p.iter().any(|q| q.name == "d" && q.value == Some(3.0) && q.unit == "mm"));
/// assert_eq!(p.iter().map(|q| q.name.as_str()).collect::<Vec<_>>(), ["a", "b", "l", "d"]);
/// assert!(emcraft_io::solidcraft::parameters(b"not a design").is_err());
/// assert!(emcraft_io::solidcraft::is_design("post.solidcraft") && !emcraft_io::solidcraft::is_design("post.3mf"));
/// ```
pub fn parameters(bytes: &[u8]) -> Result<Vec<Parameter>> {
    Ok(design(bytes)?
        .parameters()
        .into_iter()
        .filter(|p| p.source == sx::ParameterSource::User)
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

/// The chord tolerance a design is tessellated at when none is given, as a share of each body's
/// size: 2e-3, a twentieth of a millimetre on a 25 mm part. The mesh's own curved elements follow
/// the design's exact surfaces, so the tessellation only has to carry the shape to the mesher;
/// SolidCraft's own default (2e-4, for drawing) made curved walls' triangles a third of a typical
/// element and thin, and the mesher spent most of its time mending the tetrahedra on them (a
/// helical guide: 37 s for 90 000 tetrahedra, 8 s for 25 000 at this tolerance).
pub const MESH_TOLERANCE: f64 = 2e-3;

/// The design's bodies with `overrides` (parameter name, expression as the design takes it:
/// `3 mm`, `w / 2`) at chord tolerance `tolerance_mm` ([`MESH_TOLERANCE`] of each body's size
/// when `None`).
pub fn read(bytes: &[u8], overrides: &[(String, String)], tolerance_mm: Option<f64>) -> Result<Vec<Body>> {
    let tolerance = tolerance_mm.map_or(sx::Tolerance::Relative(MESH_TOLERANCE), sx::Tolerance::Absolute);
    let options = sx::Options::default().with_overrides(overrides.iter().map(|(n, e)| (n.as_str(), e.as_str()))).with_tolerance(tolerance);
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
        .map(|f| match &f.surface {
            sx::Surface::Plane { point: p, normal } => Some(FaceSurface::Plane { normal: vector(*normal), point: point(*p) }),
            sx::Surface::Cylinder { origin, axis, radius } => {
                Some(FaceSurface::Cylinder { origin: point(*origin), axis: vector(*axis), radius: radius.to_metres() })
            }
            sx::Surface::Sphere { center, radius } => Some(FaceSurface::Sphere { center: point(*center), radius: radius.to_metres() }),
            sx::Surface::Cone { apex, axis, half_angle } => {
                Some(FaceSurface::Cone { apex: point(*apex), axis: vector(*axis), half_angle: half_angle.0 })
            }
            // A free-form patch: its curved elements bent onto it (a malformed one is meshed
            // from its triangles, as any surface EMCraft has no exact form for).
            sx::Surface::Nurbs { u, v, net } => {
                let knots = |k: &sx::Knots| emcraft_model::nurbs::Knots { degree: k.degree as usize, knots: k.knots.clone() };
                let net = net.iter().map(|row| row.iter().map(|c| (point(c.point), c.weight)).collect()).collect();
                let n = emcraft_model::nurbs::Nurbs { u: knots(u), v: knots(v), net };
                n.check().is_ok().then(|| FaceSurface::Nurbs(Box::new(n)))
            }
            // Tori (a fit recovers them) and surfaces EMCraft has no exact form for: meshed from
            // their triangles.
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
        // A cache of its own: other tests opening other designs at the same time cannot
        // replace the kept one.
        let kept = Kept::new();
        let a = kept.design(POST).unwrap();
        assert!(Arc::ptr_eq(&a, &kept.design(POST).unwrap()), "the same bytes reuse the design");
        let fresh: Vec<Body> = {
            let d = sx::open(POST).unwrap();
            let options = sx::Options::default().with_override("d", "4 mm").with_tolerance(sx::Tolerance::Relative(MESH_TOLERANCE));
            let s = d.snapshot(&options).unwrap();
            s.bodies.iter().map(body).collect::<Result<_>>().unwrap()
        };
        let overrides = [("d".to_string(), "4 mm".to_string())];
        let _ = read(POST, &[], None).unwrap();
        assert_eq!(read(POST, &overrides, None).unwrap(), fresh, "a kept design gives what a fresh one does");
        let mut other = POST.to_vec();
        other.push(b' ');
        if let Ok(b) = kept.design(&other) {
            assert!(!Arc::ptr_eq(&a, &b), "other bytes open their own design");
        }
    }
}
