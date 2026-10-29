//! Solid geometry: primitive and imported shapes, the solid they bound, the inclusions inside
//! it and the features combined with it.

use std::f64::consts::PI;

use serde::{Deserialize, Serialize};

use crate::error::{Result, invalid, positive};
use crate::geom::{self, Aabb, Point3, Vec3};
use crate::port::{End, PortShape};

/// A primitive body, meshed with unstructured tetrahedra.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Shape {
    /// [0, x] × [0, y] × [0, z].
    Box { size: Vec3 },
    /// Axis z through the origin, from z = 0 to `length`.
    Cylinder { radius: f64, length: f64 },
    /// Centred on the origin.
    Sphere { radius: f64 },
    /// A closed triangulated surface (imported): positions (m), outward triangles and a face
    /// tag per triangle; `surfaces[tag]` the face's exact surface where one was recovered from
    /// its vertices (a plane, cylinder, sphere, cone or torus fitted on import), else it is faceted.
    Surface {
        name: String,
        positions: Vec<Point3>,
        triangles: Vec<[u32; 3]>,
        tags: Vec<u32>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        surfaces: Vec<Option<FaceSurface>>,
    },
}

/// The exact surface of an imported face, recovered by fitting its vertices (untrimmed: the
/// face's triangles say which part of it the face is). Axes and normals are unit vectors.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FaceSurface {
    Plane {
        normal: Vec3,
        point: Point3,
    },
    /// The points at `radius` from the line through `origin` along `axis`.
    Cylinder {
        origin: Point3,
        axis: Vec3,
        radius: f64,
    },
    Sphere {
        center: Point3,
        radius: f64,
    },
    /// The points whose direction from `apex` makes `half_angle` (radians, in (0, π/2)) with
    /// `axis`, which points into the cone.
    Cone {
        apex: Point3,
        axis: Vec3,
        half_angle: f64,
    },
    /// The points at `minor` from the circle of radius `major` about `axis` through `center`, in
    /// the plane across the axis there.
    Torus {
        center: Point3,
        axis: Vec3,
        major: f64,
        minor: f64,
    },
}

