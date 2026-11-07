//! The model once meshed: the mesh's boundary faces (walls, ports, symmetry planes, periodic
//! faces, the air round an open problem), the faces between materials, and their edges.

use emcraft_model::mesh::Boundary;
use emcraft_model::{End, Model};

use super::style::{material_rgb, plane_alpha, sc, symmetry_rgb, wall_style};
use super::surface::{Kind, PeriodicPlanes, Surface};
use super::{Built, Overlay, Pick, p3};

/// A boundary face for its edges: its nodes, the key whose change draws an edge (a port's end,
/// else 0), and whether it is air.
type Face = ([u32; 3], u32, bool);

/// The mesh's boundary faces and, for materials, the faces between materials.
pub(super) fn from_mesh(session: &emcraft_engine::Session, overlay: Overlay, out: &mut Built, selected_material: Option<usize>) {
    let m = session.model();
    let Some(mesh) = &m.mesh else { return };
    let node = |n: u32| mesh.nodes.get(n as usize).map(|p| p3(*p));
    let tet_mat = |t: u32| mesh.tet_material.get(t as usize).copied().unwrap_or(0) as usize;
    let surface = Surface::of(m, mesh);
    let periodic = PeriodicPlanes::of(m, mesh);
    // The port on an end the mesh tags as one (a port on a face inside the body is drawn
    // there, not on the end): until a port is put there, the end is a wall.
    let end_port = |k: u32| m.ports.iter().position(|p| p.face.is_none() && (p.end == End::Start) == (k == 0));
    let mut faces: Vec<Face> = Vec::with_capacity(mesh.boundary.len());
    for (i, f) in mesh.boundary.iter().enumerate() {
        let (Some(a), Some(b), Some(c)) = (node(f.nodes[0]), node(f.nodes[1]), node(f.nodes[2])) else { continue };
        let port = match f.tag {
            Boundary::Port(k) => end_port(k),
            Boundary::Pec | Boundary::Symmetry(_) => None,
        };
        let kind = surface.kind(i);
        let (mut color, mut alpha) = match f.tag {
            // A periodic boundary is no wall either: tied to the opposite face.
            _ if port.is_none() && periodic.holds(mesh, &f.nodes) => (sc().periodic, plane_alpha(m, overlay, tet_mat(f.tet))),
            // A symmetry plane is no wall: its own colour in every overlay.
            Boundary::Symmetry(kind) => (symmetry_rgb(kind), plane_alpha(m, overlay, tet_mat(f.tet))),
            _ => wall_style(m, overlay, port.is_some(), tet_mat(f.tet), kind),
        };
        // A port is still selectable when its filling is hidden, but must not retain the
        // hidden dielectric's colour and look like a material cap across the empty bore.
        if port.is_some() && m.materials.get(tet_mat(f.tet)).is_some_and(|mat| session.views().hidden_materials.contains(&mat.name)) {
            color = sc().port_soft;
            alpha = 25;
        }
        let pick = port.map_or_else(|| if m.waveguide().is_some() { Pick::Guide } else { Pick::Solid }, Pick::Port);
        let air = kind == Kind::Air && port.is_none();
        if overlay == Overlay::Mesh {
            // The volume mesh's faces with their edges, drawn on the GPU on each triangle (the
            // air's faint: the mesh's sizes inside matter more), the face itself as it is.
            let wire = (super::style::wire_rgb(), if air { WIRE_ALPHA / 3 } else { WIRE_ALPHA });
            out.scene.tagged(pick.tag(), |s| s.wired_tri([a, b, c], color, alpha, wire));
        } else if alpha > 0 {
            out.scene.tagged(pick.tag(), |s| s.tri(a, b, c, color, alpha));
        }
        let key = match f.tag {
            Boundary::Port(k) if port.is_some() => 1 + k,
            _ => 0,
        };
        faces.push((f.nodes, key, air));
    }
    if let Some(t) = m.domain.wall_thickness
        && m.domain.open.is_none()
    {
        let inside = match m.waveguide() {
            Some(_) => {
                let (lo, hi) = mesh.nodes.iter().fold(([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]), |(lo, hi), q| {
                    (std::array::from_fn(|k| lo[k].min(q[k])), std::array::from_fn(|k| hi[k].max(q[k])))
                });
                super::shell::guide_box(m, lo, hi)
            }
            None => inside(m, mesh, &surface, &periodic),
        };
        super::shell::draw(m, overlay, &inside, t.get(), out);
    }
    if overlay != Overlay::Mesh || selected_material.is_some() {
        material_regions(m, mesh, overlay, selected_material, out);
    }
    outline_edges(mesh, &faces, overlay, out);
}

