//! What the 3D view draws, built from the session for one overlay: the mesh's boundary faces
//! (walls, ports) and the faces between materials once there is a mesh, a coarse tessellation of
//! the geometry (the waveguide's sections, layers and strips; boxes, cylinders, spheres and
//! imported surfaces; inclusions and Boolean features) before; ports, lumped ports, sources and
//! the open boundary's layer on top. Pictures come from [`emcraft_render::view3d`].

use emcraft_engine::Session;
use emcraft_model::mesh::Boundary;
use emcraft_model::{Axis, Combine, End, Model, Shape, Source};
use emcraft_render::Rgb;
use emcraft_render::view3d::Scene;

/// What the view puts forward (the active workflow step's subject).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Overlay {
    /// The geometry, walls opaque.
    Plain,
    /// Bodies coloured by material, the outer walls see-through.
    Materials,
    /// Ports, lumped ports and sources highlighted.
    Ports,
    /// Walls and the open boundary's layer shaded.
    Boundaries,
    /// The mesh's edges.
    Mesh,
    /// The field on its plane, inside see-through walls.
    Field,
}

impl Overlay {
    pub fn id(self) -> &'static str {
        match self {
            Overlay::Plain => "plain",
            Overlay::Materials => "materials",
            Overlay::Ports => "ports",
            Overlay::Boundaries => "boundaries",
            Overlay::Mesh => "mesh",
            Overlay::Field => "field",
        }
    }
}

/// What a picked triangle belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pick {
    /// The waveguide's walls (once meshed).
    Guide,
    Section(usize),
    Layer(usize),
    Strip(usize),
    Solid,
    Feature(usize),
    Inclusion(usize),
    Port(usize),
    /// A field plane in the Results step's view.
    Plane(usize),
    Via(usize),
}

impl Pick {
    /// The triangle tag (0 is "nothing").
    pub fn tag(self) -> u32 {
        let (kind, i) = match self {
            Pick::Guide => (1, 0),
            Pick::Section(i) => (2, i),
            Pick::Layer(i) => (3, i),
            Pick::Strip(i) => (4, i),
            Pick::Solid => (5, 0),
            Pick::Feature(i) => (6, i),
            Pick::Inclusion(i) => (7, i),
            Pick::Port(i) => (8, i),
            Pick::Plane(i) => (9, i),
            Pick::Via(i) => (10, i),
        };
        (kind << 16) | (i.min(0xFFFF) as u32)
    }

    pub fn from_tag(tag: u32) -> Option<Pick> {
        let i = (tag & 0xFFFF) as usize;
        Some(match tag >> 16 {
            1 => Pick::Guide,
            2 => Pick::Section(i),
            3 => Pick::Layer(i),
            4 => Pick::Strip(i),
            5 => Pick::Solid,
            6 => Pick::Feature(i),
            7 => Pick::Inclusion(i),
            8 => Pick::Port(i),
            9 => Pick::Plane(i),
            10 => Pick::Via(i),
            _ => return None,
        })
    }
}

/// A symmetry plane's colour: an electric wall blue, a magnetic one green.
fn symmetry_rgb(kind: emcraft_model::SymmetryKind) -> Rgb {
    match kind {
        emcraft_model::SymmetryKind::Electric => Rgb(70, 120, 220),
        emcraft_model::SymmetryKind::Magnetic => Rgb(60, 170, 110),
    }
}

/// The colour picked things turn towards.
const HIGHLIGHT: Rgb = Rgb(30, 120, 255);

/// A text placed in 3D.
#[derive(Clone, Debug, PartialEq)]
pub struct Label {
    pub at: [f64; 3],
    pub text: String,
    pub color: Rgb,
    /// The outward normal of the face it names: the label dims when that face looks away.
    pub facing: Option<[f64; 3]>,
}

/// A scene with its labels and legend.
#[derive(Clone, Debug, Default)]
pub struct Built {
    pub scene: Scene,
    pub labels: Vec<Label>,
    pub legend: Vec<(String, Rgb)>,
    /// The model's box (m).
    pub bounds: Option<([f64; 3], [f64; 3])>,
}