impl FaceSurface {
    /// The kind, for display: `plane`, `cylinder`, `sphere`, `cone`, `torus`.
    pub fn kind(&self) -> &'static str {
        match self {
            FaceSurface::Plane { .. } => "plane",
            FaceSurface::Cylinder { .. } => "cylinder",
            FaceSurface::Sphere { .. } => "sphere",
            FaceSurface::Cone { .. } => "cone",
            FaceSurface::Torus { .. } => "torus",
        }
    }

    /// Whether the surface is curved (not a plane).
    pub fn is_curved(&self) -> bool {
        !matches!(self, FaceSurface::Plane { .. })
    }

    /// The closest point on the surface, or `None` where it is not unique (on a cylinder's, cone's or torus'
    /// axis, at a sphere's centre) or the data is not finite.
    pub fn project(&self, p: Point3) -> Option<Point3> {
        let out = match *self {
            FaceSurface::Plane { normal, point } => {
                let n = normal.normalized()?;
                p - n * (p - point).dot(n)
            }
            FaceSurface::Cylinder { origin, axis, radius } => {
                let a = axis.normalized()?;
                let d = p - origin;
                let foot = origin + a * d.dot(a);
                foot + (p - foot).normalized()? * radius
            }
            FaceSurface::Sphere { center, radius } => center + (p - center).normalized()? * radius,
            FaceSurface::Cone { apex, axis, half_angle } => {
                let a = axis.normalized()?;
                let d = p - apex;
                let r = (d - a * d.dot(a)).normalized()?;
                // The generator through p's half-plane, from the apex.
                let g = a * half_angle.cos() + r * half_angle.sin();
                apex + g * d.dot(g).max(0.0)
            }
            FaceSurface::Torus { center, axis, major, minor } => {
                let ring = Self::ring(center, axis, major, p)?;
                ring + (p - ring).normalized()? * minor
            }
        };
        out.is_finite().then_some(out)
    }

    /// The distance from `p` to the surface (infinite where [`FaceSurface::project`] has no
    /// answer).
    pub fn distance(&self, p: Point3) -> f64 {
        self.project(p).map_or(f64::INFINITY, |q| q.distance(p))
    }

    /// The unit normal of the surface nearest `p` (away from a cylinder's axis or a sphere's
    /// centre, along a plane's normal).
    pub fn normal(&self, p: Point3) -> Option<Vec3> {
        match *self {
            FaceSurface::Plane { normal, .. } => normal.normalized(),
            FaceSurface::Cylinder { origin, axis, .. } => {
                let a = axis.normalized()?;
                let d = p - origin;
                (d - a * d.dot(a)).normalized()
            }
            FaceSurface::Sphere { center, .. } => (p - center).normalized(),
            FaceSurface::Cone { apex, axis, half_angle } => {
                let a = axis.normalized()?;
                let d = p - apex;
                let r = (d - a * d.dot(a)).normalized()?;
                (r * half_angle.cos() - a * half_angle.sin()).normalized()
            }
            FaceSurface::Torus { center, axis, major, .. } => (p - Self::ring(center, axis, major, p)?).normalized(),
        }
    }

    /// A torus' point on its central circle nearest `p` (none on the axis).
    fn ring(center: Point3, axis: Vec3, major: f64, p: Point3) -> Option<Point3> {
        let a = axis.normalized()?;
        let d = p - center;
        Some(center + (d - a * d.dot(a)).normalized()? * major)
    }

    /// Finite data, unit axes, positive radii.
    pub fn check(&self) -> Result<()> {
        let unit = |v: Vec3| v.is_finite() && (v.norm() - 1.0).abs() <= 1e-9;
        let ok = match *self {
            FaceSurface::Plane { normal, point } => unit(normal) && point.is_finite(),
            FaceSurface::Cylinder { origin, axis, radius } => unit(axis) && origin.is_finite() && radius.is_finite() && radius > 0.0,
            FaceSurface::Sphere { center, radius } => center.is_finite() && radius.is_finite() && radius > 0.0,
            FaceSurface::Cone { apex, axis, half_angle } => {
                unit(axis) && apex.is_finite() && half_angle > 0.0 && half_angle < std::f64::consts::FRAC_PI_2
            }
            FaceSurface::Torus { center, axis, major, minor } => {
                unit(axis) && center.is_finite() && major.is_finite() && minor.is_finite() && major > 0.0 && minor > 0.0
            }
        };
        if ok {
            Ok(())
        } else {
            Err(invalid(
                "surface",
                format!("a face's {} is not finite, its axis not a unit vector, or its radius or angle out of range", self.kind()),
            ))
        }
    }

    /// The same surface moved by `d`.
    pub fn moved(self, d: Vec3) -> FaceSurface {
        match self {
            FaceSurface::Plane { normal, point } => FaceSurface::Plane { normal, point: point + d },
            FaceSurface::Cylinder { origin, axis, radius } => FaceSurface::Cylinder { origin: origin + d, axis, radius },
            FaceSurface::Sphere { center, radius } => FaceSurface::Sphere { center: center + d, radius },
            FaceSurface::Cone { apex, axis, half_angle } => FaceSurface::Cone { apex: apex + d, axis, half_angle },
            FaceSurface::Torus { center, axis, major, minor } => FaceSurface::Torus { center: center + d, axis, major, minor },
        }
    }
}

/// Most triangles an imported surface may have.
pub const MAX_SURFACE_TRIANGLES: usize = 2_000_000;

/// The solid angle a triangle subtends at the origin, its corners given relative to it (A. Van
/// Oosterom, J. Strackee, IEEE Trans. Biomed. Eng. 30(2), 1983).
pub fn solid_angle([a, b, c]: [Vec3; 3]) -> f64 {
    let (la, lb, lc) = (a.norm(), b.norm(), c.norm());
    2.0 * geom::triple(a, b, c).atan2(la * lb * lc + a.dot(b) * lc + b.dot(c) * la + c.dot(a) * lb)
}

/// A surface's corner `v`, the origin for a missing one (the check refuses those).
fn corner(positions: &[Point3], v: u32) -> Point3 {
    positions.get(v as usize).copied().unwrap_or(Point3::ORIGIN)
}

