//! The 3D view in the viewport: the session's scene ([`crate::scene3d`]) drawn by
//! `emcraft-render`'s CPU renderer into a texture, navigated as in SolidCraft
//! ([`crate::navigation`]: the mouse mapping's orbit, pan and zoom, the wheel and pinch toward
//! the cursor, the keys, animated view changes), fit (double click or Fit), the standard views,
//! perspective or orthographic projection, labels, a legend and a view cube
//! ([`crate::view_cube`]) or the axes. While dragging or animating the
//! picture is drawn at half resolution; at rest at full resolution, supersampled when small
//! enough. A section view cuts the scene across an axis (`emcraft_render::view3d::section`: the
//! far side removed, the cut faces capped in each body's colour); both renderers draw, and
//! picking reads, the cut scene, cached per scene and cut.

use std::time::Instant;

use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, pos2, vec2};
use emcraft_render::Rgb;
use emcraft_render::view3d::{self, Camera, CameraAnim, Drag, Keep, Projection, StandardView};

use emcraft_engine::view::{FieldView, Plane};

use crate::glyphs::{self, Glyph};
use crate::handle::{Fixed, Handle};
use crate::lang::{self, args, tr, trf};
use crate::navigation::{NavMode, Navigator, OrbitAround};
use crate::scene3d::{self, Built, Overlay};
use crate::selection::{self, BoxSel};
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
    highlighted: Vec<scene3d::Pick>,
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
    /// The cut's dimensions and ports drawn on it (`crate::section3d`).
    pub section_dims: bool,
    /// The built scene cut, for the scene and the cut it was made from.
    cut: Option<(SceneKey, Cut, Built)>,
    /// The field view's plane in the scene built, when it shows one.
    facing: Option<Facing>,
    /// The browser rows of the selection, for the session's revision and the selection.
    picked: Option<((u64, Vec<scene3d::Pick>), Vec<(String, crate::browser::Row)>)>,
    picture: Option<(Key, Picture)>,
    /// Milliseconds the last picture took, and its size.
    pub last_ms: f64,
    pub last_size: [usize; 2],
    pub dragging: bool,
    /// Fit on the next frame.
    pub fit_next: bool,
    /// What is selected (by a click or a box in the view, or on its browser rows), in the order
    /// it was, highlighted; the last is the one a click took last.
    pub selected: Vec<scene3d::Pick>,
    /// The box being dragged (a plain left drag), in screen points.
    pub boxsel: Option<BoxSel>,
    /// What a row elsewhere (the port-mode table) points at while hovered: highlighted for one
    /// frame instead of the selection, which it leaves alone.
    pub hover: Option<scene3d::Pick>,
    /// Whether the last frame was drawn by the window's GPU rather than the CPU renderer.
    pub drawn_on_gpu: bool,
    /// What the left button's drag does, chosen when it was pressed: move the field view's plane
    /// or the selected port's reference plane (pressed on them), or navigate.
    left: Option<LeftDrag>,
    /// A view change in progress, stepped by the frame's clock.
    pub anim: Option<CameraAnim>,
    /// A view change asked for, begun on the next frame (it needs the view's size).
    pub goal: Option<Goal>,
    /// The orbit in progress: the camera it began with and the pointer's whole move since.
    orbit: Option<Drag>,
    /// A drag on the view cube orbiting the view.
    pub(crate) cube_drag: Option<Drag>,
    /// Inside a trackpad's two-finger scroll (between its start and end).
    gesture: bool,
    /// An arrow key turning the view that is still down.
    key_hold: Option<KeyHold>,
    /// The touchpad mapping's button-free move: its mode and the pointer's last place.
    motion: Option<(NavMode, egui::Pos2)>,
    /// The tools: face and gap ports, the ruler, the reference plane's drag.
    pub tools: crate::tools3d::Tools,
    /// Bumped whenever the scene picked from is built or cut again.
    generation: u64,
}

impl Default for View3d {
    fn default() -> Self {
        View3d {
            camera: Camera::default(),
            fitted: None,
            built: None,
            section: None,
            section_dims: true,
            cut: None,
            facing: None,
            picked: None,
            picture: None,
            last_ms: 0.0,
            last_size: [0, 0],
            dragging: false,
            fit_next: false,
            selected: Vec::new(),
            boxsel: None,
            hover: None,
            drawn_on_gpu: false,
            left: None,
            anim: None,
            goal: None,
            orbit: None,
            cube_drag: None,
            gesture: false,
            key_hold: None,
            motion: None,
            tools: crate::tools3d::Tools::default(),
            generation: 0,
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

    /// Which scene [`View3d::shown`] gives: it changes whenever that scene does.
    pub(crate) fn scene_gen(&self) -> u64 {
        let cut = matches!((&self.cut, self.section), (Some((_, c, _)), Some(now)) if *c == now);
        self.generation.wrapping_mul(2).wrapping_add(u64::from(cut))
    }

    /// What lies under `pos` in the view drawn into `rect`.
    fn pick_at(&self, rect: Rect, pos: egui::Pos2) -> Option<scene3d::Pick> {
        let at = pos - rect.min;
        let tag =
            self.shown().and_then(|b| b.pick(&self.camera, f64::from(at.x), f64::from(at.y), f64::from(rect.width()), f64::from(rect.height())));
        tag.and_then(scene3d::Pick::from_tag)
    }

    /// The scene drawn and picked: the built one, cut when the section view is on.
    pub(crate) fn shown(&self) -> Option<&Built> {
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
                let built = Built { scene, labels, legend: full.legend.clone(), bounds: full.bounds, cache: Default::default() };
                self.cut = Some((key.clone(), now, built));
                self.generation = self.generation.wrapping_add(1);
            }
            Err(e) => {
                out.push(Action::error(e));
                self.section = None;
                self.cut = None;
            }
        }
    }

