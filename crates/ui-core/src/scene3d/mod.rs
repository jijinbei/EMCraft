//! What the 3D view draws, built from the session for one overlay: the mesh's boundary faces
//! (walls, ports) and the faces between materials once there is a mesh ([`mesh`], with the
//! faces told apart in [`surface`]), a coarse tessellation of the geometry (the waveguide's
//! sections, layers and strips; boxes, cylinders, spheres and imported surfaces; inclusions and
//! Boolean features) before ([`geometry`]); ports, lumped ports, sources and the open boundary's
//! layer on top ([`extras`]); the field on its planes ([`field`]); the legend ([`legend`]) in
//! the colours of [`style`]. Pictures come from [`emcraft_render::view3d`].

use emcraft_engine::Session;
use emcraft_render::Rgb;
use emcraft_render::view3d::{Camera, Hits, Pixel, Scene};

pub use cache::Cache;

pub mod board;
mod cache;
mod extras;
pub mod faces;
mod field;
pub mod section;
pub use field::{arrow_legend, phase_arrows, plane_probe, probe_lines};
mod geometry;
mod legend;
mod mesh;
mod shell;
mod style;
mod surface;

pub use extras::{along, port_normal, port_quad, quad_centre};
pub use field::normal_axis;
pub use geometry::{BOX_EDGES, box_corner};
pub use style::{Colours, material_rgb};

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

pub use crate::pick::Pick;

/// An arrow in 3D (a port's direction, a field's arrow): its cylinder and cone are made by the
/// GPU's shaders.
#[derive(Clone, Debug, PartialEq)]
pub struct Arrow {
    pub start: [f64; 3],
    pub end: [f64; 3],
    pub color: Rgb,
    /// Cut away by the section view with the model.
    pub section_clip: bool,
    /// A field's arrow turning with the phase: its phasor and the length its largest takes.
    pub motion: Option<Motion>,
}

/// An arrow's phasor (its real and imaginary parts) and the length its largest takes.
#[derive(Clone, Debug, PartialEq)]
pub struct Motion {
    pub re: [f64; 3],
    pub im: [f64; 3],
    pub max_length: f64,
}

/// A section view's cut: across `axis` (0 x, 1 y, 2 z) at `at` (m), the `keep` side kept.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cut {
    pub axis: usize,
    pub at: f64,
    pub keep: emcraft_render::view3d::Keep,
}

impl Cut {
    /// The cut through the middle of `bounds` (m) across `axis`, the low side kept.
    ///
    /// ```
    /// use emcraft_ui_core::scene3d::Cut;
    /// let c = Cut::middle(2, ([0.0; 3], [1.0, 1.0, 4.0]));
    /// assert_eq!(c.at, 2.0);
    /// assert!(c.keeps([0.0, 0.0, 1.0]) && !c.keeps([0.0, 0.0, 3.0]));
    /// // The GPU's clipping plane round an origin: n·p ≤ d keeps the low side.
    /// assert_eq!(c.clip([0.0, 0.0, 1.0]), [0.0, 0.0, 1.0, 1.0]);
    /// ```
    pub fn middle(axis: usize, (lo, hi): ([f64; 3], [f64; 3])) -> Cut {
        let k = axis.min(2);
        Cut { axis: k, at: 0.5 * (lo[k] + hi[k]), keep: emcraft_render::view3d::Keep::Low }
    }

    /// Whether point `p` (m) lies on the side kept (or on the cut).
    pub fn keeps(&self, p: [f64; 3]) -> bool {
        self.keep.sign() * (p[self.axis.min(2)] - self.at) <= 0.0
    }

    /// The GPU's clipping plane for a scene packed round `origin` (`n·p ≤ d` kept).
    pub fn clip(&self, origin: [f64; 3]) -> [f32; 4] {
        let k = self.axis.min(2);
        let sign = self.keep.sign();
        let mut c = [0.0f32; 4];
        c[k] = sign as f32;
        c[3] = (sign * (self.at - origin[k])) as f32;
        c
    }

