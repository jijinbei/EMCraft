//! What the 3D view draws, built from the session for one overlay: the mesh's boundary faces
//! (walls, ports) and the faces between materials once there is a mesh ([`mesh`], with the
//! faces told apart in [`surface`]), a coarse tessellation of the geometry (the waveguide's
//! sections, layers and strips; boxes, cylinders, spheres and imported surfaces; inclusions and
//! Boolean features) before ([`geometry`]); ports, lumped ports, sources and the open boundary's
//! layer on top ([`extras`]); the field on its planes ([`field`]); the legend ([`legend`]) in
//! the colours of [`style`]. Pictures come from [`emcraft_render::view3d`].

use emcraft_engine::Session;
use emcraft_render::Rgb;
use emcraft_render::view3d::Scene;

mod extras;
mod field;
mod geometry;
mod legend;
mod mesh;
mod style;
mod surface;

pub(crate) use field::normal_axis;
pub(crate) use geometry::{BOX_EDGES, box_corner};
pub use style::material_rgb;

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
        Some(_) => mesh::from_mesh(m, overlay, &mut out),
        None => geometry::from_geometry(m, overlay, &mut out),
    }
    extras::extras(m, overlay, &mut out);
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
            });
        }
    }
    if let Some(tag) = selected.map(Pick::tag) {
        for t in out.scene.tris.iter_mut().filter(|t| t.tag == tag) {
            t.color = t.color.mix(style::HIGHLIGHT, 0.5);
            t.alpha = t.alpha.max(170);
        }
        highlight_outline(&mut out.scene, tag);
    }
    out
}

/// The outline of the triangles tagged `tag` (their edges that no other of them shares), so a
/// highlighted face stands out from its neighbours whatever its shading.
fn highlight_outline(scene: &mut Scene, tag: u32) {
    use std::collections::BTreeMap;
    type Key = [u64; 3];
    let key = |p: [f64; 3]| p.map(|v| (v * 1e9).round().to_bits());
    let mut edges: BTreeMap<(Key, Key), ([f64; 3], [f64; 3], u32)> = BTreeMap::new();
    for t in scene.tris.iter().filter(|t| t.tag == tag).take(200_000) {
        for (a, b) in [(t.p[0], t.p[1]), (t.p[1], t.p[2]), (t.p[2], t.p[0])] {
            let (ka, kb) = (key(a), key(b));
            let k = if ka <= kb { (ka, kb) } else { (kb, ka) };
            edges.entry(k).or_insert((a, b, 0)).2 += 1;
        }
    }
    for (a, b, n) in edges.into_values() {
        if n == 1 {
            scene.line(a, b, style::HIGHLIGHT, 255);
        }
    }
}

#[cfg(test)]
mod tests;
