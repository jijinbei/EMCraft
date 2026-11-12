//! Where a port goes on the face the port tool points at ([`port_target`]): a port's own face
//! selects it; a face across z at an end of the model takes a waveguide port at once; any other
//! flat face across x, y or z of a solid opens the port dialog on that rectangle and axis (the
//! side the guide lies on guessed from where the body is); an imported cavity takes ports on its
//! openings only.

use emcraft_model::End;
use emcraft_model::geom::Point3;

use crate::pick::Pick;
use crate::scene3d::faces::Face;

/// What a click of the port tool does on the face under the pointer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PortTarget {
    /// A port's own face: selects it.
    Existing(usize),
    /// A free end of the model: a port there at once.
    End(End),
    /// A face across `axis` of a solid, not a free end: the port dialog on its rectangle (m:
    /// `face` as `model.port.create` takes it), the guide's side guessed from where the body
    /// lies.
    Inside { axis: emcraft_model::Axis, face: [f64; 5], end: End },
    /// An imported cavity's opening across x or y: a port there at once (m: `face`).
    Opening { axis: emcraft_model::Axis, face: [f64; 5], end: End },
    /// No port goes here: why.
    No(crate::lang::Text),
}

impl PortTarget {
    /// Its name for `ui.inspect`.
    pub fn id(self) -> &'static str {
        match self {
            PortTarget::Existing(_) => "existing",
            PortTarget::End(End::Start) => "start",
            PortTarget::End(End::End) => "end",
            PortTarget::Inside { .. } => "inside",
            PortTarget::Opening { .. } => "opening",
            PortTarget::No(_) => "none",
        }
    }
}

/// Why no port goes on a face that is not flat across an axis.
const FLAT_ONLY: crate::lang::Text = t!("Ports lie on flat faces across x, y or z");

/// Why no port goes on an imported cavity's wall.
pub(crate) const OPENINGS_ONLY: crate::lang::Text = t!("Ports go on the channel's openings, not on its walls");

/// What a port goes on at `face`.
pub fn port_target(m: &emcraft_model::Model, face: &Face) -> PortTarget {
    if let Some(Pick::Port(i)) = Pick::from_tag(face.tag) {
        return PortTarget::Existing(i);
    }
    let (Some(axis), Some(b)) = (face.axis(), m.bounds()) else { return PortTarget::No(FLAT_ONLY) };
    // An imported cavity: ports go on its openings only, never on its walls.
    let openings = m.openings();
    if !openings.is_empty() {
        let near = 1e-6 * (0..3).map(|d| (b.max[d] - b.min[d]).abs()).fold(1e-9, f64::max);
        let centre = Point3::from(face.centre);
        let Some((end, f)) = openings.iter().find(|(_, f)| f.axis == axis && f.holds(centre, near)) else {
            return PortTarget::No(OPENINGS_ONLY);
        };
        let existing = m.ports.iter().position(|p| match p.face {
            Some(q) => q.axis == f.axis && q.holds(centre, near),
            None => axis == emcraft_model::Axis::Z && p.end == *end,
        });
        return match existing {
            Some(i) => PortTarget::Existing(i),
            None if axis == emcraft_model::Axis::Z => PortTarget::End(*end),
            None => PortTarget::Opening { axis, face: f.values(), end: *end },
        };
    }
    let z = face.centre[2];
    let tol = 1e-6 * (b.max.z - b.min.z).abs().max(1e-9);
    if m.domain.open.is_none() && axis == emcraft_model::Axis::Z {
        let end = if (z - b.min.z.min(0.0)).abs() <= tol {
            Some(End::Start)
        } else if (z - b.max.z).abs() <= tol {
            Some(End::End)
        } else {
            None
        };
        if let Some(end) = end {
            return match m.ports.iter().position(|p| p.face.is_none() && p.end == end) {
                Some(i) => PortTarget::Existing(i),
                None => PortTarget::End(end),
            };
        }
    }
    if m.domain.solid.is_none() {
        return PortTarget::No(t!("A port inside the model needs a solid"));
    }
    // A port's own face, where the body's face is found in front of the port's.
    let k = axis.index();
    let centre = Point3::from(face.centre);
    let near = 1e-6 * (0..3).map(|d| (b.max[d] - b.min[d]).abs()).fold(1e-9, f64::max);
    if let Some(i) = m.ports.iter().position(|p| p.face.is_some_and(|f| f.axis == axis && f.holds(centre, near))) {
        return PortTarget::Existing(i);
    }
    // The guide lies where the body is: above the face along its axis, or below it.
    let emcraft_model::port::FrameAxes { u, v, .. } = emcraft_model::Frame::new(axis).axes();
    let h = 1e-4 * (b.max[k] - b.min[k]).abs().max(1e-9);
    let ahead = centre.with(k, centre[k] + h);
    let end = if m.material_at(ahead).is_some() { End::Start } else { End::End };
    let face = [face.centre[k], face.lo[u], face.lo[v], face.hi[u] - face.lo[u], face.hi[v] - face.lo[v]];
    PortTarget::Inside { axis, face, end }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A flat face across `axis` (0 x, 1 y, 2 z) with its centre at `c`.
    fn face(axis: usize, c: [f64; 3]) -> Face {
        let mut normal = [0.0; 3];
        normal[axis] = 1.0;
        Face { tag: 0, normal, tris: Vec::new(), outline: Vec::new(), corners: Vec::new(), edges: Vec::new(), centre: c, lo: c, hi: c }
    }

    /// An imported cavity along y (a 10 × 100 × 10 mm block, a 5 × 5 mm channel through it):
    /// its ends take ports at once, its walls none — the side wall and the wall at the highest
    /// z, which a z port took before.
    #[test]
    fn a_cavity_takes_ports_on_its_openings_only() {
        let mut m = emcraft_model::Model::new();
        let (lo, hi) = ([0.0025, 0.0, 0.0025], [0.0075, 0.1, 0.0075]);
        let positions = (0..8).map(|i| Point3::new([lo[0], hi[0]][i & 1], [lo[1], hi[1]][(i >> 1) & 1], [lo[2], hi[2]][(i >> 2) & 1])).collect();
        let quads = [[0, 4, 6, 2], [1, 3, 7, 5], [0, 1, 5, 4], [2, 6, 7, 3], [0, 2, 3, 1], [4, 5, 7, 6]];
        let triangles = quads.iter().flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]]).collect();
        let tags = vec![6, 6, 7, 7, 2, 2, 3, 3, 8, 8, 9, 9];
        let shape = emcraft_model::Shape::Surface { name: emcraft_model::CAVITY.into(), positions, triangles, tags, surfaces: Vec::new() };
        m.set_solid(emcraft_model::Solid { shape, material: "vacuum".into() }).unwrap();
        let end = port_target(&m, &face(1, [0.005, 0.1, 0.005]));
        let PortTarget::Opening { axis, face: f, end: End::End } = end else { panic!("{end:?}") };
        assert_eq!(axis, emcraft_model::Axis::Y);
        assert!((f[0] - 0.1).abs() < 1e-12 && (f[3] - 0.005).abs() < 1e-12, "{f:?}");
        for wall in [face(0, [0.0075, 0.05, 0.005]), face(2, [0.005, 0.05, 0.0075])] {
            assert_eq!(port_target(&m, &wall), PortTarget::No(OPENINGS_ONLY));
        }
    }
}
