//! The model before meshing, coarsely tessellated: a waveguide's sections, layers, strips and
//! vias; a solid's primitive (box, cylinder, sphere, imported surface), its Boolean features and
//! inclusions; and the ports on its ends.

use emcraft_model::{Axis, Combine, Model, Shape};
use emcraft_render::Rgb;
use emcraft_render::view3d::Scene;

use super::extras::port_face;
use super::shell::{Inside, Side};
use super::style::{Colours, material_rgb};
use super::{Built, Overlay, Pick, merged, p3, tagged};

/// Segments round a circle.
pub(super) const ROUND: usize = 48;

/// A box's corner `i` between `lo` and `hi`: bits 0, 1 and 2 of `i` pick the high side along
/// x, y and z.
pub fn box_corner(lo: [f64; 3], hi: [f64; 3], i: usize) -> [f64; 3] {
    [if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] }]
}

/// A box's six faces, as corners ([`box_corner`]) outward.
const BOX_FACES: [[usize; 4]; 6] = [[0, 1, 3, 2], [4, 6, 7, 5], [0, 4, 5, 1], [2, 3, 7, 6], [0, 2, 6, 4], [1, 5, 7, 3]];

/// A box's twelve edges, as corners ([`box_corner`]).
pub const BOX_EDGES: [(usize, usize); 12] = [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)];

/// What a body is drawn in: its colour and opacity, and its edges' colour.
#[derive(Clone, Copy)]
struct Paint {
    color: Rgb,
    alpha: u8,
    edge: Rgb,
}

/// A box's faces and edges, its corners placed by `corner`.
fn box_shape(corner: impl Fn(usize) -> [f64; 3], paint: Paint) -> Scene {
    let mut s = Scene::default();
    for q in BOX_FACES {
        s.quad(q.map(&corner), paint.color, paint.alpha);
    }
    for (a, b) in BOX_EDGES {
        s.line(corner(a), corner(b), paint.edge, 200);
    }
    s
}

/// A box's six faces and twelve edges.
fn cuboid(lo: [f64; 3], hi: [f64; 3], paint: Paint) -> Scene {
    box_shape(|i| box_corner(lo, hi, i), paint)
}