const METAL: Rgb = Rgb(170, 178, 190);
const METAL_DARK: Rgb = Rgb(120, 126, 136);
const PORT: Rgb = Rgb(240, 140, 40);
const PORT_SOFT: Rgb = Rgb(235, 180, 120);
const EDGE: Rgb = Rgb(40, 46, 56);
const PML: Rgb = Rgb(150, 110, 210);
const SOURCE: Rgb = Rgb(220, 40, 60);

/// A material's colour: vacuum pale blue, the others from a fixed palette by their index.
pub fn material_rgb(model: &Model, index: usize) -> Rgb {
    const PALETTE: [Rgb; 6] = [Rgb(214, 150, 60), Rgb(90, 170, 110), Rgb(170, 110, 200), Rgb(60, 150, 190), Rgb(200, 100, 110), Rgb(150, 150, 70)];
    match model.materials.get(index) {
        Some(m) if m.eps_r <= 1.0 + 1e-12 && m.mu_r <= 1.0 + 1e-12 => Rgb(175, 200, 232),
        _ => PALETTE[index.saturating_sub(1) % PALETTE.len()],
    }
}

fn p3(p: emcraft_model::geom::Point3) -> [f64; 3] {
    [p.x, p.y, p.z]
}

/// A field plot on its plane: which plane and where along its normal (m from the model's low
/// side).
pub struct FieldLayer<'a> {
    pub plot: &'a emcraft_render::plot::FieldPlot,
    pub plane: emcraft_engine::view::Plane,
    pub at: f64,
}

/// Builds the scene of `session` for `overlay`.
pub fn build(session: &Session, overlay: Overlay) -> Built {
    build_with(session, overlay, &[], None)
}

/// Builds the scene, with the field's planes for [`Overlay::Field`] and what is selected
/// highlighted.
pub fn build_with(session: &Session, overlay: Overlay, fields: &[FieldLayer], selected: Option<Pick>) -> Built {
    let m = &session.model;
    let mut out = Built { bounds: m.bounds().map(|b| (p3(b.min), p3(b.max))), ..Default::default() };
    match &m.mesh {
        Some(_) => from_mesh(m, overlay, &mut out),
        None => from_geometry(m, overlay, &mut out),
    }
    extras(m, overlay, &mut out);
    legend(session, overlay, &mut out);
    if overlay == Overlay::Field {
        // One colour scale for every plane: the largest peak.
        let peak = fields
            .iter()
            .filter_map(|f| f.plot.scalar.as_ref())
            .map(|s| s.values.iter().filter(|v| v.is_finite()).fold(0.0, |a: f64, b| a.max(b.abs())))
            .fold(0.0, f64::max);
        for (i, f) in fields.iter().enumerate() {
            out.scene.tagged(Pick::Plane(i).tag(), |scene| {
                let mut part = Built { scene: std::mem::take(scene), bounds: out.bounds, ..Default::default() };
                field_plane(session, f, peak, i == 0, &mut part);
                *scene = part.scene;
                out.legend.extend(part.legend);
            });
        }
    }
    if let Some(tag) = selected.map(Pick::tag) {
        for t in out.scene.tris.iter_mut().filter(|t| t.tag == tag) {
            t.color = t.color.mix(HIGHLIGHT, 0.5);
            t.alpha = t.alpha.max(170);
        }
    }
    out
}

/// Most cells along the field plane's longer side.
const FIELD_CELLS: usize = 120;

