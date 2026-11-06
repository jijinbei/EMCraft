//! What the 3D view draws, built from the session for one overlay: the mesh's boundary faces
//! (walls, ports) and the faces between materials once there is a mesh ([`mesh`], with the
//! faces told apart in [`surface`]), a coarse tessellation of the geometry (the waveguide's
//! sections, layers and strips; boxes, cylinders, spheres and imported surfaces; inclusions and
//! Boolean features) before ([`geometry`]); ports, lumped ports, sources and the open boundary's
//! layer on top ([`extras`]); the field on its planes ([`field`]); the legend ([`legend`]) in
//! the colours of [`style`]. Pictures come from [`emcraft_render::view3d`].

use emcraft_engine::Session;
use emcraft_render::Rgb;
use emcraft_render::view3d::{Camera, Hit, Scene};

pub use cache::Cache;

pub(crate) mod board;
mod cache;
mod extras;
pub(crate) mod faces;
mod field;
mod geometry;
mod legend;
mod mesh;
mod shell;
mod style;
mod surface;

pub(crate) use extras::{along, port_normal, port_quad, quad_centre};
pub(crate) use field::normal_axis;
pub(crate) use geometry::{BOX_EDGES, box_corner};
pub use style::material_rgb;
pub(crate) use style::wire_rgb;

emcraft_macros::named_enum! {
    /// What the view puts forward (the active workflow step's subject).
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum Overlay {
        /// The geometry, walls opaque.
        Plain = "plain",
        /// Bodies coloured by material, the outer walls see-through.
        Materials = "materials",
        /// Ports, lumped ports and sources highlighted.
        Ports = "ports",
        /// Walls and the open boundary's layer shaded.
        Boundaries = "boundaries",
        /// The mesh's edges.
        Mesh = "mesh",
        /// The field on its plane, inside see-through walls.
        Field = "field",
        /// A circuit board: its layers' copper, substrates and vias ([`board`]).
        Board = "board",
    }
}

/// What a picked triangle belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pick {
    /// The waveguide's walls (once meshed).
    Guide,
    Material(usize),
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
    /// A lumped port's marker.
    Lumped(usize),
    /// A board's net's copper (an index into [`board::nets`]).
    Net(usize),
}

impl Pick {
    pub(crate) fn display_object(self, m: &emcraft_model::Model) -> Option<emcraft_engine::visibility::DisplayObject> {
        use emcraft_engine::visibility::{DisplayObject as O, GeometryObject as G};
        Some(match self {
            Self::Material(i) => O::Material(m.materials.get(i)?.name.clone()),
            Self::Guide => O::Geometry(G::Guide),
            Self::Solid => O::Geometry(G::Solid),
            Self::Section(i) => O::Geometry(G::Section(i)),
            Self::Layer(i) => O::Geometry(G::Layer(i)),
            Self::Strip(i) => O::Geometry(G::Strip(i)),
            Self::Via(i) => O::Geometry(G::Via(i)),
            Self::Feature(i) => O::Geometry(G::Feature(i)),
            Self::Inclusion(i) => O::Geometry(G::Inclusion(i)),
            _ => return None,
        })
    }

    /// The triangle tag (0 is "nothing").
    pub fn tag(self) -> u32 {
        let (kind, i) = match self {
            Pick::Guide => (1, 0),
            Pick::Material(i) => (12, i),
            Pick::Section(i) => (2, i),
            Pick::Layer(i) => (3, i),
            Pick::Strip(i) => (4, i),
            Pick::Solid => (5, 0),
            Pick::Feature(i) => (6, i),
            Pick::Inclusion(i) => (7, i),
            Pick::Port(i) => (8, i),
            Pick::Plane(i) => (9, i),
            Pick::Via(i) => (10, i),
            Pick::Lumped(i) => (11, i),
            Pick::Net(i) => (13, i),
        };
        (kind << 16) | (i.min(0xFFFF) as u32)
    }

    pub fn from_tag(tag: u32) -> Option<Pick> {
        let i = (tag & 0xFFFF) as usize;
        Some(match tag >> 16 {
            1 => Pick::Guide,
            12 => Pick::Material(i),
            2 => Pick::Section(i),
            3 => Pick::Layer(i),
            4 => Pick::Strip(i),
            5 => Pick::Solid,
            6 => Pick::Feature(i),
            7 => Pick::Inclusion(i),
            8 => Pick::Port(i),
            9 => Pick::Plane(i),
            10 => Pick::Via(i),
            11 => Pick::Lumped(i),
            13 => Pick::Net(i),
            _ => return None,
        })
    }
}