    /// What is drawn highlighted: the hovered row's part, else the selection.
    pub fn highlighted(&self) -> Vec<scene3d::Pick> {
        self.hover.map_or_else(|| self.selected.clone(), |h| vec![h])
    }

    /// The only part selected, when one is.
    pub fn single(&self) -> Option<scene3d::Pick> {
        match self.selected[..] {
            [p] => Some(p),
            _ => None,
        }
    }

    /// Turns to a standard view (animated); the home (isometric) view also frames the model.
    pub fn standard(&mut self, v: StandardView) {
        self.goal = Some(if v == StandardView::Iso { Goal::Home } else { Goal::View(v) });
    }

    /// Whether a view change is under way or asked for.
    pub fn animating(&self) -> bool {
        self.anim.is_some() || self.goal.is_some()
    }

    /// Sets the camera at once, ending any view change.
    pub fn set_camera(&mut self, c: Camera) {
        self.camera = c.sanitized();
        self.anim = None;
        self.goal = None;
        self.orbit = None;
    }

    /// Stops a view change where it is (the person moves the camera).
    fn stop_animation(&mut self) {
        self.anim = None;
        self.goal = None;
    }

    /// The camera a view change starts from: where one under way is going, so rapid clicks and
    /// key taps add up instead of starting again from the middle of a move.
    fn heading(&self) -> Camera {
        self.anim.map_or(self.camera, |a| a.to)
    }

    /// Begins the view change `goal` in a picture of `aspect`, framing `frame` where it frames.
    fn begin(&mut self, goal: Goal, frame: ([f64; 3], [f64; 3]), aspect: f64, now: f64) {
        let mut to = self.heading();
        match goal {
            Goal::View(view) => to.standard(view),
            Goal::Home => {
                to.standard(StandardView::Iso);
                to.fit(frame.0, frame.1, aspect);
            }
            Goal::Fit => to.fit(frame.0, frame.1, aspect),
            Goal::Face(lo, hi, view) => {
                to.standard(view);
                to.fit(lo, hi, aspect);
            }
            Goal::Turn { right, up, degrees } => to = to.stepped(right, up, degrees),
            Goal::From(dir) => to = to.looking_from(dir),
        }
        let to = to.sanitized();
        self.orbit = None;
        self.anim = (to != self.camera).then(|| CameraAnim::new(self.camera, to, now));
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
            "selected": self.selected.last().map(|p| format!("{p:?}")),
            "selection": self.selected.iter().map(|p| format!("{p:?}")).collect::<Vec<_>>(),
            "box": self.boxsel.map(|b| serde_json::json!({"crossing": b.crossing, "rect": [b.rect.min.x, b.rect.min.y, b.rect.max.x, b.rect.max.y]})),
            "hover": self.hover.map(|p| format!("{p:?}")),
            "gpu": self.drawn_on_gpu,
            "animating": self.animating(),
            "section": self.section.map(|s| s.inspect(l)),
            "section_dims": self.section_dims,
            "tools": self.tools.inspect(l),
        })
    }
}

use crate::theme::{color32 as color, rgb};

static EMPTY: Built = Built {
    scene: view3d::Scene { tris: Vec::new(), lines: Vec::new(), tag: 0 },
    labels: Vec::new(),
    legend: Vec::new(),
    bounds: None,
    cache: scene3d::Cache::none(),
};

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
        v.anim = None;
    }
    // View changes step by the frame's clock (the scenarios' clock is their frames').
    let now = ui.input(|i| i.time);
    if let Some(goal) = v.goal.take() {
        v.begin(goal, frame, aspect, now);
    }
    if let Some(a) = v.anim {
        let (c, done) = a.sample(now);
        v.camera = c;
        if done {
            v.anim = None;
        } else {
            ui.ctx().request_repaint();
        }
    }
    let grip = crate::tools3d::grip(app, v, rect).filter(|_| !app.busy());
    let edit = navigate(app, v, ui, rect, frame, grip);
    keys(app, v, ui, rect, now);
    if let Some(d) = edit.plane_drag {
        move_plane(app, &v.camera, field, rect, bounds, d);
    }
    tools(app, v, ui, rect, &edit, out);
    if overlay == Overlay::Field {
        publish_plane(app, &v.camera, field, rect, bounds);
    }
    let picked = picked_rows(app, v);
    if edit.select {
        out.push(Action::Select { row: picked.last().map(|(k, _)| k.clone()), picks: v.selected.clone() });
    }
    let single = picked.first().filter(|_| v.single().is_some());
    if edit.double
        && let Some((_, row)) = single
        && !app.busy()
    {
        out.extend(row.edit_action());
    }
    // Moving pictures (an orbit, a playing field) draw on the GPU.
    let gpu = app.prefs.gpu3d || v.dragging || v.anim.is_some() || (overlay == Overlay::Field && field.player.playing);
    let p = ui.painter().with_clip_rect(rect);
    draw(v, ui, &p, rect, key, gpu, t);
    labels(&p, rect, &v.camera, &built.labels, t);
    if let (Some(cut), true, Some(b)) = (v.section, v.section_dims, v.shown()) {
        crate::section3d::annotate(app, &b.scene, &v.camera, &p, rect, cut, bounds);
    }
    legend(&p, rect, &built.legend, t);
    crate::tools3d::draw(app, v, &p, rect);
    if let Some(b) = v.boxsel {
        paint_box(&p, b, t);
    }
    if let Some((_, row)) = single {
        let hint = if row.editable() && row.item.is_none() { tr("  ·  double-click to edit") } else { "" };
        // Its symbols set as mathematics; a port with its role.
        let text = match &row.role {
            Some(role) => format!("{}  ·  {}{hint}", row.tex(), crate::math::texify(&role.tip())),
            None => format!("{}{hint}", row.tex()),
        };
        let item = row.item.clone();
        let card = Card { edit: item.clone().map(Action::EditItem), delete: item.map(|i| Action::DeleteItems(vec![i])) };
        let card = selection_card(ui, &p, rect, &text, card, !app.busy(), t, out);
        crate::tools3d::ref_field(app, v, ui, card.center_bottom(), out);
    } else if v.selected.len() > 1 {
        // Several parts: how many, and Delete for those that are items of the model.
        let items: Vec<crate::browser::Item> = picked.iter().filter_map(|(_, r)| r.item.clone()).collect();
        let text = trf("{n} selected", args!(n = v.selected.len()));
        let card = Card { edit: None, delete: (!items.is_empty()).then_some(Action::DeleteItems(items)) };
        selection_card(ui, &p, rect, &text, card, !app.busy(), t, out);
    }
    let facing = v.facing.map(|f| facing_plane(app.session.display().length, bounds.0, f));
    match app.prefs.navigator() {
        Navigator::Cube => crate::view_cube::show(v, ui, rect, out),
        Navigator::Axes => navigator(&v.camera, ui, rect, out),
    }
    nav_bar(v, field.player.playing, ui, rect, facing, bounds, overlay, out);
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
    let built = scene3d::build_with(&app.session, overlay, &fl, &key.highlighted);
    v.built = Some((key, built));
    v.generation = v.generation.wrapping_add(1);
}

