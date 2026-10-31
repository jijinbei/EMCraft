//! The model before meshing, coarsely tessellated: a waveguide's sections, layers, strips and
//! vias; a solid's primitive (box, cylinder, sphere, imported surface), its Boolean features and
//! inclusions; and the ports on its ends.

use emcraft_model::{Axis, Combine, Model, Shape, Span, Waveguide};
use emcraft_render::Rgb;
use emcraft_render::view3d::Scene;

use super::extras::port_face;
use super::shell::{Inside, Side};
use super::style::{material_rgb, sc};
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
            s.line(corner(a), corner(b), sc().edge, 200);
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
                let (a0, a1, b0, b1) = (at(k, 0.0), at(k + 1, 0.0), at(k, length.get()), at(k + 1, length.get()));
                s.quad([a0, a1, b1, b0], color, alpha);
                s.tri(place([0.0, 0.0, 0.0]), a1, a0, color, alpha);
                s.tri(place([0.0, 0.0, length.get()]), b0, b1, color, alpha);
                s.line(a0, a1, sc().edge, 200);
                s.line(b0, b1, sc().edge, 200);
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
                s.line(at(i, nv / 2), at(i + 1, nv / 2), sc().edge, 160);
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
    let tint = |name: &str| if overlay == Overlay::Materials { material_rgb(m, mat(name)) } else { sc().metal };
    let see_through = matches!(overlay, Overlay::Ports | Overlay::Boundaries | Overlay::Field);
    if let Some(w) = &m.waveguide {
        let mut z = 0.0;
        // A planar circuit's conductors lie inside: its walls let them show.
        let alpha = if see_through || !w.plain() {
            70
        } else if overlay == Overlay::Materials {
            200
        } else {
            255
        };
        for (i, s) in w.sections.iter().enumerate() {
            let a = Aperture::of(w, i);
            out.scene.tagged(Pick::Section(i).tag(), |scene| {
                cuboid(scene, [a.x.start, a.y.start, z], [a.x.end(), a.y.end(), z + s.length], tint(&s.material), alpha, true)
            });
            z += s.length;
        }
        let mut y = 0.0;
        for (i, layer) in w.layers.iter().enumerate() {
            out.scene.tagged(Pick::Layer(i).tag(), |scene| {
                cuboid(scene, [0.0, y, 0.0], [w.width.get(), y + layer.thickness, w.length()], tint(&layer.material), 200, true)
            });
            y += layer.thickness;
        }
        for (i, st) in w.strips.iter().enumerate() {
            out.scene.tagged(Pick::Strip(i).tag(), |scene| {
                scene.quad(
                    [[st.x0, st.y, st.z0], [st.x0 + st.width, st.y, st.z0], [st.x0 + st.width, st.y, st.z1], [st.x0, st.y, st.z1]],
                    sc().strip,
                    255,
                )
            });
        }
        for (i, v) in w.vias.iter().enumerate() {
            out.scene
                .tagged(Pick::Via(i).tag(), |scene| cuboid(scene, [v.x0, v.y0, v.z0], [v.x0 + v.wx, v.y1, v.z0 + v.wz], sc().metal_dark, 255, true));
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
                Combine::Subtract => (sc().metal_dark, 255),
                Combine::Intersect => (sc().intersect, 60),
            };
            out.scene.tagged(Pick::Feature(i).tag(), |scene| shape(scene, &f.shape, &turned(f.axis, f.at), color, alpha));
        }
        for (i, inc) in m.inclusions.iter().enumerate() {
            let color = match &inc.material {
                Some(name) => material_rgb(m, mat(name)),
                None => sc().metal_dark,
            };
            out.scene.tagged(Pick::Inclusion(i).tag(), |scene| shape(scene, &inc.shape, &turned(Axis::Z, inc.at), color, 255));
        }
    }
    if let Some(t) = m.wall_thickness
        && m.open.is_none()
        && let Some(inside) = inside(m)
    {
        super::shell::draw(m, overlay, &inside, t.get(), out);
    }
    // Ports on the ends (the mesh tags them once meshed).
    let (color, alpha) = if overlay == Overlay::Ports { (sc().port, 255) } else { (sc().port_soft, 220) };
    for (i, p) in m.ports.iter().enumerate() {
        let face = port_face(m, p);
        out.scene.tagged(Pick::Port(i).tag(), |scene| {
            for [a, b, c] in face {
                scene.tri(a, b, c, color, alpha);
            }
        });
    }
}

/// The inside's surface before meshing, its faces on ports open: a waveguide's sections (no
/// planar circuit) on the grid of their apertures' sides, so the faces meet edge to edge where
/// one aperture steps to the next; a solid's own faces (no Boolean features). `None` for the
/// rest, whose walls are drawn thick once meshed.
fn inside(m: &Model) -> Option<Inside> {
    let mut out = Inside::default();
    let on_port = super::shell::on_ports(m);
    if let Some(w) = &m.waveguide {
        if !w.plain() {
            return None;
        }
        guide_inside(w, |q| {
            let c = [0, 1, 2].map(|k| q.iter().map(|p| p[k]).sum::<f64>() / 4.0);
            out.quad(q, Side::open_if(on_port(c)));
        });
        return Some(out);
    }
    let solid = m.solid.as_ref()?;
    if !m.features.is_empty() {
        return None;
    }
    let mut scene = Scene::default();
    shape(&mut scene, &solid.shape, &|q| q, sc().metal, 255);
    for t in &scene.tris {
        let c = [0, 1, 2].map(|k| (t.p[0][k] + t.p[1][k] + t.p[2][k]) / 3.0);
        out.tri(t.p, Side::open_if(on_port(c)));
    }
    Some(out)
}