/// The mesh's outer walls, turned out of the inside, and the ports and planes they end on.
fn inside(m: &Model, mesh: &emcraft_model::Mesh, surface: &Surface, periodic: &PeriodicPlanes) -> super::shell::Inside {
    let mut out = super::shell::Inside::default();
    let on_port = super::shell::on_ports(m);
    let interior = super::shell::interior(m);
    for (i, f) in mesh.boundary.iter().enumerate() {
        if surface.kind(i) != Kind::Wall {
            continue;
        }
        let (Some(a), Some(b), Some(c)) =
            (mesh.nodes.get(f.nodes[0] as usize), mesh.nodes.get(f.nodes[1] as usize), mesh.nodes.get(f.nodes[2] as usize))
        else {
            continue;
        };
        // Out of the inside: away from the tetrahedron's fourth corner.
        let fourth = mesh.tets.get(f.tet as usize).and_then(|t| t.iter().find(|n| !f.nodes.contains(n))).and_then(|n| mesh.nodes.get(*n as usize));
        let inward = fourth.is_some_and(|q| (*b - *a).cross(*c - *a).dot(*q - *a) > 0.0);
        let p = if inward { [p3(*a), p3(*c), p3(*b)] } else { [p3(*a), p3(*b), p3(*c)] };
        let centre = [0, 1, 2].map(|k| (p[0][k] + p[1][k] + p[2][k]) / 3.0);
        let open = matches!(f.tag, Boundary::Symmetry(_)) || periodic.holds(mesh, &f.nodes) || on_port(centre);
        out.wall_or_inner(p, super::shell::Side::open_if(open), &interior);
    }
    out
}

/// The complete closed surface of each material region, including faces next to walls.
/// Shared faces within one region are omitted; vacuum is shown only while selected.
fn material_regions(m: &Model, mesh: &emcraft_model::Mesh, overlay: Overlay, selected: Option<usize>, out: &mut Built) {
    let material = |t: u32| mesh.tet_material.get(t as usize).copied().unwrap_or(0) as usize;
    let mut keyed = Vec::with_capacity(mesh.tets.len() * 4);
    for (t, tet) in mesh.tets.iter().enumerate() {
        for skip in 0..4 {
            let mut face = [tet[(skip + 1) % 4], tet[(skip + 2) % 4], tet[(skip + 3) % 4]];
            face.sort_unstable();
            keyed.push((face, t as u32));
        }
    }
    keyed.sort_unstable();
    for group in keyed.chunk_by(|a, b| a.0 == b.0) {
        if group.len() == 2 && material(group[0].1) == material(group[1].1) {
            continue;
        }
        for &(face, tet) in group {
            let mat = material(tet);
            let Some(region) = m.materials.get(mat) else { continue };
            if region.name == "vacuum" && selected != Some(mat) {
                continue;
            }
            let [Some(a), Some(mut b), Some(mut c)] = face.map(|n| mesh.nodes.get(n as usize).copied()) else { continue };
            let fourth = mesh.tets.get(tet as usize).and_then(|t| t.iter().find(|n| !face.contains(n))).and_then(|n| mesh.nodes.get(*n as usize));
            if fourth.is_some_and(|p| (b - a).cross(c - a).dot(*p - a) > 0.0) {
                std::mem::swap(&mut b, &mut c);
            }
            let alpha = match overlay {
                Overlay::Field => 65,
                _ => 220,
            };
            out.scene.tagged(Pick::Material(mat).tag(), |s| s.tri(p3(a), p3(b), p3(c), material_rgb(m, mat), alpha));
        }
    }
}

/// The opacity of the mesh's edges in the Mesh step (the air's a third of it).
const WIRE_ALPHA: u8 = 90;

/// The outline of the boundary `faces` (sharp folds, and where the faces' keys change); the
/// Mesh step draws every edge on its triangles instead ([`emcraft_render::view3d::Scene::wired_tri`]).
fn outline_edges(mesh: &emcraft_model::Mesh, faces: &[Face], overlay: Overlay, out: &mut Built) {
    if overlay == Overlay::Mesh {
        return;
    }
    let node = |n: u32| mesh.nodes.get(n as usize).copied();
    let normal = |i: usize| {
        let f = faces.get(i)?.0;
        let (a, b, c) = (node(f[0])?, node(f[1])?, node(f[2])?);
        (b - a).cross(c - a).normalized()
    };
    let mut edges: Vec<([u32; 2], usize)> = Vec::with_capacity(faces.len() * 3);
    for (i, (f, _, _)) in faces.iter().enumerate() {
        for (a, b) in [(f[0], f[1]), (f[1], f[2]), (f[2], f[0])] {
            edges.push(([a.min(b), a.max(b)], i));
        }
    }
    edges.sort_unstable();
    let alpha = match overlay {
        Overlay::Mesh => 90,
        Overlay::Materials | Overlay::Ports | Overlay::Boundaries | Overlay::Field => 120,
        Overlay::Plain | Overlay::Board => 200,
    };
    let cos_sharp = 25f64.to_radians().cos();
    for group in edges.chunk_by(|x, y| x.0 == y.0) {
        let show = overlay == Overlay::Mesh
            || group.len() != 2
            || faces.get(group[0].1).map(|f| f.1) != faces.get(group[1].1).map(|f| f.1)
            || normal(group[0].1).zip(normal(group[1].1)).is_some_and(|(a, b)| a.dot(b).abs() < cos_sharp);
        // Edges of the air alone stay faint: the mesh's sizes inside matter more.
        let air = group.iter().all(|e| faces.get(e.1).is_some_and(|f| f.2));
        let [a, b] = group[0].0;
        if show && let (Some(a), Some(b)) = (node(a), node(b)) {
            out.scene.line(
                p3(a),
                p3(b),
                if overlay == Overlay::Mesh { super::style::wire_rgb() } else { sc().edge },
                if air { alpha / 3 } else { alpha },
            );
        }
    }
}

