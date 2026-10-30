//! The 3D view in the viewport: the session's scene ([`crate::scene3d`]) drawn by
//! `emcraft-render`'s CPU renderer into a texture, with orbit (left drag), pan (middle or shift
//! drag), zoom (wheel), fit (double click or Fit), the standard views, perspective or
//! orthographic projection, labels, a legend and an orientation indicator. While dragging the
//! picture is drawn at half resolution; at rest at full resolution, supersampled when small
//! enough. A section view cuts the scene across an axis (`emcraft_render::view3d::section`: the
//! far side removed, the cut faces capped in each body's colour); both renderers draw, and
//! picking reads, the cut scene, cached per scene and cut.

use std::time::Instant;

use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, pos2, vec2};
use emcraft_render::Rgb;
use emcraft_render::view3d::{self, Camera, Keep, Projection, StandardView};

use emcraft_engine::view::{FieldView, Plane};

use crate::glyphs::{self, Glyph};
use crate::handle::{Fixed, Handle};
use crate::lang::{self, args, tr, trf};
use crate::scene3d::{self, Built, Overlay};
use crate::state::FieldState;
use crate::theme::Tokens;
use crate::{Action, Actions, EmcraftApp, Hint, Picture, UiExt, scenario};

/// The most pixels the view is drawn with (beyond, it is drawn smaller and scaled up).
const MAX_PIXELS: f64 = 2.4e6;
/// Supersampled at rest up to this many output pixels.
const SUPERSAMPLE_PIXELS: f64 = 1.3e6;

/// What the scene was built from: the session's revision, the overlay, the field's planes (in
/// the Results step), what is highlighted and the theme (its colours).
#[derive(Clone, Debug, PartialEq)]
struct SceneKey {
    revision: u64,
    overlay: Overlay,
    field: Option<FieldKey>,
    highlighted: Option<scene3d::Pick>,
    theme: crate::theme::ThemeKey,
    /// The language its labels and legend are in.
    language: &'static str,
}

/// The field's state the Results step's planes are drawn from: the field view, its plane's
/// position (display units) and the planes kept beside it.
#[derive(Clone, Debug, PartialEq)]
struct FieldKey {
    view: FieldView,
    at: Option<f64>,
    kept: Vec<(Plane, f64)>,
}

/// The field view's plane as the scene shows it: its picture's ranges (display units), the
/// plane and its position along the normal (m from the model's low side).
#[derive(Clone, Copy, Debug)]
struct Facing {
    x: (f64, f64),
    y: (f64, f64),
    plane: Plane,
    at: f64,
}

/// A section view's cut: across `axis` (0 x, 1 y, 2 z) at `at` (m), the `keep` side kept.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cut {
    pub axis: usize,
    pub at: f64,
    pub keep: Keep,
}

impl Cut {
    /// The cut through the middle of `bounds` (m) across `axis`, the low side kept.
    pub fn middle(axis: usize, (lo, hi): ([f64; 3], [f64; 3])) -> Cut {
        let k = axis.min(2);
        Cut { axis: k, at: 0.5 * (lo[k] + hi[k]), keep: Keep::Low }
    }

    /// The render's section, outlined in the theme's scene edge colour.
    fn section(self) -> view3d::Section {
        view3d::Section { outline: Some(crate::theme::scene().edge), ..view3d::Section::across(self.axis, self.at, self.keep) }
    }

    /// The cut for `ui.inspect`, `at` in display units.
    pub fn inspect(self, l: emcraft_model::units::Length) -> serde_json::Value {
        let axis = crate::handle::AXES.get(self.axis).copied().unwrap_or("x");
        serde_json::json!({"axis": axis, "at": l.from_si(self.at), "keep": if self.keep == Keep::Low { "low" } else { "high" }})
    }
}

/// What the picture was drawn from.
#[derive(Clone, Debug, PartialEq)]
struct Key {
    scene: SceneKey,
    section: Option<Cut>,
    camera: Camera,
    size: [usize; 2],
    supersample: usize,
}

/// The 3D view's state.
pub struct View3d {
    pub camera: Camera,
    /// The box last fitted to: a new geometry is framed once.
    fitted: Option<([f64; 3], [f64; 3])>,
    built: Option<(SceneKey, Built)>,
    /// The section view's cut, when it is on.
    pub section: Option<Cut>,
    /// The built scene cut, for the scene and the cut it was made from.
    cut: Option<(SceneKey, Cut, Built)>,
    /// The field view's plane in the scene built, when it shows one.
    facing: Option<Facing>,
    /// The browser row of the selection, at the session's revision.
    picked: Option<((u64, scene3d::Pick), Option<(String, crate::browser::Row)>)>,
    picture: Option<(Key, Picture)>,
    /// Milliseconds the last picture took, and its size.
    pub last_ms: f64,
    pub last_size: [usize; 2],
    pub dragging: bool,
    /// Fit on the next frame.
    pub fit_next: bool,
    /// What is selected (by a click in the view or on its browser row), highlighted.
    pub selected: Option<scene3d::Pick>,
    /// What a row elsewhere (the port-mode table) points at while hovered: highlighted for one
    /// frame instead of the selection, which it leaves alone.
    pub hover: Option<scene3d::Pick>,
    /// Whether the last frame was drawn by the window's GPU rather than the CPU renderer.
    pub drawn_on_gpu: bool,
    /// A drag that began on the field view's plane moves it instead of orbiting.
    moving_plane: bool,
}