/// A text placed in 3D.
#[derive(Clone, Debug, PartialEq)]
pub struct Label {
    pub at: [f64; 3],
    pub text: String,
    pub color: Rgb,
    /// The outward normal of the face it names: the label dims when that face looks away.
    pub facing: Option<[f64; 3]>,
    /// The point it names when it stands off from it, joined to it by a leader line.
    pub anchor: Option<[f64; 3]>,
}

/// A scene with its labels and legend.
#[derive(Clone, Debug, Default)]
pub struct Built {
    pub scene: Scene,
    pub(crate) gpu_scene: Option<Scene>,
    pub(crate) arrows: Vec<crate::gpu3d::Arrow>,
    pub labels: Vec<Label>,
    pub legend: Vec<(String, Rgb)>,
    /// The model's box (m).
    pub bounds: Option<([f64; 3], [f64; 3])>,
    /// Its picking index and coarser copy, made once ([`Cache`]).
    pub cache: Cache,
}

impl Built {
    /// [`emcraft_render::view3d::hits`] on its scene, through its index once made.
    pub fn hits(&self, camera: &Camera, x: f64, y: f64, w: f64, h: f64) -> Option<(Hit, Option<Hit>)> {
        match self.cache.index(&self.scene) {
            Some(index) => camera.ray(x, y, w, h).and_then(|r| index.hits(&self.scene, r)),
            None => emcraft_render::view3d::hits(&self.scene, camera, x, y, w, h),
        }
    }

    /// [`emcraft_render::view3d::pick`] on its scene, through its index once made.
    pub fn pick(&self, camera: &Camera, x: f64, y: f64, w: f64, h: f64) -> Option<u32> {
        self.hits(camera, x, y, w, h).map(|(any, solid)| solid.unwrap_or(any).tag)
    }

    /// What a moving picture draws: its coarser copy when it has one made, else its scene.
    pub fn moving(&self) -> &Scene {
        self.cache.coarse(&self.scene).unwrap_or(&self.scene)
    }
}

/// A picture's label as plain text for the legend: TeX math in Unicode, subscripts written on
/// the line (the UI's font lacks most subscript letters: `$E_z$` reads `Ez`).
fn plain(label: &str) -> String {
    emcraft_render::math::to_unicode(&label.replace('_', ""))
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
    /// The colour map is the GPU's (`gpu3d::field`): the plane is one quad here.
    pub gpu: bool,
}

/// A field plane's corners (m) — the picture's low corner and its far ends along the picture's
/// axes — for a plane `at` (m from the model's low corner `lo`) with picture spans `x`, `y`
/// (display units).
pub fn field_frame_of(session: &Session, plane: emcraft_engine::view::Plane, at: f64, x: (f64, f64), y: (f64, f64), lo: [f64; 3]) -> [[f64; 3]; 3] {
    field::plane_frame(session, plane, at, x, y, lo)
}

/// The perimeter of every mesh cell intersected by a section plane, in SI metres.
/// No filled faces or artificial diagonals are added. Invalid planes give an empty scene.
///
/// ```
/// use emcraft_ui_egui::scene3d::mesh_slice;
/// let model = emcraft_model::Model::default();
/// assert!(mesh_slice(&model, 0, 0.005).tris.is_empty());
/// assert!(mesh_slice(&model, 3, f64::NAN).lines.is_empty());
/// ```
pub fn mesh_slice(m: &emcraft_model::Model, axis: usize, at: f64) -> Scene {
    mesh::slice(m, axis, at, true)
}

pub(crate) fn section_outline(m: &emcraft_model::Model, axis: usize, at: f64) -> Scene {
    mesh::slice(m, axis, at, false)
}

/// Builds the scene of `session` for `overlay`.
pub fn build(session: &Session, overlay: Overlay) -> Built {
    build_with(session, overlay, &[], &[])
}

/// A model cut by symmetry planes drawn whole: what is meshed and its mirror images across each
/// plane, so the view shows the model, not the half it is solved on. The Mesh step tells them
/// apart: the images faint and without the mesh's edges.
fn mirrored_halves(m: &emcraft_model::Model, overlay: Overlay, scene: &mut emcraft_render::view3d::Scene) {
    let faint = overlay == Overlay::Mesh;
    for s in &m.symmetry {
        let k = s.axis.index();
        let mirror = |p: [f64; 3]| {
            let mut q = p;
            q[k] = 2.0 * s.at - p[k];
            q
        };
        let tris: Vec<_> = scene
            .tris
            .iter()
            .map(|t| {
                let mut c = *t;
                // Mirrored, the winding turns over: two corners exchanged keep it outward.
                c.p = [mirror(t.p[0]), mirror(t.p[2]), mirror(t.p[1])];
                if faint {
                    c.alpha = t.alpha / 3;
                    c.wire = None;
                }
                c
            })
            .collect();
        scene.tris.extend(tris);
        if !faint {
            let lines: Vec<_> = scene.lines.iter().map(|l| emcraft_render::view3d::Line { a: mirror(l.a), b: mirror(l.b), ..*l }).collect();
            scene.lines.extend(lines);
        }
    }
}

