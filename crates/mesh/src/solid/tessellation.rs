//! Bodies as tagged triangles: the primitive shapes tessellated with an exact surface per face,
//! moved into place, combined with the solid's features (a mesh Boolean, [`crate::csg`]), and
//! the interfaces regions share taken from and handed between their meshes.

use std::f64::consts::PI;

use emcraft_model::{Aabb, Axis, Combine, FaceSurface, Feature, Point3, Shape, Vec3};
use emcraft_tetmesh::tet::geometry::AnalyticSurface;

use super::mesher::{MPoint, MVec, RegionFace, RegionMesh, mesher_point, mesher_vec, model_point, model_vec};
use crate::{MeshError, Result};

/// A tessellated body: triangles tagged by face, each face's exact surface, and which faces are
/// the ends a port may close (z = 0, z = length).
#[derive(Clone, Debug, Default)]
pub(super) struct Tessellation {
    pub(super) positions: Vec<Point3>,
    pub(super) triangles: Vec<[u32; 3]>,
    pub(super) tri_face: Vec<u32>,
    pub(super) analytic: Vec<Option<AnalyticSurface>>,
    pub(super) start_face: Option<u32>,
    pub(super) end_face: Option<u32>,
}

impl Tessellation {
    fn point(&mut self, p: Point3) -> u32 {
        self.positions.push(p);
        (self.positions.len() - 1) as u32
    }
}

fn divisions(len: f64, size: f64) -> usize {
    ((len / size).ceil() as usize).clamp(1, 2000)
}

/// A box [0, x] × [0, y] × [0, z]: faces x0, x1, y0, y1, z0 (0..6 in that order), each a grid
/// of about `size`, triangles oriented outwards.
fn tessellate_box(size_xyz: Vec3, h: f64) -> Tessellation {
    let mut t = Tessellation { start_face: Some(4), end_face: Some(5), ..Tessellation::default() };
    let n = [0, 1, 2].map(|d| divisions(size_xyz[d], h));
    // Shared vertices on the box's surface: a map from integer lattice coordinates.
    let mut index = std::collections::HashMap::new();
    let mut vertex = |t: &mut Tessellation, i: usize, j: usize, k: usize| -> u32 {
        *index.entry((i, j, k)).or_insert_with(|| {
            t.point(Point3::new(size_xyz.x * i as f64 / n[0] as f64, size_xyz.y * j as f64 / n[1] as f64, size_xyz.z * k as f64 / n[2] as f64))
        })
    };
    // (fixed axis, at 0 or 1, outward normal sign)
    for (face, (axis, high)) in [(0usize, false), (0, true), (1, false), (1, true), (2, false), (2, true)].into_iter().enumerate() {
        let (u, v) = match axis {
            0 => (1, 2),
            1 => (2, 0),
            _ => (0, 1),
        };
        let normal = MVec::ZERO.with(axis, if high { 1.0 } else { -1.0 });
        let point = MPoint::ORIGIN.with(axis, if high { size_xyz[axis] } else { 0.0 });
        t.analytic.push(Some(AnalyticSurface::Plane { normal, point }));
        for a in 0..n[u] {
            for b in 0..n[v] {
                let corner = |da: usize, db: usize| {
                    let mut ijk = [0usize; 3];
                    ijk[axis] = if high { n[axis] } else { 0 };
                    ijk[u] = a + da;
                    ijk[v] = b + db;
                    ijk
                };
                let q = [corner(0, 0), corner(1, 0), corner(1, 1), corner(0, 1)].map(|c| vertex(&mut t, c[0], c[1], c[2]));
                // (u, v, axis) is right-handed: counter-clockwise in (u, v) faces +axis.
                let tris = if high { [[q[0], q[1], q[2]], [q[0], q[2], q[3]]] } else { [[q[0], q[2], q[1]], [q[0], q[3], q[2]]] };
                for tri in tris {
                    t.triangles.push(tri);
                    t.tri_face.push(face as u32);
                }
            }
        }
    }
    t
}

/// The sides of a cylinder's tessellated polygon.
fn cylinder_sides(r: f64, h: f64) -> usize {
    divisions(2.0 * PI * r, h).max(12)
}