/// The browser rows of what is selected in the view, in its order (looked up again only when the
/// selection or the session changed).
fn picked_rows(app: &EmcraftApp, v: &mut View3d) -> Vec<(String, crate::browser::Row)> {
    if v.selected.is_empty() {
        return Vec::new();
    }
    let key = (app.revision, v.selected.clone());
    if v.picked.as_ref().is_none_or(|(k, _)| *k != key) {
        let rows = crate::browser::rows_of(app, &key.1);
        v.picked = Some((key, rows));
    }
    v.picked.as_ref().map(|(_, rows)| rows.clone()).unwrap_or_default()
}

/// What the pointer asked of the view this frame.
struct Gesture {
    /// The selection changed (a click or a box).
    select: bool,
    /// A double-click on something picked: edit it.
    double: bool,
    /// The pointer's move this frame while dragging the field view's plane.
    plane_drag: Option<egui::Vec2>,
    grip: GripDrag,
    /// A click with a tool on: the tool takes it.
    tool_click: bool,
}

/// A drag of the reference plane's grip this frame: it began, moved it, or let it go.
#[derive(Default)]
struct GripDrag {
    started: bool,
    moved: Option<egui::Vec2>,
    released: bool,
}

/// The tools' share of the frame: the reference plane's drag, what the pointer is over, a
/// tool's click and Escape.
fn tools(app: &EmcraftApp, v: &mut View3d, ui: &egui::Ui, rect: Rect, edit: &Gesture, out: &mut Actions) {
    if edit.grip.started {
        crate::tools3d::start_drag(app, v);
    }
    if let Some(d) = edit.grip.moved {
        crate::tools3d::drag(app, v, rect, d);
    }
    if edit.grip.released {
        crate::tools3d::end_drag(app, v, out);
    }
    crate::tools3d::hover(app, v, rect, ui.input(|i| i.pointer.hover_pos()));
    if edit.tool_click && !app.busy() {
        crate::tools3d::click(app, v, out);
    }
    escape(app, v, ui, out);
}

/// Escape drops a box being dragged, clears what a tool picked, then turns it off; with no tool
/// on it clears the selection (while no dialog, menu or field takes the key).
fn escape(app: &EmcraftApp, v: &mut View3d, ui: &egui::Ui, out: &mut Actions) {
    let idle = app.dialog.is_none() && app.settings.is_none() && app.files.prompt.is_none() && !ui.ctx().text_edit_focused();
    if !idle || !ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        return;
    }
    if v.boxsel.take().is_some() {
        v.left = None;
    } else if v.tools.tool.is_some() || v.tools.measured.is_some() {
        v.tools.escape();
    } else if (!v.selected.is_empty() || app.browser.selected.is_some()) && !egui::Popup::is_any_open(ui.ctx()) {
        v.selected.clear();
        out.push(Action::Select { row: None, picks: Vec::new() });
    }
}

/// A view change asked of the 3D view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Goal {
    /// A standard view's direction, keeping the framing.
    View(StandardView),
    /// The isometric view, framing the model.
    Home,
    /// The model framed, keeping the direction.
    Fit,
    /// A box (m) seen from a standard view, framed (facing the field's plane).
    Face([f64; 3], [f64; 3], StandardView),
    /// Turned toward the screen's right and up by degrees (arrow keys, the cube's arrows).
    Turn { right: f64, up: f64, degrees: f64 },
    /// Looking from a direction (the view cube's faces and corners), keeping the framing.
    From([f64; 3]),
}

/// What the left button's drag does.
#[derive(Clone, Copy, Debug, PartialEq)]
enum LeftDrag {
    /// Moves the selected port's reference plane (pressed on its grip).
    Grip,
    /// Moves the field view's plane (pressed on it).
    Plane,
    /// Navigates; `shift` when the press needed Shift (so Shift does not also snap).
    Nav(NavMode, bool),
    /// Draws a selection box (on empty space or on the model, no navigation bound); `add` when
    /// Shift was held at the press (adds to what is selected).
    Box { add: bool },
}

/// An arrow key turning the view that is still down.
#[derive(Clone, Copy, Debug, PartialEq)]
struct KeyHold {
    key: egui::Key,
    right: f64,
    up: f64,
    shift: bool,
    since: f64,
}

/// The distances (m) a zoom keeps within, for a model of box `bounds`.
fn zoom_limits((lo, hi): ([f64; 3], [f64; 3])) -> [f64; 2] {
    let size = (0..3).map(|i| (hi[i] - lo[i]).powi(2)).sum::<f64>().sqrt();
    let size = if size.is_finite() && size > 0.0 { size } else { 1.0 };
    [size * 1e-4, size * 1e3]
}