/// The field's colour map on its plane, cell by cell (cells outside the mesh left out).
fn field_plane(session: &Session, f: &FieldLayer, shared_peak: f64, legend: bool, out: &mut Built) {
    use emcraft_engine::view::Plane;
    use emcraft_render::plot::ScalarKind;
    let (Some(s), Some((lo, _))) = (f.plot.scalar.as_ref(), out.bounds) else { return };
    let (nx, ny) = (s.nx, s.ny);
    if nx < 2 || ny < 2 || s.values.len() < nx * ny {
        return;
    }
    let l = session.display.length;
    let (h0, h1, v0, v1) = (l.to_si(f.plot.x.min), l.to_si(f.plot.x.max), l.to_si(f.plot.y.min), l.to_si(f.plot.y.max));
    let point = |u: f64, v: f64| match f.plane {
        Plane::H => [v, lo[1] + f.at, u],
        Plane::E => [lo[0] + f.at, v, u],
        Plane::Cross => [u, v, lo[2] + f.at],
    };
    let own = s.range.filter(|r| *r > 0.0).unwrap_or_else(|| s.values.iter().filter(|v| v.is_finite()).fold(0.0, |a: f64, b| a.max(b.abs())));
    let peak = if shared_peak > 0.0 { shared_peak } else { own };
    let color = |v: f64| match s.kind {
        ScalarKind::Signed => emcraft_render::colormap::diverging(v / peak.max(1e-300)),
        ScalarKind::Magnitude => emcraft_render::colormap::sequential(v / peak.max(1e-300)),
    };
    let step = nx.max(ny).div_ceil(FIELD_CELLS).max(1);
    let at = |i: usize, j: usize| s.values.get(j * nx + i).copied().unwrap_or(f64::NAN);
    let (mut j, du, dv) = (0, (h1 - h0) / (nx - 1) as f64, (v1 - v0) / (ny - 1) as f64);
    while j + 1 < ny {
        let j1 = (j + step).min(ny - 1);
        let mut i = 0;
        while i + 1 < nx {
            let i1 = (i + step).min(nx - 1);
            let corners = [at(i, j), at(i1, j), at(i1, j1), at(i, j1)];
            if corners.iter().all(|v| v.is_finite()) {
                let mean = corners.iter().sum::<f64>() / 4.0;
                let (u0, u1, w0, w1) = (h0 + du * i as f64, h0 + du * i1 as f64, v0 + dv * j as f64, v0 + dv * j1 as f64);
                out.scene.quad([point(u0, w0), point(u1, w0), point(u1, w1), point(u0, w1)], color(mean), 255);
            }
            i = i1;
        }
        j = j1;
    }
    if legend {
        out.legend.push((f.plot.scalar.as_ref().map_or(String::new(), |s| emcraft_render::math::to_unicode(&s.label)), color(peak)));
    }
    field_arrows(session, f, lo, legend, out);
}

/// The plot's arrows (E's, H's) on the field's plane, filled (a shaft and a head), a hair off
/// the plane on both sides so they show from either.
fn field_arrows(session: &Session, f: &FieldLayer, lo: [f64; 3], legend: bool, out: &mut Built) {
    use emcraft_engine::view::Plane;
    let l = session.display.length;
    let normal = match f.plane {
        Plane::H => 1,
        Plane::E => 0,
        Plane::Cross => 2,
    };
    let (xa, xb) = f.plot.x.span();
    let lift = 1e-3 * l.to_si(xb - xa);
    // Plane coordinates (display units) to the model's (m), `side` lifts off the plane.
    let point = |p: [f64; 2], side: f64| {
        let (u, v) = (l.to_si(p[0]), l.to_si(p[1]));
        let mut q = match f.plane {
            Plane::H => [v, lo[1] + f.at, u],
            Plane::E => [lo[0] + f.at, v, u],
            Plane::Cross => [u, v, lo[2] + f.at],
        };
        q[normal] += side;
        q
    };
    for set in &f.plot.arrows {
        let largest = set.arrows.iter().map(|(_, v)| v[0].hypot(v[1])).filter(|m| m.is_finite()).fold(0.0, f64::max);
        let scale = set.scale.filter(|s| *s > 0.0 && s.is_finite()).unwrap_or(largest);
        if !(scale > 0.0) {
            continue;
        }
        let k = f.plot.arrow_length / scale;
        for (p, v) in &set.arrows {
            let d = [v[0] * k, v[1] * k];
            let len = d[0].hypot(d[1]);
            if !(len.is_finite() && len > 0.08 * f.plot.arrow_length) {
                continue;
            }
            let (u, n) = ([d[0] / len, d[1] / len], [-d[1] / len, d[0] / len]);
            // In plane coordinates: `along` the arrow from its tail, `across` it.
            let at = |along: f64, across: f64| [p[0] - d[0] / 2.0 + along * u[0] + across * n[0], p[1] - d[1] / 2.0 + along * u[1] + across * n[1]];
            let (head, shaft) = (0.4 * len, (0.07 * f.plot.arrow_length).min(0.15 * len));
            for side in [lift, -lift] {
                let q = |along: f64, across: f64| point(at(along, across), side);
                out.scene.quad([q(0.0, -shaft / 2.0), q(len - head, -shaft / 2.0), q(len - head, shaft / 2.0), q(0.0, shaft / 2.0)], set.color, 255);
                out.scene.tri(q(len - head, -0.45 * head), q(len, 0.0), q(len - head, 0.45 * head), set.color, 255);
            }
        }
        if legend {
            out.legend.push((emcraft_render::math::to_unicode(&set.label), set.color));
        }
    }
}