impl Shape {
    /// The kind of primitive, for display.
    pub fn kind(&self) -> &'static str {
        match self {
            Shape::Box { .. } => "Box",
            Shape::Cylinder { .. } => "Cylinder",
            Shape::Sphere { .. } => "Sphere",
            Shape::Surface { .. } => "Surface",
        }
    }

    /// The bounding box.
    pub fn bounds(&self) -> Aabb {
        match *self {
            Shape::Box { size } => Aabb::new(Point3::ORIGIN, size.to_point()),
            Shape::Cylinder { radius: r, length } => Aabb::new(Point3::new(-r, -r, 0.0), Point3::new(r, r, length)),
            Shape::Sphere { radius: r } => Aabb::new(Point3::new(-r, -r, -r), Point3::new(r, r, r)),
            Shape::Surface { ref positions, .. } => Aabb::around(positions.iter().copied()).unwrap_or_default(),
        }
    }

    /// Whether a point in the shape's own frame lies inside it (an imported surface by its winding
    /// number).
    pub fn contains(&self, p: Point3) -> bool {
        match *self {
            Shape::Box { size } => (0..3).all(|d| p[d] > 0.0 && p[d] < size[d]),
            Shape::Cylinder { radius, length } => p.z > 0.0 && p.z < length && p.x * p.x + p.y * p.y < radius * radius,
            Shape::Sphere { radius } => p.to_vec().norm_sqr() < radius * radius,
            Shape::Surface { ref positions, ref triangles, .. } => {
                let total: f64 = triangles.iter().map(|t| solid_angle(t.map(|v| corner(positions, v) - p))).sum();
                total / (4.0 * PI) > 0.5
            }
        }
    }

    /// An imported surface's triangles on its lowest and highest z planes, the faces a port can
    /// close (empty for the primitives, whose ends the mesher knows).
    pub fn end_triangles(&self) -> [Vec<usize>; 2] {
        let Shape::Surface { positions, triangles, .. } = self else { return [Vec::new(), Vec::new()] };
        let Aabb { min: lo, max: hi } = self.bounds();
        let tol = 1e-9 * (0..3).map(|d| hi[d] - lo[d]).fold(0.0, f64::max);
        let on = |t: &[u32; 3], z: f64| t.iter().all(|v| positions.get(*v as usize).is_some_and(|p| (p[2] - z).abs() <= tol));
        [lo[2], hi[2]].map(|z| triangles.iter().enumerate().filter(|(_, t)| on(t, z)).map(|(i, _)| i).collect())
    }

    pub fn volume(&self) -> f64 {
        match *self {
            Shape::Box { size } => size.x * size.y * size.z,
            Shape::Cylinder { radius, length } => PI * radius * radius * length,
            Shape::Sphere { radius } => 4.0 / 3.0 * PI * radius.powi(3),
            Shape::Surface { ref positions, ref triangles, .. } => triangles
                .iter()
                .map(|t| {
                    let [a, b, c] = t.map(|v| corner(positions, v).to_vec());
                    geom::triple(a, b, c) / 6.0
                })
                .sum(),
        }
    }

    /// Checks the dimensions are finite and positive; an imported surface must be closed,
    /// consistently oriented and enclose a volume.
    pub fn check(&self) -> Result<()> {
        match self {
            Shape::Box { size } => {
                for (v, axis) in size.iter().zip(["x", "y", "z"]) {
                    positive(&format!("box size {axis}"), v)?;
                }
            }
            Shape::Cylinder { radius, length } => {
                positive("cylinder radius", *radius)?;
                positive("cylinder length", *length)?;
            }
            Shape::Sphere { radius } => {
                positive("sphere radius", *radius)?;
            }
            Shape::Surface { positions, triangles, tags, surfaces, .. } => {
                if surfaces.len() > triangles.len() {
                    return Err(invalid("surface", "has more face surfaces than triangles"));
                }
                for f in surfaces.iter().flatten() {
                    f.check()?;
                }
                if triangles.len() < 4 || triangles.len() > MAX_SURFACE_TRIANGLES {
                    return Err(invalid("surface", format!("has {} triangles; it needs 4 to {MAX_SURFACE_TRIANGLES}", triangles.len())));
                }
                if tags.len() != triangles.len() {
                    return Err(invalid("surface", "needs a face tag per triangle"));
                }
                if !positions.iter().all(|c| c.is_finite()) {
                    return Err(invalid("surface", "has a coordinate that is not a finite number"));
                }
                // Closed and oriented: every edge held once each way. Ordered, so the edge
                // reported is the same each time.
                let mut half = std::collections::BTreeMap::new();
                for t in triangles {
                    if t.iter().any(|v| *v as usize >= positions.len()) || t[0] == t[1] || t[1] == t[2] || t[0] == t[2] {
                        return Err(invalid("surface", "has a triangle with a missing or repeated corner"));
                    }
                    for edge in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                        *half.entry(edge).or_insert(0u32) += 1;
                    }
                }
                if let Some(((a, b), _)) = half.iter().find(|((a, b), n)| **n != 1 || half.get(&(*b, *a)) != Some(&1)) {
                    return Err(invalid(
                        "surface",
                        format!(
                            "is not closed and consistently oriented (its edge between vertices {a} and {b}); a solid needs a watertight surface"
                        ),
                    ));
                }
                if !(self.volume() > 0.0) {
                    return Err(invalid("surface", "encloses no volume (its triangles may face inwards)"));
                }
            }
        }
        Ok(())
    }

    /// The cross-section of the shape's flat face at `end` (z = 0 or its top), the face a port
    /// closes: a box's rectangle, a cylinder's disc, or an imported surface's end plane when it
    /// is a whole rectangle or a disc.
    pub fn end_face(&self, end: End) -> Result<PortShape> {
        match self {
            Shape::Box { size } => Ok(PortShape::Rect { x0: 0.0, y0: 0.0, a: size.x, b: size.y }),
            Shape::Cylinder { radius, .. } => Ok(PortShape::Circle { cx: 0.0, cy: 0.0, r: *radius }),
            Shape::Sphere { .. } => Err(invalid("port", "a sphere has no flat end faces for ports")),
            Shape::Surface { positions, triangles, .. } => {
                let [start, stop] = self.end_triangles();
                let tris = match end {
                    End::Start => start,
                    End::End => stop,
                };
                if tris.is_empty() {
                    return Err(invalid("port", "the imported surface has no flat face on its end plane (its lowest or highest z)"));
                }
                // The end plane's triangles: a full rectangle, or a disc (a polygon round it).
                let mut lo = [f64::INFINITY; 2];
                let mut hi = [f64::NEG_INFINITY; 2];
                let mut area = 0.0;
                for t in tris.iter().filter_map(|&i| triangles.get(i)) {
                    let [a, b, c] = t.map(|v| corner(positions, v));
                    area += 0.5 * ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs();
                    for p in [a, b, c] {
                        for d in 0..2 {
                            lo[d] = lo[d].min(p[d]);
                            hi[d] = hi[d].max(p[d]);
                        }
                    }
                }
                let (w, h) = (hi[0] - lo[0], hi[1] - lo[1]);
                if (area - w * h).abs() <= 1e-6 * w * h {
                    Ok(PortShape::Rect { x0: lo[0], y0: lo[1], a: w, b: h })
                } else if (w - h).abs() <= 1e-3 * w && (area / (PI * w * h / 4.0) - 1.0).abs() < 0.05 {
                    Ok(PortShape::Circle { cx: 0.5 * (lo[0] + hi[0]), cy: 0.5 * (lo[1] + hi[1]), r: 0.25 * (w + h) })
                } else {
                    Err(invalid("port", "the end face of the imported surface is neither a whole rectangle nor a disc"))
                }
            }
        }
    }
}

