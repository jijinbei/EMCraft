//! The model once meshed: the mesh's boundary faces (walls, ports, symmetry planes, periodic
//! faces, the air round an open problem), the faces between materials, and their edges.

use emcraft_model::mesh::Boundary;
use emcraft_model::{End, Model};

use super::style::{EDGE, PERIODIC, material_rgb, plane_alpha, symmetry_rgb, wall_style};
use super::surface::{Kind, PeriodicPlanes, Surface};
use super::{Built, Overlay, Pick, p3};

/// A boundary face for its edges: its nodes, the key whose change draws an edge (a port's end,
/// else 0), and whether it is air.
type Face = ([u32; 3], u32, bool);

/// The mesh's boundary faces and, for materials, the faces between materials.
pub(super) fn from_mesh(m: &Model, overlay: Overlay, out: &mut Built) {
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
        let (color, alpha) = match f.tag {
            // A periodic boundary is no wall either: tied to the opposite face.
            _ if port.is_none() && periodic.holds(mesh, &f.nodes) => (PERIODIC, plane_alpha(m, overlay, tet_mat(f.tet))),
            // A symmetry plane is no wall: its own colour in every overlay.
            Boundary::Symmetry(kind) => (symmetry_rgb(kind), plane_alpha(m, overlay, tet_mat(f.tet))),
            _ => wall_style(m, overlay, port.is_some(), tet_mat(f.tet), kind),
        };
        let pick = port.map(Pick::Port).unwrap_or(if m.waveguide.is_some() { Pick::Guide } else { Pick::Solid });
        if alpha > 0 {
            out.scene.tagged(pick.tag(), |s| s.tri(a, b, c, color, alpha));
        }
        let key = match f.tag {
            Boundary::Port(k) if port.is_some() => 1 + k,
            _ => 0,
        };
        faces.push((f.nodes, key, kind == Kind::Air && port.is_none()));
    }
    // Dielectric bodies inside show through the see-through surfaces in every view but the
    // field's.
    if overlay != Overlay::Field {
        material_interfaces(m, mesh, out);
    }
    outline_edges(mesh, &faces, overlay, out);
}

/// The faces between materials, in the denser one's colour (opaque under the see-through
/// walls): faces two tetrahedra of different materials share.
fn material_interfaces(m: &Model, mesh: &emcraft_model::Mesh, out: &mut Built) {
    let tet_mat = |t: u32| mesh.tet_material.get(t as usize).copied().unwrap_or(0) as usize;
    let eps = |i: usize| m.materials.get(i).map_or(1.0, |x| x.eps_r);
    let node = |n: u32| mesh.nodes.get(n as usize).map(|p| p3(*p));
    let mut keyed: Vec<([u32; 3], u32)> = Vec::with_capacity(mesh.tets.len() * 4);
    for (t, tet) in mesh.tets.iter().enumerate() {
        for skip in 0..4 {
            let mut f = [tet[(skip + 1) % 4], tet[(skip + 2) % 4], tet[(skip + 3) % 4]];
            f.sort_unstable();
            keyed.push((f, t as u32));
        }
    }
    keyed.sort_unstable();
    for w in keyed.windows(2) {
        let (ma, mb) = (tet_mat(w[0].1), tet_mat(w[1].1));
        if w[0].0 != w[1].0 || ma == mb {
            continue;
        }
        let denser = if eps(ma) >= eps(mb) { ma } else { mb };
        if let (Some(a), Some(b), Some(c)) = (node(w[0].0[0]), node(w[0].0[1]), node(w[0].0[2])) {
            out.scene.tri(a, b, c, material_rgb(m, denser), 235);
        }
    }
}

/// Edges of the boundary `faces`: every one for the mesh view, else the outline (sharp folds,
/// and where the faces' keys change).
fn outline_edges(mesh: &emcraft_model::Mesh, faces: &[Face], overlay: Overlay, out: &mut Built) {
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
        Overlay::Mesh => 150,
        Overlay::Materials | Overlay::Ports | Overlay::Boundaries | Overlay::Field => 120,
        Overlay::Plain => 200,
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
            out.scene.line(p3(a), p3(b), EDGE, if air { alpha / 3 } else { alpha });
        }
    }
}