/// A sector of a rotationally symmetric model drawn whole: what is meshed and its n − 1 turns
/// about the axis. The Mesh step tells them apart as it does mirror images: faint, no edges.
fn turned_sectors(m: &emcraft_model::Model, overlay: Overlay, scene: &mut emcraft_render::view3d::Scene) {
    let Some(r) = m.rotation else { return };
    let faint = overlay == Overlay::Mesh;
    let (tris, lines) = (scene.tris.clone(), scene.lines.clone());
    for k in 1..r.n {
        let angle = f64::from(k) * r.angle();
        let turn = |p: [f64; 3]| {
            let q = r.turned_by(emcraft_model::Point3::new(p[0], p[1], p[2]), angle);
            [q.x, q.y, q.z]
        };
        // A turn keeps the winding.
        scene.tris.extend(tris.iter().map(|t| {
            let mut c = *t;
            c.p = t.p.map(turn);
            if faint {
                c.alpha = t.alpha / 3;
                c.wire = None;
            }
            c
        }));
        if !faint {
            scene.lines.extend(lines.iter().map(|l| emcraft_render::view3d::Line { a: turn(l.a), b: turn(l.b), ..*l }));
        }
    }
}

/// Builds the scene, with the field's planes for [`Overlay::Field`] and what is selected
/// highlighted.
pub fn build_with(session: &Session, overlay: Overlay, fields: &[FieldLayer], selected: &[Pick]) -> Built {
    let m = session.model();
    if overlay == Overlay::Board {
        let mut out = Built::default();
        if let Some(b) = &m.pcb {
            let route = session.views().pcb.net().unwrap_or("");
            let reference = session.views().pcb.conditions().map_or("GND", |c| c.ground.as_str());
            board::build(b, route, reference, &mut out);
        }
        return out;
    }
    let mut out = Built { bounds: m.bounds().map(|b| (p3(b.min), p3(b.max))), ..Default::default() };
    match &m.mesh {
        Some(_) => {
            mesh::from_mesh(
                session,
                overlay,
                &mut out,
                selected.iter().find_map(|p| {
                    if let Pick::Material(i) = p {
                        m.materials.get(*i).filter(|m| !session.views().hidden_materials.contains(&m.name)).map(|_| *i)
                    } else {
                        None
                    }
                }),
            );
            mirrored_halves(m, overlay, &mut out.scene);
            turned_sectors(m, overlay, &mut out.scene);
        }
        None => {
            geometry::from_geometry(m, overlay, &mut out);
            extras::symmetry_planes(m, &mut out);
            extras::sector_cuts(m, &mut out);
            // A board's region before it is meshed: its copper and substrates in place.
            if let Some(b) = &m.pcb {
                board::in_model(b, &mut out);
            }
        }
    }
    geometry::part_walls(m, &mut out);
    present_materials(session, selected, &mut out);
    filter_geometry(session, &mut out);
    extras::extras(m, session.driven_port(), overlay, &mut out);
    legend::legend(session, overlay, &mut out);
    if overlay == Overlay::Field {
        // One colour scale for every plane: the largest peak.
        let peak = fields.iter().filter_map(|f| f.plot.scalar.as_ref()).map(|s| field::largest_magnitude(&s.values)).fold(0.0, f64::max);
        for (i, f) in fields.iter().enumerate() {
            out.scene.tagged(Pick::Plane(i).tag(), |scene| {
                let mut part = Built { scene: std::mem::take(scene), bounds: out.bounds, ..Default::default() };
                field::field_plane(session, f, peak, i == 0, &mut part);
                *scene = part.scene;
                out.legend.extend(part.legend);
                out.arrows.extend(part.arrows);
            });
        }
    }
    let tags: std::collections::BTreeSet<u32> = selected.iter().map(|p| p.tag()).collect();
    if !tags.is_empty() {
        for t in out.scene.tris.iter_mut().filter(|t| tags.contains(&t.tag)) {
            t.color = t.color.mix(style::sc().highlight, 0.5);
            t.alpha = t.alpha.max(170);
        }
        highlight_outline(&mut out.scene, &tags);
    }
    out
}

