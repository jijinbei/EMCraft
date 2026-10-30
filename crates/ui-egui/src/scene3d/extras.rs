//! What the 3D view draws over the model: ports' faces, outlines and labels, lumped ports,
//! sources, and the open boundary's layer.

use emcraft_model::{End, Model, Source};

use super::geometry::{BOX_EDGES, ROUND, box_corner};
use super::style::sc;
use super::{Built, Label, Overlay, Pick, p3};
use crate::lang::args;

/// Cells across a port's face where its cross-section is sampled (a face that features reach).
const PORT_CELLS: usize = 96;

/// A port's face as triangles: its rectangle, its circle, or — where Boolean features reach the
/// end face, so no closed form describes it — the cells of a grid over the end whose centres lie
/// in the body (a coaxial line's annulus).
pub(super) fn port_face(m: &Model, p: &emcraft_model::Port) -> Vec<[[f64; 3]; 3]> {
    let Some(z) = port_z(m, p) else { return Vec::new() };
    match m.port_shape_of(p) {
        Ok(emcraft_model::PortShape::Rect { x0, y0, a, b }) => {
            let q = [[x0, y0, z], [x0 + a, y0, z], [x0 + a, y0 + b, z], [x0, y0 + b, z]];
            vec![[q[0], q[1], q[2]], [q[0], q[2], q[3]]]
        }
        Ok(emcraft_model::PortShape::Circle { cx, cy, r }) => {
            let at = |k: usize| {
                let a = std::f64::consts::TAU * k as f64 / ROUND as f64;
                [cx + r * a.cos(), cy + r * a.sin(), z]
            };
            (0..ROUND).map(|k| [[cx, cy, z], at(k), at(k + 1)]).collect()
        }
        Err(_) => {
            let Some(b) = m.bounds() else { return Vec::new() };
            // Just inside the body from the end face.
            let inside = z + if p.end == End::Start { 1e-6 } else { -1e-6 } * (b.max.z - b.min.z).max(1e-9);
            let (dx, dy) = ((b.max.x - b.min.x) / PORT_CELLS as f64, (b.max.y - b.min.y) / PORT_CELLS as f64);
            let mut out = Vec::new();
            for j in 0..PORT_CELLS {
                for i in 0..PORT_CELLS {
                    let (x, y) = (b.min.x + dx * i as f64, b.min.y + dy * j as f64);
                    if m.material_at(emcraft_model::Point3::new(x + dx / 2.0, y + dy / 2.0, inside)).is_some() {
                        let q = [[x, y, z], [x + dx, y, z], [x + dx, y + dy, z], [x, y + dy, z]];
                        out.push([q[0], q[1], q[2]]);
                        out.push([q[0], q[2], q[3]]);
                    }
                }
            }
            out
        }
    }
}

/// The edges of a set of triangles that only one of them has: their outline.
fn outline(tris: &[[[f64; 3]; 3]]) -> Vec<([f64; 3], [f64; 3])> {
    let key = |p: [f64; 3]| p.map(|c| (c * 1e9).round() as i64);
    let mut edges: Vec<([i64; 3], [i64; 3], [f64; 3], [f64; 3])> = Vec::with_capacity(tris.len() * 3);
    for t in tris {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let (ka, kb) = (key(a), key(b));
            edges.push(if ka <= kb { (ka, kb, a, b) } else { (kb, ka, b, a) });
        }
    }
    edges.sort_by_key(|e| (e.0, e.1));
    edges.chunk_by(|x, y| (x.0, x.1) == (y.0, y.1)).filter(|g| g.len() == 1).map(|g| (g[0].2, g[0].3)).collect()
}

/// The plane of a port's face.
fn port_z(m: &Model, p: &emcraft_model::Port) -> Option<f64> {
    Some(match (p.face, p.end) {
        (Some(f), _) => f.z,
        (None, End::Start) => m.bounds()?.min.z.min(0.0),
        (None, End::End) => m.bounds()?.max.z,
    })
}

/// The direction from a port's face into the model, along z: up from a port at the start (or
/// on a face with the guide above it), down from one at the end.
pub(crate) fn port_inward(p: &emcraft_model::Port) -> f64 {
    if p.end == End::Start { 1.0 } else { -1.0 }
}

/// A port's face's bounding rectangle in 3D.
pub(crate) fn port_quad(m: &Model, p: &emcraft_model::Port) -> Option<[[f64; 3]; 4]> {
    let face = port_face(m, p);
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for q in face.iter().flatten() {
        for k in 0..2 {
            lo[k] = lo[k].min(q[k]);
            hi[k] = hi[k].max(q[k]);
        }
    }
    let z = face.first()?[0][2];
    Some([[lo[0], lo[1], z], [hi[0], lo[1], z], [hi[0], hi[1], z], [lo[0], hi[1], z]])
}

/// Ports' labels, lumped ports, sources and the open boundary's layer.
pub(super) fn extras(m: &Model, overlay: Overlay, out: &mut Built) {
    let size = out.bounds.map_or(0.01, |(lo, hi)| ((hi[0] - lo[0]).powi(2) + (hi[1] - lo[1]).powi(2) + (hi[2] - lo[2]).powi(2)).sqrt());
    if matches!(overlay, Overlay::Ports | Overlay::Plain | Overlay::Boundaries) {
        ports(m, overlay, size, out);
    }
    let strong = overlay == Overlay::Ports;
    for (i, p) in m.lumped.iter().enumerate() {
        let (a, b) = (p3(p.from), p3(p.to));
        out.scene.line(a, b, sc().port, 255);
        // A small box round the gap's middle to pick it by.
        let (c, h) = (midpoint(a, b), 0.012 * size);
        out.scene.tagged(Pick::Lumped(i).tag(), |s| s.cuboid(c.map(|v| v - h), c.map(|v| v + h), sc().port, 255));
        if strong {
            out.labels.push(Label {
                at: midpoint(a, b),
                text: crate::lang::trf("Lumped port {name}", args!(name = p.name)),
                color: sc().port,
                facing: None,
                anchor: None,
            });
        }
    }
    for s in &m.sources {
        sources(s, strong, size, out);
    }
    open_boundary(m, overlay, out);
}