/// A body of one material bounded by conducting walls (a cavity, or a guide whose end faces are
/// ports).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Solid {
    pub shape: Shape,
    pub material: String,
}

impl Solid {
    /// The shape's dimensions are finite and positive ([`Shape::check`]).
    pub fn check_shape(&self) -> Result<()> {
        self.shape.check()
    }
}

/// A body inside the solid: a dielectric region (`material`) or a conductor (`None`), placed
/// with its own origin at `at`. It must lie strictly inside the solid and apart from the others.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Inclusion {
    pub shape: Shape,
    pub at: Point3,
    /// The filling; `None` for a perfect conductor (a hole in the mesh).
    pub material: Option<String>,
}

impl Inclusion {
    /// Whether a point lies inside the body.
    pub fn contains(&self, p: Point3) -> bool {
        self.shape.contains((p - self.at).to_point())
    }

    /// The bounding box.
    pub fn bounds(&self) -> Aabb {
        let b = self.shape.bounds();
        Aabb::new(b.min + self.at.to_vec(), b.max + self.at.to_vec())
    }
}

/// A coordinate axis.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Axis {
    X,
    Y,
    #[default]
    Z,
}

impl Axis {
    /// The axis' component index: 0, 1 or 2.
    pub fn index(self) -> usize {
        match self {
            Axis::X => 0,
            Axis::Y => 1,
            Axis::Z => 2,
        }
    }

    /// A shape's own coordinates turned so its z axis lies along this one (a cyclic turn of
    /// the axes, keeping handedness): x → (z, x, y), y → (y, z, x).
    pub fn turn(self, p: Vec3) -> Vec3 {
        match self {
            Axis::X => Vec3::new(p.z, p.x, p.y),
            Axis::Y => Vec3::new(p.y, p.z, p.x),
            Axis::Z => p,
        }
    }

