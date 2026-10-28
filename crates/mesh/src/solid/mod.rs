//! Unstructured tetrahedral meshes of primitive bodies (box, cylinder, sphere) by the ported
//! mesher ([`emcraft_tetmesh`]): each body is tessellated with a tag per face and that face's
//! exact surface, the tessellation tetrahedralized with its boundary recovered, then refined,
//! smoothed and improved to the requested size, the boundary vertices finally put on the exact
//! surfaces (so a cylinder's or a sphere's wall is approximated by flat triangles whose corners
//! lie on it).
//!
//! The bodies' tessellations are in `tessellation`, the crossing into the mesher in `mesher`, the
//! bending of edges onto curved surfaces in `curved`; here the regions are meshed until their
//! interfaces agree and joined.

mod curved;
mod mesher;
mod tessellation;

use emcraft_model::{Aabb, Boundary, BoundaryFace, Buckets, Mesh, Model, Point3, Shape, SphericalLayer, Vec3, positive};
use emcraft_tetmesh::size::{LocalSize, Region};

use crate::{MeshError, Result, too_large};
use curved::bend;
use mesher::{RegionFace, RegionMesh, mesher_point, region};
use tessellation::{Tessellation, append, combined, extract, interface_size, joined, placed, same_triangles, tessellate};

/// The model's solid meshed with tetrahedra of about `size`: its end faces (box z0/z1, the
/// cylinder's caps) tagged as ports 0 and 1, the rest as conducting walls. Inclusions make holes
/// in it (conductors) or regions of their own (dielectrics), each meshed on its own with the
/// shared interface's triangles kept, the regions then joined node for node.
pub fn solid(model: &Model, size: f64) -> Result<Mesh> {
    solid_with(model, size, true)
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
    solid_refined(model, sizes, curved, &[])
}

/// The tetrahedra a body of `volume` meshed at `size` has, about.
fn tets_in(volume: f64, size: f64) -> f64 {
    volume / (size.powi(3) / 6.0) * 1.5
}

/// The tetrahedra local refinements add, about: the body's bounding box in voxels (at most 64
/// a side), each at the smallest size of the balls that reach it, counted against the uniform
/// `size`.
fn refined_estimate(points: &[Point3], size: f64, refine: &[Refinement]) -> f64 {
    let Some(Aabb { min: lo, max: hi }) = Aabb::around(points.iter().copied()) else { return 0.0 };
    let ext = hi - lo;
    let cell = (ext.x.max(ext.y).max(ext.z) / 64.0).max(f64::MIN_POSITIVE);
    let voxels = Voxels { lo, step: Vec3::new(cell, cell, cell), dims: [ext.x, ext.y, ext.z].map(|e| ((e / cell).ceil() as usize).clamp(1, 64)) };
    let dims = voxels.dims;
    let mut h = vec![size; dims[0] * dims[1] * dims[2]];
    for r in refine {
        let a = [0, 1, 2].map(|d| voxels.index(r.center[d] - r.radius, d));
        let b = [0, 1, 2].map(|d| voxels.index(r.center[d] + r.radius, d));
        for k in a[2]..=b[2] {
            for j in a[1]..=b[1] {
                for i in a[0]..=b[0] {
                    if let Some(v) = h.get_mut((k * dims[1] + j) * dims[0] + i) {
                        *v = v.min(r.size);
                    }
                }
            }
        }
    }
    let voxel = cell.powi(3);
    h.iter().map(|v| tets_in(voxel, *v) - tets_in(voxel, size)).sum::<f64>().max(0.0)
}

/// The most local refinements one mesh takes.
pub const MAX_REFINEMENTS: usize = 200_000;

/// A local refinement: elements of about `size` within `radius` of `center`, the size growing
/// away from it at the mesher's grading rate (adaptive refinement marks the tetrahedra of a
/// previous mesh this way, [`crate::adapt`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Refinement {
    pub center: Point3,
    pub radius: f64,
    pub size: f64,
}