/// A cylinder about z, radius r, from z = 0 to `len`: faces side (0), bottom (1), top (2).
fn tessellate_cylinder(r: f64, len: f64, h: f64) -> Tessellation {
    let mut t = Tessellation { start_face: Some(1), end_face: Some(2), ..Tessellation::default() };
    t.analytic.push(Some(AnalyticSurface::Cylinder { origin: MPoint::ORIGIN, axis: MVec::new(0.0, 0.0, 1.0), radius: r }));
    t.analytic.push(Some(AnalyticSurface::Plane { normal: MVec::new(0.0, 0.0, -1.0), point: MPoint::ORIGIN }));
    t.analytic.push(Some(AnalyticSurface::Plane { normal: MVec::new(0.0, 0.0, 1.0), point: MPoint::new(0.0, 0.0, len) }));
    let nt = cylinder_sides(r, h);
    let nz = divisions(len, h);
    let nr = divisions(r, h);
    let ring = |t: &mut Tessellation, z: f64, rr: f64, count: usize| -> Vec<u32> {
        (0..count)
            .map(|k| {
                let a = 2.0 * PI * k as f64 / count as f64;
                t.point(Point3::new(rr * a.cos(), rr * a.sin(), z))
            })
            .collect()
    };
    let rings: Vec<Vec<u32>> = (0..=nz).map(|k| ring(&mut t, len * k as f64 / nz as f64, r, nt)).collect();
    for k in 0..nz {
        for j in 0..nt {
            let (a, b) = (rings[k][j], rings[k][(j + 1) % nt]);
            let (c, d) = (rings[k + 1][(j + 1) % nt], rings[k + 1][j]);
            for tri in [[a, b, c], [a, c, d]] {
                t.triangles.push(tri);
                t.tri_face.push(0);
            }
        }
    }
    // Caps: concentric rings of the same count, a fan in the middle.
    for (face, z, outer) in [(1u32, 0.0, &rings[0]), (2u32, len, &rings[nz])] {
        let mut prev = outer.clone();
        for k in (1..nr).rev() {
            let inner = ring(&mut t, z, r * k as f64 / nr as f64, nt);
            for j in 0..nt {
                let (a, b) = (prev[j], prev[(j + 1) % nt]);
                let (c, d) = (inner[(j + 1) % nt], inner[j]);
                let tris = if face == 2 { [[a, b, c], [a, c, d]] } else { [[a, c, b], [a, d, c]] };
                for tri in tris {
                    t.triangles.push(tri);
                    t.tri_face.push(face);
                }
            }
            prev = inner;
        }
        let centre = t.point(Point3::new(0.0, 0.0, z));
        for j in 0..nt {
            let (a, b) = (prev[j], prev[(j + 1) % nt]);
            t.triangles.push(if face == 2 { [a, b, centre] } else { [a, centre, b] });
            t.tri_face.push(face);
        }
    }
    t
}

/// A sphere about the origin: an icosahedron subdivided until its edges are about `size`, its
/// vertices on the sphere; one face (0).
fn tessellate_sphere(r: f64, h: f64) -> Tessellation {
    let mut t = Tessellation::default();
    t.analytic.push(Some(AnalyticSurface::Sphere { center: MPoint::ORIGIN, radius: r }));
    let g = (1.0 + 5f64.sqrt()) / 2.0;
    let raw = [
        [-1.0, g, 0.0],
        [1.0, g, 0.0],
        [-1.0, -g, 0.0],
        [1.0, -g, 0.0],
        [0.0, -1.0, g],
        [0.0, 1.0, g],
        [0.0, -1.0, -g],
        [0.0, 1.0, -g],
        [g, 0.0, -1.0],
        [g, 0.0, 1.0],
        [-g, 0.0, -1.0],
        [-g, 0.0, 1.0],
    ];
    // Onto the sphere along the ray from its centre.
    let on = |p: Vec3| Point3::ORIGIN + p / p.norm() * r;
    let mut pts: Vec<Point3> = raw.iter().map(|p| on(Vec3::from(*p))).collect();
    let mut tris: Vec<[u32; 3]> = vec![
        [0, 11, 5],
        [0, 5, 1],
        [0, 1, 7],
        [0, 7, 10],
        [0, 10, 11],
        [1, 5, 9],
        [5, 11, 4],
        [11, 10, 2],
        [10, 7, 6],
        [7, 1, 8],
        [3, 9, 4],
        [3, 4, 2],
        [3, 2, 6],
        [3, 6, 8],
        [3, 8, 9],
        [4, 9, 5],
        [2, 4, 11],
        [6, 2, 10],
        [8, 6, 7],
        [9, 8, 1],
    ];
    // Edge length of the icosahedron inscribed in radius r: r · 1.0515.
    let mut edge = r * 1.0515;
    while edge > h && tris.len() < 200_000 {
        let mut mid = std::collections::HashMap::new();
        let mut midpoint = |a: u32, b: u32, pts: &mut Vec<Point3>| -> u32 {
            let key = (a.min(b), a.max(b));
            *mid.entry(key).or_insert_with(|| {
                let (p, q) = (pts[a as usize], pts[b as usize]);
                pts.push(on(p.midpoint(q).to_vec()));
                (pts.len() - 1) as u32
            })
        };
        let mut next = Vec::with_capacity(tris.len() * 4);
        for [a, b, c] in tris {
            let (ab, bc, ca) = (midpoint(a, b, &mut pts), midpoint(b, c, &mut pts), midpoint(c, a, &mut pts));
            next.extend([[a, ab, ca], [b, bc, ab], [c, ca, bc], [ab, bc, ca]]);
        }
        tris = next;
        edge /= 2.0;
    }
    t.positions = pts;
    t.tri_face = vec![0; tris.len()];
    t.triangles = tris;
    t
}