/// A section's aperture: its spans across x and y (m).
#[derive(Clone, Copy)]
struct Aperture {
    x: Span,
    y: Span,
}

impl Aperture {
    /// Section `i`'s (the guide's whole width and height where it has none of its own).
    fn of(w: &Waveguide, i: usize) -> Aperture {
        Aperture { x: w.aperture(i).unwrap_or(w.across_x()), y: w.aperture_y(i).unwrap_or(w.across_y()) }
    }

    /// Whether it holds the point (x, y), its sides included.
    fn holds(self, x: f64, y: f64) -> bool {
        self.x.start <= x && x <= self.x.end() && self.y.start <= y && y <= self.y.end()
    }
}

/// The values sorted and each once: where the grid's lines lie.
fn breaks(v: impl Iterator<Item = f64>) -> Vec<f64> {
    let mut v: Vec<f64> = v.collect();
    v.sort_by(f64::total_cmp);
    v.dedup();
    v
}

/// The cells of the grid `v` between `lo` and `hi`.
fn cells(v: &[f64], lo: f64, hi: f64) -> impl Iterator<Item = (f64, f64)> + '_ {
    v.windows(2).filter(move |p| p[0] >= lo && p[1] <= hi).map(|p| (p[0], p[1]))
}

/// A plain guide's inside as quadrilaterals to `put`, turned outward: each section's four
/// sides, the steps where one aperture meets the next, and the two ends — all cut on the grid
/// of every aperture's sides.
fn guide_inside(w: &Waveguide, mut put: impl FnMut([[f64; 3]; 4])) {
    let apertures: Vec<Aperture> = (0..w.sections.len()).map(|i| Aperture::of(w, i)).collect();
    let xs = breaks(apertures.iter().flat_map(|a| [a.x.start, a.x.end()]));
    let ys = breaks(apertures.iter().flat_map(|a| [a.y.start, a.y.end()]));
    let mut z = 0.0;
    for (i, (s, a)) in w.sections.iter().zip(&apertures).enumerate() {
        let (z0, z1) = (z, z + s.length);
        sides(*a, &xs, &ys, z0, z1, &mut put);
        let before = i.checked_sub(1).and_then(|k| apertures.get(k)).copied();
        let last = i + 1 == apertures.len();
        steps(before, *a, last, &xs, &ys, (z0, z1), &mut put);
        z = z1;
    }
}

/// A section's four sides from `z0` to `z1`, outward, cut where any aperture's side lies.
fn sides(a: Aperture, xs: &[f64], ys: &[f64], z0: f64, z1: f64, put: &mut impl FnMut([[f64; 3]; 4])) {
    let (x0, x1, y0, y1) = (a.x.start, a.x.end(), a.y.start, a.y.end());
    for (u0, u1) in cells(xs, x0, x1) {
        put([[u0, y0, z0], [u1, y0, z0], [u1, y0, z1], [u0, y0, z1]]);
        put([[u0, y1, z0], [u0, y1, z1], [u1, y1, z1], [u1, y1, z0]]);
    }
    for (v0, v1) in cells(ys, y0, y1) {
        put([[x0, v0, z0], [x0, v0, z1], [x0, v1, z1], [x0, v1, z0]]);
        put([[x1, v0, z0], [x1, v1, z0], [x1, v1, z1], [x1, v0, z1]]);
    }
}

/// Where section `a` (from `z0` to `z1`) starts: the cells of one aperture and not the other's
/// (`before`, none for the first: all of its own), each facing out of the section it belongs
/// to; and, for the `last`, its end.
fn steps(before: Option<Aperture>, a: Aperture, last: bool, xs: &[f64], ys: &[f64], (z0, z1): (f64, f64), put: &mut impl FnMut([[f64; 3]; 4])) {
    for (x0, x1) in xs.windows(2).map(|p| (p[0], p[1])) {
        for (y0, y1) in ys.windows(2).map(|p| (p[0], p[1])) {
            let (cx, cy) = (0.5 * (x0 + x1), 0.5 * (y0 + y1));
            let (low, high) = (before.is_some_and(|b| b.holds(cx, cy)), a.holds(cx, cy));
            if high && !low {
                put([[x0, y0, z0], [x0, y1, z0], [x1, y1, z0], [x1, y0, z0]]);
            } else if low && !high {
                put([[x0, y0, z0], [x1, y0, z0], [x1, y1, z0], [x0, y1, z0]]);
            }
            if last && high {
                put([[x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]]);
            }
        }
    }
}