impl Default for View3d {
    fn default() -> Self {
        View3d {
            camera: Camera::default(),
            fitted: None,
            built: None,
            section: None,
            cut: None,
            facing: None,
            picked: None,
            picture: None,
            last_ms: 0.0,
            last_size: [0, 0],
            dragging: false,
            fit_next: false,
            selected: None,
            hover: None,
            drawn_on_gpu: false,
            moving_plane: false,
        }
    }
}

impl View3d {
    /// Whether the scene held was built for `key`.
    fn is_built(&self, key: &SceneKey) -> bool {
        self.built.as_ref().is_some_and(|(k, _)| k == key)
    }

    /// The model's box (m) as last built.
    pub fn bounds(&self) -> Option<([f64; 3], [f64; 3])> {
        self.built.as_ref().and_then(|(_, b)| b.bounds.or_else(|| b.scene.bounds()))
    }

    /// The scene drawn and picked: the built one, cut when the section view is on.
    fn shown(&self) -> Option<&Built> {
        match (&self.cut, self.section) {
            (Some((k, c, b)), Some(now)) if *c == now && self.built.as_ref().is_some_and(|(bk, _)| bk == k) => Some(b),
            _ => self.built.as_ref().map(|(_, b)| b),
        }
    }

    /// The scene shown: labels, legend and bounds (the whole model's), without the scene.
    fn built_shallow(&self) -> Built {
        let mut b = self.shown().map_or_else(|| EMPTY.clone_shallow(), Built::clone_shallow);
        if let Some((_, full)) = &self.built {
            b.bounds = full.bounds.or_else(|| full.scene.bounds());
        }
        b
    }

    /// Cuts the built scene when the section view is on and the cut held is not of this scene
    /// and cut; a cut the renderer refuses turns the view off and says why.
    fn update_cut(&mut self, model: &emcraft_model::Model, out: &mut Actions) {
        let (Some(now), Some((key, full))) = (self.section, &self.built) else {
            self.cut = None;
            return;
        };
        if self.cut.as_ref().is_some_and(|(k, c, _)| k == key && *c == now) {
            return;
        }
        let s = now.section();
        match view3d::section::section(&full.scene, &s) {
            Ok(mut scene) => {
                // A meshed model's cut is the mesh's: every material filled, the air too.
                let caps = scene3d::mesh_slice(model, now.axis, now.at, now.keep == Keep::Low);
                scene.tris.extend(caps.tris);
                let labels = full.labels.iter().filter(|l| s.keeps(l.at)).cloned().collect();
                let built = Built { scene, labels, legend: full.legend.clone(), bounds: full.bounds };
                self.cut = Some((key.clone(), now, built));
            }
            Err(e) => {
                out.push(Action::error(e));
                self.section = None;
                self.cut = None;
            }
        }
    }

    /// What is drawn highlighted: the hovered row's part, else the selection.
    pub fn highlighted(&self) -> Option<scene3d::Pick> {
        self.hover.or(self.selected)
    }

    pub fn standard(&mut self, v: StandardView) {
        self.camera.standard(v);
        self.fit_next = true;
    }

    pub fn toggle_projection(&mut self) {
        self.camera.projection = match self.camera.projection {
            Projection::Perspective => Projection::Orthographic,
            Projection::Orthographic => Projection::Perspective,
        };
        self.fit_next = true;
    }

    /// The state for `ui.inspect` (lengths in `l`).
    pub fn inspect(&self, l: emcraft_model::units::Length) -> serde_json::Value {
        let c = &self.camera;
        let b = self.built.as_ref().zip(self.shown()).map(|((k, _), b)| (k.overlay.id(), b.scene.tris.len(), b.scene.lines.len()));
        serde_json::json!({
            "yaw": c.yaw.to_degrees(), "pitch": c.pitch.to_degrees(), "distance": c.distance,
            "target": c.target,
            "projection": match c.projection { Projection::Perspective => "perspective", Projection::Orthographic => "orthographic" },
            "overlay": b.map(|x| x.0), "triangles": b.map(|x| x.1), "lines": b.map(|x| x.2),
            "ms": self.last_ms, "size": self.last_size,
            "selected": self.selected.map(|p| format!("{p:?}")),
            "hover": self.hover.map(|p| format!("{p:?}")),
            "gpu": self.drawn_on_gpu,
            "section": self.section.map(|s| s.inspect(l)),
        })
    }
}

use crate::theme::{color32 as color, rgb};

static EMPTY: Built =
    Built { scene: view3d::Scene { tris: Vec::new(), lines: Vec::new(), tag: 0 }, labels: Vec::new(), legend: Vec::new(), bounds: None };