pub(super) fn tessellate(shape: &Shape, size: f64) -> Tessellation {
    match *shape {
        Shape::Box { size: xyz } => tessellate_box(xyz, size),
        Shape::Cylinder { radius, length } => tessellate_cylinder(radius, length, size),
        Shape::Sphere { radius } => tessellate_sphere(radius, size),
        Shape::Surface { .. } => tessellate_surface(shape),
    }
}

/// An imported surface as it is (the mesher refines it to the size): its faces as tagged, each
/// curved face with the exact surface fitted to it on import (so points the mesher adds on it go
/// onto it, as on a native cylinder; flat faces are exact as they are), the triangles on its
/// lowest and highest z planes made the end faces a port may close, with their planes as exact
/// surfaces.
fn tessellate_surface(shape: &Shape) -> Tessellation {
    let Shape::Surface { positions, triangles, tags, surfaces, .. } = shape else { return Tessellation::default() };
    let mut tri_face = tags.clone();
    let faces = tags.iter().map(|t| t + 1).max().unwrap_or(0);
    let mut analytic: Vec<Option<AnalyticSurface>> =
        (0..faces as usize).map(|f| surfaces.get(f).copied().flatten().filter(FaceSurface::is_curved).map(exact)).collect();
    let Aabb { min: lo, max: hi } = shape.bounds();
    let ends = shape.end_triangles();
    let mut tag_of = [None, None];
    for (k, (list, z, n)) in [(&ends[0], lo.z, -1.0), (&ends[1], hi.z, 1.0)].into_iter().enumerate() {
        if list.is_empty() {
            continue;
        }
        let tag = analytic.len() as u32;
        analytic.push(Some(AnalyticSurface::Plane { normal: MVec::new(0.0, 0.0, n), point: MPoint::new(0.0, 0.0, z) }));
        for &i in list {
            tri_face[i] = tag;
        }
        tag_of[k] = Some(tag);
    }
    Tessellation { positions: positions.clone(), triangles: triangles.clone(), tri_face, analytic, start_face: tag_of[0], end_face: tag_of[1] }
}

/// A fitted face's surface as the mesher's.
fn exact(f: FaceSurface) -> AnalyticSurface {
    let (p, v) = (mesher_point, mesher_vec);
    match f {
        FaceSurface::Plane { normal, point } => AnalyticSurface::Plane { normal: v(normal), point: p(point) },
        FaceSurface::Cylinder { origin, axis, radius } => AnalyticSurface::Cylinder { origin: p(origin), axis: v(axis), radius },
        FaceSurface::Sphere { center, radius } => AnalyticSurface::Sphere { center: p(center), radius },
        FaceSurface::Cone { apex, axis, half_angle } => AnalyticSurface::Cone { apex: p(apex), axis: v(axis), half_angle },
    }
}

/// The tessellation size of an inclusion: its interface is kept as tessellated (frozen), so a
/// curved one is divided finely enough for its curvature — about π/16 of its radius for a
/// dielectric's interface, π/6 for a conductor's surface (no region inside to match, and its
/// edges are bent onto it with curved elements; a thin wire would otherwise fill with tiny
/// elements along its whole length).
pub(super) fn interface_size(shape: &Shape, size: f64, conductor: bool) -> f64 {
    let per = if conductor { 6.0 } else { 16.0 };
    match *shape {
        Shape::Box { .. } | Shape::Surface { .. } => size,
        Shape::Cylinder { radius, .. } | Shape::Sphere { radius } => size.min(radius * PI / per),
    }
}

/// A tessellation turned by `axis` and moved to `at`, its exact surfaces with it.
fn transformed(t: Tessellation, axis: Axis, at: Point3) -> Tessellation {
    let place = |p: Point3| at + axis.turn(p.to_vec());
    // The exact surfaces' points and directions are the mesher's.
    let place_a = |p: MPoint| mesher_point(place(model_point(p)));
    let turn_a = |v: MVec| mesher_vec(axis.turn(model_vec(v)));
    let analytic = t
        .analytic
        .iter()
        .map(|a| {
            a.map(|a| match a {
                AnalyticSurface::Plane { normal, point } => AnalyticSurface::Plane { normal: turn_a(normal), point: place_a(point) },
                AnalyticSurface::Cylinder { origin, axis: v, radius } => {
                    AnalyticSurface::Cylinder { origin: place_a(origin), axis: turn_a(v), radius }
                }
                AnalyticSurface::Sphere { center, radius } => AnalyticSurface::Sphere { center: place_a(center), radius },
                AnalyticSurface::Cone { apex, axis: v, half_angle } => AnalyticSurface::Cone { apex: place_a(apex), axis: turn_a(v), half_angle },
                AnalyticSurface::Torus { center, axis: v, major, minor } => {
                    AnalyticSurface::Torus { center: place_a(center), axis: turn_a(v), major, minor }
                }
            })
        })
        .collect();
    Tessellation { positions: t.positions.into_iter().map(place).collect(), analytic, ..t }
}