/// A primitive placed by `place` (its own coordinates to the world's).
fn shape(sh: &Shape, place: &dyn Fn([f64; 3]) -> [f64; 3], paint: Paint) -> Scene {
    use std::f64::consts::{PI, TAU};
    let Paint { color, alpha, edge } = paint;
    let mut s = Scene::default();
    match sh {
        Shape::Box { size } => return box_shape(|i| place(box_corner([0.0; 3], [size.x, size.y, size.z], i)), paint),
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
                s.line(a0, a1, edge, 200);
                s.line(b0, b1, edge, 200);
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
                s.line(at(i, nv / 2), at(i + 1, nv / 2), edge, 160);
            }
        }
        // A board is drawn by its own view (the PCB view).
        Shape::Board(_) => {}
        Shape::Sheet { size: [a, b] } => {
            let c = [[0.0, 0.0], [a.get(), 0.0], [a.get(), b.get()], [0.0, b.get()]].map(|[u, v]| place([u, v, 0.0]));
            s.quad(c, color, alpha);
            for k in 0..4 {
                s.line(c[k], c[(k + 1) % 4], edge, 200);
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
    s
}

/// A primitive's own coordinates to the world's: turned from z to `axis`, then moved to `at`.
fn turned(axis: Axis, at: emcraft_model::geom::Point3) -> impl Fn([f64; 3]) -> [f64; 3] {
    move |q: [f64; 3]| {
        let v = axis.turn(emcraft_model::geom::Vec3::new(q[0], q[1], q[2]));
        [at.x + v.x, at.y + v.y, at.z + v.z]
    }
}

/// The geometry before meshing.
pub(super) fn from_geometry(colours: &Colours, m: &Model, overlay: Overlay) -> Built {
    let mat = |name: &str| m.materials.iter().position(|x| x.name == name).unwrap_or(0);
    let tint = |name: &str| if overlay == Overlay::Materials { material_rgb(colours, m, mat(name)) } else { colours.scene.metal };
    let see_through = matches!(overlay, Overlay::Ports | Overlay::Boundaries | Overlay::Field);
    let layers = [guide(colours, m, &tint, see_through, overlay), solid(colours, m, &tint, see_through, overlay)];
    let shell =
        m.domain.wall_thickness.filter(|_| m.domain.open.is_none()).and_then(|t| Some(super::shell::draw(colours, m, overlay, &inside(m)?, t.get())));
    layers.into_iter().chain(shell).chain([ports(colours, m, overlay)]).fold(Built::default(), Built::merge)
}

/// Ports on the ends (the mesh tags them once meshed).
fn ports(colours: &Colours, m: &Model, overlay: Overlay) -> Built {
    let (color, alpha) = if overlay == Overlay::Ports { (colours.scene.port, 255) } else { (colours.scene.port_soft, 220) };
    let scenes = m.ports.iter().enumerate().map(|(i, p)| {
        let mut scene = Scene::default();
        for [a, b, c] in port_face(m, p) {
            scene.tri(a, b, c, color, alpha);
        }
        tagged(Pick::Port(i), scene)
    });
    Built::of(scenes.fold(Scene::default(), merged))
}

/// A waveguide's sections, layers, strips and vias.
fn guide(colours: &Colours, m: &Model, tint: &dyn Fn(&str) -> Rgb, see_through: bool, overlay: Overlay) -> Built {
    let Some(w) = m.waveguide() else { return Built::default() };
    let paint = |color: Rgb, alpha: u8| Paint { color, alpha, edge: colours.scene.edge };
    // A planar circuit's conductors lie inside: its walls let them show.
    let alpha = if see_through || !w.plain() {
        70
    } else if overlay == Overlay::Materials {
        200
    } else {
        255
    };
    let starts = w.sections.iter().scan(0.0, |z, s| Some(std::mem::replace(z, *z + s.length)));
    let sections = w.sections.iter().zip(starts).enumerate().map(|(i, (s, z))| {
        let [x, y] = w.opening(i);
        tagged(Pick::Section(i), cuboid([x.start, y.start, z], [x.end(), y.end(), z + s.length], paint(tint(&s.material), alpha)))
    });
    let bottoms = w.layers.iter().scan(0.0, |y, l| Some(std::mem::replace(y, *y + l.thickness)));
    let layers = w.layers.iter().zip(bottoms).enumerate().map(|(i, (l, y))| {
        tagged(Pick::Layer(i), cuboid([0.0, y, 0.0], [w.width.get(), y + l.thickness, w.length()], paint(tint(&l.material), 200)))
    });
    let strips = w.strips.iter().enumerate().map(|(i, st)| {
        let mut strip = Scene::default();
        strip.quad(
            [[st.x0, st.y, st.z0], [st.x0 + st.width, st.y, st.z0], [st.x0 + st.width, st.y, st.z1], [st.x0, st.y, st.z1]],
            colours.scene.strip,
            255,
        );
        tagged(Pick::Strip(i), strip)
    });
    let vias = w
        .vias
        .iter()
        .enumerate()
        .map(|(i, v)| tagged(Pick::Via(i), cuboid([v.x0, v.y0, v.z0], [v.x0 + v.wx, v.y1, v.z0 + v.wz], paint(colours.scene.metal_dark, 255))));
    Built::of(sections.chain(layers).chain(strips).chain(vias).fold(Scene::default(), merged))
}

/// A solid's primitive, its Boolean features and inclusions.
fn solid(colours: &Colours, m: &Model, tint: &dyn Fn(&str) -> Rgb, see_through: bool, overlay: Overlay) -> Built {
    let Some(solid) = &m.domain.solid else { return Built::default() };
    let paint = |color: Rgb, alpha: u8| Paint { color, alpha, edge: colours.scene.edge };
    let mat = |name: &str| m.materials.iter().position(|x| x.name == name).unwrap_or(0);
    let hollow = !m.inclusions().is_empty() || m.features().iter().any(|f| f.combine != Combine::Add);
    let alpha = if m.domain.open.is_some() || m.analysis() == emcraft_model::Analysis::Particles {
        // The air around an open problem; a particle run's box, its particles inside.
        20
    } else if see_through || hollow {
        90
    } else if overlay == Overlay::Materials {
        210
    } else {
        255
    };
    let body = tagged(Pick::Solid, shape(&solid.shape, &|q| q, paint(tint(&solid.material), alpha)));
    let carved: Vec<usize> = (0..m.features().len()).filter(|&i| is_carved(m, i)).collect();
    let features = m.features().iter().enumerate().filter(|(i, _)| !carved.contains(i)).map(|(i, f)| {
        let (color, alpha) = match f.combine {
            Combine::Add => (tint(&solid.material), alpha),
            Combine::Subtract => (colours.scene.metal_dark, 255),
            Combine::Intersect => (colours.scene.intersect, 60),
        };
        tagged(Pick::Feature(i), shape(&f.shape, &turned(f.axis, f.at), paint(color, alpha)))
    });
    let inclusions = m.inclusions().iter().enumerate().map(|(i, inc)| {
        let color = inc.material.as_ref().map_or(colours.scene.metal_dark, |name| material_rgb(colours, m, mat(name)));
        tagged(Pick::Inclusion(i), shape(&inc.shape, &turned(Axis::Z, inc.at), paint(color, 255)))
    });
    let carved = carved.first().and_then(|&first| Some(tagged(Pick::Feature(first), carved_metal(m, &solid.shape, colours.scene.metal_dark)?)));
    Built::of(std::iter::once(body).chain(carved).chain(features).chain(inclusions).fold(Scene::default(), merged))
}

/// Imported metal carved by the air placed after it (a horn's solid block and the air of its
/// flare), drawn as the mesh will have it: the region's surface, less the region's own outer
/// faces. `None` when the mesher refuses the region.
fn carved_metal(m: &Model, region: &Shape, colour: Rgb) -> Option<Scene> {
    let surface = region_surface(m)?;
    let bounds = region.bounds();
    let tol = 1e-9 * (bounds.max - bounds.min).norm();
    let outer = |c: [f64; 3]| (0..3).any(|k| (c[k] - bounds.min[k]).abs() <= tol || (c[k] - bounds.max[k]).abs() <= tol);
    let mut scene = Scene::default();
    for t in &surface.triangles {
        let [a, b, c] = t.map(|v| surface.positions.get(v as usize).map_or([0.0; 3], |q| p3(*q)));
        let centre = [0, 1, 2].map(|k| (a[k] + b[k] + c[k]) / 3.0);
        if !outer(centre) {
            scene.tri(a, c, b, colour, 255);
        }
    }
    Some(scene)
}

/// Whether metal feature `i` is an imported surface the region's own material placed after it
/// carves (their boxes meet): drawn from the region's surface ([`region_surface`]).
fn is_carved(m: &Model, i: usize) -> bool {
    let features = m.features();
    features.get(i).is_some_and(|f| {
        f.combine == Combine::Subtract
            && matches!(f.shape, Shape::Surface { .. })
            && features.iter().skip(i + 1).any(|g| g.combine == Combine::Add && g.bounds().overlaps(f.bounds()))
    })
}

/// The region's surface as the mesher will combine it, at a twentieth of the model's size;
/// `None` when the mesher refuses it (the bodies are drawn whole then).
fn region_surface(m: &Model) -> Option<emcraft_mesh::csg::Surface> {
    let b = m.bounds()?;
    let size = emcraft_model::Metres::new((b.max - b.min).norm() / 20.0).ok()?;
    emcraft_mesh::solid::region_surface(m, size).ok()
}

/// The inside's surface before meshing, its faces on ports open: a waveguide's box
/// ([`super::shell::guide_box`]: its outer walls, the irises and steps inside them metal of
/// its own); a solid's own faces (no Boolean features). `None` for the rest, whose walls are
/// drawn thick once meshed.
fn inside(m: &Model) -> Option<Inside> {
    if let Some(w) = m.waveguide() {
        let b = m.bounds()?;
        return w.plain().then(|| super::shell::guide_box(m, b.min.to_array(), b.max.to_array()));
    }
    let solid = m.domain.solid.as_ref()?;
    if !m.features().is_empty() {
        return None;
    }
    let mut out = Inside::default();
    let on_port = super::shell::on_ports(m);
    let interior = super::shell::interior(m);
    let scene = shape(&solid.shape, &|q| q, Paint { color: Rgb::BLACK, alpha: 255, edge: Rgb::BLACK });
    for t in &scene.tris {
        let c = [0, 1, 2].map(|k| (t.p[0][k] + t.p[1][k] + t.p[2][k]) / 3.0);
        out.wall_or_inner(t.p, Side::open_if(on_port(c)), &interior);
    }
    Some(out)
}

/// An imported part's metal round the air it encloses, before and after meshing: see-through,
/// so the region inside shows and takes the clicks.
pub(super) fn part_walls(colours: &Colours, m: &Model) -> Built {
    // The walls (the region body is the region, drawn as it).
    let paint = Paint { color: colours.scene.metal_dark, alpha: 40, edge: colours.scene.edge };
    let walls = m
        .parts()
        .enumerate()
        .filter(|(_, (_, b))| b.kind == emcraft_model::Kind::Walls)
        .map(|(i, (_, b))| tagged(Pick::Part(i), shape(&b.shape, &|q| q, paint)));
    Built::of(walls.fold(Scene::default(), merged))
}