/// The mesh cut across `axis` (0 x, 1 y, 2 z) at `at` (m): each tetrahedron the plane crosses
/// gives its polygon perimeter, without a triangulation diagonal or a filled face. Shared
/// edges are drawn once. With `wire`, internal edges show the volume mesh; otherwise only
/// the domain's outline and material boundaries remain. Coordinates are SI metres.
pub(crate) fn slice(m: &Model, axis: usize, at: f64, wire: bool) -> emcraft_render::view3d::Scene {
    let mut out = emcraft_render::view3d::Scene::default();
    let Some(mesh) = &m.mesh else { return out };
    if axis > 2 || !at.is_finite() {
        return out;
    }
    let k = axis;
    let (u, v) = ((k + 1) % 3, (k + 2) % 3);
    // Coordinates relative to the mesh bounds keep the edge keys small and finite.
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for q in mesh.nodes.iter().map(|q| p3(*q)) {
        for c in 0..3 {
            lo[c] = lo[c].min(q[c]);
            hi[c] = hi[c].max(q[c]);
        }
    }
    if !(lo[k] < at && at < hi[k]) {
        return out;
    }
    let size = (0..3).map(|c| hi[c] - lo[c]).fold(0.0, f64::max);
    let key = |p: [f64; 3]| std::array::from_fn::<_, 3, _>(|c| ((p[c] - lo[c]) / size * 1e9).round() as i64);
    type EdgeKey = ([i64; 3], [i64; 3]);
    let mut edges: std::collections::BTreeMap<EdgeKey, ([f64; 3], [f64; 3], usize, usize, bool)> = std::collections::BTreeMap::new();
    for (t, tet) in mesh.tets.iter().enumerate() {
        let Some(p) = tet.iter().map(|n| mesh.nodes.get(*n as usize).map(|q| p3(*q))).collect::<Option<Vec<[f64; 3]>>>() else { continue };
        // Sides by the sign of the distance, the plane itself counted high, so a node on it
        // is met once.
        let d: Vec<f64> = p.iter().map(|q| q[k] - at).collect();
        let mut pts: Vec<[f64; 3]> = Vec::with_capacity(4);
        for i in 0..4 {
            for j in i + 1..4 {
                if (d[i] < 0.0) != (d[j] < 0.0) {
                    let s = d[i] / (d[i] - d[j]);
                    let mut q = [0.0; 3];
                    for c in 0..3 {
                        q[c] = p[i][c] + s * (p[j][c] - p[i][c]);
                    }
                    q[k] = at;
                    pts.push(q);
                }
            }
        }
        // A node on the plane is met by each of its edges: once is enough.
        pts.sort_by(|a, b| a.iter().zip(b).fold(std::cmp::Ordering::Equal, |o, (x, y)| o.then(x.total_cmp(y))));
        pts.dedup();
        if pts.len() < 3 {
            continue;
        }
        // Around the centre in the plane, turning so the face looks at the removed side.
        let n = pts.len() as f64;
        let c = [pts.iter().map(|q| q[u]).sum::<f64>() / n, pts.iter().map(|q| q[v]).sum::<f64>() / n];
        let angle = |q: &[f64; 3]| (q[v] - c[1]).atan2(q[u] - c[0]);
        pts.sort_by(|a, b| angle(a).total_cmp(&angle(b)));
        let material = mesh.tet_material.get(t).copied().unwrap_or(0) as usize;
        for i in 0..pts.len() {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            let (ka, kb) = (key(a), key(b));
            if ka == kb {
                continue;
            }
            let (ka, kb, a, b) = if ka < kb { (ka, kb, a, b) } else { (kb, ka, b, a) };
            let e = edges.entry((ka, kb)).or_insert((a, b, 0, material, false));
            e.2 += 1;
            e.4 |= e.3 != material;
        }
    }
    for (a, b, count, _, interface) in edges.into_values() {
        if wire || count == 1 || interface {
            out.line(a, b, super::style::wire_rgb(), if count == 1 || interface { 255 } else { 210 });
        }
    }
    out
}
