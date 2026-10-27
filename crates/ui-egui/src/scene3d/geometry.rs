//! The model before meshing, coarsely tessellated: a waveguide's sections, layers, strips and
//! vias; a solid's primitive (box, cylinder, sphere, imported surface), its Boolean features and
//! inclusions; and the ports on its ends.

use emcraft_model::{Axis, Combine, Model, Shape};
use emcraft_render::Rgb;
use emcraft_render::view3d::Scene;

use super::extras::port_face;
use super::style::{EDGE, METAL, METAL_DARK, PORT, PORT_SOFT, material_rgb};
use super::{Built, Overlay, Pick, p3};

/// Segments round a circle.
pub(super) const ROUND: usize = 48;

/// A box's corner `i` between `lo` and `hi`: bits 0, 1 and 2 of `i` pick the high side along
/// x, y and z.
pub(crate) fn box_corner(lo: [f64; 3], hi: [f64; 3], i: usize) -> [f64; 3] {
    [if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] }]
}

/// A box's six faces, as corners ([`box_corner`]) outward.
const BOX_FACES: [[usize; 4]; 6] = [[0, 1, 3, 2], [4, 6, 7, 5], [0, 4, 5, 1], [2, 3, 7, 6], [0, 2, 6, 4], [1, 5, 7, 3]];

/// A box's twelve edges, as corners ([`box_corner`]).
pub(crate) const BOX_EDGES: [(usize, usize); 12] = [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)];

/// A box's faces and, with `edges`, its edges, its corners placed by `corner`.
fn box_shape(s: &mut Scene, corner: impl Fn(usize) -> [f64; 3], color: Rgb, alpha: u8, edges: bool) {
    for q in BOX_FACES {
        s.quad(q.map(&corner), color, alpha);
    }
    if edges {
        for (a, b) in BOX_EDGES {
            s.line(corner(a), corner(b), EDGE, 200);
        }
    }
}

/// A box's six faces and twelve edges.
fn cuboid(s: &mut Scene, lo: [f64; 3], hi: [f64; 3], color: Rgb, alpha: u8, edges: bool) {
    box_shape(s, |i| box_corner(lo, hi, i), color, alpha, edges);
}

/// A primitive placed by `place` (its own coordinates to the world's).
fn shape(s: &mut Scene, sh: &Shape, place: &dyn Fn([f64; 3]) -> [f64; 3], color: Rgb, alpha: u8) {
    use std::f64::consts::{PI, TAU};
    match sh {
        Shape::Box { size } => box_shape(s, |i| place(box_corner([0.0; 3], [size.x, size.y, size.z], i)), color, alpha, true),
        Shape::Cylinder { radius: r, length } => {
            let at = |k: usize, z: f64| {
                let a = TAU * k as f64 / ROUND as f64;
                place([r * a.cos(), r * a.sin(), z])
            };
            for k in 0..ROUND {
                let (a0, a1, b0, b1) = (at(k, 0.0), at(k + 1, 0.0), at(k, *length), at(k + 1, *length));
                s.quad([a0, a1, b1, b0], color, alpha);
                s.tri(place([0.0, 0.0, 0.0]), a1, a0, color, alpha);
                s.tri(place([0.0, 0.0, *length]), b0, b1, color, alpha);
                s.line(a0, a1, EDGE, 200);
                s.line(b0, b1, EDGE, 200);
            }
        }
        Shape::Sphere { radius: r } => {
            let (nu, nv) = (ROUND, ROUND / 2);
            let at = |i: usize, j: usize| {
                let (u, v) = (TAU * i as f64 / nu as f64, PI * j as f64 / nv as f64 - PI / 2.0);
                place([r * v.cos() * u.cos(), r * v.cos() * u.sin(), r * v.sin()])
            };
            for i in 0..nu {
                for j in 0..nv {
                    s.quad([at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)], color, alpha);
                }
                s.line(at(i, nv / 2), at(i + 1, nv / 2), EDGE, 160);
            }
        }
        Shape::Surface { positions, triangles, .. } => {
            for t in triangles.iter().take(emcraft_render::view3d::MAX_PRIMITIVES) {
                let p = |v: u32| positions.get(v as usize).map(|q| place(p3(*q)));
                if let (Some(a), Some(b), Some(c)) = (p(t[0]), p(t[1]), p(t[2])) {
                    s.tri(a, b, c, color, alpha);
                }
            }
        }
    }
}