/// A flat face across an axis: the plane `axis` = `at`, its outward normal along +axis
/// (`outward` 1) or −axis (−1).
#[derive(Clone, Copy, Debug)]
struct FlatFace {
    axis: usize,
    at: f64,
    outward: f64,
}

impl FlatFace {
    /// The low and the high face across `axis` at `lo` and `hi`.
    fn pair(axis: usize, lo: f64, hi: f64) -> [FlatFace; 2] {
        [FlatFace { axis, at: lo, outward: -1.0 }, FlatFace { axis, at: hi, outward: 1.0 }]
    }
}

/// The flat faces of a body in place.
fn flat_faces(shape: &Shape, at: Point3, axis: Axis) -> Vec<FlatFace> {
    match *shape {
        Shape::Box { size } => {
            let s = axis.turn(size);
            (0..3).flat_map(|d| FlatFace::pair(d, at[d], at[d] + s[d])).collect()
        }
        Shape::Cylinder { length, .. } => {
            let d = axis.index();
            FlatFace::pair(d, at[d], at[d] + length).to_vec()
        }
        Shape::Sphere { .. } | Shape::Surface { .. } => Vec::new(),
    }
}

/// Whether the feature's cross-section across `d` (its flat face there) lies within the solid's
/// flat face across `d` (the solid at the origin along z, as tessellated: a cylinder's cap is its
/// inscribed polygon).
fn face_within_solid(f: &Feature, d: usize, solid: &Shape, size: f64, tol: f64) -> bool {
    let o = [(d + 1) % 3, (d + 2) % 3];
    // The feature's section: a rectangle [lo, hi], or a disk (centre, radius) for a cylinder's cap.
    let section = match f.shape {
        Shape::Box { size: s } => {
            let s = f.axis.turn(s);
            Some((o.map(|k| f.at[k]), o.map(|k| f.at[k] + s[k]), None))
        }
        Shape::Cylinder { radius, .. } if f.axis.index() == d => {
            Some((o.map(|k| f.at[k] - radius), o.map(|k| f.at[k] + radius), Some((o.map(|k| f.at[k]), radius))))
        }
        _ => None,
    };
    let Some((lo, hi, disk)) = section else { return false };
    match *solid {
        Shape::Box { size: s } => (0..2).all(|k| lo[k] >= -tol && hi[k] <= s[o[k]] + tol),
        Shape::Cylinder { radius, .. } if d == 2 => {
            let inner = radius * (PI / cylinder_sides(radius, size) as f64).cos();
            match disk {
                Some((c, r)) => c[0].hypot(c[1]) + r < inner - tol,
                None => [lo[0], hi[0]].iter().all(|x| [lo[1], hi[1]].iter().all(|y| x.hypot(*y) < inner - tol)),
            }
        }
        _ => false,
    }
}