fn legend(session: &Session, overlay: Overlay, out: &mut Built) {
    let m = &session.model;
    let l = session.display.length;
    match overlay {
        Overlay::Materials => {
            for (i, mat) in m.materials.iter().enumerate() {
                out.legend.push((format!("{} (εr {})", mat.name, mat.eps_r), material_rgb(m, i)));
            }
        }
        Overlay::Ports => {
            for p in &m.ports {
                out.legend.push((format!("port {} ({})", p.name, if p.end == End::Start { "start" } else { "end" }), PORT));
            }
            for p in &m.lumped {
                out.legend.push((format!("lumped port {}", p.name), PORT));
            }
            if !m.sources.is_empty() {
                out.legend.push((format!("{} source(s)", m.sources.len()), SOURCE));
            }
        }
        Overlay::Boundaries => {
            for s in &m.symmetry {
                let wall = match s.kind {
                    emcraft_model::SymmetryKind::Electric => "electric wall",
                    emcraft_model::SymmetryKind::Magnetic => "magnetic wall",
                };
                let axis = if s.axis == Axis::X { "x" } else { "y" };
                out.legend.push((format!("symmetry: {wall}, {axis} = {} {}", l.from_si(s.at), l.symbol()), symmetry_rgb(s.kind)));
            }
            out.legend.push((
                match m.wall_conductivity {
                    Some(s) => format!("walls σ = {s:.3e} S/m"),
                    None => "walls: perfect conductor".into(),
                },
                METAL,
            ));
            if let Some(o) = &m.open {
                out.legend.push((format!("open boundary: PML {} {}", l.from_si(o.thickness), l.symbol()), PML));
            }
        }
        Overlay::Plain | Overlay::Mesh | Overlay::Field => {}
    }
}