/// Draws the 3D view (its state `v`) in `rect` for `overlay`, the field view's planes (`field`)
/// in it when the overlay shows them.
pub fn show(app: &EmcraftApp, v: &mut View3d, field: &mut FieldState, ui: &mut egui::Ui, rect: Rect, overlay: Overlay, out: &mut Actions) {
    let t = &Tokens::get();
    // The scene, built again only when what it shows changed (the field's planes, for the
    // Results step: the field view's and those kept).
    let key = scene_key(app, v, field, overlay);
    if !v.is_built(&key) {
        build(app, v, field, overlay, key.clone(), out);
    }
    v.update_cut(app.session.model(), out);
    let built = v.built_shallow();
    let Some(bounds) = built.bounds.or_else(|| built.scene.bounds()) else {
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            tr("No geometry yet: open a sample or start at step 1, Geometry."),
            FontId::proportional(t.font_heading),
            t.text_dim,
        );
        return;
    };
    let frame = focus(app.session.model(), bounds);
    let aspect = (rect.width() / rect.height().max(1.0)) as f64;
    if v.fitted != Some(bounds) || v.fit_next {
        v.camera.fit(frame.0, frame.1, aspect);
        v.fitted = Some(bounds);
        v.fit_next = false;
    }
    let edit = navigate(v, ui, rect, frame);
    if let Some(d) = edit.plane_drag {
        move_plane(app, &v.camera, field, rect, bounds, d);
    }
    if overlay == Overlay::Field {
        publish_plane(app, &v.camera, field, rect, bounds);
    }
    let picked = picked_row(app, v);
    if edit.clicked {
        out.push(Action::Select { row: picked.as_ref().map(|(k, _)| k.clone()), pick: v.selected });
    }
    if edit.double
        && let Some((_, row)) = &picked
        && !app.busy()
    {
        out.extend(row.edit_action());
    }
    // Moving pictures (an orbit, a playing field) draw on the GPU.
    let gpu = app.prefs.gpu3d || v.dragging || (overlay == Overlay::Field && field.player.playing);
    let p = ui.painter().with_clip_rect(rect);
    draw(v, ui, &p, rect, key, gpu, t);
    labels(&p, rect, &v.camera, &built.labels, t);
    legend(&p, rect, &built.legend, t);
    if let Some((_, row)) = &picked {
        selection_card(&p, rect, row, t);
    }
    let facing = v.facing.map(|f| facing_plane(app.session.display().length, bounds.0, f));
    navigator(&v.camera, ui, rect, out);
    nav_bar(v, field.player.playing, ui, rect, facing, bounds, out);
    if v.section.is_some() {
        section_bar(v, ui, rect, bounds, app.session.display().length);
    }
}

/// What a fit frames: an open problem's structure (its bodies, ports and sources) with room
/// around it, not the air and the layer that fill most of its box; else the whole scene.
fn focus(m: &emcraft_model::Model, (lo, hi): ([f64; 3], [f64; 3])) -> ([f64; 3], [f64; 3]) {
    if m.open.is_none() {
        return (lo, hi);
    }
    let Some(emcraft_model::Aabb { min, max }) = emcraft_model::Aabb::around(m.radiators()) else { return (lo, hi) };
    let (a, b) = ([min.x, min.y, min.z], [max.x, max.y, max.z]);
    // Twice the structure's largest side around its middle, at least a quarter of the box.
    let side = (0..3).map(|i| b[i] - a[i]).fold(0.0f64, f64::max).max(0.25 * (0..3).map(|i| hi[i] - lo[i]).fold(0.0f64, f64::max));
    let half = side;
    let c = [0, 1, 2].map(|i| 0.5 * (a[i] + b[i]));
    ([0, 1, 2].map(|i| (c[i] - half).max(lo[i])), [0, 1, 2].map(|i| (c[i] + half).min(hi[i])))
}

/// The field view's plane's middle on screen, as the handle `view3d:plane`.
fn publish_plane(app: &EmcraftApp, cam: &Camera, field: &FieldState, rect: Rect, (lo, hi): ([f64; 3], [f64; 3])) {
    let Some((span, _)) = crate::viewport::plane_span_of(app, field.view.plane) else { return };
    let axis = normal(field.view.plane);
    let frac = if span > 0.0 { field.at.unwrap_or(span / 2.0) / span } else { 0.5 };
    let mut c = [0, 1, 2].map(|i| 0.5 * (lo[i] + hi[i]));
    c[axis] = lo[axis] + frac * (hi[axis] - lo[axis]);
    if let Some(p) = cam.project(c, f64::from(rect.width()), f64::from(rect.height())) {
        scenario::publish_handle(Fixed::View3dPlane, rect.min + vec2(p[0] as f32, p[1] as f32));
    }
}

/// The normal axis of a field plane (0 x, 1 y, 2 z).
fn normal(plane: Plane) -> usize {
    match plane {
        Plane::E => 0,
        Plane::H => 1,
        Plane::Cross => 2,
    }
}