/// The refinements as the mesher's local sizes (balls), checked.
fn local_sizes(refine: &[Refinement]) -> Result<Vec<LocalSize>> {
    if refine.len() > MAX_REFINEMENTS {
        return Err(MeshError::Invalid { what: "refinement".into(), why: format!("{} local sizes; the limit is {MAX_REFINEMENTS}", refine.len()) });
    }
    refine
        .iter()
        .map(|r| {
            if !r.center.is_finite() {
                return Err(MeshError::Invalid { what: "refinement".into(), why: "a centre is not finite".into() });
            }
            positive("refinement size", r.size)?;
            positive("refinement radius", r.radius)?;
            Ok(LocalSize { region: Region::Sphere { center: mesher_point(r.center), radius: r.radius }, size: r.size })
        })
        .collect()
}

/// The element size of each inclusion: its material's for a dielectric, the solid's `size`
/// for a conductor (a hole).
fn inclusion_sizes(model: &Model, sizes: &Sizes, size: f64) -> Result<Vec<f64>> {
    model
        .inclusions
        .iter()
        .map(|inc| match &inc.material {
            Some(name) => Ok(positive("element size", sizes.of(model.material(name)?.0))?),
            None => Ok(size),
        })
        .collect()
}

/// The model's spherical layer, if it has one, checked: no features with it, and a positive
/// inner radius.
fn spherical_layer(model: &Model) -> Result<Option<SphericalLayer>> {
    let layer = model.spherical_layer();
    if let Some(SphericalLayer { inner, .. }) = layer {
        if !model.features.is_empty() {
            return Err(MeshError::Invalid { what: "spherical layer".into(), why: "not supported yet: features with a spherical layer".into() });
        }
        if !(inner > 0.0 && inner.is_finite()) {
            return Err(MeshError::Invalid { what: "spherical layer".into(), why: "its inner radius must be positive".into() });
        }
    }
    Ok(layer)
}

/// [`solid_sized`] refined locally: the size field is at most each refinement's size in its ball,
/// graded away from it.
pub fn solid_refined(model: &Model, sizes: &Sizes, curved: bool, refine: &[Refinement]) -> Result<Mesh> {
    let locals = local_sizes(refine)?;
    let s = model.solid.as_ref().ok_or_else(|| MeshError::Invalid { what: "model".into(), why: "has no solid".into() })?;
    let (material, _) = model.material(&s.material)?;
    let size = positive("element size", sizes.of(material))?;
    let inner_size = inclusion_sizes(model, sizes, size)?;
    let estimate = model.inclusions.iter().zip(&inner_size).fold(tets_in(s.shape.volume(), size), |e, (inc, h)| e + tets_in(inc.shape.volume(), *h));
    too_large(estimate)?;
    // A spherical layer (model.open, shape sphere): the solid's region is the ball inside the
    // layer's inner sphere, its surface an interface (tagged per triangle and frozen, like an
    // inclusion's) with the layer's shell, meshed as a region of its own between it and the
    // solid's sphere — so the shell's tetrahedra are a layer of their own.
    let layer = spherical_layer(model)?;
    let mut main = match layer {
        Some(SphericalLayer { inner, .. }) => {
            let ball = Shape::Sphere { radius: inner };
            placed(tessellate(&ball, interface_size(&ball, size, false)), Point3::ORIGIN)
        }
        None => tessellate(&s.shape, size),
    };
    if !refine.is_empty() {
        too_large(estimate + refined_estimate(&main.positions, size, refine))?;
    }
    for (k, f) in joined(&model.features, &s.shape, size)?.iter().enumerate() {
        main = combined(main, f, k, &s.shape, size)?;
    }
    let body = Body { model, shape: &s.shape, material: material as u32, size, inner_size, locals, shell: layer.is_some() };
    let regions = body.regions(main)?;
    let mut mesh = body.join(&regions)?;
    if curved {
        bend(&mut mesh, model, &s.shape, size);
    }
    crate::check_tetrahedra(&mesh)?;
    mesh.buckets = Some(buckets(&mesh));
    Ok(mesh)
}

