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
            _ if port.is_none() && periodic.holds(mesh, &f.nodes) => (sc().periodic, plane_alpha(m, overlay, tet_mat(f.tet))),
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
    if let Some(t) = m.wall_thickness
        && m.open.is_none()
    {
        super::shell::draw(m, overlay, &inside(m, mesh, &surface, &periodic), t, out);
    }
    // Dielectric bodies inside show through the see-through surfaces in every view but the
    // field's.
    if overlay != Overlay::Field {
        material_interfaces(m, mesh, out);
    }
    outline_edges(mesh, &faces, overlay, out);
}

/// The mesh's outer walls, turned out of the inside, and the ports and planes they end on.
fn inside(m: &Model, mesh: &emcraft_model::Mesh, surface: &Surface, periodic: &PeriodicPlanes) -> super::shell::Inside {
    let mut out = super::shell::Inside::default();
    let on_port = super::shell::on_ports(m);
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
        out.tri(p, super::shell::Side::open_if(open));
    }
    out
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
            out.scene.line(p3(a), p3(b), sc().edge, if air { alpha / 3 } else { alpha });
        }
    }
}

/// The mesh cut across `axis` (0 x, 1 y, 2 z) at `at` (m): each tetrahedron the plane crosses
/// gives its cut (a triangle or a quadrilateral) in its material's colour, facing the removed
/// side (`keep_low`: the high side is removed), so a section view shows every material filled,
/// the air too, and conductors (not meshed) in metal behind. Tetrahedra in the mesh's order: the same
/// mesh gives the same cut.
pub(crate) fn slice(m: &Model, axis: usize, at: f64, keep_low: bool) -> emcraft_render::view3d::Scene {
    let mut out = emcraft_render::view3d::Scene::default();
    let Some(mesh) = &m.mesh else { return out };
    let k = axis.min(2);
    let (u, v) = ((k + 1) % 3, (k + 2) % 3);
    let tag = if m.waveguide.is_some() { Pick::Guide.tag() } else { Pick::Solid.tag() };
    // Conductors are not meshed: the mesh's box on the plane, in metal and a hair behind the
    // cut, shows through where no tetrahedron is.
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
    out.tagged(tag, |s| {
        let size = (0..3).map(|c| hi[c] - lo[c]).fold(0.0, f64::max);
        let behind = at + if keep_low { -1e-6 } else { 1e-6 } * size;
        let corner = |a: f64, b: f64| {
            let mut q = [0.0; 3];
            (q[k], q[u], q[v]) = (behind, a, b);
            q
        };
        let mut rect = [corner(lo[u], lo[v]), corner(hi[u], lo[v]), corner(hi[u], hi[v]), corner(lo[u], hi[v])];
        if !keep_low {
            rect.reverse();
        }
        s.tri(rect[0], rect[1], rect[2], sc().metal, 255);
        s.tri(rect[0], rect[2], rect[3], sc().metal, 255);
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
            if !keep_low {
                pts.reverse();
            }
            let colour = material_rgb(m, mesh.tet_material.get(t).copied().unwrap_or(0) as usize);
            for w in 1..pts.len() - 1 {
                s.tri(pts[0], pts[w], pts[w + 1], colour, 255);
            }
        }
    });
    out
}