/// The feature's body with each flat face that lies in one of the solid's moved off it by a
/// step, so the two surfaces meet in general position with the same result: a face flush with
/// the solid's (both bodies behind it) moves outwards for a difference or an intersection (what
/// sticks out is not kept); a face touching the solid's from outside moves into it for a union
/// and away for a difference; a union's face flush with the solid's from inside moves back into
/// the feature when the solid's face holds it (refused otherwise: the union's face there would
/// move).
fn feature_body(f: &Feature, solid: &Shape, size: f64) -> Result<(Shape, Point3)> {
    let step = 0.1 * size;
    let base = flat_faces(solid, Point3::ORIGIN, Axis::Z);
    let scale = solid.bounds().max.iter().fold(0.0f64, |m, v| m.max(v.abs())).max(size);
    let tol = 1e-9 * scale;
    // Box: the lo/hi planes along each axis in place; a cylinder: its caps along its axis.
    let (shape, at) = (f.shape.clone(), f.at);
    let mut lo_hi = flat_faces(&shape, at, f.axis);
    for face in lo_hi.iter_mut() {
        for b in &base {
            if b.axis != face.axis || (face.at - b.at).abs() > tol {
                continue;
            }
            // `out`: along the feature face's outward normal; for a face touching the solid's
            // from outside, that is into the solid.
            let same = b.outward == face.outward;
            let out = face.outward * step;
            face.at += match (f.combine, same) {
                (Combine::Subtract | Combine::Intersect, true) => out,
                (Combine::Subtract, false) => -out,
                (Combine::Add, false) => out,
                // A union flush with the solid's face from inside: the solid's face bounds the
                // union there, so the feature's face draws back into it — when the solid's face
                // holds all of it; elsewhere the union's face would move.
                (Combine::Add, true) if face_within_solid(f, face.axis, solid, size, tol) => -out,
                (Combine::Add, true) => {
                    return Err(MeshError::Invalid {
                        what: "feature".into(),
                        why: "not supported yet: a union whose face is flush with the solid's and reaches past the solid's face (let it stand out of the face, or reach into the body)"
                            .into(),
                    });
                }
                (Combine::Intersect, false) => {
                    return Err(MeshError::Invalid { what: "feature".into(), why: "its intersection with the solid is only a face".into() });
                }
            };
        }
    }
    let span = |d: usize| {
        let lo = lo_hi.iter().find(|x| x.axis == d && x.outward < 0.0).map(|x| x.at);
        let hi = lo_hi.iter().find(|x| x.axis == d && x.outward > 0.0).map(|x| x.at);
        lo.zip(hi)
    };
    Ok(match shape {
        Shape::Box { .. } => {
            let (x, y, z) = (span(0).unwrap_or((0.0, 0.0)), span(1).unwrap_or((0.0, 0.0)), span(2).unwrap_or((0.0, 0.0)));
            // Back in the feature's own frame: the box is axis-aligned whatever the turn.
            let lo = Point3::new(x.0, y.0, z.0);
            let size = Vec3::new(x.1 - x.0, y.1 - y.0, z.1 - z.0);
            (Shape::Box { size: f.axis.unturn(size) }, lo)
        }
        Shape::Cylinder { radius, .. } => {
            let d = f.axis.index();
            let (a, b) = span(d).unwrap_or((at[d], at[d]));
            (Shape::Cylinder { radius, length: b - a }, at.with(d, a))
        }
        other => (other, at),
    })
}

/// A feature's cross-section across axis `d`, in the other two coordinates ((d + 1) % 3,
/// (d + 2) % 3): a cylinder along `d` is a disk (its tessellated polygon lies between the circles
/// of radius `inner` and `r`), a box a rectangle.
#[derive(Clone, Copy, Debug)]
enum Section {
    Disk { c: [f64; 2], r: f64, inner: f64 },
    Rect { lo: [f64; 2], hi: [f64; 2] },
}

impl Section {
    fn of(f: &Feature, d: usize, size: f64) -> Option<Section> {
        let o = [(d + 1) % 3, (d + 2) % 3];
        match f.shape {
            Shape::Box { .. } => {
                let b = f.bounds();
                Some(Section::Rect { lo: o.map(|k| b.min[k]), hi: o.map(|k| b.max[k]) })
            }
            Shape::Cylinder { radius, .. } if f.axis.index() == d => {
                Some(Section::Disk { c: o.map(|k| f.at[k]), r: radius, inner: radius * (PI / cylinder_sides(radius, size) as f64).cos() })
            }
            _ => None,
        }
    }

    /// Whether this section's tessellation lies strictly inside `outer`'s, more than `gap` from
    /// its edge.
    fn within(self, outer: Section, gap: f64) -> bool {
        match (self, outer) {
            (Section::Disk { c, r, .. }, Section::Disk { c: o, inner, .. }) => (c[0] - o[0]).hypot(c[1] - o[1]) + r + gap < inner,
            (Section::Disk { c, r, .. }, Section::Rect { lo, hi }) => (0..2).all(|k| c[k] - r - gap > lo[k] && c[k] + r + gap < hi[k]),
            (Section::Rect { lo, hi }, Section::Disk { c, inner, .. }) => {
                [lo[0], hi[0]].iter().all(|x| [lo[1], hi[1]].iter().all(|y| (x - c[0]).hypot(y - c[1]) + gap < inner))
            }
            (Section::Rect { lo, hi }, Section::Rect { lo: a, hi: b }) => (0..2).all(|k| lo[k] - gap > a[k] && hi[k] + gap < b[k]),
        }
    }

    /// Whether the two sections are the same shape and place within `tol`.
    fn same(self, other: Section, tol: f64) -> bool {
        let near = |a: f64, b: f64| (a - b).abs() <= tol;
        match (self, other) {
            (Section::Disk { c, r, .. }, Section::Disk { c: o, r: s, .. }) => near(c[0], o[0]) && near(c[1], o[1]) && near(r, s),
            (Section::Rect { lo, hi }, Section::Rect { lo: a, hi: b }) => (0..2).all(|k| near(lo[k], a[k]) && near(hi[k], b[k])),
            _ => false,
        }
    }

