//! Curved geometry: the edges of tetrahedra on the model's cylinders and spheres bent onto them
//! (quadratic geometry), interface vertices snapped onto them, and tetrahedra that would fold
//! straightened again.

use emcraft_model::{Aabb, Axis, FaceSurface, Mesh, Model, Point3, Shape, Vec3};

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
    /// A curved face of an imported surface, the surface fitted to it on import: only triangles
    /// within `within` (the face's vertices' box, grown by the arcs' bulge) lie on it.
    Fitted {
        surface: FaceSurface,
        within: Aabb,
    },
}

impl Curved {
    /// A shape's curved surfaces, placed at `at` with its z axis along `axis`.
    fn of(shape: &Shape, at: Point3, axis: Axis) -> Vec<Curved> {
        match shape {
            Shape::Box { .. } => Vec::new(),
            Shape::Cylinder { radius, .. } => vec![Curved::Cylinder { c: at, axis: axis.index(), r: *radius }],
            Shape::Sphere { radius } => vec![Curved::Sphere { c: at, r: *radius }],
            Shape::Surface { .. } => fitted(shape, at, axis),
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
            Curved::Fitted { surface, .. } => surface.project(p).unwrap_or(p),
        }
    }

    /// Whether a triangle with this unit normal can lie on the surface (a cylinder's caps share
    /// its rim but face along the axis).
    fn faces(&self, normal: Vec3) -> bool {
        match self {
            Curved::Cylinder { axis, .. } => normal[*axis].abs() < 0.5,
            Curved::Sphere { .. } | Curved::Fitted { .. } => true,
        }
    }

    /// Whether triangle `p` (unit normal `normal`) lies on the surface: [`Curved::faces`], and
    /// for a fitted face, the triangle within the face's box and facing along the surface's
    /// normal at its centroid (the untrimmed surface runs on past the face, and its neighbours
    /// may come close).
    fn holds(&self, p: &[Point3; 3], normal: Vec3) -> bool {
        match self {
            Curved::Fitted { surface, within } => {
                let on_face = p.iter().all(|q| within.contains(*q));
                let along = Point3::centroid(*p).and_then(|g| surface.normal(g)).is_some_and(|n| n.dot(normal).abs() > 0.7);
                on_face && along
            }
            _ => self.faces(normal),
        }
    }
}

/// The curved faces of an imported surface with the surfaces fitted to them, placed at `at`
/// with its z axis along `axis`.
fn fitted(shape: &Shape, at: Point3, axis: Axis) -> Vec<Curved> {
    let Shape::Surface { positions, triangles, tags, surfaces, .. } = shape else { return Vec::new() };
    let place = |p: Point3| at + axis.turn(p.to_vec());
    let mut out = Vec::new();
    for (face, f) in surfaces.iter().enumerate() {
        let Some(f) = f.filter(FaceSurface::is_curved) else { continue };
        // The face's vertices' box, grown by its longest edge's share: an arc between two
        // vertices bulges past them by its sagitta (at most an eighth of the chord for the
        // angles a fit accepts).
        let mut corners = Vec::new();
        let mut longest = 0.0f64;
        for (t, _) in triangles.iter().zip(tags).filter(|(_, g)| **g as usize == face) {
            let [a, b, c] = t.map(|v| positions.get(v as usize).copied().unwrap_or(Point3::ORIGIN));
            longest = longest.max(a.distance(b)).max(b.distance(c)).max(c.distance(a));
            corners.extend([a, b, c]);
        }
        let Some(b) = Aabb::around(corners.into_iter().map(place)) else { continue };
        let surface = match f {
            FaceSurface::Plane { normal, point } => FaceSurface::Plane { normal: axis.turn(normal), point: place(point) },
            FaceSurface::Cylinder { origin, axis: a, radius } => FaceSurface::Cylinder { origin: place(origin), axis: axis.turn(a), radius },
            FaceSurface::Sphere { center, radius } => FaceSurface::Sphere { center: place(center), radius },
            FaceSurface::Cone { apex, axis: a, half_angle } => FaceSurface::Cone { apex: place(apex), axis: axis.turn(a), half_angle },
        };
        out.push(Curved::Fitted { surface, within: b.grown(0.15 * longest + 1e-9 * b.size().norm()) });
    }
    out
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
    surfaces.iter().find(|s| s.holds(p, normal) && p.iter().all(|q| s.distance(*q) < 0.2 * size))
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
    let surfaces: Vec<Curved> = Curved::of(shape, Point3::ORIGIN, Axis::Z)
        .into_iter()
        .chain(model.inclusions.iter().flat_map(|i| Curved::of(&i.shape, i.at, Axis::Z)))
        .chain(model.features.iter().flat_map(|f| Curved::of(&f.shape, f.at, f.axis)))
        .collect();
    // One topology serves both: moving nodes does not change it.
    let topo = crate::Topology::new(mesh);
    snap_interfaces(mesh, &topo, &surfaces, size);
    mesh.curved = curve(mesh, &topo, &surfaces, size);
    unfold(mesh);
}

#[cfg(test)]
mod tests {
    use emcraft_model::{Combine, FaceSurface, Feature, Model, Point3, Shape, Solid, Vec3};

    use crate::solid::solid_with;

