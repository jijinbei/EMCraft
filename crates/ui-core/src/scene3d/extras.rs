//! What the 3D view draws over the model: ports' faces, outlines and labels (the driven port
//! named the input, the others outputs, each in its role's colour), lumped ports, sources, and
//! the open boundary's layer.

use emcraft_model::{End, Model, Source};

use super::geometry::{BOX_EDGES, ROUND, box_corner};
use super::style::Colours;
use super::{Built, Label, Overlay, Pick, p3};
use crate::lang::args;

/// Cells across a port's face where its cross-section is sampled (a face that features reach).
const PORT_CELLS: usize = 96;

/// A port's face as triangles: its rectangle, its circle, or — where Boolean features reach the
/// end face, so no closed form describes it — the cells of a grid over the end whose centres lie
/// in the body (a coaxial line's annulus). Shapes are in the port's own axes (u, v), turned onto
/// its face.
pub(super) fn port_face(m: &Model, p: &emcraft_model::Port) -> Vec<[[f64; 3]; 3]> {
    let Some(z) = port_z(m, p) else { return Vec::new() };
    let frame = p.frame();
    let w = frame.axes().sign * z;
    let on = |u: f64, v: f64| p3(frame.global(emcraft_model::Point3::new(u, v, w)));
    match m.port_shape_of(p) {
        Ok(emcraft_model::PortShape::Rect { x0, y0, a, b }) => {
            let q = [on(x0, y0), on(x0 + a, y0), on(x0 + a, y0 + b), on(x0, y0 + b)];
            vec![[q[0], q[1], q[2]], [q[0], q[2], q[3]]]
        }
        Ok(emcraft_model::PortShape::Circle { cx, cy, r }) => {
            let at = |k: usize| {
                let a = std::f64::consts::TAU * k as f64 / ROUND as f64;
                on(cx + r * a.cos(), cy + r * a.sin())
            };
            (0..ROUND).map(|k| [on(cx, cy), at(k), at(k + 1)]).collect()
        }
        Err(_) => surface_face(m, &on, frame.axis.index()).unwrap_or_else(|| grid_face(m, p, z)),
    }
}

/// How close to a port's plane (relative to the model's size) an imported surface's vertex
/// counts as on it.
const ON_PLANE: f64 = 1e-7;

/// An imported region's own triangles in a port's plane (the opening's cap), each turned to
/// face along the port's axis as the grid's cells do; `None` unless the region is one surface
/// with nothing combined into it, or when none lie there. `on` places the port's (u, v) on its
/// plane, `k` its axis.
fn surface_face(m: &Model, on: &dyn Fn(f64, f64) -> [f64; 3], k: usize) -> Option<Vec<[[f64; 3]; 3]>> {
    let solid = m.domain.solid.as_ref()?;
    let emcraft_model::Shape::Surface { positions, triangles, .. } = &solid.shape else { return None };
    if !m.features().is_empty() || !m.inclusions().is_empty() {
        return None;
    }
    let b = m.bounds()?;
    let tol = ON_PLANE * (b.max - b.min).norm();
    let at = on(0.0, 0.0)[k];
    let point = |v: u32| positions.get(v as usize).map(|q| p3(*q));
    let face: Vec<[[f64; 3]; 3]> = triangles
        .iter()
        .filter_map(|t| {
            let [a, b, c] = [point(t[0])?, point(t[1])?, point(t[2])?];
            if ![a, b, c].iter().all(|q| (q[k] - at).abs() <= tol) {
                return None;
            }
            // (b − a) × (c − a) along the axis: positive keeps the order, else two corners swap.
            let (i, j) = ((k + 1) % 3, (k + 2) % 3);
            let normal = (b[i] - a[i]) * (c[j] - a[j]) - (b[j] - a[j]) * (c[i] - a[i]);
            Some(if normal >= 0.0 { [a, b, c] } else { [a, c, b] })
        })
        .collect();
    (!face.is_empty()).then_some(face)
}