    /// Whether the two sections share an area.
    fn overlap(self, other: Section) -> bool {
        match (self, other) {
            (Section::Disk { c, r, .. }, Section::Disk { c: o, r: s, .. }) => (c[0] - o[0]).hypot(c[1] - o[1]) < r + s,
            (Section::Rect { lo, hi }, Section::Rect { lo: a, hi: b }) => (0..2).all(|k| lo[k] < b[k] && a[k] < hi[k]),
            (Section::Disk { c, r, .. }, Section::Rect { lo, hi }) | (Section::Rect { lo, hi }, Section::Disk { c, r, .. }) => {
                (c[0] - c[0].clamp(lo[0], hi[0])).hypot(c[1] - c[1].clamp(lo[1], hi[1])) < r
            }
        }
    }
}

/// Two features of one kind meeting end to end in a plane: the pair of faces (one of each) on
/// it, with opposite outward normals.
fn touching(a: &Feature, b: &Feature, tol: f64) -> Option<(FlatFace, FlatFace)> {
    let (fa, fb) = (flat_faces(&a.shape, a.at, a.axis), flat_faces(&b.shape, b.at, b.axis));
    fa.iter().find_map(|x| fb.iter().find(|y| x.axis == y.axis && x.outward != y.outward && (x.at - y.at).abs() <= tol).map(|y| (*x, *y)))
}

/// The two features as one when they meet end to end with the same cross-section (cylinders
/// of one radius on one axis, boxes of one rectangle).
fn merged(a: &Feature, b: &Feature) -> Option<Feature> {
    match (&a.shape, &b.shape) {
        (Shape::Cylinder { radius, length }, Shape::Cylinder { length: other, .. }) => {
            let d = a.axis.index();
            let lo = a.at[d].min(b.at[d]);
            let hi = (a.at[d] + length).max(b.at[d] + other);
            Some(Feature { shape: Shape::Cylinder { radius: *radius, length: hi - lo }, at: a.at.with(d, lo), ..a.clone() })
        }
        (Shape::Box { .. }, Shape::Box { .. }) => {
            let (p, q) = (a.bounds(), b.bounds());
            let (lo, hi) = (p.min.min(q.min), p.max.max(q.max));
            Some(Feature { shape: Shape::Box { size: hi - lo }, at: lo, axis: Axis::Z, ..a.clone() })
        }
        _ => None,
    }
}

/// `f` with its flat face `face` moved outwards by `by`.
fn extended(f: &Feature, face: FlatFace, by: f64) -> Feature {
    let d = face.axis;
    let low = face.outward < 0.0;
    let at = if low { f.at.with(d, f.at[d] - by) } else { f.at };
    let shape = match f.shape {
        Shape::Cylinder { radius, length } => Shape::Cylinder { radius, length: length + by },
        Shape::Box { size } => {
            let s = f.axis.turn(size);
            Shape::Box { size: f.axis.unturn(s.with(d, s[d] + by)) }
        }
        ref other => other.clone(),
    };
    Feature { shape, at, ..f.clone() }
}

/// The most features one solid takes.
pub(super) const MAX_FEATURES: usize = 1000;