/// Moves the field view's plane along its normal by the pointer's move `d` on the view: the
/// move's share of the normal's direction on screen, over the model's span.
fn move_plane(app: &EmcraftApp, cam: &Camera, field: &mut FieldState, rect: Rect, (lo, hi): ([f64; 3], [f64; 3]), d: egui::Vec2) {
    let Some((span, _)) = crate::viewport::plane_span_of(app, field.view.plane) else { return };
    let axis = normal(field.view.plane);
    let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
    let mid = [0, 1, 2].map(|i| 0.5 * (lo[i] + hi[i]));
    let (mut a, mut b) = (mid, mid);
    a[axis] = lo[axis];
    b[axis] = hi[axis];
    let (Some(pa), Some(pb)) = (cam.project(a, w, h), cam.project(b, w, h)) else { return };
    let along = vec2((pb[0] - pa[0]) as f32, (pb[1] - pa[1]) as f32);
    let len2 = along.length_sq();
    if len2 < 4.0 {
        return;
    }
    let at = field.at.unwrap_or(span / 2.0);
    field.at = Some((at + f64::from(d.dot(along) / len2) * span).clamp(0.0, span));
    field.pinned = None;
}

/// What the scene for `overlay` is built from now.
fn scene_key(app: &EmcraftApp, v: &View3d, field: &mut FieldState, overlay: Overlay) -> SceneKey {
    let planes = (overlay == Overlay::Field).then(|| {
        let l = app.session.display().length;
        field.view.at = field.at.map(|v| l.to_si(v));
        FieldKey { view: field.view.clone(), at: field.at, kept: field.kept.iter().map(|k| (k.plane, k.at)).collect() }
    });
    SceneKey { revision: app.revision, overlay, field: planes, highlighted: v.highlighted(), theme: crate::theme::key(), language: lang::current() }
}

/// Builds the scene for `key`, with the field's planes (sampled and plotted here) when it
/// shows them; a field that cannot be plotted is reported once and left out.
fn build(app: &EmcraftApp, v: &mut View3d, field: &mut FieldState, overlay: Overlay, key: SceneKey, out: &mut Actions) {
    let layers = if key.field.is_some() {
        crate::viewport::field_plots(app, field).unwrap_or_else(|e| {
            out.push(Action::error(e));
            Vec::new()
        })
    } else {
        Vec::new()
    };
    let fl: Vec<scene3d::FieldLayer> = layers.iter().map(|(plot, plane, at)| scene3d::FieldLayer { plot, plane: *plane, at: *at }).collect();
    v.facing = layers.first().map(|(plot, plane, at)| Facing { x: plot.x.span(), y: plot.y.span(), plane: *plane, at: *at });
    v.built = Some((key, scene3d::build_with(&app.session, overlay, &fl, v.highlighted())));
}

/// The browser row of what is selected in the view (looked up again only when the selection or
/// the session changed).
fn picked_row(app: &EmcraftApp, v: &mut View3d) -> Option<(String, crate::browser::Row)> {
    let key = (app.revision, v.selected?);
    if v.picked.as_ref().is_none_or(|(k, _)| *k != key) {
        let row = crate::browser::row_of(app, key.1);
        v.picked = Some((key, row));
    }
    v.picked.as_ref().and_then(|(_, row)| row.clone())
}

/// What the pointer asked of the view this frame.
struct Gesture {
    clicked: bool,
    /// A double-click on something picked: edit it.
    double: bool,
    /// The pointer's move this frame while dragging the field view's plane.
    plane_drag: Option<egui::Vec2>,
}

/// Orbit, pan and zoom by the pointer; a click picks what is under it (empty space clears), a
/// double-click on nothing fits the view.
fn navigate(v: &mut View3d, ui: &mut egui::Ui, rect: Rect, bounds: ([f64; 3], [f64; 3])) -> Gesture {
    let resp = ui.hit(rect, Fixed::View3d.into(), Sense::click_and_drag());
    let (shift, middle, scroll) = ui.input(|i| {
        // Wheel steps: lines as they are, points and pages scaled.
        let steps: f32 = i
            .raw
            .events
            .iter()
            .filter_map(|e| match e {
                egui::Event::MouseWheel { unit, delta, .. } => Some(match unit {
                    egui::MouseWheelUnit::Line => delta.y,
                    egui::MouseWheelUnit::Point => delta.y / 40.0,
                    egui::MouseWheelUnit::Page => delta.y * 5.0,
                }),
                _ => None,
            })
            .sum();
        (i.modifiers.shift, i.pointer.middle_down(), steps)
    });
    let d = resp.drag_delta();
    // A drag starting on the field view's own plane (the first) moves it.
    if resp.drag_started_by(egui::PointerButton::Primary) && !shift {
        v.moving_plane = resp.interact_pointer_pos().is_some_and(|pos| {
            let at = pos - rect.min;
            let tag = v
                .shown()
                .and_then(|b| view3d::pick(&b.scene, &v.camera, f64::from(at.x), f64::from(at.y), f64::from(rect.width()), f64::from(rect.height())));
            tag.and_then(scene3d::Pick::from_tag) == Some(scene3d::Pick::Plane(0))
        });
    }
    if !resp.dragged() {
        v.moving_plane = false;
    }
    let plane_drag = (v.moving_plane && resp.dragged()).then_some(d);
    if resp.dragged() && (d.x != 0.0 || d.y != 0.0) && !v.moving_plane {
        if middle || shift || resp.dragged_by(egui::PointerButton::Middle) {
            v.camera.pan(d.x as f64, d.y as f64, rect.height() as f64);
        } else if resp.dragged_by(egui::PointerButton::Primary) {
            v.camera.orbit(d.x as f64, d.y as f64);
        }
    }
    v.dragging = resp.dragged();
    if resp.hovered() && scroll != 0.0 {
        v.camera.zoom(scroll as f64);
    }
    let mut double = false;
    if (resp.clicked() || resp.double_clicked())
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let at = pos - rect.min;
        let tag = v
            .shown()
            .and_then(|b| view3d::pick(&b.scene, &v.camera, f64::from(at.x), f64::from(at.y), f64::from(rect.width()), f64::from(rect.height())));
        v.selected = tag.and_then(scene3d::Pick::from_tag);
        if resp.double_clicked() {
            match v.selected {
                Some(_) => double = true,
                None => v.camera.fit(bounds.0, bounds.1, (rect.width() / rect.height().max(1.0)) as f64),
            }
        }
    }
    Gesture { clicked: resp.clicked(), double, plane_drag }
}