/// The model point under `pos` on the view drawn into `rect`: the surface there, else where the
/// pointer's ray meets the plane through the target facing the camera.
fn zoom_anchor(v: &View3d, rect: Rect, pos: Option<egui::Pos2>) -> Option<[f64; 3]> {
    let at = pos? - rect.min;
    let (x, y, w, h) = (f64::from(at.x), f64::from(at.y), f64::from(rect.width()), f64::from(rect.height()));
    v.shown().and_then(|b| view3d::surface_at(&b.scene, &v.camera, x, y, w, h)).or_else(|| v.camera.on_target_plane(x, y, w, h))
}

/// What an orbit beginning with the pointer at `pos` turns about (Settings ▸ 3D view ▸ Orbit
/// around): the target (none), the origin, or the model point under the cursor (over empty
/// space, the middle of what a fit frames), chosen once when the orbit begins.
fn pivot(app: &EmcraftApp, v: &View3d, rect: Rect, pos: Option<egui::Pos2>, frame: ([f64; 3], [f64; 3])) -> Option<[f64; 3]> {
    match app.prefs.orbit_around() {
        OrbitAround::ViewCentre => None,
        OrbitAround::Origin => Some([0.0; 3]),
        OrbitAround::Cursor => Some(
            pos.and_then(|p| {
                let at = p - rect.min;
                v.shown().and_then(|b| {
                    view3d::surface_at(&b.scene, &v.camera, f64::from(at.x), f64::from(at.y), f64::from(rect.width()), f64::from(rect.height()))
                })
            })
            .unwrap_or_else(|| [0, 1, 2].map(|i| 0.5 * (frame.0[i] + frame.1[i]))),
        ),
    }
}

/// A wheel event's move in points, as egui counts them: lines at its line speed, pages at
/// `page` points.
fn wheel_points(unit: egui::MouseWheelUnit, delta: egui::Vec2, o: &egui::InputOptions, page: f32) -> egui::Vec2 {
    match unit {
        egui::MouseWheelUnit::Point => delta,
        egui::MouseWheelUnit::Line => delta * o.line_scroll_speed,
        egui::MouseWheelUnit::Page => delta * page,
    }
}

/// The wheel's vertical share, with egui's modifiers that turn it sideways or upright.
fn wheel_vertical(points: egui::Vec2, m: egui::Modifiers, o: &egui::InputOptions) -> f32 {
    let (sideways, upright) = (m.matches_any(o.horizontal_scroll_modifier), m.matches_any(o.vertical_scroll_modifier));
    if sideways && !upright {
        0.0
    } else if upright && !sideways {
        points.x + points.y
    } else {
        points.y
    }
}

/// This frame's wheel and trackpad: wheel zoom (points), pinch factor, two-finger pan and
/// Shift + two-finger move.
#[derive(Default)]
struct Wheel {
    zoom: f32,
    pinch: f32,
    pan: egui::Vec2,
    shift_pan: egui::Vec2,
}

/// Reads the wheel events: a mouse wheel's lines and pages zoom; a trackpad's two-finger scroll
/// (point moves between its start and end) pans, with Shift orbits where the mapping says so;
/// bare point moves are a mouse with fine steps; the zoom modifier (Ctrl) and pinch zoom.
fn wheel(v: &mut View3d, ui: &egui::Ui, nav: &crate::navigation::NavPreset) -> Wheel {
    let o = ui.ctx().options(|o| o.input_options);
    let page = ui.ctx().viewport_rect().height();
    let mut w = Wheel { pinch: 1.0, ..Wheel::default() };
    ui.input(|i| {
        for e in &i.events {
            match e {
                egui::Event::Zoom(f) if f.is_finite() && *f > 0.0 => w.pinch *= f,
                egui::Event::MouseWheel { unit, delta, modifiers, phase } => {
                    let point = *unit == egui::MouseWheelUnit::Point;
                    match phase {
                        egui::TouchPhase::Start => {
                            v.gesture |= point;
                            continue;
                        }
                        egui::TouchPhase::End | egui::TouchPhase::Cancel => {
                            v.gesture &= !point;
                            continue;
                        }
                        egui::TouchPhase::Move => {}
                    }
                    let pts = wheel_points(*unit, *delta, &o, page);
                    if !pts.is_finite() {
                        continue;
                    }
                    if modifiers.matches_any(o.zoom_modifier) {
                        let f = (o.scroll_zoom_speed * (pts.x + pts.y)).exp();
                        if f.is_finite() && f > 0.0 {
                            w.pinch *= f;
                        }
                        continue;
                    }
                    let shift = modifiers.shift && !modifiers.ctrl && !modifiers.command && !modifiers.alt;
                    if point && v.gesture && nav.scroll_pans() {
                        if shift && nav.shift_scroll_orbits() {
                            w.shift_pan += pts;
                        } else {
                            w.pan += pts;
                        }
                    } else if point {
                        // Bare point moves are a mouse: up and down zoom, sideways pans.
                        w.zoom += wheel_vertical(pts, *modifiers, &o);
                        w.pan.x -= pts.x;
                    } else {
                        w.zoom += wheel_vertical(pts, *modifiers, &o);
                    }
                }
                _ => {}
            }
        }
    });
    w
}