/// The features with those that meet end to end in a plane made to meet in general position,
/// for the mesh Boolean: two features in a row of one kind (added or subtracted, and every
/// feature between them too, so their order does not matter) whose flat faces touch would
/// leave two boundaries a hair apart there (each feature is moved by its own millionth of the
/// size). With the same cross-section they become one feature; when one's cross-section lies
/// inside the other's, the smaller reaches into the larger by a tenth of the size (at most half
/// the larger's length) — the same union. Other overlapping cross-sections are refused. A
/// coaxial line whose inner conductor changes radius is two subtracted cylinders meeting so.
pub(super) fn joined(features: &[Feature], solid: &Shape, size: f64) -> Result<Vec<Feature>> {
    if features.len() > MAX_FEATURES {
        return Err(MeshError::Invalid { what: "features".into(), why: format!("{}; the limit is {MAX_FEATURES}", features.len()) });
    }
    let scale = solid.bounds().max.iter().fold(0.0f64, |m, v| m.max(v.abs())).max(size);
    let tol = 1e-9 * scale;
    // (feature number, feature).
    let mut out: Vec<(usize, Feature)> = features.iter().cloned().enumerate().collect();
    // Whether features i and j (and every one between them) are all added or all subtracted.
    let in_row = |out: &[(usize, Feature)], i: usize, j: usize| {
        let (a, b) = (i.min(j), i.max(j));
        let c = out.get(i).map(|(_, f)| f.combine);
        matches!(c, Some(Combine::Add | Combine::Subtract)) && out.get(a..=b).is_some_and(|r| r.iter().all(|(_, f)| Some(f.combine) == c))
    };
    // Merge: each round joins one pair, so at most one round per feature.
    for _ in 0..features.len() {
        let mut pair = None;
        'search: for i in 0..out.len() {
            for j in i + 1..out.len() {
                let (a, b) = (&out[i].1, &out[j].1);
                let Some((x, _)) = touching(a, b, tol) else { continue };
                let same = Section::of(a, x.axis, size).zip(Section::of(b, x.axis, size)).is_some_and(|(p, q)| p.same(q, tol));
                if same
                    && in_row(&out, i, j)
                    && let Some(m) = merged(a, b)
                {
                    pair = Some((i, j, m));
                    break 'search;
                }
            }
        }
        let Some((i, j, m)) = pair else { break };
        if let Some(slot) = out.get_mut(i) {
            slot.1 = m;
        }
        if j < out.len() {
            out.remove(j);
        }
    }
    // Reach into the larger: each face moves once.
    let mut moved: Vec<Feature> = out.iter().map(|(_, f)| f.clone()).collect();
    let mut done: Vec<(usize, usize, i64)> = Vec::new();
    for i in 0..out.len() {
        for j in 0..out.len() {
            if i == j || !in_row(&out, i, j) {
                continue;
            }
            let ((ni, a), (nj, b)) = (&out[i], &out[j]);
            let Some((x, y)) = touching(a, b, tol) else { continue };
            let (Some(p), Some(q)) = (Section::of(a, x.axis, size), Section::of(b, x.axis, size)) else { continue };
            if !p.overlap(q) {
                continue;
            }
            let key = (i, x.axis, x.outward as i64);
            if p.within(q, tol) {
                if !done.contains(&key) {
                    // The larger's length across the plane.
                    let ends: Vec<f64> = flat_faces(&b.shape, b.at, b.axis).iter().filter(|f| f.axis == y.axis).map(|f| f.at).collect();
                    let depth = ends.iter().fold(f64::NEG_INFINITY, |m, v| m.max(*v)) - ends.iter().fold(f64::INFINITY, |m, v| m.min(*v));
                    let by = (0.1 * size).min(0.5 * depth);
                    if let Some(f) = moved.get_mut(i) {
                        *f = extended(f, x, by);
                    }
                    done.push(key);
                }
            } else if !q.within(p, tol) && !p.same(q, tol) {
                return Err(MeshError::Invalid {
                    what: "features".into(),
                    why: format!(
                        "not supported yet: features {} and {} meet face to face where neither's cross-section holds the other's; let one reach into the other, or give them the same cross-section",
                        ni.min(nj) + 1,
                        ni.max(nj) + 1
                    ),
                });
            }
        }
    }
    Ok(moved)
}

/// The solid's tessellation combined with a feature: the feature's faces get tags after the
/// solid's, with their exact surfaces.
pub(super) fn combined(main: Tessellation, f: &Feature, k: usize, solid: &Shape, size: f64) -> Result<Tessellation> {
    let (shape, at) = feature_body(f, solid, size)?;
    // An imported body: in place, its faces flush with the region's (or an earlier body's) moved
    // off them (`crate::flush`).
    let imported = matches!(shape, Shape::Surface { .. });
    let (shape, at, axis) = match shape {
        Shape::Surface { name, positions, triangles, tags, surfaces } => {
            let placed: Vec<Point3> = positions.iter().map(|p| at + f.axis.turn(p.to_vec())).collect();
            let gaps = crate::flush::Gaps { step: 0.1 * size, flush: 1e-5 * size, near: 0.1 * size, mm: 1e3 };
            let positions = crate::flush::moved(&name, &placed, &triangles, &main.positions, &main.triangles, f.combine, gaps)?;
            (Shape::Surface { name, positions, triangles, tags, surfaces }, Point3::ORIGIN, Axis::Z)
        }
        other => (other, at, f.axis),
    };
    // General position against the solid's tessellation: a round body turned about its own
    // axis by an odd angle (it is the same body; an imported one is not round, and stays as it
    // is), and every feature moved by up to a millionth of the element size (far below the mesh's
    // resolution, far above rounding), so no vertex of one lies exactly on a grid line or plane
    // of the other.
    let mut own = tessellate(&shape, size);
    if matches!(shape, Shape::Cylinder { .. } | Shape::Sphere { .. }) {
        let (sn, cs) = 0.2345f64.sin_cos();
        for p in &mut own.positions {
            *p = Point3::new(cs * p.x - sn * p.y, sn * p.x + cs * p.y, p.z);
        }
    }
    // Each feature by its own amount (the fractional parts of multiples of irrational numbers),
    // so two features' grids that line up exactly do not stay lined up.
    let jitter = Vec3::new(0.618_033_988_75, 0.414_213_562_37, 0.732_050_807_57).map(|v| ((k + 1) as f64 * v).fract() * 1e-6 * size);
    let tool = transformed(own, axis, at + jitter);
    let base = main.tri_face.iter().map(|t| t + 1).max().unwrap_or(0).max(main.analytic.len() as u32);
    let op = match f.combine {
        Combine::Add => crate::csg::Op::Union,
        Combine::Subtract => crate::csg::Op::Difference,
        Combine::Intersect => crate::csg::Op::Intersection,
    };
    let a = crate::csg::Surface { positions: main.positions, triangles: main.triangles, tags: main.tri_face };
    let b = crate::csg::Surface { positions: tool.positions, triangles: tool.triangles, tags: tool.tri_face };
    // An imported body's flat faces cross the region's in straight lines: the points its
    // triangles' edges leave along them are collapsed away.
    let out = if imported { crate::csg::combine_simplified(&a, &b, op, base)? } else { crate::csg::combine(&a, &b, op, base)? };
    let mut analytic = main.analytic;
    analytic.resize(base as usize, None);
    // A difference's tool faces now face the other way.
    analytic.extend(tool.analytic.into_iter().map(|a| match (a, op) {
        (Some(AnalyticSurface::Plane { normal, point }), crate::csg::Op::Difference) => {
            Some(AnalyticSurface::Plane { normal: normal.map(|v| -v), point })
        }
        (a, _) => a,
    }));
    Ok(Tessellation {
        positions: out.positions,
        triangles: out.triangles,
        tri_face: out.tags,
        analytic,
        start_face: main.start_face,
        end_face: main.end_face,
    })
}