    /// The cut for `ui.inspect`, `at` in display units.
    pub fn inspect(self, l: emcraft_model::units::Length) -> serde_json::Value {
        let axis = crate::handle::AXES.get(self.axis).copied().unwrap_or("x");
        serde_json::json!({"axis": axis, "at": l.from_si(self.at), "keep": self.keep.id()})
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

/// A label as it falls on a view `size` points big: where its text centres and where its leader
/// line ends (points from the view's corner), and whether the face it names looks away (it is
/// then dimmed and drawn without its ground).
#[derive(Clone, Debug, PartialEq)]
pub struct Placed<'a> {
    pub label: &'a Label,
    pub at: crate::geom::Point,
    pub anchor: Option<crate::geom::Point>,
    pub away: bool,
}

/// The labels seen through `camera` on a view `size` points big: those whose point falls inside
/// it, in their order.
///
/// ```
/// use emcraft_render::view3d::Camera;
/// use emcraft_ui_core::geom::Vector;
/// use emcraft_ui_core::scene3d::{Label, placed};
/// let mut camera = Camera::default();
/// camera.fit([-1.0; 3], [1.0; 3], 1.0);
/// let label = |at| Label { at, text: "port 1".into(), color: emcraft_render::Rgb(0, 0, 0), facing: None, anchor: None };
/// let shown = [label([0.0; 3]), label([1e9, 0.0, 0.0])];
/// let p = placed(&shown, &camera, Vector::new(400.0, 400.0));
/// // The far one falls outside the view; the middle one at its centre.
/// assert_eq!(p.len(), 1);
/// assert!((p[0].at.x - 200.0).abs() < 1.0 && (p[0].at.y - 200.0).abs() < 1.0);
/// ```
pub fn placed<'a>(labels: &'a [Label], camera: &Camera, size: crate::geom::Vector) -> Vec<Placed<'a>> {
    let (w, h) = (f64::from(size.x), f64::from(size.y));
    let on_view = |p: [f64; 3]| camera.project(p, w, h).map(|q| crate::geom::Point::new(q[0] as f32, q[1] as f32));
    let inside = |p: crate::geom::Point| (0.0..=size.x).contains(&p.x) && (0.0..=size.y).contains(&p.y);
    let eye = camera.basis().eye;
    labels
        .iter()
        .filter_map(|l| {
            let at = on_view(l.at).filter(|p| inside(*p))?;
            let away = l.facing.is_some_and(|n| (0..3).map(|k| n[k] * (eye[k] - l.at[k])).sum::<f64>() < 0.0);
            Some(Placed { label: l, at, anchor: l.anchor.and_then(on_view), away })
        })
        .collect()
}

/// A colour bar: the quantity coloured (TeX, with its unit) and its scale.
#[derive(Clone, Debug, PartialEq)]
pub struct ColourBar {
    pub label: String,
    pub scale: emcraft_render::colormap::ColourScale,
}

/// A scene with its labels and legend.
#[derive(Clone, Debug, Default)]
pub struct Built {
    pub scene: Scene,
    pub gpu_scene: Option<Scene>,
    pub arrows: Vec<Arrow>,
    pub labels: Vec<Label>,
    pub legend: Vec<(String, Rgb)>,
    /// The field plane's colour bar: its quantity (TeX) and scale.
    pub colour_bar: Option<ColourBar>,
    /// The field's arrows: what they show and their scale, in their colour.
    pub arrow_legend: Vec<(String, Rgb)>,
    /// The model's box (m).
    pub bounds: Option<([f64; 3], [f64; 3])>,
    /// Its picking index and coarser copy, made once ([`Cache`]).
    pub cache: Cache,
}

/// `scene` tagged as `pick`'s: what it draws picks it.
pub fn tagged(pick: Pick, scene: Scene) -> Scene {
    let tag = pick.tag();
    Scene { tris: scene.tris.into_iter().map(|t| emcraft_render::view3d::Tri { tag, ..t }).collect(), ..scene }
}

/// `b` drawn over `a`.
pub fn merged(a: Scene, b: Scene) -> Scene {
    Scene { tris: a.tris.into_iter().chain(b.tris).collect(), lines: a.lines.into_iter().chain(b.lines).collect(), tag: a.tag }
}

impl Built {
    /// A scene alone, nothing named on it.
    pub fn of(scene: Scene) -> Built {
        Built { scene, ..Built::default() }
    }

    /// `other` drawn over it: its scene and arrows after these, its labels and legend's lines
    /// after these, its colour bar where this has none.
    pub fn merge(self, other: Built) -> Built {
        Built {
            scene: merged(self.scene, other.scene),
            gpu_scene: self.gpu_scene.or(other.gpu_scene),
            arrows: self.arrows.into_iter().chain(other.arrows).collect(),
            labels: self.labels.into_iter().chain(other.labels).collect(),
            legend: self.legend.into_iter().chain(other.legend).collect(),
            colour_bar: self.colour_bar.or(other.colour_bar),
            arrow_legend: self.arrow_legend.into_iter().chain(other.arrow_legend).collect(),
            bounds: self.bounds.or(other.bounds),
            cache: Cache::default(),
        }
    }