fn midpoint(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [0.5 * (a[0] + b[0]), 0.5 * (a[1] + b[1]), 0.5 * (a[2] + b[2])]
}

/// `a` moved by `k` along `d`.
fn along(a: [f64; 3], d: [f64; 3], k: f64) -> [f64; 3] {
    [a[0] + d[0] * k, a[1] + d[1] * k, a[2] + d[2] * k]
}

/// The ports' labels and, when ports are the subject, their outlines and the direction into
/// the guide.
fn ports(m: &Model, overlay: Overlay, size: f64, out: &mut Built) {
    for p in &m.ports {
        let Some(q) = port_quad(m, p) else { continue };
        let c = [0.25 * (q[0][0] + q[1][0] + q[2][0] + q[3][0]), 0.25 * (q[0][1] + q[1][1] + q[2][1] + q[3][1]), q[0][2]];
        let facing = match (p.face, p.end) {
            (Some(_), _) => None,
            (None, End::Start) => Some([0.0, 0.0, -1.0]),
            (None, End::End) => Some([0.0, 0.0, 1.0]),
        };
        // The name stands off the face, outside the model, on a leader: the face's arrow points
        // the other way, into the model.
        let out_of = [c[0], c[1], c[2] - port_inward(p) * 0.15 * size];
        out.labels.push(Label { at: out_of, text: crate::lang::trf("Port {name}", args!(name = p.name)), color: sc().port, facing, anchor: Some(c) });
        if overlay == Overlay::Ports {
            for (a, b) in outline(&port_face(m, p)) {
                out.scene.line(a, b, sc().port, 255);
            }
        }
    }
}

/// A source: a dipole as a short line along its direction, a plane wave as a line coming in
/// towards the model's middle.
fn sources(s: &Source, strong: bool, size: f64, out: &mut Built) {
    match *s {
        Source::Dipole { at, direction, .. } => {
            let (a, d) = (p3(at), [direction.x, direction.y, direction.z]);
            let h = 0.08 * size;
            out.scene.line(along(a, d, -h), along(a, d, h), sc().source, 255);
            if strong {
                out.labels.push(Label { at: a, text: crate::lang::tr("Dipole").into(), color: sc().source, facing: None, anchor: None });
            }
        }
        Source::PlaneWave { direction, .. } => {
            if let Some((lo, hi)) = out.bounds {
                let c = midpoint(lo, hi);
                let d = [direction.x, direction.y, direction.z];
                let h = 0.5 * size;
                let a = along(c, d, -h);
                out.scene.line(a, [c[0] - d[0] * 0.3 * h, c[1] - d[1] * 0.3 * h, c[2] - d[2] * 0.3 * h], sc().source, 255);
                if strong {
                    out.labels.push(Label { at: a, text: crate::lang::tr("Plane wave").into(), color: sc().source, facing: None, anchor: None });
                }
            }
        }
    }
}

/// The open boundary: the layer's inner face as a frame. When boundaries are the subject, the
/// layer's thickness is shaded as a band round each outer face (a picture frame), so what lies
/// inside stays clear, and its corners join the inner frame's.
fn open_boundary(m: &Model, overlay: Overlay, out: &mut Built) {
    let (Some(o), Some((lo, hi))) = (&m.open, out.bounds) else { return };
    let t = o.thickness;
    let (a, b) = ([lo[0] + t, lo[1] + t, lo[2] + t], [hi[0] - t, hi[1] - t, hi[2] - t]);
    if overlay == Overlay::Boundaries {
        for axis in 0..3 {
            let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
            for side in [lo[axis], hi[axis]] {
                // The face's band: four strips t wide along its edges.
                let at = |x: f64, y: f64| {
                    let mut p = [0.0; 3];
                    p[axis] = side;
                    p[u] = x;
                    p[v] = y;
                    p
                };
                let (u0, u1, v0, v1) = (lo[u], hi[u], lo[v], hi[v]);
                let (ui0, ui1, vi0, vi1) = (u0 + t, u1 - t, v0 + t, v1 - t);
                for (x0, x1, y0, y1) in [(u0, u1, v0, vi0), (u0, u1, vi1, v1), (u0, ui0, vi0, vi1), (ui1, u1, vi0, vi1)] {
                    out.scene.quad([at(x0, y0), at(x1, y0), at(x1, y1), at(x0, y1)], sc().pml, 70);
                }
            }
        }
        for i in 0..8 {
            out.scene.line(box_corner(lo, hi, i), box_corner(a, b, i), sc().pml, 200);
        }
        // On the band, at a corner of the outer box.
        out.labels.push(Label { at: [hi[0], hi[1], lo[2] + 0.5 * t], text: "PML".into(), color: sc().pml, facing: None, anchor: None });
    }
    for (i, j) in BOX_EDGES {
        out.scene.line(box_corner(a, b, i), box_corner(a, b, j), sc().pml, 255);
    }
}