/// Orbit, pan and zoom by the mouse mapping (Settings ▸ 3D view), the wheel and the trackpad; a
/// click picks what is under it (empty space clears), a double-click on nothing fits the view, a
/// middle double-click goes home (fits, in the mapping that does). A plain left press on the
/// reference plane's `grip` or on the field view's plane drags that instead, as SolidCraft's
/// in-view handles take a plain press on them: the mapping's left-button navigation is for
/// empty space or with its keys held, and Ctrl+left drag orbits in every mapping. With a tool
/// on, a click is the tool's.
fn navigate(app: &EmcraftApp, v: &mut View3d, ui: &mut egui::Ui, rect: Rect, frame: ([f64; 3], [f64; 3]), grip: Option<egui::Pos2>) -> Gesture {
    use crate::navigation::{DRAG_ZOOM_PER_POINT, NavButton, OPENSCAD, WHEEL_ZOOM_PER_POINT};
    let nav = app.prefs.nav();
    let resp = ui.hit(rect, Fixed::View3d.into(), Sense::click_and_drag());
    let (mods, hover, press, latest, space, any_down) = ui.input(|i| {
        (i.modifiers, i.pointer.hover_pos(), i.pointer.press_origin(), i.pointer.latest_pos(), i.key_down(egui::Key::Space), i.pointer.any_down())
    });
    let space = space && !ui.ctx().egui_wants_keyboard_input();
    let h = f64::from(rect.height());
    let limits = zoom_limits(frame);
    let sign = if nav.reverse_zoom { -1.0 } else { 1.0 };
    let ctrl = mods.ctrl || mods.command;
    // The left button: chosen once, when the drag begins.
    let mut grip_drag = GripDrag::default();
    if resp.drag_started_by(egui::PointerButton::Primary) {
        let at = press.or(resp.interact_pointer_pos());
        let plain = !mods.shift && !ctrl && !mods.alt;
        v.left = if space {
            Some(LeftDrag::Nav(NavMode::Pan, false))
        } else if plain && grip.zip(at).is_some_and(|(g, p)| g.distance(p) <= crate::tools3d::GRIP_PX) {
            Some(LeftDrag::Grip)
        } else if plain && at.is_some_and(|p| v.pick_at(rect, p) == Some(scene3d::Pick::Plane(0))) {
            Some(LeftDrag::Plane)
        } else if let Some(m) = nav.mode(NavButton::Left, mods) {
            Some(LeftDrag::Nav(m, mods.shift))
        } else if ctrl && !mods.shift && !mods.alt {
            Some(LeftDrag::Nav(NavMode::Orbit, false))
        } else if v.tools.tool.is_none() {
            Some(LeftDrag::Box { add: mods.shift })
        } else {
            None
        };
        grip_drag.started = v.left == Some(LeftDrag::Grip);
    }
    grip_drag.released = v.left == Some(LeftDrag::Grip) && !resp.dragged();
    // The box follows the pointer from the press; let go, it selects.
    let mut select = false;
    if let Some(LeftDrag::Box { add }) = v.left {
        if resp.dragged() {
            v.boxsel = press.zip(latest).map(|(a, b)| BoxSel::from_drag(a, b)).filter(|b| b.rect.is_finite());
        } else if let Some(b) = v.boxsel.take() {
            box_select(v, rect, b, add);
            select = true;
        }
    }
    if !resp.dragged() {
        v.left = None;
        v.boxsel = None;
    }
    let boxing = matches!(v.left, Some(LeftDrag::Box { .. }));
    let d = resp.drag_delta();
    let plane_drag = (v.left == Some(LeftDrag::Plane) && resp.dragged()).then_some(d);
    grip_drag.moved = (v.left == Some(LeftDrag::Grip) && resp.dragged()).then_some(d);
    // The middle and right buttons by the mapping, the left as chosen at its press.
    let button = if resp.dragged_by(egui::PointerButton::Middle) {
        Some(NavButton::Middle)
    } else if resp.dragged_by(egui::PointerButton::Secondary) {
        Some(NavButton::Right)
    } else {
        None
    };
    let (mode, needs_shift) = match (button, v.left) {
        (Some(b), _) => (nav.mode(b, mods), nav.mode(b, egui::Modifiers { shift: false, ..mods }) != nav.mode(b, mods)),
        (None, Some(LeftDrag::Nav(m, shift))) if resp.dragged_by(egui::PointerButton::Primary) => (Some(m), shift),
        _ => (None, false),
    };
    // The drag's first frame catches up the way from the press, so none of it is lost.
    let d = if resp.drag_started() { press.zip(latest).map_or(d, |(a, b)| b - a) } else { d };
    let d = if d.is_finite() { d } else { egui::Vec2::ZERO };
    let mut orbiting = false;
    match mode {
        Some(NavMode::Orbit) => {
            v.stop_animation();
            let snap = mods.shift && !needs_shift && nav.id != OPENSCAD;
            let o = match v.orbit {
                Some(o) => o,
                None => Drag::new(v.camera, pivot(app, v, rect, press.or(hover), frame)),
            };
            let mut o = o;
            o.orbit(&mut v.camera, f64::from(d.x), f64::from(d.y), snap);
            v.orbit = Some(o);
            orbiting = true;
        }
        Some(NavMode::Pan) if d != egui::Vec2::ZERO => {
            v.stop_animation();
            v.camera.pan(f64::from(d.x), f64::from(d.y), h);
        }
        Some(NavMode::Zoom) if d != egui::Vec2::ZERO => {
            v.stop_animation();
            v.camera.zoom_at(None, (f64::from(d.y) * DRAG_ZOOM_PER_POINT).exp(), limits);
        }
        _ => {}
    }
    // The touchpad mapping: Shift or Alt held and the pointer moved, no button down.
    let inside = resp.hovered() || resp.contains_pointer();
    let motion = nav.motion_mode(mods).zip(hover).filter(|_| inside && !any_down && !ui.ctx().egui_wants_keyboard_input());
    let before = v.motion;
    v.motion = motion;
    if let Some((m, at)) = motion {
        let step = before.filter(|(b, _)| *b == m).map_or(egui::Vec2::ZERO, |(_, last)| at - last);
        let step = if step.is_finite() && step.length() <= 1e4 { step } else { egui::Vec2::ZERO };
        if step != egui::Vec2::ZERO {
            v.stop_animation();
            if m == NavMode::Orbit {
                let mut o = v.orbit.unwrap_or_else(|| Drag::new(v.camera, pivot(app, v, rect, Some(at), frame)));
                o.orbit(&mut v.camera, f64::from(step.x), f64::from(step.y), false);
                v.orbit = Some(o);
            } else {
                v.camera.pan(f64::from(step.x), f64::from(step.y), h);
            }
        }
        orbiting |= m == NavMode::Orbit;
    }
    // The wheel and the trackpad, toward the point under the cursor.
    let w = wheel(v, ui, nav);
    if inside {
        if w.pinch != 1.0 && w.pinch.is_finite() {
            v.stop_animation();
            v.camera.zoom_at(zoom_anchor(v, rect, hover), f64::from(w.pinch).powf(-sign), limits);
        }
        if w.zoom != 0.0 && w.zoom.is_finite() {
            v.stop_animation();
            v.camera.zoom_at(zoom_anchor(v, rect, hover), (-f64::from(w.zoom) * WHEEL_ZOOM_PER_POINT * sign).exp(), limits);
        }
        if w.pan != egui::Vec2::ZERO && w.pan.is_finite() {
            v.stop_animation();
            v.camera.pan(f64::from(w.pan.x), f64::from(w.pan.y), h);
        }
        if w.shift_pan != egui::Vec2::ZERO && w.shift_pan.is_finite() {
            v.stop_animation();
            let mut o = v.orbit.unwrap_or_else(|| Drag::new(v.camera, pivot(app, v, rect, hover, frame)));
            o.orbit(&mut v.camera, f64::from(w.shift_pan.x), f64::from(w.shift_pan.y), false);
            v.orbit = Some(o);
            orbiting = true;
        }
    }
    if !orbiting && !(v.gesture && v.orbit.is_some()) {
        v.orbit = None;
    }
    // A box leaves the picture as it is.
    v.dragging = (resp.dragged() && !boxing) || orbiting;
    if resp.double_clicked_by(egui::PointerButton::Middle) && inside {
        v.goal = Some(if nav.middle_double_fits { Goal::Fit } else { Goal::Home });
    }
    let mut double = false;
    if v.tools.tool.is_some() {
        return Gesture { select: false, double, plane_drag, grip: grip_drag, tool_click: resp.clicked() };
    }
    if (resp.clicked() || resp.double_clicked())
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let pick = v.pick_at(rect, pos);
        if resp.double_clicked() {
            v.selected = pick.into_iter().collect();
            match pick {
                Some(_) => double = true,
                None => v.goal = Some(Goal::Fit),
            }
        } else if mods.shift || ctrl {
            // Shift or Ctrl toggles the part (on nothing, the selection stays), as in SolidCraft.
            if let Some(p) = pick {
                selection::toggle(&mut v.selected, p);
            }
        } else {
            v.selected = pick.into_iter().collect();
        }
    }
    Gesture { select: select || resp.clicked(), double, plane_drag, grip: grip_drag, tool_click: false }
}