/// Material regions are display objects; hiding them never removes physical walls or ports.
fn present_materials(session: &Session, selected: &[Pick], out: &mut Built) {
    let m = session.model();
    out.scene.tris.retain_mut(|t| {
        let index = match Pick::from_tag(t.tag) {
            Some(Pick::Material(i)) => Some(i),
            Some(Pick::Section(i)) if m.mesh.is_none() => {
                m.waveguide.as_ref().and_then(|w| w.sections.get(i)).and_then(|s| m.material(&s.material).ok()).map(|(i, _)| i)
            }
            Some(Pick::Layer(i)) => m.waveguide.as_ref().and_then(|w| w.layers.get(i)).and_then(|s| m.material(&s.material).ok()).map(|(i, _)| i),
            Some(Pick::Solid) if m.mesh.is_none() => m.solid.as_ref().and_then(|s| m.material(&s.material).ok()).map(|(i, _)| i),
            Some(Pick::Inclusion(i)) => m.inclusions.get(i).and_then(|s| s.material.as_ref()).and_then(|s| m.material(s).ok()).map(|(i, _)| i),
            _ => None,
        };
        let Some(i) = index else { return true };
        let Some(mat) = m.materials.get(i) else { return true };
        if session.views().hidden_materials.contains(&mat.name) {
            return false;
        }
        if selected.contains(&Pick::Material(i)) {
            t.tag = Pick::Material(i).tag();
            t.color = style::sc().highlight;
            t.alpha = 120;
        } else if mat.name == "vacuum" {
            // Section caps are artificial region boundaries, not metal closing the bore.
            if matches!(Pick::from_tag(t.tag), Some(Pick::Section(_))) {
                if t.p.iter().all(|p| (p[2] - t.p[0][2]).abs() < 1e-12) {
                    let z = t.p[0][2];
                    let length = m.waveguide.as_ref().map_or(0.0, emcraft_model::Waveguide::length);
                    let start = z.abs() < 1e-12;
                    let end = (z - length).abs() < 1e-12;
                    let port = m
                        .ports
                        .iter()
                        .any(|p| p.face.is_none() && ((start && p.end == emcraft_model::End::Start) || (end && p.end == emcraft_model::End::End)));
                    if (!start && !end) || port {
                        return false;
                    }
                }
                t.color = style::sc().metal;
                t.alpha = 45;
            } else {
                t.color = style::sc().metal;
                t.alpha = if m.open.is_some() { 12 } else { 45 };
            }
        }
        true
    });
}

/// The same geometry targets hide mesh surfaces, region surfaces and wire outlines.
fn filter_geometry(session: &Session, out: &mut Built) {
    use emcraft_engine::visibility::GeometryObject;
    let m = session.model();
    let hidden: Vec<_> = session.views().hidden_geometry.iter().filter_map(|s| GeometryObject::parse(s)).collect();
    if hidden.is_empty() {
        return;
    }
    let at = |p: [f64; 3]| emcraft_model::Point3::new(p[0], p[1], p[2]);
    out.scene.tris.retain(|t| {
        let centre = [0, 1, 2].map(|k| (t.p[0][k] + t.p[1][k] + t.p[2][k]) / 3.0);
        !hidden.iter().any(|g| g.contains(m, at(centre)))
    });
    out.scene.lines.retain(|l| {
        let centre = [0, 1, 2].map(|k| (l.a[k] + l.b[k]) / 2.0);
        !hidden.iter().any(|g| g.contains(m, at(centre)))
    });
}

/// The outline of each part tagged in `tags` (the edges of its triangles that no other of them
/// shares), so a highlighted face stands out from its neighbours whatever its shading.
fn highlight_outline(scene: &mut Scene, tags: &std::collections::BTreeSet<u32>) {
    use std::collections::BTreeMap;
    type Key = [u64; 3];
    let key = |p: [f64; 3]| p.map(|v| (v * 1e9).round().to_bits());
    let mut edges: BTreeMap<(u32, Key, Key), ([f64; 3], [f64; 3], u32)> = BTreeMap::new();
    for t in scene.tris.iter().filter(|t| tags.contains(&t.tag)).take(200_000) {
        for (a, b) in [(t.p[0], t.p[1]), (t.p[1], t.p[2]), (t.p[2], t.p[0])] {
            let (ka, kb) = (key(a), key(b));
            let k = if ka <= kb { (t.tag, ka, kb) } else { (t.tag, kb, ka) };
            edges.entry(k).or_insert((a, b, 0)).2 += 1;
        }
    }
    for (a, b, n) in edges.into_values() {
        if n == 1 {
            scene.line(a, b, style::sc().highlight, 255);
        }
    }
}

#[cfg(test)]
mod tests;