/// The scene drawn into `rect`: by the window's GPU (every frame, full size) or the CPU
/// renderer's picture (cached by `scene_key`, the camera and the size).
fn draw(v: &mut View3d, ui: &egui::Ui, p: &egui::Painter, rect: Rect, scene_key: SceneKey, gpu: bool, t: &Tokens) {
    v.drawn_on_gpu = gpu;
    if gpu {
        let began = Instant::now();
        p.add(gradient(rect, t.viewport_top, t.viewport_bottom));
        if let Some(b) = v.shown() {
            p.add(mesh_of(&view3d::flatten(&b.scene, &v.camera, f64::from(rect.width()), f64::from(rect.height())), rect.min, 1.4));
        }
        v.last_ms = began.elapsed().as_secs_f64() * 1e3;
        return;
    }
    let ppp = ui.ctx().pixels_per_point() as f64;
    let mut scale = ppp;
    let full = rect.width() as f64 * rect.height() as f64 * ppp * ppp;
    if full > MAX_PIXELS {
        scale *= (MAX_PIXELS / full).sqrt();
    }
    let size = [((rect.width() as f64 * scale).round() as usize).max(1), ((rect.height() as f64 * scale).round() as usize).max(1)];
    let supersample = if (size[0] * size[1]) as f64 <= SUPERSAMPLE_PIXELS { 2 } else { 1 };
    let key = Key { scene: scene_key, section: v.section, camera: v.camera, size, supersample };
    if v.picture.as_ref().is_none_or(|(k, _)| *k != key) {
        let began = Instant::now();
        let opt = view3d::Options {
            width: size[0],
            height: size[1],
            supersample,
            background: (rgb(t.viewport_top), rgb(t.viewport_bottom)),
            line_width: (scale / ppp).clamp(0.5, 2.0) * ppp.max(1.0),
        };
        let canvas = match v.shown() {
            Some(b) => view3d::render(&b.scene, &v.camera, &opt),
            None => emcraft_render::Canvas::new(size[0], size[1], rgb(t.viewport_top)),
        };
        v.last_ms = began.elapsed().as_secs_f64() * 1e3;
        v.last_size = size;
        v.picture = Some((key, Picture::new(&canvas)));
    }
    if let Some((_, pic)) = v.picture.as_mut() {
        let id = pic.texture(ui.ctx());
        p.image(id, rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    }
}

/// The scene's labels where they project, faint on faces turned away.
fn labels(p: &egui::Painter, rect: Rect, cam: &Camera, labels: &[scene3d::Label], t: &Tokens) {
    let font = FontId::proportional(t.font_small);
    let b = cam.basis();
    for l in labels {
        let Some(q) = cam.project(l.at, rect.width() as f64, rect.height() as f64) else { continue };
        let at = rect.min + vec2(q[0] as f32, q[1] as f32);
        if !rect.contains(at) {
            continue;
        }
        let away = l.facing.is_some_and(|n| n[0] * (b.eye[0] - l.at[0]) + n[1] * (b.eye[1] - l.at[1]) + n[2] * (b.eye[2] - l.at[2]) < 0.0);
        let ink = if away { t.text_dim } else { color(l.color) };
        let g = p.layout_no_wrap(l.text.clone(), font.clone(), ink);
        let r = Rect::from_center_size(at, g.size() + vec2(8.0, 4.0));
        if !away {
            p.rect_filled(r, 3.0, t.panel.gamma_multiply(0.85));
        }
        p.galley(r.min + vec2(4.0, 2.0), g, ink);
    }
}

/// The selection's name on a card at the top, with how to edit it.
fn selection_card(p: &egui::Painter, rect: Rect, row: &crate::browser::Row, t: &Tokens) {
    let hint = if row.editable() { tr("  ·  double-click to edit") } else { "" };
    let g = p.layout_no_wrap(format!("{}{hint}", row.shown.trim()), FontId::proportional(t.font_small), t.text);
    let r = Rect::from_min_size(pos2(rect.center().x - g.size().x / 2.0 - 10.0, rect.top() + 10.0), g.size() + vec2(20.0, 10.0));
    p.rect_filled(r, 6.0, t.panel.gamma_multiply(0.92));
    p.rect_stroke(r, 6.0, Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
    p.galley(r.min + vec2(10.0, 5.0), g, t.text);
    scenario::publish_target(Fixed::View3dSelected, r);
}

/// A field plane's box in the model (m) and the standard view that faces it.
fn facing_plane(l: emcraft_model::units::Length, lo: [f64; 3], f: Facing) -> ([f64; 3], [f64; 3], StandardView) {
    let Facing { x: (h0, h1), y: (v0, v1), plane, at } = f;
    let (h0, h1, v0, v1) = (l.to_si(h0), l.to_si(h1), l.to_si(v0), l.to_si(v1));
    match plane {
        Plane::H => ([v0, lo[1] + at, h0], [v1, lo[1] + at, h1], StandardView::Top),
        Plane::E => ([lo[0] + at, v0, h0], [lo[0] + at, v1, h1], StandardView::Right),
        Plane::Cross => ([h0, v0, lo[2] + at], [h1, v1, lo[2] + at], StandardView::Front),
    }
}

impl Built {
    /// The parts drawn over the picture (labels, legend, bounds), without the scene.
    fn clone_shallow(&self) -> Built {
        Built {
            scene: Default::default(),
            labels: self.labels.clone(),
            legend: self.legend.clone(),
            bounds: self.bounds.or_else(|| self.scene.bounds()),
        }
    }
}

fn legend(p: &egui::Painter, rect: Rect, items: &[(String, Rgb)], t: &Tokens) {
    if items.is_empty() {
        return;
    }
    let font = FontId::proportional(t.font_small);
    let row = 18.0;
    let width = items.iter().map(|(s, _)| p.layout_no_wrap(s.clone(), font.clone(), t.text).size().x).fold(0.0, f32::max) + 34.0;
    let r = Rect::from_min_size(rect.min + vec2(10.0, 10.0), vec2(width, row * items.len() as f32 + 8.0));
    p.rect_filled(r, 6.0, t.panel.gamma_multiply(0.9));
    p.rect_stroke(r, 6.0, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    for (i, (s, c)) in items.iter().enumerate() {
        let y = r.top() + 4.0 + row * (i as f32 + 0.5);
        p.rect_filled(Rect::from_center_size(pos2(r.left() + 14.0, y), vec2(12.0, 12.0)), 2.0, color(*c));
        p.text(pos2(r.left() + 26.0, y), Align2::LEFT_CENTER, s, font.clone(), t.text);
    }
}

/// The standard view looking from the side of axis `k` given by `sign` (its eye there).
fn view_from(k: usize, sign: f64) -> Option<StandardView> {
    StandardView::ALL.into_iter().filter(|v| *v != StandardView::Iso).find(|v| {
        let mut c = Camera::default();
        c.standard(*v);
        // The eye sits against the view direction.
        -c.basis().forward[k] * sign > 0.99
    })
}

/// The axes navigator in the bottom-left corner: the six signed axes as tips round a disc, the
/// positive ones filled with their letter, the negative ones rings; a click views the model
/// from that side.
fn navigator(cam: &Camera, ui: &mut egui::Ui, rect: Rect, out: &mut Actions) {
    let t = &Tokens::get();
    let c = pos2(rect.left() + 52.0, rect.bottom() - 52.0);
    let p = ui.painter().with_clip_rect(rect);
    p.circle_filled(c, 44.0, t.panel.gamma_multiply(0.75));
    let axes = view3d::axes_on_screen(cam);
    let forward = cam.basis().forward;
    // Farthest first, so the tips towards the eye are drawn (and hit) on top.
    let mut tips: Vec<(usize, f64)> = (0..3).flat_map(|k| [(k, 1.0), (k, -1.0)]).collect();
    tips.sort_by(|a, b| (forward[b.0] * b.1).total_cmp(&(forward[a.0] * a.1)));
    let mut clicked = None;
    for (k, sign) in tips {
        let dir = vec2(axes[k][0] as f32, axes[k][1] as f32) * sign as f32;
        let tip = c + dir * 30.0;
        // The axes' colours: x red, y green, z blue.
        let color = [t.axis_x, t.axis_y, t.axis_z][k];
        if sign > 0.0 {
            p.line_segment([c, tip], Stroke::new(2.0, color));
        }
        let r = Rect::from_center_size(tip, vec2(18.0, 18.0));
        let Some(view) = view_from(k, sign) else { continue };
        let resp = ui.hit(r, Handle::Standard(view), Sense::click());
        let name = format!("{}{}", if sign > 0.0 { "+" } else { "−" }, ["X", "Y", "Z"][k]);
        let hover = resp.hovered();
        if sign > 0.0 {
            p.circle_filled(tip, if hover { 10.0 } else { 9.0 }, color);
            p.text(tip, Align2::CENTER_CENTER, &name[1..], FontId::proportional(t.font_small), t.axis_label);
        } else {
            p.circle_filled(tip, 7.0, t.panel);
            p.circle_stroke(tip, if hover { 8.0 } else { 7.0 }, Stroke::new(2.0, color));
        }
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, view.id()));
        if resp.on_hover_text(trf("View from {side} ({view})", args!(side = name, view = view.id()))).clicked() {
            clicked = Some(view);
        }
    }
    if let Some(v) = clicked {
        out.push(Action::StandardView(v));
    }
}

/// The view's tools as icons in a bar at the bottom centre: fit, the home (isometric) view,
/// facing the field's plane (in Results), the section view, perspective or orthographic.
fn nav_bar(
    v: &mut View3d,
    playing: bool,
    ui: &mut egui::Ui,
    rect: Rect,
    facing: Option<([f64; 3], [f64; 3], StandardView)>,
    bounds: ([f64; 3], [f64; 3]),
    out: &mut Actions,
) {
    let t = &Tokens::get();
    let ortho = v.camera.projection == Projection::Orthographic;
    let mut items: Vec<(Glyph, Handle, &str)> = vec![
        (Glyph::Fit, Fixed::View3dFit.into(), "Fit the model to the view (double-click the view)"),
        (Glyph::Home, Handle::Standard(StandardView::Iso), "Home view (isometric)"),
    ];
    if facing.is_some() {
        items.push((Glyph::Lookat, Fixed::View3dFace.into(), "Face the field's plane"));
        // The field's phase, played like a video.
        items.push(if playing {
            (Glyph::Pause, Fixed::View3dPlay.into(), "Pause the field's animation")
        } else {
            (Glyph::Play, Fixed::View3dPlay.into(), "Play the field's animation")
        });
    }
    items.push((Glyph::Section, Fixed::View3dSection.into(), if v.section.is_some() { "Section view: on" } else { "Section view" }));
    items.push(if ortho {
        (Glyph::Orthographic, Fixed::View3dProjection.into(), "Orthographic — click for perspective")
    } else {
        (Glyph::Perspective, Fixed::View3dProjection.into(), "Perspective — click for orthographic")
    });
    let (size, pitch) = (28.0, 32.0);
    let width = pitch * items.len() as f32 + 6.0;
    let bar = Rect::from_center_size(pos2(rect.center().x, rect.bottom() - 26.0), vec2(width, size + 8.0));
    let p = ui.painter().with_clip_rect(rect);
    let near = ui.input(|i| i.pointer.hover_pos()).is_some_and(|q| bar.expand(24.0).contains(q));
    p.rect_filled(bar, 8.0, t.panel.gamma_multiply(if near { 0.95 } else { 0.6 }));
    p.rect_stroke(bar, 8.0, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    for (i, (glyph, handle, tip)) in items.into_iter().enumerate() {
        let tip = tr(tip);
        let r = Rect::from_center_size(pos2(bar.left() + 3.0 + pitch * (i as f32 + 0.5), bar.center().y), vec2(size, size));
        let resp = ui.hit(r, handle, Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tip));
        let on = handle == Handle::Fixed(Fixed::View3dSection) && v.section.is_some();
        if resp.hovered() || on {
            p.rect_filled(r, 5.0, if on { t.accent_soft } else { t.hover });
        }
        let ink = if near || resp.hovered() || on { t.text } else { t.text.gamma_multiply(0.7) };
        glyphs::paint(&p, r.shrink(4.0), glyph, ink, t.accent);
        if resp.on_hover_text(tip).clicked() {
            match handle {
                Handle::Fixed(Fixed::View3dFit) => v.fit_next = true,
                Handle::Standard(side) => out.push(Action::StandardView(side)),
                Handle::Fixed(Fixed::View3dPlay) => out.push(Action::SetPlaying(!playing)),
                Handle::Fixed(Fixed::View3dSection) => v.section = v.section.is_none().then(|| Cut::middle(0, bounds)),
                Handle::Fixed(Fixed::View3dFace) => {
                    if let Some((lo, hi, side)) = facing {
                        v.camera.standard(side);
                        v.camera.fit(lo, hi, (rect.width() / rect.height().max(1.0)) as f64);
                    }
                }
                _ => v.toggle_projection(),
            }
        }
    }
}

/// The section view's controls in a bar over the tools: the axis it cuts across (x, y, z), where
/// along it (a slider over the model's extent, the value in display units `l`) and which side
/// stays (flip).
fn section_bar(v: &mut View3d, ui: &mut egui::Ui, rect: Rect, (lo, hi): ([f64; 3], [f64; 3]), l: emcraft_model::units::Length) {
    let Some(mut cut) = v.section else { return };
    let t = &Tokens::get();
    let k = cut.axis.min(2);
    let (size, pitch, slider_w, value_w) = (24.0, 28.0, 170.0, 84.0);
    let width = 3.0 * pitch + slider_w + value_w + pitch + 16.0;
    let bar = Rect::from_center_size(pos2(rect.center().x, rect.bottom() - 70.0), vec2(width, size + 8.0));
    let p = ui.painter().with_clip_rect(rect);
    p.rect_filled(bar, 8.0, t.panel.gamma_multiply(0.95));
    p.rect_stroke(bar, 8.0, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    let font = FontId::proportional(t.font_label);
    let mut x = bar.left() + 6.0;
    for (axis, name) in crate::handle::AXES.into_iter().enumerate() {
        let r = Rect::from_min_size(pos2(x, bar.center().y - size / 2.0), vec2(size, size));
        let resp = ui.hit(r, Handle::SectionAxis(axis), Sense::click());
        let tip = trf("Cut across {axis}", args!(axis = name));
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &tip));
        let chosen = axis == k;
        if chosen || resp.hovered() {
            p.rect_filled(r, 5.0, if chosen { t.accent_soft } else { t.hover });
        }
        let ink = [t.axis_x, t.axis_y, t.axis_z][axis];
        p.text(r.center(), Align2::CENTER_CENTER, name, font.clone(), if chosen { ink } else { t.text_dim });
        if resp.on_hover_text(tip).clicked() && !chosen {
            cut = Cut { axis, ..Cut::middle(axis, (lo, hi)) };
        }
        x += pitch;
    }
    // The position, in display units over the model's extent on the axis.
    let (a, b) = (l.from_si(lo[cut.axis]), l.from_si(hi[cut.axis]));
    let mut at = l.from_si(cut.at).clamp(a.min(b), a.max(b));
    let sr = Rect::from_min_size(pos2(x + 4.0, bar.center().y - 10.0), vec2(slider_w - 8.0, 20.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(sr).layout(egui::Layout::left_to_right(egui::Align::Center)));
    child.spacing_mut().slider_width = sr.width();
    let s = child.add(egui::Slider::new(&mut at, a.min(b)..=a.max(b)).show_value(false));
    scenario::publish_target(Fixed::View3dSectionAt, s.rect);
    if s.hint("Where the cut lies").changed() && at.is_finite() {
        cut.at = l.to_si(at);
    }
    x += slider_w;
    let value = format!("{} {}", format!("{at:.3}").trim_end_matches('0').trim_end_matches('.'), l.symbol());
    p.text(pos2(x, bar.center().y), Align2::LEFT_CENTER, value, font, t.text);
    x += value_w;
    let r = Rect::from_min_size(pos2(x, bar.center().y - size / 2.0), vec2(size, size));
    let resp = ui.hit(r, Fixed::View3dSectionFlip.into(), Sense::click());
    let tip = tr("Keep the other side");
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tip));
    if resp.hovered() {
        p.rect_filled(r, 5.0, t.hover);
    }
    glyphs::paint(&p, r.shrink(3.0), Glyph::Flip, t.text, t.accent);
    if resp.on_hover_text(tip).clicked() {
        cut.keep = if cut.keep == Keep::Low { Keep::High } else { Keep::Low };
    }
    v.section = Some(cut);
}