/// Selects the parts the box `b` (screen points over the view in `rect`) takes, added to what is
/// selected when `add` (Shift held), else instead of it.
fn box_select(v: &mut View3d, rect: Rect, b: BoxSel, add: bool) {
    let local = BoxSel { rect: b.rect.translate(-rect.min.to_vec2()), crossing: b.crossing };
    let picks =
        v.shown().map(|s| selection::boxed(&s.scene, &v.camera, f64::from(rect.width()), f64::from(rect.height()), local)).unwrap_or_default();
    if !add {
        v.selected.clear();
    }
    selection::add(&mut v.selected, &picks);
}

/// The selection box: by window a solid outline, by crossing a dashed one in another colour, as
/// SolidCraft draws them.
fn paint_box(p: &egui::Painter, b: BoxSel, t: &Tokens) {
    let ink = if b.crossing { t.success } else { t.accent };
    p.rect_filled(b.rect, 0.0, ink.gamma_multiply(0.12));
    if b.crossing {
        let c = [b.rect.left_top(), b.rect.right_top(), b.rect.right_bottom(), b.rect.left_bottom()];
        for i in 0..4 {
            p.extend(egui::Shape::dashed_line(&[c[i], c[(i + 1) % 4]], Stroke::new(1.2, ink), 6.0, 4.0));
        }
    } else {
        p.rect_stroke(b.rect, 0.0, Stroke::new(1.2, ink), egui::StrokeKind::Inside);
    }
}

/// Whether the keys are the 3D view's: no field, menu, dialog card's field, search or page
/// has them.
fn keys_free(app: &EmcraftApp, ui: &egui::Ui) -> bool {
    app.settings.is_none()
        && app.files.prompt.is_none()
        && app.files.confirm.is_none()
        && !app.search.open
        && ui.ctx().memory(|m| m.focused().is_none())
        && !ui.ctx().egui_wants_keyboard_input()
        && !egui::Popup::is_any_open(ui.ctx())
}

