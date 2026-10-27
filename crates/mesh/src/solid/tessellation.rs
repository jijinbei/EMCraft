//! Bodies as tagged triangles: the primitive shapes tessellated with an exact surface per face,
//! moved into place, combined with the solid's features (a mesh Boolean, [`crate::csg`]), and
//! the interfaces regions share taken from and handed between their meshes.

use std::f64::consts::PI;

use emcraft_model::{Aabb, Axis, Combine, Feature, Point3, Shape, Vec3};
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

/// A cylinder about z, radius r, from z = 0 to `len`: faces side (0), bottom (1), top (2).
fn tessellate_cylinder(r: f64, len: f64, h: f64) -> Tessellation {
    let mut t = Tessellation { start_face: Some(1), end_face: Some(2), ..Tessellation::default() };
    t.analytic.push(Some(AnalyticSurface::Cylinder { origin: MPoint::ORIGIN, axis: MVec::new(0.0, 0.0, 1.0), radius: r }));
    t.analytic.push(Some(AnalyticSurface::Plane { normal: MVec::new(0.0, 0.0, -1.0), point: MPoint::ORIGIN }));
    t.analytic.push(Some(AnalyticSurface::Plane { normal: MVec::new(0.0, 0.0, 1.0), point: MPoint::new(0.0, 0.0, len) }));
    let nt = divisions(2.0 * PI * r, h).max(12);
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

/// An imported surface as it is (the mesher refines it to the size): its faces as tagged, the
/// triangles on its lowest and highest z planes made the end faces a port may close, with
/// their planes as exact surfaces.
fn tessellate_surface(shape: &Shape) -> Tessellation {
    let Shape::Surface { positions, triangles, tags, .. } = shape else { return Tessellation::default() };
    let mut tri_face = tags.clone();
    let faces = tags.iter().map(|t| t + 1).max().unwrap_or(0);
    let mut analytic = vec![None; faces as usize];
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

/// The feature's body with each flat face that lies in one of the solid's moved off it by a
/// step, so the two surfaces meet in general position with the same result: a face flush with
/// the solid's (both bodies behind it) moves outwards for a difference or an intersection (what
/// sticks out is not kept); a face touching the solid's from outside moves into it for a union
/// and away for a difference. A union flush with the solid's faces is refused.
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
                (Combine::Add, true) => {
                    return Err(MeshError::Invalid {
                        what: "feature".into(),
                        why: "not supported yet: a union whose face is flush with the solid's (let it stand out of the face, or reach into the body)"
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

/// The solid's tessellation combined with a feature: the feature's faces get tags after the
/// solid's, with their exact surfaces.
pub(super) fn combined(main: Tessellation, f: &Feature, k: usize, solid: &Shape, size: f64) -> Result<Tessellation> {
    let (shape, at) = feature_body(f, solid, size)?;
    // General position against the solid's tessellation: a round body turned about its own
    // axis by an odd angle (it is the same body), and every feature moved by up to a millionth of
    // the element size (far below the mesh's resolution, far above rounding), so no vertex of one
    // lies exactly on a grid line or plane of the other.
    let mut own = tessellate(&shape, size);
    if !matches!(shape, Shape::Box { .. }) {
        let (sn, cs) = 0.2345f64.sin_cos();
        for p in &mut own.positions {
            *p = Point3::new(cs * p.x - sn * p.y, sn * p.x + cs * p.y, p.z);
        }
    }
    // Each feature by its own amount (the fractional parts of multiples of irrational numbers),
    // so two features' grids that line up exactly do not stay lined up.
    let jitter = Vec3::new(0.618_033_988_75, 0.414_213_562_37, 0.732_050_807_57).map(|v| ((k + 1) as f64 * v).fract() * 1e-6 * size);
    let tool = transformed(own, f.axis, at + jitter);
    let base = main.tri_face.iter().map(|t| t + 1).max().unwrap_or(0).max(main.analytic.len() as u32);
    let op = match f.combine {
        Combine::Add => crate::csg::Op::Union,
        Combine::Subtract => crate::csg::Op::Difference,
        Combine::Intersect => crate::csg::Op::Intersection,
    };
    let a = crate::csg::Surface { positions: main.positions, triangles: main.triangles, tags: main.tri_face };
    let b = crate::csg::Surface { positions: tool.positions, triangles: tool.triangles, tags: tool.tri_face };
    let out = crate::csg::combine(&a, &b, op, base)?;
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