/// What a solid is meshed from: the model (its inclusions), the solid's shape, material and
/// element size, each inclusion's size, the local sizes, and whether a spherical layer's shell
/// is a region of its own.
struct Body<'a> {
    model: &'a Model,
    shape: &'a Shape,
    material: u32,
    size: f64,
    inner_size: Vec<f64>,
    locals: Vec<LocalSize>,
    shell: bool,
}

/// A range of face tags: `first..first + count`.
#[derive(Clone, Copy, Debug)]
struct Tags {
    first: u32,
    count: u32,
}

impl Tags {
    fn contains(self, tag: u32) -> bool {
        (self.first..self.first + self.count).contains(&tag)
    }
}

/// The regions of a solid, meshed so that they agree on their interfaces: the outer region (the
/// solid around its inclusions, with a spherical layer the ball inside it), the layer's shell
/// when there is one, then each dielectric inclusion, each with its material.
struct Regions {
    meshes: Vec<(RegionMesh, u32)>,
    /// The solid's faces that are ports.
    start_face: Option<u32>,
    end_face: Option<u32>,
    /// The first tag of the inclusions' interfaces in the outer region (after the solid's own
    /// faces, with a spherical layer the inner sphere's triangles).
    first_tag: u32,
    /// Each inclusion's interface tags in the outer region.
    interfaces: Vec<Tags>,
}

/// `seen` in place of `current` when its triangles differ (corners within 1e-9 of the size);
/// whether it did.
fn adopt(current: &mut Tessellation, seen: Tessellation, size: f64) -> bool {
    let differs = !same_triangles(&seen, current, size);
    if differs {
        *current = seen;
    }
    differs
}

/// The most rounds of meshing the regions again until their interfaces agree.
const MAX_INTERFACE_ROUNDS: usize = 6;

