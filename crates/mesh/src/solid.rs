//! Unstructured tetrahedral meshes of primitive bodies (box, cylinder, sphere) by the ported
//! mesher ([`emcraft_tetmesh`]): each body is tessellated with a tag per face and that face's
//! exact surface, the tessellation tetrahedralized with its boundary recovered, then refined,
//! smoothed and improved to the requested size, the boundary vertices finally put on the exact
//! surfaces (so a cylinder's or a sphere's wall is approximated by flat triangles whose corners
//! lie on it).

use std::f64::consts::PI;

use emcraft_model::{Boundary, BoundaryFace, Buckets, Mesh, Model, Shape, positive};
use emcraft_tetmesh::cdt::{CdtOptions, tetrahedralize};
use emcraft_tetmesh::surface::{BoundarySurface, SurfaceOptions};
use emcraft_tetmesh::tet::geometry::AnalyticSurface;
use emcraft_tetmesh::tet::{QualityOptions, quality_mesh};

use crate::{MAX_TETS, MeshError, Result};

/// A tessellated body: triangles tagged by face, each face's exact surface, and which faces are
/// the ends a port may close (z = 0, z = length).
struct Tessellation {
    positions: Vec<[f64; 3]>,
    triangles: Vec<[u32; 3]>,
    tri_face: Vec<u32>,
    analytic: Vec<Option<AnalyticSurface>>,
    start_face: Option<u32>,
    end_face: Option<u32>,
}

impl Tessellation {
    fn point(&mut self, p: [f64; 3]) -> u32 {
        self.positions.push(p);
        (self.positions.len() - 1) as u32
    }
}

fn divisions(len: f64, size: f64) -> usize {
    ((len / size).ceil() as usize).clamp(1, 2000)
}