    /// The parts drawn over the picture (labels, legend, bounds), without the scene.
    pub fn clone_shallow(&self) -> Built {
        Built {
            scene: Default::default(),
            gpu_scene: None,
            arrows: Vec::new(),
            labels: self.labels.clone(),
            legend: self.legend.clone(),
            colour_bar: self.colour_bar.clone(),
            arrow_legend: self.arrow_legend.clone(),
            bounds: self.bounds.or_else(|| self.scene.bounds()),
            cache: Default::default(),
        }
    }

    /// [`emcraft_render::view3d::hits`] on its scene, through its index once made.
    pub fn hits(&self, camera: &Camera, at: Pixel) -> Option<Hits> {
        let found = match self.cache.index(&self.scene) {
            Some(index) => camera.ray(at).and_then(|r| index.hits(&self.scene, r)),
            None => emcraft_render::view3d::hits(&self.scene, camera, at),
        };
        // An imported part's see-through metal round the region: what it holds is what the
        // pointer means where the ray reaches it (the part itself where it does not).
        let part = |tag: u32| matches!(Pick::from_tag(tag), Some(Pick::Part(_)));
        match found {
            Some(Hits { nearest, solid }) if part(nearest.tag) => {
                let inside = camera.ray(at).and_then(|r| emcraft_render::view3d::nearest_where(&self.scene, r, |t| !part(t)));
                Some(Hits { nearest: inside.unwrap_or(nearest), solid })
            }
            other => other,
        }
    }

    /// [`emcraft_render::view3d::pick`] on its scene, through its index once made.
    pub fn pick(&self, camera: &Camera, at: Pixel) -> Option<u32> {
        self.hits(camera, at).map(|h| h.picked().tag)
    }