impl Body<'_> {
    /// Meshes the regions: boundary recovery may add points on an interface in one region; that
    /// region's triangulation then becomes the other's input, until both keep the same (frozen)
    /// triangles (at most [`MAX_INTERFACE_ROUNDS`] rounds). `main` is the solid's tessellation
    /// (with a spherical layer, its inner sphere's).
    fn regions(&self, mut main: Tessellation) -> Result<Regions> {
        let (model, size) = (self.model, self.size);
        // The interfaces' current triangulations, oriented out of each inclusion.
        let mut interfaces: Vec<Tessellation> = model
            .inclusions
            .iter()
            .zip(&self.inner_size)
            .map(|(inc, h)| placed(tessellate(&inc.shape, interface_size(&inc.shape, size.min(*h), inc.material.is_none())), inc.at))
            .collect();
        let mut out = Regions { meshes: Vec::new(), start_face: main.start_face, end_face: main.end_face, first_tag: 0, interfaces: Vec::new() };
        for _ in 0..MAX_INTERFACE_ROUNDS {
            // Tags: the solid's faces (with a spherical layer, the inner sphere's triangles), then
            // each interface's triangles in turn.
            out.first_tag = main.tri_face.iter().map(|f| f + 1).max().unwrap_or(0);
            let mut outer = main.clone();
            out.interfaces.clear();
            let mut next = out.first_tag;
            for t in &interfaces {
                let count = t.triangles.len() as u32;
                append(&mut outer, t, next, false);
                out.interfaces.push(Tags { first: next, count });
                next += count;
            }
            let frozen_from = if self.shell { 0 } else { out.first_tag };
            let outer_mesh = region(&outer, size, (frozen_from..next).collect(), &self.locals)?;
            let mut changed = false;
            for (t, tags) in interfaces.iter_mut().zip(&out.interfaces) {
                changed |= adopt(t, extract(&outer_mesh, tags.first, tags.count, false), size);
            }
            // The layer's shell: the solid's sphere outside, the inner sphere as a hole (tags 1..).
            let shell = if self.shell {
                changed |= adopt(&mut main, extract(&outer_mesh, 0, out.first_tag, true), size);
                let mut t = tessellate(self.shape, size);
                append(&mut t, &main, 1, false);
                let n = main.triangles.len() as u32;
                let shell = region(&t, size, (1..1 + n).collect(), &self.locals)?;
                changed |= adopt(&mut main, extract(&shell, 1, n, false), size);
                Some(shell)
            } else {
                None
            };
            out.meshes = vec![(outer_mesh, self.material)];
            out.meshes.extend(shell.map(|m| (m, self.material)));
            for ((inc, t), h) in model.inclusions.iter().zip(&mut interfaces).zip(&self.inner_size) {
                let Some(name) = &inc.material else { continue };
                let (m, _) = model.material(name)?;
                let n = t.triangles.len() as u32;
                let inner = region(t, *h, (0..n).collect(), &self.locals)?;
                changed |= adopt(t, extract(&inner, 0, n, true), size);
                out.meshes.push((inner, m as u32));
            }
            if !changed {
                break;
            }
        }
        Ok(out)
    }

    /// Whether face `face` of region `r` lies on an interface with a dielectric (the outer
    /// region's tags on a dielectric's interface; every face of an inner region): with a
    /// spherical layer, region 1 is its shell (tag 0 its outer wall, the rest the inner sphere)
    /// and the solid region's own faces are that inner sphere.
    fn on_dielectric(&self, regions: &Regions, r: usize, face: u32) -> bool {
        if self.shell && r == 1 {
            return face >= 1;
        }
        r > 0
            || (self.shell && face < regions.first_tag)
            || regions.interfaces.iter().zip(&self.model.inclusions).any(|(tags, inc)| inc.material.is_some() && tags.contains(face))
    }

    /// The regions joined into one mesh: nodes with the same coordinates (the frozen interfaces'
    /// vertices) become one; the outer region's faces are walls or ports, the shell's outer wall
    /// a wall, and every interface triangle must be seen from both sides.
    fn join(&self, regions: &Regions) -> Result<Mesh> {
        let shell = if self.shell { 1 } else { usize::MAX };
        let mut nodes = Nodes::new(1e-9 * self.size);
        let mut tets = Vec::new();
        let mut tet_material = Vec::new();
        let mut boundary = Vec::new();
        let mut interface_sides: std::collections::HashMap<[u32; 3], u32> = std::collections::HashMap::new();
        for (r, (mesh, mat)) in regions.meshes.iter().enumerate() {
            let map: Vec<u32> = mesh.points.iter().map(|p| nodes.id(*p)).collect();
            let first = tets.len() as u32;
            tets.extend(mesh.tets.iter().map(|t| t.map(|v| map[v as usize])));
            tet_material.extend(std::iter::repeat_n(*mat, mesh.tets.len()));
            for &RegionFace { nodes, tet, tag: face } in &mesh.boundary {
                let n = nodes.map(|v| map[v as usize]);
                if self.on_dielectric(regions, r, face) {
                    let mut key = n;
                    key.sort_unstable();
                    *interface_sides.entry(key).or_default() += 1;
                    continue;
                }
                let tag = match r {
                    0 if Some(face) == regions.start_face => Boundary::Port(0),
                    0 if Some(face) == regions.end_face => Boundary::Port(1),
                    0 => Boundary::Pec,
                    r if r == shell => Boundary::Pec,
                    _ => continue,
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
        Ok(Mesh { tet_material, nodes: nodes.points, tets, boundary, grid: None, buckets: None, curved: Vec::new() })
    }
}

/// The nodes of a joined mesh: a point within `tol` of one already there (the regions' snapping
/// to the exact surfaces may differ in the last bits) is that node, found through a hash on
/// cells of that size, checking the neighbours.
struct Nodes {
    tol: f64,
    index: std::collections::HashMap<[i64; 3], u32>,
    points: Vec<Point3>,
}

impl Nodes {
    fn new(tol: f64) -> Nodes {
        Nodes { tol, index: std::collections::HashMap::new(), points: Vec::new() }
    }

    /// The node at `p`, added when there is none.
    fn id(&mut self, p: Point3) -> u32 {
        let tol = self.tol;
        let k = [p.x, p.y, p.z].map(|v| (v / tol).floor() as i64);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if let Some(&i) = self.index.get(&[k[0] + dx, k[1] + dy, k[2] + dz]) {
                        let q = self.points[i as usize];
                        if (q - p).iter().all(|d| d.abs() <= tol) {
                            return i;
                        }
                    }
                }
            }
        }
        self.points.push(p);
        let i = (self.points.len() - 1) as u32;
        self.index.insert(k, i);
        i
    }
}