    /// The inverse of [`Axis::turn`]: global coordinates into the shape's own.
    pub fn unturn(self, p: Vec3) -> Vec3 {
        match self {
            Axis::X => Vec3::new(p.y, p.z, p.x),
            Axis::Y => Vec3::new(p.z, p.x, p.y),
            Axis::Z => p,
        }
    }
}

/// How a feature combines with the solid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Combine {
    /// The union: the body grows (a stub, a probe's housing).
    Add,
    /// The difference: the body loses the feature's volume, which becomes conductor (a post, a
    /// screw, a notch).
    Subtract,
    /// The intersection: only what both hold remains.
    Intersect,
}

impl Combine {
    /// The operation's name, for display.
    pub fn kind(self) -> &'static str {
        match self {
            Combine::Add => "add",
            Combine::Subtract => "subtract",
            Combine::Intersect => "intersect",
        }
    }

    /// Whether a point lies in the combination, given whether it lies in the body so far and
    /// in the feature.
    pub fn apply(self, body: bool, feature: bool) -> bool {
        match self {
            Combine::Add => body || feature,
            Combine::Subtract => body && !feature,
            Combine::Intersect => body && feature,
        }
    }
}

/// A primitive combined with the solid's shape: its own origin at `at`, its z axis turned onto
/// `axis`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Feature {
    pub combine: Combine,
    pub shape: Shape,
    pub at: Point3,
    #[serde(default)]
    pub axis: Axis,
}

impl Feature {
    /// Whether a point lies inside the feature's body.
    pub fn contains(&self, p: Point3) -> bool {
        self.shape.contains(self.axis.unturn(p - self.at).to_point())
    }

    /// The bounding box.
    pub fn bounds(&self) -> Aabb {
        let own = self.shape.bounds();
        let (a, b) = (self.axis.turn(own.min.to_vec()), self.axis.turn(own.max.to_vec()));
        Aabb::new(self.at + a.min(b), self.at + a.max(b))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tetrahedron's surface: (0,0,0), (1,0,0), (0,1,0), (0,0,1), triangles facing out.
    fn tetrahedron(triangles: Vec<[u32; 3]>) -> Shape {
        let positions = vec![Point3::ORIGIN, Point3::new(1.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0), Point3::new(0.0, 0.0, 1.0)];
        let tags = vec![0; triangles.len()];
        Shape::Surface { name: "tet".into(), positions, triangles, tags, surfaces: Vec::new() }
    }

    #[test]
    fn shapes_are_checked() {
        assert!((Shape::Sphere { radius: 1.0 }.volume() - 4.18879).abs() < 1e-5);
        assert!(Shape::Box { size: Vec3::new(1.0, 0.0, 1.0) }.check().is_err());
        assert!(Shape::Cylinder { radius: 1.0, length: f64::NAN }.check().is_err());
        let closed = tetrahedron(vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]]);
        closed.check().unwrap();
        assert!((closed.volume() - 1.0 / 6.0).abs() < 1e-15);
        assert!(closed.contains(Point3::new(0.1, 0.1, 0.1)) && !closed.contains(Point3::new(1.0, 1.0, 1.0)));
        assert_eq!(closed.end_triangles(), [vec![0], Vec::new()]);
        assert!(closed.end_face(End::Start).is_err(), "a triangle is neither a rectangle nor a disc");
        let inwards = tetrahedron(vec![[0, 1, 2], [0, 3, 1], [0, 2, 3], [1, 3, 2]]);
        assert!(inwards.check().is_err());
        let open = tetrahedron(vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [0, 3, 2]]);
        let Err(e) = open.check() else { panic!("an open surface passed") };
        assert!(e.to_string().contains("between vertices 0 and 2"), "{e}");
    }

    #[test]
    fn features_turn_onto_their_axis() {
        // A box turned onto y: its own (x, y, z) lie along global (y, z, x).
        let f = Feature { combine: Combine::Add, shape: Shape::Box { size: Vec3::new(1.0, 2.0, 3.0) }, at: Point3::ORIGIN, axis: Axis::Y };
        let b = f.bounds();
        assert_eq!(b.max, Point3::new(2.0, 3.0, 1.0));
        assert!(f.contains(b.center()) && !f.contains(b.center().with(0, b.max.x + 0.1)));
        for axis in [Axis::X, Axis::Y, Axis::Z] {
            let v = Vec3::new(1.0, 2.0, 3.0);
            assert_eq!(axis.unturn(axis.turn(v)), v);
        }
    }
}