    /// What a moving picture draws: its coarser copy when it has one made, else its scene.
    pub fn moving(&self) -> &Scene {
        self.cache.coarse(&self.scene).unwrap_or(&self.scene)
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
/// use emcraft_ui_core::scene3d::mesh_slice;
/// let model = emcraft_model::Model::default();
/// let ink = emcraft_render::Rgb::BLACK;
/// assert!(mesh_slice(&model, 0, 0.005, ink).tris.is_empty());
/// assert!(mesh_slice(&model, 3, f64::NAN, ink).lines.is_empty());
/// ```
pub fn mesh_slice(m: &emcraft_model::Model, axis: usize, at: f64, colour: Rgb) -> Scene {
    mesh::slice(m, axis, at, true, colour)
}

/// The outline a section plane cuts: the domain's and the materials' boundaries, in `colour`.
pub fn section_outline(m: &emcraft_model::Model, axis: usize, at: f64, colour: Rgb) -> Scene {
    mesh::slice(m, axis, at, false, colour)
}

/// Builds the scene of `session` for `overlay` in `colours`.
pub fn build(session: &Session, overlay: Overlay, colours: &Colours) -> Built {
    build_with(session, overlay, colours, &[], &[])
}

/// A point's image under one of the ways a model solved on a part of itself is drawn whole.
type Image<'a> = Box<dyn Fn([f64; 3]) -> [f64; 3] + 'a>;

/// The ways a model solved on a part of itself is drawn whole: mirrored across each symmetry
/// plane, then turned about the axis of a rotationally symmetric one (n − 1 turns of its
/// sector).
fn images(m: &emcraft_model::Model) -> (Vec<Image<'_>>, Vec<Image<'_>>) {
    let mirrors = m.domain.symmetry.iter().map(|s| {
        let k = s.axis.index();
        Box::new(move |p: [f64; 3]| {
            let mut q = p;
            q[k] = 2.0 * s.at - p[k];
            q
        }) as Image<'_>
    });
    let turns = m.domain.rotation.into_iter().flat_map(|r| {
        (1..r.n).map(move |k| {
            let angle = f64::from(k) * r.angle();
            Box::new(move |p: [f64; 3]| {
                let q = r.turned_by(emcraft_model::Point3::new(p[0], p[1], p[2]), angle);
                [q.x, q.y, q.z]
            }) as Image<'_>
        })
    });
    (mirrors.collect(), turns.collect())
}

/// `scene` with its images under `images` added: a mirror's over everything drawn before it
/// (two mirrors make four quarters), the turns all of the scene as it stands (`cumulative`
/// false). A mirror turns the winding over (`flip`: two corners exchanged keep it outward).
/// `faint` (the Mesh step tells them apart): the images a third as opaque, without the mesh's
/// edges or lines.
fn with_images(scene: Scene, images: &[Image<'_>], cumulative: bool, flip: bool, faint: bool) -> Scene {
    // A triangle the image leaves where it is (one on a mirror plane: a strip there) is not
    // drawn again over itself.
    let fixed = |image: &Image<'_>, p: &[[f64; 3]; 3]| {
        p.iter().all(|q| {
            let r = image(*q);
            (0..3).all(|k| (r[k] - q[k]).abs() <= 1e-9 * (1.0 + q[k].abs()))
        })
    };
    let image_of = |from: &Scene, image: &Image<'_>| Scene {
        tris: from
            .tris
            .iter()
            .filter(|t| !fixed(image, &t.p))
            .map(|t| {
                let p = t.p.map(image);
                emcraft_render::view3d::Tri {
                    p: if flip { [p[0], p[2], p[1]] } else { p },
                    alpha: if faint { t.alpha / 3 } else { t.alpha },
                    wire: if faint { None } else { t.wire },
                    ..*t
                }
            })
            .collect(),
        lines: if faint {
            Vec::new()
        } else {
            from.lines.iter().map(|l| emcraft_render::view3d::Line { a: image(l.a), b: image(l.b), ..*l }).collect()
        },
        tag: from.tag,
    };
    if cumulative {
        images.iter().fold(scene, |s, image| {
            let copy = image_of(&s, image);
            merged(s, copy)
        })
    } else {
        let copies: Vec<Scene> = images.iter().map(|image| image_of(&scene, image)).collect();
        copies.into_iter().fold(scene, merged)
    }
}

/// The material picked, if one is (and shown).
fn selected_material(session: &Session, selected: &[Pick]) -> Option<usize> {
    let m = session.model();
    selected.iter().find_map(|p| match p {
        Pick::Material(i) => m.materials.get(*i).filter(|m| !session.views().hidden_materials.contains(&m.name)).map(|_| *i),
        _ => None,
    })
}

/// Builds the scene, with the field's planes for [`Overlay::Field`] and what is selected
/// highlighted, in `colours`: the model (meshed or not), the parts' walls, the materials as
/// shown and the geometry hidden left out, then the symmetry planes, ports and sources, the
/// legend and the field over it.
pub fn build_with(session: &Session, overlay: Overlay, colours: &Colours, fields: &[FieldLayer], selected: &[Pick]) -> Built {
    let m = session.model();
    if overlay == Overlay::Board {
        let route = session.views().pcb.net().unwrap_or("");
        let reference = session.views().pcb.conditions().map_or("GND", |c| c.ground.as_str());
        return m.pcb().map_or_else(Built::default, |b| board::build(colours, b, route, reference));
    }
    let bounds = m.bounds().map(|b| (p3(b.min), p3(b.max)));
    let body = match &m.mesh {
        Some(_) => {
            let meshed = mesh::from_mesh(colours, session, overlay, selected_material(session, selected));
            let (mirrors, turns) = images(m);
            let faint = overlay == Overlay::Mesh;
            let whole = with_images(with_images(meshed.scene, &mirrors, true, true, faint), &turns, false, false, faint);
            Built { scene: whole, ..meshed }
        }
        // A board's region before it is meshed: its copper and substrates in place.
        None => geometry::from_geometry(colours, m, overlay)
            .merge(extras::sector_cuts(colours, m, bounds))
            .merge(m.pcb().map_or_else(Built::default, |b| board::in_model(colours, b))),
    };
    let model = present_materials(colours, session, selected, body.merge(geometry::part_walls(colours, m)));
    let built = filter_geometry(session, model)
        .merge(extras::symmetry_planes(colours, session, bounds))
        .merge(extras::extras(colours, m, session.driven_port(), overlay, bounds))
        .merge(Built { legend: legend::legend(colours, session, overlay), ..Built::default() })
        .merge(field_planes(colours, session, overlay, fields, bounds));
    highlighted(colours, Built { bounds, ..built }, selected)
}

/// The field's planes on one colour scale, the largest range's: each plane's value at full
/// colour (its peak over the period, or the clamp asked for), else its largest value
/// ([`Overlay::Field`] only).
fn field_planes(colours: &Colours, session: &Session, overlay: Overlay, fields: &[FieldLayer], bounds: Option<([f64; 3], [f64; 3])>) -> Built {
    if overlay != Overlay::Field {
        return Built::default();
    }
    let range =
        |s: &emcraft_render::plot::Scalar| s.range.filter(|r| *r > 0.0 && r.is_finite()).unwrap_or_else(|| field::largest_magnitude(&s.values));
    let peak = fields.iter().filter_map(|f| f.plot.scalar.as_ref()).map(range).fold(0.0, f64::max);
    fields
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let plane = field::field_plane(colours, session, f, peak, i == 0, bounds);
            Built { scene: tagged(Pick::Plane(i), plane.scene), ..plane }
        })
        .fold(Built::default(), Built::merge)
}