/// A primitive's own coordinates to the world's: turned from z to `axis`, then moved to `at`.
fn turned(axis: Axis, at: emcraft_model::geom::Point3) -> impl Fn([f64; 3]) -> [f64; 3] {
    move |q: [f64; 3]| {
        let v = axis.turn(emcraft_model::geom::Vec3::new(q[0], q[1], q[2]));
        [at.x + v.x, at.y + v.y, at.z + v.z]
    }
}

/// The geometry before meshing.
pub(super) fn from_geometry(m: &Model, overlay: Overlay, out: &mut Built) {
    let mat = |name: &str| m.materials.iter().position(|x| x.name == name).unwrap_or(0);
    let tint = |name: &str| if overlay == Overlay::Materials { material_rgb(m, mat(name)) } else { METAL };
    let see_through = matches!(overlay, Overlay::Ports | Overlay::Boundaries | Overlay::Field);
    if let Some(w) = &m.waveguide {
        let mut z = 0.0;
        // A planar circuit's conductors lie inside: its walls let them show.
        let layered = !w.layers.is_empty() || !w.strips.is_empty() || !w.vias.is_empty();
        let alpha = if see_through || layered {
            70
        } else if overlay == Overlay::Materials {
            200
        } else {
            255
        };
        for (i, s) in w.sections.iter().enumerate() {
            let emcraft_model::Span { start: x0, width: a } = w.aperture(i).unwrap_or(w.across_x());
            let emcraft_model::Span { start: y0, width: b } = w.aperture_y(i).unwrap_or(w.across_y());
            out.scene
                .tagged(Pick::Section(i).tag(), |scene| cuboid(scene, [x0, y0, z], [x0 + a, y0 + b, z + s.length], tint(&s.material), alpha, true));
            z += s.length;
        }
        let mut y = 0.0;
        for (i, layer) in w.layers.iter().enumerate() {
            out.scene.tagged(Pick::Layer(i).tag(), |scene| {
                cuboid(scene, [0.0, y, 0.0], [w.width, y + layer.thickness, w.length()], tint(&layer.material), 200, true)
            });
            y += layer.thickness;
        }
        for (i, st) in w.strips.iter().enumerate() {
            out.scene.tagged(Pick::Strip(i).tag(), |scene| {
                scene.quad(
                    [[st.x0, st.y, st.z0], [st.x0 + st.width, st.y, st.z0], [st.x0 + st.width, st.y, st.z1], [st.x0, st.y, st.z1]],
                    PORT_SOFT.mix(Rgb(200, 160, 40), 0.6),
                    255,
                )
            });
        }
        for (i, v) in w.vias.iter().enumerate() {
            out.scene.tagged(Pick::Via(i).tag(), |scene| cuboid(scene, [v.x0, v.y0, v.z0], [v.x0 + v.wx, v.y1, v.z0 + v.wz], METAL_DARK, 255, true));
        }
    }
    if let Some(solid) = &m.solid {
        let hollow = !m.inclusions.is_empty() || m.features.iter().any(|f| f.combine != Combine::Add);
        let alpha = if m.open.is_some() {
            // The air around an open problem.
            20
        } else if see_through || hollow {
            90
        } else if overlay == Overlay::Materials {
            210
        } else {
            255
        };
        out.scene.tagged(Pick::Solid.tag(), |scene| shape(scene, &solid.shape, &|q| q, tint(&solid.material), alpha));
        for (i, f) in m.features.iter().enumerate() {
            let (color, alpha) = match f.combine {
                Combine::Add => (tint(&solid.material), alpha),
                Combine::Subtract => (METAL_DARK, 255),
                Combine::Intersect => (Rgb(120, 160, 200), 60),
            };
            out.scene.tagged(Pick::Feature(i).tag(), |scene| shape(scene, &f.shape, &turned(f.axis, f.at), color, alpha));
        }
        for (i, inc) in m.inclusions.iter().enumerate() {
            let color = match &inc.material {
                Some(name) => material_rgb(m, mat(name)),
                None => METAL_DARK,
            };
            out.scene.tagged(Pick::Inclusion(i).tag(), |scene| shape(scene, &inc.shape, &turned(Axis::Z, inc.at), color, 255));
        }
    }
    // Ports on the ends (the mesh tags them once meshed).
    let (color, alpha) = if overlay == Overlay::Ports { (PORT, 255) } else { (PORT_SOFT, 220) };
    for (i, p) in m.ports.iter().enumerate() {
        let face = port_face(m, p);
        out.scene.tagged(Pick::Port(i).tag(), |scene| {
            for [a, b, c] in face {
                scene.tri(a, b, c, color, alpha);
            }
        });
    }
}