/// The view keys, as SolidCraft's: the arrows turn the view 15° (with Shift a quarter turn),
/// animated, and held past a tap keep turning it at a steady rate; Ctrl+arrows pan a tenth of the
/// view; Ctrl+1…7 turn to Front, Back, Left, Right, Top, Bottom and the isometric view.
fn keys(app: &EmcraftApp, v: &mut View3d, ui: &egui::Ui, rect: Rect, now: f64) {
    use crate::navigation::{ARROWS, DIGIT_VIEWS, HOLD_DEGREES_PER_SECOND, KEY_PAN_SHARE, KEY_STEP_DEGREES, TAP_SECONDS};
    if !keys_free(app, ui) {
        v.key_hold = None;
        return;
    }
    let presses: Vec<(egui::Key, egui::Modifiers, bool)> = ui.input(|i| {
        i.events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Key { key, pressed: true, repeat, modifiers, .. } => Some((*key, *modifiers, *repeat)),
                _ => None,
            })
            .collect()
    });
    for (key, m, repeat) in presses {
        let ctrl = m.ctrl || m.command;
        if m.alt {
            continue;
        }
        if let Some(&(_, right, up)) = ARROWS.iter().find(|(k, _, _)| *k == key) {
            if ctrl {
                // The model moves with the arrow, as under a pan drag.
                let step = f64::from(rect.height() * KEY_PAN_SHARE);
                v.stop_animation();
                v.camera.pan(right * step, -up * step, f64::from(rect.height()));
            } else if !repeat {
                v.key_hold = Some(KeyHold { key, right, up, shift: m.shift, since: now });
                v.goal = Some(Goal::Turn { right, up, degrees: KEY_STEP_DEGREES[usize::from(m.shift)] });
            }
        } else if ctrl
            && !m.shift
            && let Some(&(_, view)) = DIGIT_VIEWS.iter().find(|(k, _)| *k == key)
        {
            v.goal = Some(Goal::View(view));
        }
    }
    let Some(hold) = v.key_hold else { return };
    let (down, dt) = ui.input(|i| (i.key_down(hold.key), f64::from(i.stable_dt)));
    if !down {
        v.key_hold = None;
        return;
    }
    ui.ctx().request_repaint();
    if now - hold.since < TAP_SECONDS {
        return;
    }
    let rate = if hold.shift { 2.0 * HOLD_DEGREES_PER_SECOND } else { HOLD_DEGREES_PER_SECOND };
    let degrees = rate * if dt.is_finite() { dt.clamp(0.0, 0.1) } else { 0.0 };
    if degrees > 1e-6 {
        v.stop_animation();
        v.camera = v.camera.stepped(hold.right, hold.up, degrees);
    }
}

/// The scene drawn into `rect`: by the window's GPU (every frame, full size) or the CPU
/// renderer's picture (cached by `scene_key`, the camera and the size).
fn draw(v: &mut View3d, ui: &egui::Ui, p: &egui::Painter, rect: Rect, scene_key: SceneKey, gpu: bool, t: &Tokens) {
    v.drawn_on_gpu = gpu;
    if gpu {
        let began = Instant::now();
        p.add(gradient(rect, t.viewport_top, t.viewport_bottom));
        if let Some(b) = v.shown() {
            // While the view moves, a large scene is drawn from its coarser copy.
            let scene = if v.dragging { b.moving() } else { &b.scene };
            p.add(mesh_of(&view3d::flatten(scene, &v.camera, f64::from(rect.width()), f64::from(rect.height())), rect.min, 1.4));
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
        if let Some(q) = l.anchor.and_then(|a| cam.project(a, rect.width() as f64, rect.height() as f64)) {
            let to = rect.min + vec2(q[0] as f32, q[1] as f32);
            p.line_segment([at, to], Stroke::new(1.0, ink));
            p.circle_filled(to, 2.5, ink);
        }
        let g = p.layout_no_wrap(l.text.clone(), font.clone(), ink);
        let r = Rect::from_center_size(at, g.size() + vec2(8.0, 4.0));
        if !away {
            p.rect_filled(r, 3.0, t.panel.gamma_multiply(0.85));
        }
        p.galley(r.min + vec2(4.0, 2.0), g, ink);
    }
}

/// What a selection card's buttons do: Edit (one item of the model), Delete (its items).
struct Card {
    edit: Option<Action>,
    delete: Option<Action>,
}

/// The selection on a card at the top: its name (or how many parts; TeX, its symbols set as
/// mathematics), with Edit and Delete beside it for items of the model (a port, a body).
fn selection_card(ui: &mut egui::Ui, p: &egui::Painter, rect: Rect, tex: &str, card: Card, enabled: bool, t: &Tokens, out: &mut Actions) -> Rect {
    let g = p.layout_job(crate::math::job(tex, t.font_small, t.text));
    let (size, pitch) = (20.0, 24.0);
    let actions: Vec<(Glyph, Fixed, &str, Action)> = [
        card.edit.map(|a| (Glyph::Edit, Fixed::View3dSelectedEdit, "Edit", a)),
        card.delete.map(|a| (Glyph::Delete, Fixed::View3dSelectedDelete, "Delete", a)),
    ]
    .into_iter()
    .flatten()
    .collect();
    let buttons = if actions.is_empty() { 0.0 } else { actions.len() as f32 * pitch + 4.0 };
    let width = g.size().x + 20.0 + buttons;
    let r = Rect::from_min_size(pos2(rect.center().x - width / 2.0, rect.top() + 10.0), vec2(width, g.size().y.max(size) + 10.0));
    p.rect_filled(r, 6.0, t.panel.gamma_multiply(0.92));
    p.rect_stroke(r, 6.0, Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
    p.galley(pos2(r.left() + 10.0, r.center().y - g.size().y / 2.0), g, t.text);
    scenario::publish_target(Fixed::View3dSelected, r);
    let mut x = r.right() - buttons;
    for (glyph, handle, tip, action) in actions {
        let b = Rect::from_center_size(pos2(x + pitch / 2.0, r.center().y), vec2(size, size));
        let resp = ui.hit(b, handle.into(), if enabled { Sense::click() } else { Sense::hover() });
        if resp.hovered() && enabled {
            p.rect_filled(b, 4.0, t.hover);
        }
        glyphs::paint(p, b.shrink(2.0), glyph, if enabled { t.text } else { t.text_dim }, t.accent);
        if resp.hint(tip).clicked() && enabled {
            out.push(action);
        }
        x += pitch;
    }
    r
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
            cache: Default::default(),
        }
    }
}