/// A box [0, x] × [0, y] × [0, z]: faces x0, x1, y0, y1, z0 (0..6 in that order), each a grid
/// of about `size`, triangles oriented outwards.
fn tessellate_box(size_xyz: [f64; 3], h: f64) -> Tessellation {
    let mut t = Tessellation {
        positions: Vec::new(),
        triangles: Vec::new(),
        tri_face: Vec::new(),
        analytic: Vec::new(),
        start_face: Some(4),
        end_face: Some(5),
    };
    let n = size_xyz.map(|l| divisions(l, h));
    // Shared vertices on the box's surface: a map from integer lattice coordinates.
    let mut index = std::collections::HashMap::new();
    let mut vertex = |t: &mut Tessellation, i: usize, j: usize, k: usize| -> u32 {
        *index.entry((i, j, k)).or_insert_with(|| {
            t.point([size_xyz[0] * i as f64 / n[0] as f64, size_xyz[1] * j as f64 / n[1] as f64, size_xyz[2] * k as f64 / n[2] as f64])
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
                t.point([rr * a.cos(), rr * a.sin(), z])
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
        let centre = t.point([0.0, 0.0, z]);
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
    let on = |p: [f64; 3]| {
        let l = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
        [p[0] / l * r, p[1] / l * r, p[2] / l * r]
    };
    let mut pts: Vec<[f64; 3]> = raw.iter().map(|p| on(*p)).collect();
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
        let mut midpoint = |a: u32, b: u32, pts: &mut Vec<[f64; 3]>| -> u32 {
            let key = (a.min(b), a.max(b));
            *mid.entry(key).or_insert_with(|| {
                let (p, q) = (pts[a as usize], pts[b as usize]);
                pts.push(on([(p[0] + q[0]) / 2.0, (p[1] + q[1]) / 2.0, (p[2] + q[2]) / 2.0]));
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
    }
}

/// The tessellation size of an inclusion: its interface is kept as tessellated (frozen), so a
/// curved one is divided finely enough for its curvature (about π/16 of its radius).
fn interface_size(shape: &Shape, size: f64) -> f64 {
    match *shape {
        Shape::Box { .. } => size,
        Shape::Cylinder { radius, .. } | Shape::Sphere { radius } => size.min(radius * PI / 16.0),
    }
}

/// An inclusion's tessellation moved to its place, one face tag per triangle (`0..n`). Its
/// faces get no exact surfaces: the interface is frozen as tessellated, and a tag per triangle
/// keeps boundary recovery from merging coplanar triangles into one facet and triangulating it
/// differently in the two regions.
fn placed(t: Tessellation, at: [f64; 3]) -> Tessellation {
    let shift = |p: [f64; 3]| [p[0] + at[0], p[1] + at[1], p[2] + at[2]];
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
        let p = |v: u32| mesh.points.get(v as usize).copied().unwrap_or([0.0; 3]);
        let (a, b, c, d) = (p(nodes[0]), p(nodes[1]), p(nodes[2]), p(apex));
        let n = [
            (b[1] - a[1]) * (c[2] - a[2]) - (b[2] - a[2]) * (c[1] - a[1]),
            (b[2] - a[2]) * (c[0] - a[0]) - (b[0] - a[0]) * (c[2] - a[2]),
            (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]),
        ];
        let toward_apex = n[0] * (d[0] - a[0]) + n[1] * (d[1] - a[1]) + n[2] * (d[2] - a[2]) > 0.0;
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
                let mut v = tri.map(|i| t.positions.get(i as usize).copied().unwrap_or([0.0; 3]).map(|c| (c / q).round() as i64));
                v.sort_unstable();
                v
            })
            .collect()
    };
    keys(a) == keys(b)
}

/// One region's mesh: points, tetrahedra and boundary faces (corners, face tag).
struct RegionMesh {
    points: Vec<[f64; 3]>,
    tets: Vec<[u32; 4]>,
    boundary: Vec<([u32; 3], u32, u32)>,
}

/// Meshes the region a tessellation bounds, keeping the triangles of the `frozen` faces as they
/// are (interfaces another region shares).
fn region(t: &Tessellation, size: f64, frozen: Vec<u32>) -> Result<RegionMesh> {
    let fail = |e: emcraft_tetmesh::Error| MeshError::Invalid { what: "tetrahedral meshing".into(), why: e.to_string() };
    let surface = BoundarySurface::new(&t.positions, &t.triangles, &t.tri_face, &SurfaceOptions::default()).map_err(fail)?;
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
    Ok(RegionMesh { points: q.points, tets, boundary })
}

/// The model's solid meshed with tetrahedra of about `size`: its end faces (box z0/z1, the
/// cylinder's caps) tagged as ports 0 and 1, the rest as conducting walls. Inclusions make holes
/// in it (conductors) or regions of their own (dielectrics), each meshed on its own with the
/// shared interface's triangles kept, the regions then joined node for node.
pub fn solid(model: &Model, size: f64) -> Result<Mesh> {
    let size = positive("element size", size)?;
    let s = model.solid.as_ref().ok_or_else(|| MeshError::Invalid { what: "model".into(), why: "has no solid".into() })?;
    let (material, _) = model.material(&s.material)?;
    let estimate = s.shape.volume() / (size.powi(3) / 6.0) * 1.5;
    if !(estimate <= MAX_TETS as f64) {
        return Err(MeshError::TooLarge { tets: estimate.min(usize::MAX as f64) as usize, limit: MAX_TETS });
    }
    let main = tessellate(&s.shape, size);
    let (start_face, end_face) = (main.start_face, main.end_face);
    // The interfaces' current triangulations, oriented out of each inclusion. Boundary recovery
    // may add points on an interface in one region; that region's triangulation then becomes the
    // other's input, until both keep the same (frozen) triangles.
    let mut interfaces: Vec<Tessellation> =
        model.inclusions.iter().map(|inc| placed(tessellate(&inc.shape, interface_size(&inc.shape, size)), inc.at)).collect();
    // Tags: the solid's faces, then each interface's triangles in turn.
    let first_tag = main.tri_face.iter().map(|f| f + 1).max().unwrap_or(0);
    let mut bases: Vec<(u32, u32)> = Vec::new();
    let mut regions: Vec<(RegionMesh, u32)> = Vec::new();
    for _ in 0..6 {
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
        let outer_mesh = region(&outer, size, (first_tag..next).collect())?;
        let mut changed = false;
        for (k, t) in interfaces.iter_mut().enumerate() {
            let seen = extract(&outer_mesh, bases[k].0, bases[k].1, false);
            if !same_triangles(&seen, t, size) {
                *t = seen;
                changed = true;
            }
        }
        regions = vec![(outer_mesh, material as u32)];
        for (k, inc) in model.inclusions.iter().enumerate() {
            let Some(name) = &inc.material else { continue };
            let (m, _) = model.material(name)?;
            let n = interfaces[k].triangles.len() as u32;
            let inner = region(&interfaces[k], size, (0..n).collect())?;
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
    // Outer-region tags on a dielectric's interface (the inner regions' tags are all interface).
    let dielectric =
        |r: usize, face: u32| r > 0 || bases.iter().zip(&model.inclusions).any(|((b, n), inc)| inc.material.is_some() && (*b..b + n).contains(&face));
    let mut index: std::collections::HashMap<[i64; 3], u32> = std::collections::HashMap::new();
    let mut nodes: Vec<[f64; 3]> = Vec::new();
    let mut tets = Vec::new();
    let mut tet_material = Vec::new();
    let mut boundary = Vec::new();
    let mut interface_sides: std::collections::HashMap<[u32; 3], u32> = std::collections::HashMap::new();
    for (r, (mesh, mat)) in regions.iter().enumerate() {
        // Coincident within 1e-9 of the element size (the regions' snapping to the exact surfaces
        // may differ in the last bits): a hash on cells of that size, checking the neighbours.
        let tol = 1e-9 * size;
        let key = |p: &[f64; 3]| p.map(|v| (v / tol).floor() as i64);
        let map: Vec<u32> = mesh
            .points
            .iter()
            .map(|p| {
                let k = key(p);
                for dx in -1..=1 {
                    for dy in -1..=1 {
                        for dz in -1..=1 {
                            if let Some(&i) = index.get(&[k[0] + dx, k[1] + dy, k[2] + dz]) {
                                let q: [f64; 3] = nodes[i as usize];
                                if (0..3).all(|d| (q[d] - p[d]).abs() <= tol) {
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
    let mut mesh = Mesh { tet_material, nodes, tets, boundary, grid: None, buckets: None };
    mesh.buckets = Some(buckets(&mesh));
    Ok(mesh)
}

/// The tetrahedra binned on a uniform grid of about one tetrahedron per bucket.
pub fn buckets(mesh: &Mesh) -> Buckets {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in &mesh.nodes {
        for d in 0..3 {
            lo[d] = lo[d].min(p[d]);
            hi[d] = hi[d].max(p[d]);
        }
    }
    let ext = [0, 1, 2].map(|d| (hi[d] - lo[d]).max(f64::MIN_POSITIVE));
    let per_side = (mesh.tets.len().max(1) as f64).cbrt().ceil() as usize;
    let longest = ext.iter().copied().fold(0.0, f64::max);
    let dims = ext.map(|e| ((e / longest * per_side as f64).ceil() as usize).clamp(1, 512));
    let step = [0, 1, 2].map(|d| ext[d] / dims[d] as f64);
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
        let mesh = mesh_of(Shape::Box { size: [0.02, 0.01, 0.03] }, 0.004);
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

    #[test]
    fn inclusions_make_regions_that_meet_node_for_node() {
        use emcraft_model::{Inclusion, Material};
        let mut m = Model::new();
        m.add_material(Material::new("alumina", 9.8, 1.0).unwrap()).unwrap();
        m.set_solid(Solid { shape: Shape::Sphere { radius: 0.01 }, material: "vacuum".into() }).unwrap();
        m.add_inclusion(Inclusion { shape: Shape::Sphere { radius: 0.004 }, at: [0.0; 3], material: Some("alumina".into()) }).unwrap();
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
        assert!(m.add_inclusion(Inclusion { shape: Shape::Sphere { radius: 0.004 }, at: [0.008, 0.0, 0.0], material: None }).is_err());
    }

    #[test]
    fn points_are_located_in_unstructured_meshes() {
        let mesh = mesh_of(Shape::Cylinder { radius: 0.01, length: 0.02 }, 0.003);
        for p in [[0.0, 0.0, 0.01], [0.007, 0.0, 0.001], [-0.003, 0.004, 0.019]] {
            let (t, l) = crate::locate(&mesh, p).unwrap();
            assert!(l.iter().all(|x| *x >= -1e-9), "{l:?}");
            let c = mesh.corners(t).unwrap();
            for d in 0..3 {
                let q: f64 = (0..4).map(|i| l[i] * c[i][d]).sum();
                assert!((q - p[d]).abs() < 1e-12);
            }
        }
        assert!(crate::locate(&mesh, [0.02, 0.0, 0.01]).is_none());
    }
}