    /// An imported closed cylinder (axis z through (cx, cy), radius r, z from 0 to l): `n`
    /// facets round, the side in two halves (tags 0, 1) as a CAD kernel splits it at its seam,
    /// the caps tags 2 and 3; with the fitted surfaces when `fitted`.
    fn imported_cylinder(c: [f64; 2], r: f64, l: f64, n: usize, fitted: bool) -> Shape {
        let mut positions = Vec::new();
        for z in [0.0, l] {
            for i in 0..n {
                let t = 2.0 * std::f64::consts::PI * i as f64 / n as f64;
                positions.push(Point3::new(c[0] + r * t.cos(), c[1] + r * t.sin(), z));
            }
        }
        positions.push(Point3::new(c[0], c[1], 0.0));
        positions.push(Point3::new(c[0], c[1], l));
        let (n32, bottom, top) = (n as u32, 2 * n as u32, 2 * n as u32 + 1);
        let (mut triangles, mut tags) = (Vec::new(), Vec::new());
        for i in 0..n32 {
            let j = (i + 1) % n32;
            let half = if 2 * i < n32 { 0 } else { 1 };
            triangles.extend([[i, j, n32 + j], [i, n32 + j, n32 + i], [bottom, j, i], [top, n32 + i, n32 + j]]);
            tags.extend([half, half, 2, 3]);
        }
        let side = FaceSurface::Cylinder { origin: Point3::new(c[0], c[1], 0.0), axis: Vec3::new(0.0, 0.0, 1.0), radius: r };
        let surfaces = if fitted {
            vec![
                Some(side),
                Some(side),
                Some(FaceSurface::Plane { normal: Vec3::new(0.0, 0.0, -1.0), point: Point3::ORIGIN }),
                Some(FaceSurface::Plane { normal: Vec3::new(0.0, 0.0, 1.0), point: Point3::new(0.0, 0.0, l) }),
            ]
        } else {
            Vec::new()
        };
        Shape::Surface { name: "can".into(), positions, triangles, tags, surfaces }
    }

    #[test]
    fn imported_curved_faces_follow_their_fitted_surfaces() {
        let (r, l, size) = (0.01, 0.02, 0.003);
        let rho = |p: Point3| p.x.hypot(p.y);
        let mesh_of = |fitted: bool| {
            let mut m = Model::new();
            m.set_solid(Solid { shape: imported_cylinder([0.0, 0.0], r, l, 26, fitted), material: "vacuum".into() }).unwrap();
            solid_with(&m, size, true).unwrap()
        };
        let mesh = mesh_of(true);
        // Every boundary node off the caps on the cylinder (the mesher's added ones too), every
        // curved midpoint on it.
        let side: Vec<Point3> = mesh
            .boundary
            .iter()
            .flat_map(|f| f.nodes)
            .map(|n| mesh.nodes[n as usize])
            .filter(|p| p.z.abs() > 1e-12 && (p.z - l).abs() > 1e-12)
            .collect();
        assert!(side.len() > 26);
        assert!(side.iter().all(|p| (rho(*p) - r).abs() < 1e-12 * r), "a side node off the cylinder");
        assert!(mesh.curved.len() > 50);
        assert!(mesh.curved.iter().all(|(_, p)| (rho(*p) - r).abs() < 1e-12 * r));
        // As the native cylinder's: about the same edges bent.
        let mut native = Model::new();
        native.set_solid(Solid { shape: Shape::Cylinder { radius: r, length: l }, material: "vacuum".into() }).unwrap();
        let native = solid_with(&native, size, true).unwrap();
        let ratio = mesh.curved.len() as f64 / native.curved.len() as f64;
        assert!(ratio > 0.7 && ratio < 1.4, "{} curved edges against {}", mesh.curved.len(), native.curved.len());
        // Faceted without the fit: nothing bent, and the mesher's points on the facets.
        let flat = mesh_of(false);
        assert!(flat.curved.is_empty());
        assert!(flat.boundary.iter().flat_map(|f| f.nodes).any(|n| rho(flat.nodes[n as usize]) < r * (1.0 - 1e-4)));
    }

    #[test]
    fn an_imported_metal_post_is_bent_onto_its_cylinder() {
        // WR-90 with an imported post (a feature) subtracted, standing along z away from the ends.
        let (a, b, l, r) = (0.02286, 0.01016, 0.03, 0.0015);
        let mut m = Model::new();
        m.set_solid(Solid { shape: Shape::Box { size: Vec3::new(a, b, l) }, material: "vacuum".into() }).unwrap();
        let mut post = imported_cylinder([a / 2.0, b / 2.0], r, 0.01, 26, true);
        if let Shape::Surface { positions, surfaces, .. } = &mut post {
            positions.iter_mut().for_each(|p| p.z += 0.01);
            *surfaces = surfaces.iter().map(|f| f.map(|f| f.moved(Vec3::new(0.0, 0.0, 0.01)))).collect();
        }
        m.add_feature(Feature { combine: Combine::Subtract, shape: post, at: Point3::ORIGIN, axis: emcraft_model::Axis::Z }).unwrap();
        let mesh = solid_with(&m, 0.003, true).unwrap();
        let rho = |p: Point3| (p.x - a / 2.0).hypot(p.y - b / 2.0);
        assert!(mesh.curved.len() > 20);
        assert!(mesh.curved.iter().all(|(_, p)| (rho(*p) - r).abs() < 1e-9), "a midpoint off the post");
        // The hole's wall is the post's: every boundary node off the guide's walls and the
        // post's ends on the cylinder.
        let wall = |p: Point3| {
            [p.x, p.y, p.z].iter().zip([a, b, l]).any(|(v, s)| v.abs() < 1e-8 || (v - s).abs() < 1e-8)
                || (p.z - 0.01).abs() < 1e-8
                || (p.z - 0.02).abs() < 1e-8
        };
        let hole: Vec<Point3> = mesh.boundary.iter().flat_map(|f| f.nodes).map(|n| mesh.nodes[n as usize]).filter(|p| !wall(*p)).collect();
        assert!(hole.len() > 26);
        for p in hole {
            assert!((rho(p) - r).abs() < 1e-9 * r + 1e-8, "{p:?} is {} from the axis", rho(p));
        }
    }
}
