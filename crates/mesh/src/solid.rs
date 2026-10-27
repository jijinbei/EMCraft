//! Unstructured tetrahedral meshes of primitive bodies (box, cylinder, sphere) by the ported
//! mesher ([`emcraft_tetmesh`]): each body is tessellated with a tag per face and that face's
//! exact surface, the tessellation tetrahedralized with its boundary recovered, then refined,
//! smoothed and improved to the requested size, the boundary vertices finally put on the exact
//! surfaces (so a cylinder's or a sphere's wall is approximated by flat triangles whose corners
//! lie on it).

use std::f64::consts::PI;

use emcraft_model::{Aabb, Axis, Boundary, BoundaryFace, Buckets, Combine, Feature, Mesh, Model, Point3, Shape, Vec3, positive};
use emcraft_tetmesh::cdt::{CdtOptions, tetrahedralize};
use emcraft_tetmesh::surface::{BoundarySurface, SurfaceOptions};
use emcraft_tetmesh::tet::geometry::AnalyticSurface;
use emcraft_tetmesh::tet::{QualityOptions, quality_mesh};

use crate::{MAX_TETS, MeshError, Result};

/// A tessellated body: triangles tagged by face, each face's exact surface, and which faces are
/// the ends a port may close (z = 0, z = length).
struct Tessellation {
    positions: Vec<Point3>,
    triangles: Vec<[u32; 3]>,
    tri_face: Vec<u32>,
    analytic: Vec<Option<AnalyticSurface>>,
    start_face: Option<u32>,
    end_face: Option<u32>,
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
    let mut t = Tessellation {
        positions: Vec::new(),
        triangles: Vec::new(),
        tri_face: Vec::new(),
        analytic: Vec::new(),
        start_face: Some(4),
        end_face: Some(5),
    };
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
        let mut normal = [0.0; 3];
        normal[axis] = if high { 1.0 } else { -1.0 };
        let mut point = [0.0; 3];
        point[axis] = if high { size_xyz[axis] } else { 0.0 };
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
    let mut t = Tessellation {
        positions: Vec::new(),
        triangles: Vec::new(),
        tri_face: Vec::new(),
        analytic: Vec::new(),
        start_face: Some(1),
        end_face: Some(2),
    };
    t.analytic.push(Some(AnalyticSurface::Cylinder { origin: [0.0; 3], axis: [0.0, 0.0, 1.0], radius: r }));
    t.analytic.push(Some(AnalyticSurface::Plane { normal: [0.0, 0.0, -1.0], point: [0.0; 3] }));
    t.analytic.push(Some(AnalyticSurface::Plane { normal: [0.0, 0.0, 1.0], point: [0.0, 0.0, len] }));
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
    let mut t =
        Tessellation { positions: Vec::new(), triangles: Vec::new(), tri_face: Vec::new(), analytic: Vec::new(), start_face: None, end_face: None };
    t.analytic.push(Some(AnalyticSurface::Sphere { center: [0.0; 3], radius: r }));
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

fn tessellate(shape: &Shape, size: f64) -> Tessellation {
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
    let Shape::Surface { positions, triangles, tags, .. } = shape else {
        return Tessellation {
            positions: Vec::new(),
            triangles: Vec::new(),
            tri_face: Vec::new(),
            analytic: Vec::new(),
            start_face: None,
            end_face: None,
        };
    };
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
        analytic.push(Some(AnalyticSurface::Plane { normal: [0.0, 0.0, n], point: [0.0, 0.0, z] }));
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
fn interface_size(shape: &Shape, size: f64, conductor: bool) -> f64 {
    let per = if conductor { 6.0 } else { 16.0 };
    match *shape {
        Shape::Box { .. } | Shape::Surface { .. } => size,
        Shape::Cylinder { radius, .. } | Shape::Sphere { radius } => size.min(radius * PI / per),
    }
}

/// A tessellation turned by `axis` and moved to `at`, its exact surfaces with it.
fn transformed(t: Tessellation, axis: Axis, at: Point3) -> Tessellation {
    let place = |p: Point3| at + axis.turn(p.to_vec());
    // The exact surfaces' points and directions are the mesher's arrays.
    let place_a = |p: [f64; 3]| place(p.into()).to_array();
    let turn_a = |v: [f64; 3]| axis.turn(v.into()).to_array();
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

/// The flat faces of a body in place: (axis, coordinate, outward sign).
fn flat_faces(shape: &Shape, at: Point3, axis: Axis) -> Vec<(usize, f64, f64)> {
    match *shape {
        Shape::Box { size } => {
            let s = axis.turn(size);
            (0..3).flat_map(|d| [(d, at[d], -1.0), (d, at[d] + s[d], 1.0)]).collect()
        }
        Shape::Cylinder { length, .. } => {
            let d = axis.index();
            vec![(d, at[d], -1.0), (d, at[d] + length, 1.0)]
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
    let mut lo_hi: Vec<(usize, f64, f64)> = flat_faces(&shape, at, f.axis);
    for face in lo_hi.iter_mut() {
        for &(d, c, n) in &base {
            if d != face.0 || (face.1 - c).abs() > tol {
                continue;
            }
            // `out`: along the feature face's outward normal; for a face touching the solid's
            // from outside, that is into the solid.
            let same = n == face.2;
            let out = face.2 * step;
            face.1 += match (f.combine, same) {
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
        let lo = lo_hi.iter().find(|x| x.0 == d && x.2 < 0.0).map(|x| x.1);
        let hi = lo_hi.iter().find(|x| x.0 == d && x.2 > 0.0).map(|x| x.1);
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
fn combined(main: Tessellation, f: &Feature, k: usize, solid: &Shape, size: f64) -> Result<Tessellation> {
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
fn placed(t: Tessellation, at: Point3) -> Tessellation {
    let shift = |p: Point3| p + at.to_vec();
    let n = t.triangles.len() as u32;
    Tessellation {
        positions: t.positions.into_iter().map(shift).collect(),
        triangles: t.triangles,
        tri_face: (0..n).collect(),
        analytic: Vec::new(),
        start_face: None,
        end_face: None,
    }
}

/// Appends interface `b` to `a` with its tags offset by `base`, turned inside out when `outward`
/// is false (the hole it makes in the outer region).
fn append(a: &mut Tessellation, b: &Tessellation, base: u32, outward: bool) {
    let off = a.positions.len() as u32;
    a.positions.extend(&b.positions);
    a.triangles.extend(b.triangles.iter().map(|&[x, y, z]| if outward { [x, y, z] } else { [x, z, y] }.map(|v| v + off)));
    a.tri_face.extend(b.tri_face.iter().map(|f| base + f));
}

/// The triangles a region's mesh kept on tags `base..base + count`, as a tessellation oriented out
/// of the inclusion (`inside`: the mesh is the inclusion's own, its boundary already points out
/// of it), one tag per triangle again.
fn extract(mesh: &RegionMesh, base: u32, count: u32, inside: bool) -> Tessellation {
    let mut index = std::collections::HashMap::new();
    let mut t =
        Tessellation { positions: Vec::new(), triangles: Vec::new(), tri_face: Vec::new(), analytic: Vec::new(), start_face: None, end_face: None };
    for (nodes, tet, face) in &mesh.boundary {
        if !(base..base + count).contains(face) {
            continue;
        }
        // Region boundary faces are listed with the corners of their tetrahedron; orient them
        // by the tetrahedron's fourth corner (outwards from the region).
        let Some(tv) = mesh.tets.get(*tet as usize) else { continue };
        let Some(&apex) = tv.iter().find(|v| !nodes.contains(v)) else { continue };
        let p = |v: u32| mesh.points.get(v as usize).copied().unwrap_or(Point3::ORIGIN);
        let (a, b, c, d) = (p(nodes[0]), p(nodes[1]), p(nodes[2]), p(apex));
        let toward_apex = (b - a).cross(c - a).dot(d - a) > 0.0;
        // Outward from the region: away from the apex. Out of the inclusion: the same when the
        // region is the inclusion, the opposite when it is the outer region.
        let flip = toward_apex == inside;
        let tri = if flip { [nodes[0], nodes[2], nodes[1]] } else { *nodes };
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
fn same_triangles(a: &Tessellation, b: &Tessellation, size: f64) -> bool {
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

/// One region's mesh: points, tetrahedra and boundary faces (corners, face tag).
struct RegionMesh {
    points: Vec<Point3>,
    tets: Vec<[u32; 4]>,
    boundary: Vec<([u32; 3], u32, u32)>,
}

/// Meshes the region a tessellation bounds, keeping the triangles of the `frozen` faces as they
/// are (interfaces another region shares).
fn region(t: &Tessellation, size: f64, frozen: Vec<u32>) -> Result<RegionMesh> {
    let fail = |e: emcraft_tetmesh::Error| MeshError::Invalid { what: "tetrahedral meshing".into(), why: e.to_string() };
    // The mesher takes and gives coordinates as arrays.
    let positions: Vec<[f64; 3]> = t.positions.iter().map(|p| p.to_array()).collect();
    let surface = BoundarySurface::new(&positions, &t.triangles, &t.tri_face, &SurfaceOptions::default()).map_err(fail)?;
    let cdt = tetrahedralize(&surface, &CdtOptions::default()).map_err(fail)?;
    let options = QualityOptions { size: Some(size), max_tets: MAX_TETS, frozen_faces: frozen, ..QualityOptions::default() };
    let mut analytic = t.analytic.clone();
    analytic.resize(t.tri_face.iter().map(|f| *f as usize + 1).max().unwrap_or(0).max(analytic.len()), None);
    let q = quality_mesh(&surface, &cdt, analytic, &options).map_err(fail)?;
    let tets: Vec<[u32; 4]> = q.elements.iter().filter_map(|e| <[u32; 4]>::try_from(e.as_slice()).ok()).collect();
    if tets.len() != q.elements.len() {
        return Err(MeshError::Invalid { what: "tetrahedral meshing".into(), why: "elements other than linear tetrahedra".into() });
    }
    let boundary = q
        .boundary
        .iter()
        .filter_map(|b| {
            let tet = tets.get(b.tet as usize)?;
            let k = b.opposite as usize;
            let n: Vec<u32> = (0..4).filter(|&i| i != k).map(|i| tet[i]).collect();
            Some(([n[0], n[1], n[2]], b.tet, b.face))
        })
        .collect();
    Ok(RegionMesh { points: q.points.into_iter().map(Point3::from).collect(), tets, boundary })
}

/// The model's solid meshed with tetrahedra of about `size`: its end faces (box z0/z1, the
/// cylinder's caps) tagged as ports 0 and 1, the rest as conducting walls. Inclusions make holes
/// in it (conductors) or regions of their own (dielectrics), each meshed on its own with the
/// shared interface's triangles kept, the regions then joined node for node.
pub fn solid(model: &Model, size: f64) -> Result<Mesh> {
    solid_with(model, size, true)
}

/// A curved surface of the model: the edges on it are bent onto it.
#[derive(Clone, Copy, Debug)]
enum Curved {
    /// The side of a cylinder through `c` along `axis`.
    Cylinder {
        c: Point3,
        axis: usize,
        r: f64,
    },
    Sphere {
        c: Point3,
        r: f64,
    },
}

impl Curved {
    fn of(shape: &Shape, at: Point3, axis: Axis) -> Option<Curved> {
        match *shape {
            Shape::Box { .. } | Shape::Surface { .. } => None,
            Shape::Cylinder { radius, .. } => Some(Curved::Cylinder { c: at, axis: axis.index(), r: radius }),
            Shape::Sphere { radius } => Some(Curved::Sphere { c: at, r: radius }),
        }
    }

    fn distance(&self, p: Point3) -> f64 {
        self.project(p).distance(p)
    }

    /// The nearest point on the surface (radially).
    fn project(&self, p: Point3) -> Point3 {
        match *self {
            Curved::Cylinder { c, axis, r } => {
                let d = (p - c).with(axis, 0.0);
                let rho = d.norm().max(f64::MIN_POSITIVE);
                (c + d * r / rho).with(axis, p[axis])
            }
            Curved::Sphere { c, r } => {
                let d = p - c;
                c + d * r / d.norm().max(f64::MIN_POSITIVE)
            }
        }
    }

    /// Whether a triangle with this unit normal can lie on the surface (a cylinder's caps share
    /// its rim but face along the axis).
    fn faces(&self, normal: Vec3) -> bool {
        match self {
            Curved::Cylinder { axis, .. } => normal[*axis].abs() < 0.5,
            Curved::Sphere { .. } => true,
        }
    }
}

/// The faces between a material and the outside or another material (boundary and interface
/// triangles): their corner nodes, and whether a tetrahedron lies on both sides (an interface).
fn surface_faces<'a>(mesh: &'a Mesh, topo: &'a crate::Topology) -> impl Iterator<Item = ([u32; 3], bool)> + 'a {
    let material = |t: u32| mesh.tet_material.get(t as usize).copied();
    topo.face_tets
        .iter()
        .zip(&topo.faces)
        .filter(move |(tets, _)| tets[1] == u32::MAX || material(tets[0]) != material(tets[1]))
        .map(|(tets, f)| (*f, tets[1] != u32::MAX))
}

/// Moves the vertices of interface triangles onto their curved surface: the points boundary
/// recovery added there sit on chords of the tessellation. A move that would turn a tetrahedron
/// inside out is not made.
fn snap_interfaces(mesh: &mut Mesh, topo: &crate::Topology, surfaces: &[Curved], size: f64) {
    let mut node_tets: Vec<Vec<u32>> = vec![Vec::new(); mesh.nodes.len()];
    for (t, tet) in mesh.tets.iter().enumerate() {
        for &n in tet {
            node_tets[n as usize].push(t as u32);
        }
    }
    // Interfaces only (both sides tetrahedra of different materials).
    let interfaces: Vec<[u32; 3]> = surface_faces(mesh, topo).filter(|(_, interface)| *interface).map(|(f, _)| f).collect();
    for nodes in interfaces {
        let p = nodes.map(|n| mesh.nodes[n as usize]);
        let Some(s) = surface_of(&p, surfaces, size).copied() else { continue };
        for &n in &nodes {
            let old = mesh.nodes[n as usize];
            if s.distance(old) < 1e-12 * size {
                continue;
            }
            mesh.nodes[n as usize] = s.project(old);
            if node_tets[n as usize].iter().any(|&t| !mesh.volume(t as usize).is_some_and(|v| v > 0.0)) {
                mesh.nodes[n as usize] = old;
            }
        }
    }
}

/// The curved surface a boundary or interface triangle lies on, if any: its vertices within
/// 0.2 of the element size of it (the boundary's are snapped to it, the interfaces' are as
/// tessellated, and the points boundary recovery added on them sit on chords, up to a sagitta
/// h²/8r away) and its normal across the surface (a cylinder's caps share its rim but face
/// along the axis).
fn surface_of<'a>(p: &[Point3; 3], surfaces: &'a [Curved], size: f64) -> Option<&'a Curved> {
    let normal = (p[1] - p[0]).cross(p[2] - p[0]).normalized().unwrap_or(Vec3::ZERO);
    surfaces.iter().find(|s| s.faces(normal) && p.iter().all(|q| s.distance(*q) < 0.2 * size))
}

/// The midpoints of the edges of boundary and interface triangles that lie on a curved surface,
/// moved onto it (the quadratic geometry of the tetrahedra that hold them).
fn curve(mesh: &Mesh, topo: &crate::Topology, surfaces: &[Curved], size: f64) -> Vec<([u32; 2], Point3)> {
    let mut out = std::collections::BTreeMap::new();
    for (nodes, _) in surface_faces(mesh, topo) {
        let p = nodes.map(|n| mesh.node(n).unwrap_or(Point3::new(f64::NAN, f64::NAN, f64::NAN)));
        let Some(s) = surface_of(&p, surfaces, size) else { continue };
        for (i, j) in [(0, 1), (1, 2), (0, 2)] {
            out.entry([nodes[i].min(nodes[j]), nodes[i].max(nodes[j])]).or_insert_with(|| s.project(p[i].midpoint(p[j])));
        }
    }
    out.into_iter().collect()
}

/// The smallest det DΦ of tetrahedron t's quadratic map at the points the elements check
/// ([`emcraft_fem::nedelec::least_jacobian`]).
fn least_jacobian(mesh: &Mesh, t: usize) -> f64 {
    let (Some(tet), Some(c)) = (mesh.tets.get(t), mesh.corners(t)) else { return 0.0 };
    let mids = crate::LOCAL_EDGES.map(|[a, b]| mesh.midpoint(tet[a], tet[b]));
    emcraft_fem::nedelec::least_jacobian(c, mids).unwrap_or(0.0)
}

/// Straightens the curved edges of tetrahedra whose map would come close to folding (thin
/// tetrahedra against a strongly curved surface), until none does.
fn unfold(mesh: &mut Mesh) {
    for _ in 0..20 {
        let mut straighten = std::collections::BTreeSet::new();
        for (t, tet) in mesh.tets.iter().enumerate() {
            let curved: Vec<[u32; 2]> = crate::LOCAL_EDGES
                .iter()
                .map(|[a, b]| [tet[*a].min(tet[*b]), tet[*a].max(tet[*b])])
                .filter(|e| mesh.midpoint(e[0], e[1]).is_some())
                .collect();
            if !curved.is_empty() && !(least_jacobian(mesh, t) > 0.1) {
                straighten.extend(curved);
            }
        }
        if straighten.is_empty() {
            return;
        }
        mesh.curved.retain(|(e, _)| !straighten.contains(e));
    }
}

/// [`solid`], with the edges on curved surfaces bent onto them (`curved`) or left straight.
pub fn solid_with(model: &Model, size: f64, curved: bool) -> Result<Mesh> {
    solid_sized(model, &Sizes { default: size, per_material: Vec::new() }, curved)
}

/// Element sizes: a default, and one per material (by index into the model's materials) where
/// given — a fixed number of elements per wavelength in each material, say.
#[derive(Clone, Debug, PartialEq)]
pub struct Sizes {
    pub default: f64,
    pub per_material: Vec<Option<f64>>,
}

impl Sizes {
    pub fn of(&self, material: usize) -> f64 {
        self.per_material.get(material).copied().flatten().unwrap_or(self.default)
    }
}

/// [`solid_with`], each material region meshed at its own size (the solid's own material
/// outside the inclusions, each dielectric inclusion at its material's), the interfaces at the
/// smaller of their two sides'.
pub fn solid_sized(model: &Model, sizes: &Sizes, curved: bool) -> Result<Mesh> {
    let s = model.solid.as_ref().ok_or_else(|| MeshError::Invalid { what: "model".into(), why: "has no solid".into() })?;
    let (material, _) = model.material(&s.material)?;
    let size = positive("element size", sizes.of(material))?;
    let mut inner_size = Vec::with_capacity(model.inclusions.len());
    let mut estimate = s.shape.volume() / (size.powi(3) / 6.0) * 1.5;
    for inc in &model.inclusions {
        let h = match &inc.material {
            Some(name) => positive("element size", sizes.of(model.material(name)?.0))?,
            None => size,
        };
        inner_size.push(h);
        estimate += inc.shape.volume() / (h.powi(3) / 6.0) * 1.5;
    }
    if !(estimate <= MAX_TETS as f64) {
        return Err(MeshError::TooLarge { tets: estimate.min(usize::MAX as f64) as usize, limit: MAX_TETS });
    }
    // A spherical layer (model.open, shape sphere): the solid's region is the ball inside the
    // layer's inner sphere, its surface an interface (tagged per triangle and frozen, like an
    // inclusion's) with the layer's shell, meshed as a region of its own between it and the
    // solid's sphere — so the shell's tetrahedra are a layer of their own.
    let layer = model.spherical_layer();
    if let Some((inner, _)) = layer {
        if !model.features.is_empty() {
            return Err(MeshError::Invalid { what: "spherical layer".into(), why: "not supported yet: features with a spherical layer".into() });
        }
        if !(inner > 0.0 && inner.is_finite()) {
            return Err(MeshError::Invalid { what: "spherical layer".into(), why: "its inner radius must be positive".into() });
        }
    }
    let mut main = match layer {
        Some((inner, _)) => {
            let ball = Shape::Sphere { radius: inner };
            placed(tessellate(&ball, interface_size(&ball, size, false)), Point3::ORIGIN)
        }
        None => tessellate(&s.shape, size),
    };
    for (k, f) in model.features.iter().enumerate() {
        main = combined(main, f, k, &s.shape, size)?;
    }
    let (start_face, end_face) = (main.start_face, main.end_face);
    // The interfaces' current triangulations, oriented out of each inclusion. Boundary recovery
    // may add points on an interface in one region; that region's triangulation then becomes the
    // other's input, until both keep the same (frozen) triangles.
    let mut interfaces: Vec<Tessellation> = model
        .inclusions
        .iter()
        .zip(&inner_size)
        .map(|(inc, h)| placed(tessellate(&inc.shape, interface_size(&inc.shape, size.min(*h), inc.material.is_none())), inc.at))
        .collect();
    // Tags: the solid's faces (with a spherical layer, the inner sphere's triangles), then each
    // interface's triangles in turn.
    let mut first_tag = 0;
    let mut bases: Vec<(u32, u32)> = Vec::new();
    let mut regions: Vec<(RegionMesh, u32)> = Vec::new();
    for _ in 0..6 {
        first_tag = main.tri_face.iter().map(|f| f + 1).max().unwrap_or(0);
        let mut outer = Tessellation {
            positions: main.positions.clone(),
            triangles: main.triangles.clone(),
            tri_face: main.tri_face.clone(),
            analytic: main.analytic.clone(),
            start_face,
            end_face,
        };
        bases.clear();
        let mut next = first_tag;
        for t in &interfaces {
            let n = t.triangles.len() as u32;
            append(&mut outer, t, next, false);
            bases.push((next, n));
            next += n;
        }
        let frozen_from = if layer.is_some() { 0 } else { first_tag };
        let outer_mesh = region(&outer, size, (frozen_from..next).collect())?;
        let mut changed = false;
        for (k, t) in interfaces.iter_mut().enumerate() {
            let seen = extract(&outer_mesh, bases[k].0, bases[k].1, false);
            if !same_triangles(&seen, t, size) {
                *t = seen;
                changed = true;
            }
        }
        // The layer's shell: the solid's sphere outside, the inner sphere as a hole (tags 1..).
        let shell = match layer {
            Some(_) => {
                let seen = extract(&outer_mesh, 0, first_tag, true);
                if !same_triangles(&seen, &main, size) {
                    main = seen;
                    changed = true;
                }
                let mut t = tessellate(&s.shape, size);
                append(&mut t, &main, 1, false);
                let n = main.triangles.len() as u32;
                let shell = region(&t, size, (1..1 + n).collect())?;
                let seen = extract(&shell, 1, n, false);
                if !same_triangles(&seen, &main, size) {
                    main = seen;
                    changed = true;
                }
                Some(shell)
            }
            None => None,
        };
        regions = vec![(outer_mesh, material as u32)];
        regions.extend(shell.map(|m| (m, material as u32)));
        for (k, inc) in model.inclusions.iter().enumerate() {
            let Some(name) = &inc.material else { continue };
            let (m, _) = model.material(name)?;
            let n = interfaces[k].triangles.len() as u32;
            let inner = region(&interfaces[k], inner_size[k], (0..n).collect())?;
            let seen = extract(&inner, 0, n, true);
            if !same_triangles(&seen, &interfaces[k], size) {
                interfaces[k] = seen;
                changed = true;
            }
            regions.push((inner, m as u32));
        }

        if !changed {
            break;
        }
    }
    // Join: nodes with the same coordinates (the frozen interfaces' vertices) become one.
    // Outer-region tags on a dielectric's interface (the inner regions' tags are all interface);
    // with a spherical layer, region 1 is its shell (tag 0 its outer wall, the rest the inner
    // sphere) and the solid region's own faces are that inner sphere.
    let shell = if layer.is_some() { 1 } else { usize::MAX };
    let dielectric = |r: usize, face: u32| {
        if r == shell {
            return face >= 1;
        }
        r > 0
            || (layer.is_some() && face < first_tag)
            || bases.iter().zip(&model.inclusions).any(|((b, n), inc)| inc.material.is_some() && (*b..b + n).contains(&face))
    };
    let mut index: std::collections::HashMap<[i64; 3], u32> = std::collections::HashMap::new();
    let mut nodes: Vec<Point3> = Vec::new();
    let mut tets = Vec::new();
    let mut tet_material = Vec::new();
    let mut boundary = Vec::new();
    let mut interface_sides: std::collections::HashMap<[u32; 3], u32> = std::collections::HashMap::new();
    for (r, (mesh, mat)) in regions.iter().enumerate() {
        // Coincident within 1e-9 of the element size (the regions' snapping to the exact surfaces
        // may differ in the last bits): a hash on cells of that size, checking the neighbours.
        let tol = 1e-9 * size;
        let key = |p: &Point3| [p.x, p.y, p.z].map(|v| (v / tol).floor() as i64);
        let map: Vec<u32> = mesh
            .points
            .iter()
            .map(|p| {
                let k = key(p);
                for dx in -1..=1 {
                    for dy in -1..=1 {
                        for dz in -1..=1 {
                            if let Some(&i) = index.get(&[k[0] + dx, k[1] + dy, k[2] + dz]) {
                                let q = nodes[i as usize];
                                if (q - *p).iter().all(|d| d.abs() <= tol) {
                                    return i;
                                }
                            }
                        }
                    }
                }
                nodes.push(*p);
                let i = (nodes.len() - 1) as u32;
                index.insert(k, i);
                i
            })
            .collect();
        let first = tets.len() as u32;
        tets.extend(mesh.tets.iter().map(|t| t.map(|v| map[v as usize])));
        tet_material.extend(std::iter::repeat_n(*mat, mesh.tets.len()));
        for (n, tet, face) in &mesh.boundary {
            let n = n.map(|v| map[v as usize]);
            if dielectric(r, *face) {
                let mut key = n;
                key.sort_unstable();
                *interface_sides.entry(key).or_default() += 1;
                continue;
            }
            if r == shell {
                boundary.push(BoundaryFace { nodes: n, tet: first + tet, tag: Boundary::Pec });
                continue;
            }
            if r > 0 {
                continue;
            }
            let tag = if Some(*face) == start_face {
                Boundary::Port(0)
            } else if Some(*face) == end_face {
                Boundary::Port(1)
            } else {
                Boundary::Pec
            };
            boundary.push(BoundaryFace { nodes: n, tet: first + tet, tag });
        }
    }
    // Every interface triangle seen from both sides: the regions conform.
    if interface_sides.values().any(|&c| c != 2) {
        return Err(MeshError::Invalid {
            what: "tetrahedral meshing".into(),
            why: "an interface between two materials was not kept the same from both sides (boundary recovery added points on it); try another element size".into(),
        });
    }
    let mut mesh = Mesh { tet_material, nodes, tets, boundary, grid: None, buckets: None, curved: Vec::new() };
    if curved {
        let surfaces: Vec<Curved> = std::iter::once(Curved::of(&s.shape, Point3::ORIGIN, Axis::Z))
            .chain(model.inclusions.iter().map(|i| Curved::of(&i.shape, i.at, Axis::Z)))
            .chain(model.features.iter().map(|f| Curved::of(&f.shape, f.at, f.axis)))
            .flatten()
            .collect();
        // One topology serves both: moving nodes does not change it.
        let topo = crate::Topology::new(&mesh);
        snap_interfaces(&mut mesh, &topo, &surfaces, size);
        mesh.curved = curve(&mesh, &topo, &surfaces, size);
        unfold(&mut mesh);
    }
    mesh.buckets = Some(buckets(&mesh));
    Ok(mesh)
}

/// The tetrahedra binned on a uniform grid of about one tetrahedron per bucket.
pub fn buckets(mesh: &Mesh) -> Buckets {
    let Aabb { min: lo, max: hi } = Aabb::around(mesh.nodes.iter().copied()).unwrap_or_default();
    let ext = (hi - lo).map(|e| e.max(f64::MIN_POSITIVE));
    let per_side = (mesh.tets.len().max(1) as f64).cbrt().ceil() as usize;
    let longest = ext.iter().fold(0.0, f64::max);
    let dims = [0, 1, 2].map(|d| ((ext[d] / longest * per_side as f64).ceil() as usize).clamp(1, 512));
    let step = Vec3::new(ext.x / dims[0] as f64, ext.y / dims[1] as f64, ext.z / dims[2] as f64);
    let index = |v: f64, d: usize| (((v - lo[d]) / step[d]).floor().max(0.0) as usize).min(dims[d] - 1);
    let nb = dims[0] * dims[1] * dims[2];
    let mut lists: Vec<Vec<u32>> = vec![Vec::new(); nb];
    for (t, tet) in mesh.tets.iter().enumerate() {
        let mut a = [usize::MAX; 3];
        let mut b = [0usize; 3];
        for &n in tet {
            let p = mesh.nodes[n as usize];
            for d in 0..3 {
                a[d] = a[d].min(index(p[d], d));
                b[d] = b[d].max(index(p[d], d));
            }
        }
        for k in a[2]..=b[2] {
            for j in a[1]..=b[1] {
                for i in a[0]..=b[0] {
                    lists[i + dims[0] * (j + dims[1] * k)].push(t as u32);
                }
            }
        }
    }
    let mut start = Vec::with_capacity(nb + 1);
    let mut items = Vec::new();
    start.push(0);
    for l in lists {
        items.extend(l);
        start.push(items.len() as u32);
    }
    Buckets { lo, step, dims, start, items }
}

#[cfg(test)]
mod tests {
    use super::*;
    use emcraft_model::Solid;

    fn mesh_of(shape: Shape, size: f64) -> Mesh {
        let mut m = Model::new();
        m.set_solid(Solid { shape, material: "vacuum".into() }).unwrap();
        solid(&m, size).unwrap()
    }

    fn area(mesh: &Mesh, f: &BoundaryFace) -> f64 {
        let p = f.nodes.map(|n| mesh.nodes[n as usize]);
        let u = [p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]];
        let v = [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]];
        let c = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        0.5 * (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt()
    }

    #[test]
    fn a_box_is_filled_exactly_with_its_ports_tagged() {
        let mesh = mesh_of(Shape::Box { size: Vec3::new(0.02, 0.01, 0.03) }, 0.004);
        let v = crate::volumes(&mesh);
        assert!(v.iter().all(|x| *x > 0.0));
        assert!((v.iter().sum::<f64>() - 6e-6).abs() < 1e-12 * 6e-6 + 1e-15);
        let port = |k: u32| mesh.boundary.iter().filter(|f| f.tag == Boundary::Port(k)).map(|f| area(&mesh, f)).sum::<f64>();
        assert!((port(0) - 2e-4).abs() < 1e-12 && (port(1) - 2e-4).abs() < 1e-12);
        assert!(
            mesh.boundary.iter().filter(|f| f.tag == Boundary::Port(0)).all(|f| f.nodes.iter().all(|n| mesh.nodes[*n as usize][2].abs() < 1e-12))
        );
    }

    #[test]
    fn a_cylinder_and_a_sphere_approach_their_volumes() {
        let (r, l) = (0.01, 0.02);
        let mesh = mesh_of(Shape::Cylinder { radius: r, length: l }, 0.0025);
        let v: f64 = crate::volumes(&mesh).iter().sum();
        let exact = PI * r * r * l;
        // Flat facets inscribed in the wall: a few tenths of a percent short.
        assert!(v < exact && v > 0.98 * exact, "{v} {exact}");
        // Boundary vertices lie on the exact surfaces.
        for f in &mesh.boundary {
            for n in f.nodes {
                let p = mesh.nodes[n as usize];
                let rho = (p[0] * p[0] + p[1] * p[1]).sqrt();
                assert!((rho - r).abs() < 1e-9 || p[2].abs() < 1e-12 || (p[2] - l).abs() < 1e-12, "{p:?}");
            }
        }
        let sphere = mesh_of(Shape::Sphere { radius: r }, 0.003);
        let v: f64 = crate::volumes(&sphere).iter().sum();
        let exact = 4.0 / 3.0 * PI * r.powi(3);
        // Flat facets of about 3 mm inscribed in a 10 mm sphere: 3.7 % short.
        assert!(v < exact && v > 0.95 * exact, "{v} {exact}");
        assert!(sphere.boundary.iter().all(|f| f.tag == Boundary::Pec));
    }

    /// A sphere with a spherical layer, a dielectric inclusion inside it if asked: its mesh
    /// checked to fill the sphere, its shell's tetrahedra a layer of their own.
    fn layered(with_inclusion: bool) {
        use emcraft_model::{Inclusion, Material, Open, OpenShape};
        let (r, d) = (0.02, 0.006);
        let mut m = Model::new();
        m.add_material(Material::new("alumina", 9.8, 1.0).unwrap()).unwrap();
        m.set_solid(Solid { shape: Shape::Sphere { radius: r }, material: "vacuum".into() }).unwrap();
        m.set_open(Some(Open { thickness: d, reflection: None, shape: OpenShape::Sphere })).unwrap();
        if with_inclusion {
            let ceramic = Inclusion { shape: Shape::Sphere { radius: 0.005 }, at: Point3::new(0.002, 0.0, 0.0), material: Some("alumina".into()) };
            m.add_inclusion(ceramic).unwrap();
        }
        let mesh = solid(&m, 0.004).unwrap();
        let v = crate::volumes(&mesh);
        assert!(v.iter().all(|x| *x > 0.0));
        let exact = 4.0 / 3.0 * PI * r.powi(3);
        let total: f64 = v.iter().sum();
        assert!(total < exact && total > 0.95 * exact, "{total} {exact}");
        // No tetrahedron crosses the inner sphere: each lies wholly inside or outside it.
        let radius = |n: u32| mesh.nodes[n as usize].to_vec().norm();
        let tol = 1e-9 * r;
        let mut shell = 0.0;
        for (t, tet) in mesh.tets.iter().enumerate() {
            let (lo, hi) = tet.iter().map(|&n| radius(n)).fold((f64::INFINITY, 0.0f64), |(a, b), x| (a.min(x), b.max(x)));
            assert!(hi <= r - d + tol || lo >= r - d - tol, "tetrahedron {t} crosses the layer's inner sphere: {lo} {hi}");
            if lo >= r - d - tol {
                shell += v[t];
            }
        }
        let exact_shell = 4.0 / 3.0 * PI * (r.powi(3) - (r - d).powi(3));
        assert!((shell / exact_shell - 1.0).abs() < 0.05, "{shell} {exact_shell}");
        // The walls are the outer sphere only; the dielectric keeps its region.
        assert!(mesh.boundary.iter().all(|f| f.tag == Boundary::Pec && f.nodes.iter().all(|&n| (radius(n) - r).abs() < 1e-9)));
        assert_eq!(mesh.tet_material.contains(&1), with_inclusion);
    }

    #[test]
    fn a_spherical_layer_is_a_shell_of_its_own() {
        layered(false);
    }

    #[test]
    #[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
    fn a_spherical_layer_holds_inclusions() {
        layered(true);
    }

    #[test]
    fn inclusions_make_regions_that_meet_node_for_node() {
        use emcraft_model::{Inclusion, Material};
        let mut m = Model::new();
        m.add_material(Material::new("alumina", 9.8, 1.0).unwrap()).unwrap();
        m.set_solid(Solid { shape: Shape::Sphere { radius: 0.01 }, material: "vacuum".into() }).unwrap();
        m.add_inclusion(Inclusion { shape: Shape::Sphere { radius: 0.004 }, at: Point3::new(0.0, 0.0, 0.0), material: Some("alumina".into()) })
            .unwrap();
        let mesh = solid(&m, 0.0025).unwrap();
        let v = crate::volumes(&mesh);
        assert!(v.iter().all(|x| *x > 0.0));
        let inner: f64 = v.iter().zip(&mesh.tet_material).filter(|(_, m)| **m == 1).map(|(v, _)| v).sum();
        let exact = 4.0 / 3.0 * PI * 0.004f64.powi(3);
        // Flat facets under 0.8 mm long on the 4 mm sphere.
        assert!(inner < exact && inner > 0.97 * exact, "{inner} {exact}");
        // Conforming: every face of the joined mesh is held by one or two tetrahedra, and the
        // boundary is the outer sphere only.
        let topo = crate::Topology::new(&mesh);
        let outer_faces = topo.face_tets.iter().filter(|o| o[1] == u32::MAX).count();
        assert_eq!(outer_faces, mesh.boundary.len());
        assert!(mesh.boundary.iter().all(|f| f.nodes.iter().all(|n| {
            let p = mesh.nodes[*n as usize];
            ((p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt() - 0.01).abs() < 1e-9
        })));
        // A conductor instead: a hole, its surface a wall.
        let mut c = m.clone();
        c.inclusions[0].material = None;
        let hole = solid(&c, 0.0025).unwrap();
        assert!(hole.tet_material.iter().all(|m| *m == 0));
        let walls = hole.boundary.len();
        assert!(walls > mesh.boundary.len());
        // Inclusions must lie inside.
        assert!(m.add_inclusion(Inclusion { shape: Shape::Sphere { radius: 0.004 }, at: Point3::new(0.008, 0.0, 0.0), material: None }).is_err());
    }

    #[test]
    fn edges_on_curved_walls_are_bent_onto_them() {
        let (r, l) = (0.01, 0.02);
        let mesh = mesh_of(Shape::Cylinder { radius: r, length: l }, 0.003);
        assert!(!mesh.curved.is_empty() && mesh.curved.windows(2).all(|w| w[0].0 < w[1].0));
        let boundary_edges: std::collections::HashSet<[u32; 2]> = mesh
            .boundary
            .iter()
            .flat_map(|f| [(0, 1), (1, 2), (0, 2)].map(|(i, j)| [f.nodes[i].min(f.nodes[j]), f.nodes[i].max(f.nodes[j])]))
            .collect();
        for (e, m) in &mesh.curved {
            // On the side wall, between two wall nodes, an edge of the boundary.
            assert!(((m[0] * m[0] + m[1] * m[1]).sqrt() - r).abs() < 1e-12 && boundary_edges.contains(e));
        }
        // Every edge on the side (both ends on the wall, not across a cap) is curved.
        let rho = |n: u32| {
            let p = mesh.nodes[n as usize];
            (p[0] * p[0] + p[1] * p[1]).sqrt()
        };
        let side = mesh
            .boundary
            .iter()
            .filter(|f| {
                f.nodes.iter().all(|n| (rho(*n) - r).abs() < 1e-9) && {
                    let z = f.nodes.map(|n| mesh.nodes[n as usize][2]);
                    !(z.iter().all(|z| z.abs() < 1e-12) || z.iter().all(|z| (z - l).abs() < 1e-12))
                }
            })
            .count();
        assert!(side > 0);
        for f in &mesh.boundary {
            let z = f.nodes.map(|n| mesh.nodes[n as usize][2]);
            let cap = z.iter().all(|z| z.abs() < 1e-12) || z.iter().all(|z| (z - l).abs() < 1e-12);
            if !cap && f.nodes.iter().all(|n| (rho(*n) - r).abs() < 1e-9) {
                for (i, j) in [(0, 1), (1, 2), (0, 2)] {
                    assert!(mesh.midpoint(f.nodes[i], f.nodes[j]).is_some());
                }
            }
        }
        // Straight on request; a box has nothing to bend.
        let mut m = Model::new();
        m.set_solid(Solid { shape: Shape::Cylinder { radius: r, length: l }, material: "vacuum".into() }).unwrap();
        assert!(solid_with(&m, 0.003, false).unwrap().curved.is_empty());
        assert!(mesh_of(Shape::Box { size: Vec3::new(0.02, 0.01, 0.03) }, 0.004).curved.is_empty());
    }

    #[test]
    fn features_combine_with_the_solid() {
        use emcraft_model::Feature;
        let (a, b, l) = (0.02286, 0.01016, 0.03);
        let mut m = Model::new();
        m.set_solid(Solid { shape: Shape::Box { size: Vec3::new(a, b, l) }, material: "vacuum".into() }).unwrap();
        // A post across the guide, flush with both broad walls.
        let r = 0.0015;
        m.add_feature(Feature {
            combine: Combine::Subtract,
            shape: Shape::Cylinder { radius: r, length: b },
            at: Point3::new(a / 2.0, 0.0, l / 2.0),
            axis: Axis::Y,
        })
        .unwrap();
        let mesh = solid(&m, 0.003).unwrap();
        let v: f64 = crate::volumes(&mesh).iter().sum();
        let exact = a * b * l - PI * r * r * b;
        // Flat facets on the post (12 round): the hole a little smaller than the circle.
        assert!(v > exact && v < exact + 0.05 * PI * r * r * b, "{v} {exact}");
        // The post's surface is wall; the ends are still whole ports.
        let port = |k: u32| mesh.boundary.iter().filter(|f| f.tag == Boundary::Port(k)).map(|f| area(&mesh, f)).sum::<f64>();
        assert!((port(0) - a * b).abs() < 1e-12 && (port(1) - a * b).abs() < 1e-12);
        let on_post = |f: &BoundaryFace| {
            f.nodes.iter().all(|n| {
                let p = mesh.nodes[*n as usize];
                ((p[0] - a / 2.0).hypot(p[2] - l / 2.0) - r).abs() < 1e-9
            })
        };
        assert!(mesh.boundary.iter().any(on_post));
        assert!(mesh.curved.iter().any(|(_, p)| ((p[0] - a / 2.0).hypot(p[2] - l / 2.0) - r).abs() < 1e-12));
        // A stub added on the broad wall: a box standing on it (its foot moved into the guide).
        let mut u = Model::new();
        u.set_solid(Solid { shape: Shape::Box { size: Vec3::new(a, b, l) }, material: "vacuum".into() }).unwrap();
        u.add_feature(Feature {
            combine: Combine::Add,
            shape: Shape::Box { size: Vec3::new(0.008, 0.006, 0.01) },
            at: Point3::new(0.007, b, 0.01),
            axis: Axis::Z,
        })
        .unwrap();
        let v: f64 = crate::volumes(&solid(&u, 0.003).unwrap()).iter().sum();
        // Exact but for the features' shift by a millionth of the element size (1e-9 m here,
        // over the stub's 80 mm² foot).
        assert!((v - (a * b * l + 0.008 * 0.006 * 0.01)).abs() < 1e-7 * v, "{v}");
        // Flush faces of a union are refused, not guessed.
        let mut w = Model::new();
        w.set_solid(Solid { shape: Shape::Box { size: Vec3::new(a, b, l) }, material: "vacuum".into() }).unwrap();
        w.add_feature(Feature {
            combine: Combine::Add,
            shape: Shape::Box { size: Vec3::new(0.03, b, 0.01) },
            at: Point3::new(0.0, 0.0, 0.01),
            axis: Axis::Z,
        })
        .unwrap();
        assert!(solid(&w, 0.003).is_err());
    }

    #[test]
    fn points_are_located_in_unstructured_meshes() {
        let mesh = mesh_of(Shape::Cylinder { radius: 0.01, length: 0.02 }, 0.003);
        for p in [[0.0, 0.0, 0.01], [0.007, 0.0, 0.001], [-0.003, 0.004, 0.019]].map(Point3::from) {
            let (t, l) = crate::locate(&mesh, p).unwrap();
            assert!(l.iter().all(|x| *x >= -1e-9), "{l:?}");
            let c = mesh.corners(t).unwrap();
            for d in 0..3 {
                let q: f64 = (0..4).map(|i| l[i] * c[i][d]).sum();
                assert!((q - p[d]).abs() < 1e-12);
            }
        }
        assert!(crate::locate(&mesh, Point3::new(0.02, 0.0, 0.01)).is_none());
    }

    /// The post through WR-90 of the driven gate (`a_post_across_the_guide_conserves_power`).
    fn post_model() -> Model {
        use emcraft_model::Feature;
        let (a, b, l) = (0.02286, 0.01016, 0.03);
        let mut m = Model::new();
        m.set_solid(Solid { shape: Shape::Box { size: Vec3::new(a, b, l) }, material: "vacuum".into() }).unwrap();
        m.add_feature(Feature {
            combine: Combine::Subtract,
            shape: Shape::Cylinder { radius: 0.0015, length: b },
            at: Point3::new(a / 2.0, 0.0, l / 2.0),
            axis: Axis::Y,
        })
        .unwrap();
        m
    }

    /// A digest of a mesh's nodes and tetrahedra, bit for bit (FNV-1a).
    fn digest(mesh: &Mesh) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        let mut eat = |x: u64| {
            for b in x.to_le_bytes() {
                h = (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        for p in &mesh.nodes {
            for c in [p.x, p.y, p.z] {
                eat(c.to_bits());
            }
        }
        for t in &mesh.tets {
            for v in t {
                eat(u64::from(*v));
            }
        }
        h
    }

    /// Times the post at 4 mm (the ROADMAP's quality-stage case): `cargo test --release -p
    /// emcraft-mesh post_mesh_timing -- --ignored --nocapture`.
    #[test]
    #[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
    fn post_mesh_timing() {
        // The process's CPU time (user + system, Linux), steadier than the wall clock on a busy
        // machine; 0 where /proc is missing.
        let cpu = || -> f64 {
            let stat = std::fs::read_to_string("/proc/self/stat").unwrap_or_default();
            let fields: Vec<&str> = stat.rsplit(')').next().unwrap_or("").split_whitespace().collect();
            let tick = |i: usize| fields.get(i).and_then(|f| f.parse::<f64>().ok()).unwrap_or(0.0);
            (tick(11) + tick(12)) / 100.0
        };
        let m = post_model();
        for h in [0.004, 0.003] {
            let (t, c) = (std::time::Instant::now(), cpu());
            let mesh = solid(&m, h).unwrap();
            let (s, c) = (t.elapsed().as_secs_f64(), cpu() - c);
            eprintln!(
                "post at {} mm: {s:.2} s ({c:.2} s CPU), {} tetrahedra, {} nodes, digest {:016x}",
                h * 1e3,
                mesh.tets.len(),
                mesh.nodes.len(),
                digest(&mesh)
            );
        }
    }
}