/// A vertical gradient filling `rect`.
fn gradient(rect: Rect, top: Color32, bottom: Color32) -> egui::Shape {
    let mut m = egui::Mesh::default();
    m.colored_vertex(rect.left_top(), top);
    m.colored_vertex(rect.right_top(), top);
    m.colored_vertex(rect.right_bottom(), bottom);
    m.colored_vertex(rect.left_bottom(), bottom);
    m.add_triangle(0, 1, 2);
    m.add_triangle(0, 2, 3);
    egui::Shape::mesh(m)
}

/// The projected scene as one mesh in drawing order (lines as thin quads `width` points wide),
/// offset to `origin`.
fn mesh_of(flat: &[view3d::Flat], origin: egui::Pos2, width: f32) -> egui::Shape {
    let mut m = egui::Mesh::default();
    let color = |c: [u8; 4]| Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]);
    let at = |q: [f32; 2]| origin + vec2(q[0], q[1]);
    for f in flat {
        let base = m.vertices.len() as u32;
        match *f {
            view3d::Flat::Tri { p, rgba } => {
                for q in p {
                    m.colored_vertex(at(q), color(rgba));
                }
                m.add_triangle(base, base + 1, base + 2);
            }
            view3d::Flat::Line { a, b, rgba } => {
                let d = vec2(b[0] - a[0], b[1] - a[1]);
                let n = if d.length() > 0.0 { vec2(-d.y, d.x).normalized() * (width / 2.0) } else { vec2(0.0, 0.0) };
                for q in [at(a) + n, at(b) + n, at(b) - n, at(a) - n] {
                    m.colored_vertex(q, color(rgba));
                }
                m.add_triangle(base, base + 1, base + 2);
                m.add_triangle(base, base + 2, base + 3);
            }
        }
    }
    egui::Shape::mesh(m)
}