/// A uniform grid of boxes over space: `dims` boxes of `step` along each axis from `lo`.
struct Voxels {
    lo: Point3,
    step: Vec3,
    dims: [usize; 3],
}

impl Voxels {
    fn of(b: &Buckets) -> Voxels {
        Voxels { lo: b.lo, step: b.step, dims: b.dims }
    }

    /// The box along axis `d` holding coordinate `v`, clamped to the grid.
    fn index(&self, v: f64, d: usize) -> usize {
        (((v - self.lo[d]) / self.step[d]).floor().max(0.0) as usize).min(self.dims[d].saturating_sub(1))
    }
}

/// The bucket along axis `d` of a mesh's buckets holding coordinate `v`, clamped to the grid
/// (for point location and box queries, [`crate::locate`], [`crate::tets_near`]).
pub(crate) fn bucket_index(b: &Buckets, v: f64, d: usize) -> usize {
    Voxels::of(b).index(v, d)
}

/// The tetrahedra binned on a uniform grid of about one tetrahedron per bucket.
pub fn buckets(mesh: &Mesh) -> Buckets {
    let Aabb { min: lo, max: hi } = Aabb::around(mesh.nodes.iter().copied()).unwrap_or_default();
    let ext = (hi - lo).map(|e| e.max(f64::MIN_POSITIVE));
    let per_side = (mesh.tets.len().max(1) as f64).cbrt().ceil() as usize;
    let longest = ext.iter().fold(0.0, f64::max);
    let dims = [0, 1, 2].map(|d| ((ext[d] / longest * per_side as f64).ceil() as usize).clamp(1, 512));
    let step = Vec3::new(ext.x / dims[0] as f64, ext.y / dims[1] as f64, ext.z / dims[2] as f64);
    let grid = Voxels { lo, step, dims };
    let nb = dims[0] * dims[1] * dims[2];
    let mut lists: Vec<Vec<u32>> = vec![Vec::new(); nb];
    for (t, tet) in mesh.tets.iter().enumerate() {
        let mut a = [usize::MAX; 3];
        let mut b = [0usize; 3];
        for &n in tet {
            let p = mesh.nodes[n as usize];
            for d in 0..3 {
                a[d] = a[d].min(grid.index(p[d], d));
                b[d] = b[d].max(grid.index(p[d], d));
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
    use emcraft_model::{Axis, Combine};
    use std::f64::consts::PI;

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

    /// A box [lo, hi] as an imported surface: 12 triangles, a tag per face.
    fn imported_box(name: &str, lo: [f64; 3], hi: [f64; 3]) -> Shape {
        let positions = (0..8).map(|i| Point3::new([lo[0], hi[0]][i & 1], [lo[1], hi[1]][(i >> 1) & 1], [lo[2], hi[2]][(i >> 2) & 1])).collect();
        let quads = [[0, 4, 6, 2], [1, 3, 7, 5], [0, 1, 5, 4], [2, 6, 7, 3], [0, 2, 3, 1], [4, 5, 7, 6]];
        let triangles = quads.iter().flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]]).collect();
        Shape::Surface { name: name.into(), positions, triangles, tags: (0..12).map(|i| i / 2).collect(), surfaces: Vec::new() }
    }

    /// Regression: an imported metal body touching the region's walls (an iris plate against a
    /// side wall and both broad walls) was moved by its millionth of the size, leaving slivers of
    /// air a nanometre thick along the walls and flat tetrahedra the solver refused; imported
    /// bodies were also turned about z as if they were round. Now their flush faces move out of
    /// the walls first: the volume is exact, the ports whole, the mesh small and quick.
    #[test]
    fn imported_bodies_touching_the_walls_are_subtracted() {
        use emcraft_model::Feature;
        let (a, b, l) = (0.02286, 0.01016, 0.032);
        for plates in [vec![0.0], vec![0.0, 0.01743]] {
            let mut m = Model::new();
            m.set_solid(Solid { shape: imported_box("Guide", [0.0; 3], [a, b, l]), material: "vacuum".into() }).unwrap();
            for &x in &plates {
                let shape = imported_box("Iris", [x, 0.0, 0.015], [x + 0.00543, b, 0.017]);
                m.add_feature(Feature { combine: Combine::Subtract, shape, at: Point3::ORIGIN, axis: Axis::Z }).unwrap();
            }
            let start = std::time::Instant::now();
            let mesh = solid(&m, 0.003).unwrap();
            assert!(start.elapsed().as_secs() < 20, "{:?}", start.elapsed());
            // The crossings' points on the walls' straight creases collapsed: no tiny segments.
            assert!(mesh.tets.len() < 4000, "{}", mesh.tets.len());
            let v: f64 = crate::volumes(&mesh).iter().sum();
            let exact = a * b * l - plates.len() as f64 * 0.00543 * b * 0.002;
            assert!((v - exact).abs() < 1e-7 * exact, "{v} {exact}");
            let port = |k: u32| mesh.boundary.iter().filter(|f| f.tag == Boundary::Port(k)).map(|f| area(&mesh, f)).sum::<f64>();
            assert!((port(0) - a * b).abs() < 1e-12 && (port(1) - a * b).abs() < 1e-12);
        }
        // A plate a micrometre off the wall leaves a gap no mesh resolves: refused at once.
        let mut m = Model::new();
        m.set_solid(Solid { shape: imported_box("Guide", [0.0; 3], [a, b, l]), material: "vacuum".into() }).unwrap();
        let shape = imported_box("Iris", [1e-6, 0.0, 0.015], [0.00543, b, 0.017]);
        m.add_feature(Feature { combine: Combine::Subtract, shape, at: Point3::ORIGIN, axis: Axis::Z }).unwrap();
        let e = solid(&m, 0.003).unwrap_err().to_string();
        assert!(e.contains("gap narrower"), "{e}");
    }

    /// A coaxial line's inner conductor in two pieces meeting end to end: the solid (radius
    /// 3.5 mm, 20 mm long) less two cylinders of radius 1 mm and `r2`, at z 0..10 and 10..20 mm.
    fn stepped_coax(r2: f64) -> Model {
        use emcraft_model::Feature;
        let mut m = Model::new();
        m.set_solid(Solid { shape: Shape::Cylinder { radius: 0.0035, length: 0.02 }, material: "vacuum".into() }).unwrap();
        for (radius, z) in [(0.001, 0.0), (r2, 0.01)] {
            m.add_feature(Feature {
                combine: Combine::Subtract,
                shape: Shape::Cylinder { radius, length: 0.01 },
                at: Point3::new(0.0, 0.0, z),
                axis: Axis::Z,
            })
            .unwrap();
        }
        m
    }

    /// Regression: the two cylinders' facing caps, each moved by its own millionth of the size,
    /// left two boundary parts a nanometre apart, and boundary edge recovery split the same few
    /// edges a hair at a time for many minutes. Combined as they are (not joined first), they
    /// now stop at the pass cap with an error, in seconds.
    #[test]
    fn nearly_touching_boundaries_stop_quickly() {
        let (m, size) = (stepped_coax(0.001), 0.0015);
        let s = m.solid.as_ref().unwrap();
        let mut main = tessellate(&s.shape, size);
        for (k, f) in m.features.iter().enumerate() {
            main = combined(main, f, k, &s.shape, size).unwrap();
        }
        let start = std::time::Instant::now();
        let e = region(&main, size, Vec::new(), &[]).err().expect("nearly touching boundaries are refused");
        assert!(e.to_string().contains("nearly touch"), "{e}");
        assert!(start.elapsed().as_secs() < 30, "{:?}", start.elapsed());
    }

    /// The stepped coaxial line meshes within seconds, its volume the exact one but for the
    /// flat facets on the round walls, both ends whole ports.
    fn stepped_coax_meshes(r2: f64, size: f64) {
        let start = std::time::Instant::now();
        let mesh = solid(&stepped_coax(r2), size).unwrap_or_else(|e| panic!("r2 {r2}: {e}"));
        assert!(start.elapsed().as_secs() < 60, "r2 {r2}: {:?}", start.elapsed());
        let v: f64 = crate::volumes(&mesh).iter().sum();
        let hole = |r: f64| PI * r * r * 0.01;
        let exact = PI * 0.0035f64.powi(2) * 0.02 - hole(0.001) - hole(r2);
        assert!((v - exact).abs() < 0.02 * exact, "r2 {r2}: {v} {exact}");
        let port = |k: u32| mesh.boundary.iter().filter(|f| f.tag == Boundary::Port(k)).map(|f| area(&mesh, f)).sum::<f64>();
        assert!(port(0) > 0.0 && port(1) > 0.0, "r2 {r2}");
    }

    /// Regression: features meeting end to end (a stepped inner conductor) used to hang the
    /// mesher. Now with the same cross-section they are one feature; with one inside the other,
    /// the smaller reaches into the larger; overlapping otherwise, they are refused.
    #[test]
    fn features_meeting_end_to_end_mesh() {
        stepped_coax_meshes(0.0015, 0.0015);
        // Boxes face to face: the same rectangle becomes one box.
        use emcraft_model::Feature;
        let mut m = Model::new();
        m.set_solid(Solid { shape: Shape::Box { size: Vec3::new(0.02, 0.01, 0.03) }, material: "vacuum".into() }).unwrap();
        for z in [0.005, 0.01] {
            let f = Feature {
                combine: Combine::Subtract,
                shape: Shape::Box { size: Vec3::new(0.004, 0.004, 0.005) },
                at: Point3::new(0.008, 0.003, z),
                axis: Axis::Z,
            };
            m.add_feature(f).unwrap();
        }
        let v: f64 = crate::volumes(&solid(&m, 0.003).unwrap()).iter().sum();
        let exact = 0.02 * 0.01 * 0.03 - 0.004 * 0.004 * 0.01;
        assert!((v - exact).abs() < 1e-7 * exact, "{v} {exact}");
        // Cross-sections that overlap without one holding the other are refused at once.
        let mut w = Model::new();
        w.set_solid(Solid { shape: Shape::Box { size: Vec3::new(0.02, 0.01, 0.03) }, material: "vacuum".into() }).unwrap();
        for (x, z) in [(0.008, 0.005), (0.010, 0.01)] {
            let f = Feature {
                combine: Combine::Subtract,
                shape: Shape::Box { size: Vec3::new(0.004, 0.004, 0.005) },
                at: Point3::new(x, 0.003, z),
                axis: Axis::Z,
            };
            w.add_feature(f).unwrap();
        }
        let e = solid(&w, 0.003).expect_err("overlapping sections are refused");
        assert!(e.to_string().contains("reach into"), "{e}");
    }

    #[test]
    #[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
    fn features_meeting_end_to_end_mesh_heavy() {
        let size = 0.0015;
        for r2 in [0.001, 0.0005] {
            stepped_coax_meshes(r2, size);
        }
        // One radius: the same mesh as one hole 20 mm long.
        let mut one = stepped_coax(0.001);
        one.features.truncate(1);
        if let Some(Shape::Cylinder { length, .. }) = one.features.first_mut().map(|f| &mut f.shape) {
            *length = 0.02;
        }
        let digest = |m: &Model| {
            let mesh = solid(m, size).unwrap();
            (mesh.nodes.len(), mesh.tets.len(), crate::volumes(&mesh).iter().sum::<f64>())
        };
        assert_eq!(digest(&stepped_coax(0.001)), digest(&one));
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