/// An inclusion's tessellation moved to its place, one face tag per triangle (`0..n`). Its
/// faces get no exact surfaces: the interface is frozen as tessellated, and a tag per triangle
/// keeps boundary recovery from merging coplanar triangles into one facet and triangulating it
/// differently in the two regions.
pub(super) fn placed(t: Tessellation, at: Point3) -> Tessellation {
    let shift = |p: Point3| p + at.to_vec();
    let n = t.triangles.len() as u32;
    Tessellation {
        positions: t.positions.into_iter().map(shift).collect(),
        triangles: t.triangles,
        tri_face: (0..n).collect(),
        ..Tessellation::default()
    }
}

/// Appends interface `b` to `a` with its tags offset by `base`, turned inside out when `outward`
/// is false (the hole it makes in the outer region).
pub(super) fn append(a: &mut Tessellation, b: &Tessellation, base: u32, outward: bool) {
    let off = a.positions.len() as u32;
    a.positions.extend(&b.positions);
    a.triangles.extend(b.triangles.iter().map(|&[x, y, z]| if outward { [x, y, z] } else { [x, z, y] }.map(|v| v + off)));
    a.tri_face.extend(b.tri_face.iter().map(|f| base + f));
}

/// The triangles a region's mesh kept on tags `base..base + count`, as a tessellation oriented out
/// of the inclusion (`inside`: the mesh is the inclusion's own, its boundary already points out
/// of it), one tag per triangle again.
pub(super) fn extract(mesh: &RegionMesh, base: u32, count: u32, inside: bool) -> Tessellation {
    let mut index = std::collections::HashMap::new();
    let mut t = Tessellation::default();
    for &RegionFace { nodes, tet, tag: face } in &mesh.boundary {
        if !(base..base + count).contains(&face) {
            continue;
        }
        // Region boundary faces are listed with the corners of their tetrahedron; orient them
        // by the tetrahedron's fourth corner (outwards from the region).
        let Some(tv) = mesh.tets.get(tet as usize) else { continue };
        let Some(&apex) = tv.iter().find(|v| !nodes.contains(v)) else { continue };
        let p = |v: u32| mesh.points.get(v as usize).copied().unwrap_or(Point3::ORIGIN);
        let (a, b, c, d) = (p(nodes[0]), p(nodes[1]), p(nodes[2]), p(apex));
        let toward_apex = (b - a).cross(c - a).dot(d - a) > 0.0;
        // Outward from the region: away from the apex. Out of the inclusion: the same when the
        // region is the inclusion, the opposite when it is the outer region.
        let flip = toward_apex == inside;
        let tri = if flip { [nodes[0], nodes[2], nodes[1]] } else { nodes };
        let ids = tri.map(|v| {
            *index.entry(v).or_insert_with(|| {
                t.positions.push(p(v));
                (t.positions.len() - 1) as u32
            })
        });
        t.tri_face.push(t.triangles.len() as u32);
        t.triangles.push(ids);
    }
    t
}

/// Whether two tessellations have the same triangles (corners within 1e-9 of the size).
pub(super) fn same_triangles(a: &Tessellation, b: &Tessellation, size: f64) -> bool {
    if a.triangles.len() != b.triangles.len() {
        return false;
    }
    let q = 1e-9 * size;
    let keys = |t: &Tessellation| -> std::collections::BTreeSet<[[i64; 3]; 3]> {
        t.triangles
            .iter()
            .map(|tri| {
                let mut v = tri.map(|i| {
                    let p = t.positions.get(i as usize).copied().unwrap_or(Point3::ORIGIN);
                    [p.x, p.y, p.z].map(|c| (c / q).round() as i64)
                });
                v.sort_unstable();
                v
            })
            .collect()
    };
    keys(a) == keys(b)
}
