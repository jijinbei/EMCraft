//! Curved geometry: the edges of tetrahedra on the model's cylinders and spheres bent onto them
//! (quadratic geometry), interface vertices snapped onto them, and tetrahedra that would fold
//! straightened again.

use emcraft_model::{Axis, Mesh, Model, Point3, Shape, Vec3};

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

/// Bends the mesh onto the model's curved surfaces (the solid's, the inclusions' and the
/// features'): interface vertices snapped onto them, the edges on them curved, and the curved
/// edges of tetrahedra that would fold straightened again.
pub(super) fn bend(mesh: &mut Mesh, model: &Model, shape: &Shape, size: f64) {
    let surfaces: Vec<Curved> = std::iter::once(Curved::of(shape, Point3::ORIGIN, Axis::Z))
        .chain(model.inclusions.iter().map(|i| Curved::of(&i.shape, i.at, Axis::Z)))
        .chain(model.features.iter().map(|f| Curved::of(&f.shape, f.at, f.axis)))
        .flatten()
        .collect();
    // One topology serves both: moving nodes does not change it.
    let topo = crate::Topology::new(mesh);
    snap_interfaces(mesh, &topo, &surfaces, size);
    mesh.curved = curve(mesh, &topo, &surfaces, size);
    unfold(mesh);
}