/// The mesh's boundary faces and, for materials, the faces between materials.
fn from_mesh(m: &Model, overlay: Overlay, out: &mut Built) {
    let Some(mesh) = &m.mesh else { return };
    let node = |n: u32| mesh.nodes.get(n as usize).map(|p| p3(*p));
    let tet_mat = |t: u32| mesh.tet_material.get(t as usize).copied().unwrap_or(0) as usize;
    let see_through = matches!(overlay, Overlay::Materials | Overlay::Ports | Overlay::Boundaries | Overlay::Field);
    // Faces, their keys for edges (material or tag).
    let mut faces: Vec<([u32; 3], u32)> = Vec::with_capacity(mesh.boundary.len());
    for f in &mesh.boundary {
        let Some([a, b, c]) = f.nodes.iter().map(|n| node(*n)).collect::<Option<Vec<_>>>().and_then(|v| <[[f64; 3]; 3]>::try_from(v).ok()) else {
            continue;
        };
        // An end the mesh tags as a port is a wall until a port is put there.
        let port = match f.tag {
            Boundary::Port(k) => m.ports.iter().any(|p| (p.end == End::Start) == (k == 0)),
            Boundary::Pec | Boundary::Symmetry(_) => false,
        };
        let (color, alpha) = match overlay {
            Overlay::Plain => (if port { PORT_SOFT } else { METAL }, 255),
            Overlay::Mesh => (if port { PORT_SOFT } else { Rgb(214, 220, 228) }, 255),
            Overlay::Field => (METAL, 40),
            Overlay::Materials => (material_rgb(m, tet_mat(f.tet)), if port { 200 } else { 120 }),
            Overlay::Ports => {
                if port {
                    (PORT, 255)
                } else {
                    (METAL, 55)
                }
            }
            Overlay::Boundaries => {
                if port {
                    (PORT_SOFT, 200)
                } else {
                    (METAL_DARK, 110)
                }
            }
        };
        // A symmetry plane is no wall: its own colour in every overlay.
        let (color, alpha) = match f.tag {
            Boundary::Symmetry(kind) => (symmetry_rgb(kind), if overlay == Overlay::Boundaries { 200 } else { alpha.min(120) }),
            _ => (color, alpha),
        };
        let pick = match f.tag {
            Boundary::Port(k) if port => m.ports.iter().position(|p| p.face.is_none() && (p.end == End::Start) == (k == 0)).map(Pick::Port),
            _ => None,
        }
        .unwrap_or(if m.waveguide.is_some() { Pick::Guide } else { Pick::Solid });
        out.scene.tagged(pick.tag(), |s| s.tri(a, b, c, color, alpha));
        let key = match f.tag {
            Boundary::Port(k) if port => 1 + k,
            _ => 0,
        };
        faces.push((f.nodes, key));
    }
    // Faces between materials (opaque under the see-through walls).
    if overlay == Overlay::Materials {
        let mut keyed: Vec<([u32; 3], u32)> = Vec::with_capacity(mesh.tets.len() * 4);
        for (t, tet) in mesh.tets.iter().enumerate() {
            for skip in 0..4 {
                let mut f = [0u32; 3];
                let mut k = 0;
                for (i, n) in tet.iter().enumerate() {
                    if i != skip && k < 3 {
                        f[k] = *n;
                        k += 1;
                    }
                }
                f.sort_unstable();
                keyed.push((f, t as u32));
            }
        }
        keyed.sort_unstable();
        for w in keyed.windows(2) {
            if w[0].0 == w[1].0 {
                let (ma, mb) = (tet_mat(w[0].1), tet_mat(w[1].1));
                if ma != mb {
                    let denser = if m.materials.get(ma).map_or(1.0, |x| x.eps_r) >= m.materials.get(mb).map_or(1.0, |x| x.eps_r) { ma } else { mb };
                    if let (Some(a), Some(b), Some(c)) = (node(w[0].0[0]), node(w[0].0[1]), node(w[0].0[2])) {
                        out.scene.tri(a, b, c, material_rgb(m, denser), 235);
                    }
                }
            }
        }
    }
    // Edges: every one for the mesh view, else the outline (sharp folds, where tags change).
    let mut edges: Vec<([u32; 2], usize)> = Vec::with_capacity(faces.len() * 3);
    for (i, (f, _)) in faces.iter().enumerate() {
        for (a, b) in [(f[0], f[1]), (f[1], f[2]), (f[2], f[0])] {
            edges.push(([a.min(b), a.max(b)], i));
        }
    }
    edges.sort_unstable();
    let normal = |i: usize| -> Option<[f64; 3]> {
        let f = faces.get(i)?.0;
        let (a, b, c) = (node(f[0])?, node(f[1])?, node(f[2])?);
        let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        (l > 0.0).then(|| [n[0] / l, n[1] / l, n[2] / l])
    };
    let cos_sharp = 25f64.to_radians().cos();
    let (edge_color, edge_alpha) = if overlay == Overlay::Mesh { (EDGE, 150) } else { (EDGE, if see_through { 120 } else { 200 }) };
    let mut i = 0;
    while i < edges.len() {
        let mut j = i + 1;
        while j < edges.len() && edges[j].0 == edges[i].0 {
            j += 1;
        }
        let group = &edges[i..j];
        let show = overlay == Overlay::Mesh
            || group.len() != 2
            || faces.get(group[0].1).map(|f| f.1) != faces.get(group[1].1).map(|f| f.1)
            || match (normal(group[0].1), normal(group[1].1)) {
                (Some(a), Some(b)) => (a[0] * b[0] + a[1] * b[1] + a[2] * b[2]).abs() < cos_sharp,
                _ => false,
            };
        if show && let (Some(a), Some(b)) = (node(group[0].0[0]), node(group[0].0[1])) {
            out.scene.line(a, b, edge_color, edge_alpha);
        }
        i = j;
    }
}

/// A box's six faces and twelve edges.
fn cuboid(s: &mut Scene, lo: [f64; 3], hi: [f64; 3], color: Rgb, alpha: u8, edges: bool) {
    let c = |i: usize| [if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] }];
    for q in [[0, 1, 3, 2], [4, 6, 7, 5], [0, 4, 5, 1], [2, 3, 7, 6], [0, 2, 6, 4], [1, 5, 7, 3]] {
        s.quad(q.map(c), color, alpha);
    }
    if edges {
        for (a, b) in [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)] {
            s.line(c(a), c(b), EDGE, 200);
        }
    }
}