fn legend(p: &egui::Painter, rect: Rect, items: &[(String, Rgb)], t: &Tokens) {
    if items.is_empty() {
        return;
    }
    let font = FontId::proportional(t.font_small);
    let row = 18.0;
    // Each item's symbols set as mathematics (`εr` with a lowered r).
    let galleys: Vec<_> = items.iter().map(|(s, _)| p.layout_job(crate::math::rich(s, font.size, t.text))).collect();
    let width = galleys.iter().map(|g| g.size().x).fold(0.0, f32::max) + 34.0;
    let r = Rect::from_min_size(rect.min + vec2(10.0, 10.0), vec2(width, row * items.len() as f32 + 8.0));
    p.rect_filled(r, 6.0, t.panel.gamma_multiply(0.9));
    p.rect_stroke(r, 6.0, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    for (i, ((_, c), g)) in items.iter().zip(galleys).enumerate() {
        let y = r.top() + 4.0 + row * (i as f32 + 0.5);
        p.rect_filled(Rect::from_center_size(pos2(r.left() + 14.0, y), vec2(12.0, 12.0)), 2.0, color(*c));
        p.galley(pos2(r.left() + 26.0, y - g.size().y / 2.0), g, t.text);
    }
}

/// The standard view looking from the side of axis `k` given by `sign` (its eye there).
pub(crate) fn view_from(k: usize, sign: f64) -> Option<StandardView> {
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
    overlay: Overlay,
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
    // The tools: the ports' where ports are the subject, the ruler everywhere.
    use crate::tools3d::Tool;
    for tool in Tool::ALL.into_iter().filter(|tool| !tool.ports() || overlay == Overlay::Ports) {
        items.push((tool.glyph(), Handle::Tool(tool), tool.hint()));
    }
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
        let on = (handle == Handle::Fixed(Fixed::View3dSection) && v.section.is_some())
            || matches!(handle, Handle::Tool(tool) if v.tools.tool == Some(tool));
        if resp.hovered() || on {
            p.rect_filled(r, 5.0, if on { t.accent_soft } else { t.hover });
        }
        let ink = if near || resp.hovered() || on { t.text } else { t.text.gamma_multiply(0.7) };
        glyphs::paint(&p, r.shrink(4.0), glyph, ink, t.accent);
        if resp.on_hover_text(tip).clicked() {
            match handle {
                Handle::Fixed(Fixed::View3dFit) => v.goal = Some(Goal::Fit),
                Handle::Standard(side) => out.push(Action::StandardView(side)),
                Handle::Fixed(Fixed::View3dPlay) => out.push(Action::SetPlaying(!playing)),
                Handle::Fixed(Fixed::View3dSection) => v.section = v.section.is_none().then(|| Cut::middle(0, bounds)),
                Handle::Tool(tool) => out.push(Action::Tool((v.tools.tool != Some(tool)).then_some(tool))),
                Handle::Fixed(Fixed::View3dFace) => {
                    if let Some((lo, hi, side)) = facing {
                        v.goal = Some(Goal::Face(lo, hi, side));
                    }
                }
                _ => v.toggle_projection(),
            }
        }
    }
}

/// The section view's controls in a bar over the tools: a guide's H-plane and E-plane cuts, the
/// axis it cuts across (x, y, z), where along it (a slider over the model's extent, the value in
/// display units `l`), which side stays (flip), the camera straight at the cut and the cut's
/// dimensions on or off.
fn section_bar(v: &mut View3d, ui: &mut egui::Ui, rect: Rect, (lo, hi): ([f64; 3], [f64; 3]), l: emcraft_model::units::Length) {
    let Some(mut cut) = v.section else { return };
    let t = &Tokens::get();
    let k = cut.axis.min(2);
    let (size, pitch, slider_w, value_w) = (24.0, 28.0, 170.0, 84.0);
    let width = 5.0 * pitch + 8.0 + slider_w + value_w + 3.0 * pitch + 16.0;
    let bar = Rect::from_center_size(pos2(rect.center().x, rect.bottom() - 70.0), vec2(width, size + 8.0));
    let p = ui.painter().with_clip_rect(rect);
    p.rect_filled(bar, 8.0, t.panel.gamma_multiply(0.95));
    p.rect_stroke(bar, 8.0, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    let font = FontId::proportional(t.font_label);
    let mut x = bar.left() + 6.0;
    use crate::section3d::{Mark, Preset, button};
    for (preset, handle, letter, tip) in [
        (Preset::HPlane, Fixed::View3dSectionHPlane, "H", "H-plane cut (y = b/2)"),
        (Preset::EPlane, Fixed::View3dSectionEPlane, "E", "E-plane cut (x = a/2)"),
    ] {
        let r = Rect::from_min_size(pos2(x, bar.center().y - size / 2.0), vec2(size, size));
        if button(ui, &p, r, handle, Mark::Text(letter), tip, preset.is(cut, (lo, hi))) {
            cut = preset.cut((lo, hi));
        }
        x += pitch;
    }
    p.vline(x + 1.0, (bar.top() + 6.0)..=(bar.bottom() - 6.0), Stroke::new(1.0, t.border));
    x += 8.0;
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
    x += pitch;
    let r = Rect::from_min_size(pos2(x, bar.center().y - size / 2.0), vec2(size, size));
    if button(ui, &p, r, Fixed::View3dSectionLook, Mark::Glyph(Glyph::Lookat), "Look straight at the cut", false) {
        let outline = v.shown().map_or_else(|| crate::section3d::flat(cut, (lo, hi)), |b| crate::section3d::outline(&b.scene, cut, (lo, hi)));
        v.camera.projection = Projection::Orthographic;
        v.goal = crate::section3d::look(cut, outline);
    }
    x += pitch;
    let r = Rect::from_min_size(pos2(x, bar.center().y - size / 2.0), vec2(size, size));
    let dims = v.section_dims;
    if button(ui, &p, r, Fixed::View3dSectionDims, Mark::Glyph(Glyph::Ruler), if dims { "Dimensions: on" } else { "Dimensions: off" }, dims) {
        v.section_dims = !dims;
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