/// What is selected brightened towards the highlight and outlined.
fn highlighted(colours: &Colours, built: Built, selected: &[Pick]) -> Built {
    let tags: std::collections::BTreeSet<u32> = selected.iter().map(|p| p.tag()).collect();
    if tags.is_empty() {
        return built;
    }
    let lit = |t: emcraft_render::view3d::Tri| emcraft_render::view3d::Tri {
        color: t.color.mix(colours.scene.highlight, 0.5),
        alpha: t.alpha.max(170),
        ..t
    };
    let tris = built.scene.tris.into_iter().map(|t| if tags.contains(&t.tag) { lit(t) } else { t }).collect();
    let scene = Scene { tris, ..built.scene };
    let outline = highlight_outline(&scene, &tags, colours.scene.highlight);
    Built { scene: merged(scene, outline), ..built }
}

/// Material regions are display objects; hiding them never removes physical walls or ports.
fn present_materials(colours: &Colours, session: &Session, selected: &[Pick], built: Built) -> Built {
    let m = session.model();
    let mut out = built;
    out.scene.tris.retain_mut(|t| {
        let index = match Pick::from_tag(t.tag) {
            Some(Pick::Material(i)) => Some(i),
            Some(Pick::Section(i)) if m.mesh.is_none() => {
                m.waveguide().and_then(|w| w.sections.get(i)).and_then(|s| m.material(&s.material).ok()).map(|(i, _)| i)
            }
            Some(Pick::Layer(i)) => m.waveguide().and_then(|w| w.layers.get(i)).and_then(|s| m.material(&s.material).ok()).map(|(i, _)| i),
            Some(Pick::Solid) if m.mesh.is_none() => m.domain.solid.as_ref().and_then(|s| m.material(&s.material).ok()).map(|(i, _)| i),
            Some(Pick::Inclusion(i)) => m.inclusions().get(i).and_then(|s| s.material.as_ref()).and_then(|s| m.material(s).ok()).map(|(i, _)| i),
            _ => None,
        };
        let Some(i) = index else { return true };
        let Some(mat) = m.materials.get(i) else { return true };
        if session.views().hidden_materials.contains(&mat.name) {
            return false;
        }
        if selected.contains(&Pick::Material(i)) {
            t.tag = Pick::Material(i).tag();
            t.color = colours.scene.highlight;
            t.alpha = 120;
        } else if mat.name == "vacuum" {
            // Section caps are artificial region boundaries, not metal closing the bore.
            if matches!(Pick::from_tag(t.tag), Some(Pick::Section(_))) {
                if t.p.iter().all(|p| (p[2] - t.p[0][2]).abs() < 1e-12) {
                    let z = t.p[0][2];
                    let length = m.waveguide().map_or(0.0, emcraft_model::Waveguide::length);
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
                t.color = colours.scene.metal;
                t.alpha = 45;
            } else {
                t.color = colours.scene.metal;
                t.alpha = if m.domain.open.is_some() { 12 } else { 45 };
            }
        }
        true
    });
    out
}

/// The same geometry targets hide mesh surfaces, region surfaces and wire outlines.
fn filter_geometry(session: &Session, built: Built) -> Built {
    let m = session.model();
    let hidden: Vec<_> = session.views().hidden_geometry.iter().copied().collect();
    if hidden.is_empty() {
        return built;
    }
    let mut out = built;
    let at = |p: [f64; 3]| emcraft_model::Point3::new(p[0], p[1], p[2]);
    out.scene.tris.retain(|t| {
        let centre = [0, 1, 2].map(|k| (t.p[0][k] + t.p[1][k] + t.p[2][k]) / 3.0);
        !hidden.iter().any(|g| g.contains(m, at(centre)))
    });
    out.scene.lines.retain(|l| {
        let centre = [0, 1, 2].map(|k| (l.a[k] + l.b[k]) / 2.0);
        !hidden.iter().any(|g| g.contains(m, at(centre)))
    });
    out
}

/// The outline of each part tagged in `tags` (the edges of its triangles that no other of them
/// shares), so a highlighted face stands out from its neighbours whatever its shading.
fn highlight_outline(scene: &Scene, tags: &std::collections::BTreeSet<u32>, colour: Rgb) -> Scene {
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
    let mut outline = Scene::default();
    for (a, b, _) in edges.into_values().filter(|(_, _, n)| *n == 1) {
        outline.line(a, b, colour, 255);
    }
    outline
}

#[cfg(test)]
mod tests;