/// Segments round a circle.
const ROUND: usize = 48;

/// A primitive placed by `place` (its own coordinates to the world's).
fn shape(s: &mut Scene, sh: &Shape, place: &dyn Fn([f64; 3]) -> [f64; 3], color: Rgb, alpha: u8) {
    use std::f64::consts::{PI, TAU};
    match sh {
        Shape::Box { size } => {
            let c = |i: usize| {
                place([if i & 1 == 0 { 0.0 } else { size.x }, if i & 2 == 0 { 0.0 } else { size.y }, if i & 4 == 0 { 0.0 } else { size.z }])
            };
            for q in [[0, 1, 3, 2], [4, 6, 7, 5], [0, 4, 5, 1], [2, 3, 7, 6], [0, 2, 6, 4], [1, 5, 7, 3]] {
                s.quad(q.map(c), color, alpha);
            }
            for (a, b) in [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)] {
                s.line(c(a), c(b), EDGE, 200);
            }
        }
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

fn turned(axis: Axis, at: emcraft_model::geom::Point3) -> impl Fn([f64; 3]) -> [f64; 3] {
    move |q: [f64; 3]| {
        let v = axis.turn(emcraft_model::geom::Vec3::new(q[0], q[1], q[2]));
        [at.x + v.x, at.y + v.y, at.z + v.z]
    }
}

/// The geometry before meshing.
fn from_geometry(m: &Model, overlay: Overlay, out: &mut Built) {
    let mat = |name: &str| m.materials.iter().position(|x| x.name == name).unwrap_or(0);
    let tint = |name: &str| if overlay == Overlay::Materials { material_rgb(m, mat(name)) } else { METAL };
    let see_through = matches!(overlay, Overlay::Ports | Overlay::Boundaries | Overlay::Field);
    if let Some(w) = &m.waveguide {
        let mut z = 0.0;
        // A planar circuit's conductors lie inside: its walls let them show.
        let layered = !w.layers.is_empty() || !w.strips.is_empty() || !w.vias.is_empty();
        for (i, s) in w.sections.iter().enumerate() {
            let emcraft_model::Span { start: x0, width: a } = w.aperture(i).unwrap_or(w.across_x());
            let emcraft_model::Span { start: y0, width: b } = w.aperture_y(i).unwrap_or(w.across_y());
            let alpha = if see_through || layered {
                70
            } else if overlay == Overlay::Materials {
                200
            } else {
                255
            };
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
        let alpha = if see_through || hollow {
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
    for (i, p) in m.ports.iter().enumerate() {
        let (color, alpha) = if overlay == Overlay::Ports { (PORT, 255) } else { (PORT_SOFT, 220) };
        let face = port_face(m, p);
        out.scene.tagged(Pick::Port(i).tag(), |scene| {
            for [a, b, c] in face {
                scene.tri(a, b, c, color, alpha);
            }
        });
    }
}

/// Cells across a port's face where its cross-section is sampled (a face that features reach).
const PORT_CELLS: usize = 96;

/// A port's face as triangles: its rectangle, its circle, or — where Boolean features reach the
/// end face, so no closed form describes it — the cells of a grid over the end whose centres lie
/// in the body (a coaxial line's annulus).
fn port_face(m: &Model, p: &emcraft_model::Port) -> Vec<[[f64; 3]; 3]> {
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
    let mut out = Vec::new();
    let mut i = 0;
    while i < edges.len() {
        let mut j = i + 1;
        while j < edges.len() && (edges[j].0, edges[j].1) == (edges[i].0, edges[i].1) {
            j += 1;
        }
        if j - i == 1 {
            out.push((edges[i].2, edges[i].3));
        }
        i = j;
    }
    out
}

/// The plane of a port's face.
fn port_z(m: &Model, p: &emcraft_model::Port) -> Option<f64> {
    Some(match (p.face, p.end) {
        (Some(f), _) => f.z,
        (None, End::Start) => m.bounds()?.min.z.min(0.0),
        (None, End::End) => m.bounds()?.max.z,
    })
}

/// A port's face's bounding rectangle in 3D.
fn port_quad(m: &Model, p: &emcraft_model::Port) -> Option<[[f64; 3]; 4]> {
    let face = port_face(m, p);
    let pts = face.iter().flatten();
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for q in pts {
        for k in 0..2 {
            lo[k] = lo[k].min(q[k]);
            hi[k] = hi[k].max(q[k]);
        }
    }
    let z = face.first()?[0][2];
    Some([[lo[0], lo[1], z], [hi[0], lo[1], z], [hi[0], hi[1], z], [lo[0], hi[1], z]])
}

/// Ports' labels, lumped ports, sources and the open boundary's layer.
fn extras(m: &Model, overlay: Overlay, out: &mut Built) {
    let size = out.bounds.map_or(0.01, |(lo, hi)| ((hi[0] - lo[0]).powi(2) + (hi[1] - lo[1]).powi(2) + (hi[2] - lo[2]).powi(2)).sqrt());
    if matches!(overlay, Overlay::Ports | Overlay::Plain | Overlay::Boundaries) {
        for p in &m.ports {
            if let Some(q) = port_quad(m, p) {
                let c = [0.25 * (q[0][0] + q[1][0] + q[2][0] + q[3][0]), 0.25 * (q[0][1] + q[1][1] + q[2][1] + q[3][1]), q[0][2]];
                let facing = match (p.face, p.end) {
                    (Some(_), _) => None,
                    (None, End::Start) => Some([0.0, 0.0, -1.0]),
                    (None, End::End) => Some([0.0, 0.0, 1.0]),
                };
                out.labels.push(Label { at: c, text: format!("Port {}", p.name), color: PORT, facing });
                if overlay == Overlay::Ports {
                    for (a, b) in outline(&port_face(m, p)) {
                        out.scene.line(a, b, PORT, 255);
                    }
                    // The direction into the guide.
                    let dir = if p.end == End::Start { 1.0 } else { -1.0 };
                    out.scene.line(c, [c[0], c[1], c[2] + dir * 0.12 * size], PORT, 255);
                }
            }
        }
    }
    let strong = overlay == Overlay::Ports;
    for p in &m.lumped {
        let (a, b) = (p3(p.from), p3(p.to));
        out.scene.line(a, b, PORT, 255);
        if strong {
            out.labels.push(Label {
                at: [0.5 * (a[0] + b[0]), 0.5 * (a[1] + b[1]), 0.5 * (a[2] + b[2])],
                text: format!("Lumped {}", p.name),
                color: PORT,
                facing: None,
            });
        }
    }
    for s in &m.sources {
        match *s {
            Source::Dipole { at, direction, .. } => {
                let (a, d) = (p3(at), [direction.x, direction.y, direction.z]);
                let h = 0.08 * size;
                out.scene.line([a[0] - d[0] * h, a[1] - d[1] * h, a[2] - d[2] * h], [a[0] + d[0] * h, a[1] + d[1] * h, a[2] + d[2] * h], SOURCE, 255);
                if strong {
                    out.labels.push(Label { at: a, text: "Dipole".into(), color: SOURCE, facing: None });
                }
            }
            Source::PlaneWave { direction, .. } => {
                if let Some((lo, hi)) = out.bounds {
                    let c = [0.5 * (lo[0] + hi[0]), 0.5 * (lo[1] + hi[1]), 0.5 * (lo[2] + hi[2])];
                    let d = [direction.x, direction.y, direction.z];
                    let h = 0.5 * size;
                    let a = [c[0] - d[0] * h, c[1] - d[1] * h, c[2] - d[2] * h];
                    out.scene.line(a, [c[0] - d[0] * 0.3 * h, c[1] - d[1] * 0.3 * h, c[2] - d[2] * 0.3 * h], SOURCE, 255);
                    if strong {
                        out.labels.push(Label { at: a, text: "Plane wave".into(), color: SOURCE, facing: None });
                    }
                }
            }
        }
    }
    // The open boundary: the layer's inner face, shaded when boundaries are the subject.
    if let (Some(o), Some((lo, hi))) = (&m.open, out.bounds) {
        let t = o.thickness;
        let (a, b) = ([lo[0] + t, lo[1] + t, lo[2] + t], [hi[0] - t, hi[1] - t, hi[2] - t]);
        if overlay == Overlay::Boundaries {
            cuboid(&mut out.scene, a, b, PML, 60, false);
            out.labels.push(Label { at: [b[0], b[1], b[2]], text: "PML".into(), color: PML, facing: None });
        }
        let c = |i: usize| [if i & 1 == 0 { a[0] } else { b[0] }, if i & 2 == 0 { a[1] } else { b[1] }, if i & 4 == 0 { a[2] } else { b[2] }];
        for (i, j) in [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)] {
            out.scene.line(c(i), c(j), PML, 255);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn every_overlay_of_every_sample_builds() {
        for name in ["highpass", "coaxial-line", "dipole", "loaded-sphere"] {
            let mut s = Session::new();
            let script: serde_json::Value = serde_json::from_str(emcraft_engine::sample::find(name).unwrap().script).unwrap();
            // Without the solves: geometry, then the mesh.
            let steps: Vec<serde_json::Value> =
                script.as_array().unwrap().iter().filter(|c| !c["command"].as_str().unwrap().starts_with("solve.")).cloned().collect();
            let unmeshed: Vec<serde_json::Value> = steps.iter().filter(|c| c["command"] != json!("mesh.generate")).cloned().collect();
            // Meshing the open and curved samples takes seconds: their meshes are drawn in the
            // timing report and the scenarios.
            let scripts = if name == "highpass" { vec![unmeshed, steps] } else { vec![unmeshed] };
            for script in scripts {
                s.run_script(&json!(script)).unwrap();
                for o in [Overlay::Plain, Overlay::Materials, Overlay::Ports, Overlay::Boundaries, Overlay::Mesh] {
                    let b = build(&s, o);
                    assert!(!b.scene.tris.is_empty(), "{name} {o:?}");
                    assert!(b.bounds.is_some());
                }
                s = Session::new();
            }
        }
    }

    fn meshed(name: &str, size: Option<f64>) -> Session {
        let script: serde_json::Value = serde_json::from_str(emcraft_engine::sample::find(name).unwrap().script).unwrap();
        let steps: Vec<serde_json::Value> = script
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| !c["command"].as_str().unwrap().starts_with("solve."))
            .map(|c| match (size, c["command"].as_str()) {
                (Some(h), Some("mesh.generate")) => json!({"command": "mesh.generate", "params": {"size": h}}),
                _ => c.clone(),
            })
            .collect();
        let mut s = Session::new();
        s.run_script(&json!(steps)).unwrap();
        s
    }

    /// Milliseconds per picture of the samples' meshes at the window's 3D view size (about
    /// 1180 × 620): `cargo test --release -p emcraft-ui-egui render_timings -- --ignored --nocapture`.
    #[test]
    #[ignore = "timing report: run by hand"]
    fn render_timings() {
        use emcraft_render::view3d::{Camera, Options, render};
        for (name, size) in [("highpass", None), ("coaxial-line", None), ("coaxial-line", Some(0.45)), ("coaxial-line", Some(0.25))] {
            let s = meshed(name, size);
            let faces = s.model.mesh.as_ref().map_or(0, |m| m.boundary.len());
            for o in [Overlay::Plain, Overlay::Mesh, Overlay::Materials] {
                let b = build(&s, o);
                let (lo, hi) = b.bounds.unwrap();
                let mut c = Camera::default();
                c.fit(lo, hi, 1180.0 / 620.0);
                for (ss, w, h) in [(1, 1180, 620), (2, 1180, 620), (1, 590, 310)] {
                    let opt = Options { width: w, height: h, supersample: ss, ..Default::default() };
                    let t = std::time::Instant::now();
                    let n = 5;
                    for k in 0..n {
                        c.orbit(3.0 * k as f64, 0.0);
                        let _ = render(&b.scene, &c, &opt);
                    }
                    println!(
                        "{name} ({faces} boundary faces) {o:?}: {} triangles, {} lines, {w}x{h} x{ss}: {:.1} ms",
                        b.scene.tris.len(),
                        b.scene.lines.len(),
                        t.elapsed().as_secs_f64() * 1e3 / n as f64
                    );
                }
            }
        }
    }
}