/// The cells of a grid over a port's end whose centres lie in the body: a face no closed form
/// describes, where Boolean features reach it.
fn grid_face(m: &Model, p: &emcraft_model::Port, z: f64) -> Vec<[[f64; 3]; 3]> {
    let Some(b) = m.bounds() else { return Vec::new() };
    // Just inside the body from the end face.
    let inside = z + port_inward(p) * 1e-6 * (b.max.z - b.min.z).max(1e-9);
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

/// The plane of a port's face: its coordinate along the port's axis.
fn port_z(m: &Model, p: &emcraft_model::Port) -> Option<f64> {
    Some(match (p.face, p.end) {
        (Some(f), _) => f.at,
        (None, End::Start) => m.bounds()?.min.z.min(0.0),
        (None, End::End) => m.bounds()?.max.z,
    })
}

/// The direction from a port's face into the model along its axis: up from a port at the start
/// (or on a face with the guide above it), down from one at the end.
pub fn port_inward(p: &emcraft_model::Port) -> f64 {
    if p.end == End::Start { 1.0 } else { -1.0 }
}

/// The unit vector from a port's face into the model (its axis, signed by [`port_inward`]).
pub fn port_normal(p: &emcraft_model::Port) -> [f64; 3] {
    let mut n = [0.0; 3];
    n[p.frame().axis.index()] = port_inward(p);
    n
}

/// A port's face's bounding rectangle in 3D, counter-clockwise in its own axes (u, v).
pub fn port_quad(m: &Model, p: &emcraft_model::Port) -> Option<[[f64; 3]; 4]> {
    let face = port_face(m, p);
    let frame = p.frame();
    let emcraft_model::port::FrameAxes { u, v, .. } = frame.axes();
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for q in face.iter().flatten() {
        for (k, c) in [u, v].into_iter().enumerate() {
            lo[k] = lo[k].min(q[c]);
            hi[k] = hi[k].max(q[c]);
        }
    }
    let first = face.first()?[0];
    let corner = |a: f64, b: f64| {
        let mut q = first;
        q[u] = a;
        q[v] = b;
        q
    };
    Some([corner(lo[0], lo[1]), corner(hi[0], lo[1]), corner(hi[0], hi[1]), corner(lo[0], hi[1])])
}

/// The centre of a quadrilateral (the mean of its corners).
pub fn quad_centre(q: &[[f64; 3]; 4]) -> [f64; 3] {
    [0, 1, 2].map(|k| 0.25 * (q[0][k] + q[1][k] + q[2][k] + q[3][k]))
}

/// The symmetry planes, meshed or not: each a faint rectangle across the model's box, framed, in
/// its wall's colour (the model is drawn whole either way, so the mesh's own faces on the plane,
/// which follow the model's cut, are not drawn); a plane hidden by its eye is left out.
pub(super) fn symmetry_planes(colours: &Colours, session: &emcraft_engine::Session, bounds: Option<([f64; 3], [f64; 3])>) -> Built {
    let mut out = Built::default();
    use emcraft_engine::visibility::{DisplayObject, GeometryObject};
    let m = session.model();
    let Some((lo, hi)) = bounds else { return out };
    for s in &m.domain.symmetry {
        if !session.views().visible(&DisplayObject::Geometry(GeometryObject::Symmetry(s.axis))) {
            continue;
        }
        let k = s.axis.index();
        let (u, v) = ((k + 1) % 3, (k + 2) % 3);
        let at = |a: f64, b: f64| {
            let mut p = [0.0; 3];
            p[k] = s.at;
            p[u] = a;
            p[v] = b;
            p
        };
        let q = [at(lo[u], lo[v]), at(hi[u], lo[v]), at(hi[u], hi[v]), at(lo[u], hi[v])];
        let color = super::style::symmetry_rgb(colours, s.kind);
        // A plane a guide's strips lie in is its outline alone: its fill would fight them.
        let size = (0..3).map(|i| hi[i] - lo[i]).fold(0.0, f64::max).max(1e-9);
        let strips = m.waveguide().is_some_and(|w| k == 1 && w.strips.iter().any(|st| (st.y - s.at).abs() <= 1e-6 * size));
        if !strips {
            out.scene.tri(q[0], q[1], q[2], color, 80);
            out.scene.tri(q[0], q[2], q[3], color, 80);
        }
        for i in 0..4 {
            out.scene.line(q[i], q[(i + 1) % 4], color, 235);
        }
    }
    out
}

/// A rotational symmetry's sector, drawn faint as its two cut half-planes from the axis out to
/// the model's extent: the faces tied by the harmonic's phase.
pub(super) fn sector_cuts(colours: &Colours, m: &Model, bounds: Option<([f64; 3], [f64; 3])>) -> Built {
    let mut out = Built::default();
    let (Some(r), Some((lo, hi))) = (m.domain.rotation, bounds) else { return out };
    let reach = [lo[0], hi[0]].iter().flat_map(|x| [lo[1], hi[1]].map(|y| (x - r.centre[0]).hypot(y - r.centre[1]))).fold(0.0, f64::max);
    let colour = colours.scene.periodic;
    for phi in [r.start(), r.start() + r.angle()] {
        let (s, c) = phi.sin_cos();
        let (x, y) = (r.centre[0] + reach * c, r.centre[1] + reach * s);
        let q = [[r.centre[0], r.centre[1], lo[2]], [x, y, lo[2]], [x, y, hi[2]], [r.centre[0], r.centre[1], hi[2]]];
        out.scene.tri(q[0], q[1], q[2], colour, 80);
        out.scene.tri(q[0], q[2], q[3], colour, 80);
        for i in 0..4 {
            out.scene.line(q[i], q[(i + 1) % 4], colour, 235);
        }
    }
    out
}

/// Ports' labels (`driven`: the input's index), lumped ports, sources and the open boundary's
/// layer.
pub(super) fn extras(colours: &Colours, m: &Model, driven: Option<usize>, overlay: Overlay, bounds: Option<([f64; 3], [f64; 3])>) -> Built {
    let mut out = Built::default();
    let size = bounds.map_or(0.01, |(lo, hi)| ((hi[0] - lo[0]).powi(2) + (hi[1] - lo[1]).powi(2) + (hi[2] - lo[2]).powi(2)).sqrt());
    if matches!(overlay, Overlay::Ports | Overlay::Plain | Overlay::Boundaries) {
        out = out.merge(ports(colours, m, driven, overlay, size));
    }
    // World-space glyphs, depth tested and shaded by WGSL, for every workflow overlay.
    for (i, p) in m.ports.iter().enumerate() {
        let Some(q) = port_quad(m, p) else { continue };
        let c = quad_centre(&q);
        let frame = p.frame();
        let emcraft_model::port::FrameAxes { u, v, .. } = frame.axes();
        let width = (q[2][u] - q[0][u]).abs();
        let height = (q[2][v] - q[0][v]).abs();
        let len = (0.6 * width.max(height)).min(0.25 * size).max(1e-9);
        let mut side = [0.0; 3];
        side[u] = 1.0;
        let n = port_normal(p);
        let arrow = |origin, from, to, color| super::Arrow {
            start: along(origin, n, from * len),
            end: along(origin, n, to * len),
            color,
            section_clip: false,
            motion: None,
        };
        if driven == Some(i) {
            out.arrows.push(arrow(along(c, side, 0.14 * width.min(height)), -1.1, -0.15, colours.scene.port_input));
            out.arrows.push(arrow(along(c, side, -0.14 * width.min(height)), -0.15, -1.1, colours.scene.port_input));
        } else {
            out.arrows.push(arrow(c, -0.15, -1.1, colours.scene.port_output));
        }
    }
    let strong = overlay == Overlay::Ports;
    for (i, p) in m.lumped.iter().enumerate() {
        let (a, b) = (p3(p.from), p3(p.to));
        out.arrows.push(super::Arrow { start: a, end: b, color: colours.scene.port, section_clip: false, motion: None });
        // A small box round the gap's middle to pick it by.
        let (c, h) = (midpoint(a, b), 0.012 * size);
        out.scene.tagged(Pick::Lumped(i).tag(), |s| s.cuboid(c.map(|v| v - h), c.map(|v| v + h), colours.scene.port, 255));
        if strong {
            out.labels.push(Label {
                at: midpoint(a, b),
                text: crate::lang::trf(t!("Lumped port {name}"), args!(name = p.name)),
                color: colours.scene.port,
                facing: None,
                anchor: None,
            });
        }
    }
    let sources = m.sources.iter().map(|s| sources(colours, s, strong, size, bounds));
    out = sources.fold(out, Built::merge);
    out = out.merge(open_boundary(colours, m, overlay, bounds));
    out
}

fn midpoint(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [0.5 * (a[0] + b[0]), 0.5 * (a[1] + b[1]), 0.5 * (a[2] + b[2])]
}

/// `a` moved by `k` along `d`.
pub fn along(a: [f64; 3], d: [f64; 3], k: f64) -> [f64; 3] {
    [a[0] + d[0] * k, a[1] + d[1] * k, a[2] + d[2] * k]
}

/// The ports' labels, each with its role (`driven` the input), and, when ports are the
/// subject, their outlines.
fn ports(colours: &Colours, m: &Model, driven: Option<usize>, overlay: Overlay, size: f64) -> Built {
    let mut out = Built::default();
    for (i, p) in m.ports.iter().enumerate() {
        let Some(q) = port_quad(m, p) else { continue };
        let c = quad_centre(&q);
        let facing = match p.face {
            Some(f) if f.axis == emcraft_model::Axis::Z => None,
            _ => Some(port_normal(p).map(|x| -x)),
        };
        // The name stands off the face, outside the model, on a leader: the face's arrow points
        // the other way, into the model.
        let out_of = along(c, port_normal(p), -0.15 * size);
        let (text, color) = if driven == Some(i) {
            (crate::lang::trf(t!("Port {name} · in"), args!(name = p.name)), colours.scene.port_input)
        } else {
            (crate::lang::trf(t!("Port {name} · out"), args!(name = p.name)), colours.scene.port_output)
        };
        out.labels.push(Label { at: out_of, text, color, facing, anchor: Some(c) });
        if overlay == Overlay::Ports {
            // Outlined in its role's colour.
            for (a, b) in outline(&port_face(m, p)) {
                out.scene.line(a, b, color, 255);
            }
        }
    }
    out
}

/// A source: a dipole as a short line along its direction, a plane wave as a line coming in
/// towards the model's middle.
fn sources(colours: &Colours, s: &Source, strong: bool, size: f64, bounds: Option<([f64; 3], [f64; 3])>) -> Built {
    let mut out = Built::default();
    match *s {
        Source::Dipole { at, direction, .. } => {
            let (a, d) = (p3(at), [direction.x, direction.y, direction.z]);
            let h = 0.08 * size;
            out.scene.line(along(a, d, -h), along(a, d, h), colours.scene.source, 255);
            if strong {
                out.labels.push(Label { at: a, text: crate::lang::tr(t!("Dipole")).into(), color: colours.scene.source, facing: None, anchor: None });
            }
        }
        Source::PlaneWave { direction, .. } => {
            if let Some((lo, hi)) = bounds {
                let c = midpoint(lo, hi);
                let d = [direction.x, direction.y, direction.z];
                let h = 0.5 * size;
                let a = along(c, d, -h);
                out.scene.line(a, [c[0] - d[0] * 0.3 * h, c[1] - d[1] * 0.3 * h, c[2] - d[2] * 0.3 * h], colours.scene.source, 255);
                if strong {
                    out.labels.push(Label {
                        at: a,
                        text: crate::lang::tr(t!("Plane wave")).into(),
                        color: colours.scene.source,
                        facing: None,
                        anchor: None,
                    });
                }
            }
        }
    }
    out
}

/// The open boundary: the layer's inner face as a frame. When boundaries are the subject, the
/// layer's thickness is shaded as a band round each outer face (a picture frame), so what lies
/// inside stays clear, and its corners join the inner frame's.
fn open_boundary(colours: &Colours, m: &Model, overlay: Overlay, bounds: Option<([f64; 3], [f64; 3])>) -> Built {
    let mut out = Built::default();
    let (Some(o), Some((lo, hi))) = (&m.domain.open, bounds) else { return out };
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
                    out.scene.quad([at(x0, y0), at(x1, y0), at(x1, y1), at(x0, y1)], colours.scene.pml, 70);
                }
            }
        }
        for i in 0..8 {
            out.scene.line(box_corner(lo, hi, i), box_corner(a, b, i), colours.scene.pml, 200);
        }
        // On the band, at a corner of the outer box.
        out.labels.push(Label { at: [hi[0], hi[1], lo[2] + 0.5 * t], text: "PML".into(), color: colours.scene.pml, facing: None, anchor: None });
    }
    for (i, j) in BOX_EDGES {
        out.scene.line(box_corner(a, b, i), box_corner(a, b, j), colours.scene.pml, 255);
    }
    out
}
