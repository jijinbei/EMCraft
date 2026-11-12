//! The 3D view in the viewport: the session's scene ([`crate::scene3d`]) drawn by the GPU
//! ([`crate::gpu3d`]: the window's device in a paint callback, else an offscreen device read back
//! into a texture; with no adapter at all, a message), navigated as in SolidCraft
//! ([`crate::navigation`]: the mouse mapping's orbit, pan and zoom, the wheel and pinch toward
//! the cursor, the keys, animated view changes), fit (double click or Fit), the standard views,
//! perspective or orthographic projection, labels, a legend and a view cube
//! ([`crate::view_cube`]) or the axes. At rest the picture is supersampled when small enough. A
//! section view cuts the scene across an axis (`emcraft_render::view3d::section`: the far side
//! removed, the cut faces capped in each body's colour); the GPU draws, and picking reads, the cut
//! scene, cached per scene and cut.

use std::time::Instant;

use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, pos2, vec2};
use emcraft_render::view3d::{self, Camera, CameraAnim, Drag, Projection, StandardView};

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

mod chrome;
mod input;

pub use chrome::*;
use input::*;

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
    /// The GPU colours the plane (`gpu3d::field`); without arrows the phase is left out, as
    /// the scene does not change with it.
    gpu: bool,
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

pub use emcraft_ui_core::scene3d::Cut;

/// The render's section of `cut`, outlined in the theme's scene edge colour.
fn section_of(cut: Cut) -> view3d::Section {
    view3d::Section { outline: Some(crate::theme::scene_colours().wire), ..view3d::Section::across(cut.axis, cut.at, cut.keep) }
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
    /// The offscreen picture shown, by what it was drawn from ([`crate::gpu3d::Frame::key`] and
    /// what the window adds).
    picture: Option<(u64, Picture)>,
    /// Why the GPU could not draw the view (shown in it).
    gpu_error: Option<String>,
    /// This view's id on the offscreen device ([`crate::gpu3d::picture`]).
    client: u64,
    /// Milliseconds the last picture took, and its size.
    pub last_ms: f64,
    pub last_size: [usize; 2],
    pub dragging: bool,
    /// Fit on the next frame.
    pub fit_next: bool,
    /// The box being dragged (a plain left drag), in screen points.
    pub boxsel: Option<BoxSel>,
    /// What a row elsewhere (the port-mode table) points at while hovered: highlighted for one
    /// frame instead of the selection, which it leaves alone.
    pub hover: Option<scene3d::Pick>,
    /// Whether the last frame was drawn (by the GPU; not when there is no adapter).
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
    /// What drew the last frame.
    pub drawn_by: Drawer,
    /// The scene packed for the GPU's view ([`crate::gpu3d`]).
    gpu: crate::gpu3d::Cache,
    /// The window's GPU the view can draw with (its adapter), as last seen.
    pub gpu_adapter: Option<String>,
    /// The GPU frame drawn last whose per-pixel tags picking may read ([`crate::gpu3d::pick`]).
    gpu_frame: Option<u64>,
    /// The tag the GPU found under the pointer (outlined), with where and in which frame.
    pointer_tag: Option<([u32; 2], u64, u32)>,
    /// How the last pick was made: `gpu` or `cpu`.
    pub picked_by: &'static str,
    /// The field's plane for the GPU to colour this frame, and the grid uploaded last.
    field_frame: Option<crate::gpu3d::FieldFrame>,
    field_grid: Option<std::sync::Arc<crate::gpu3d::FieldGrid>>,
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
            drawn_by: Drawer::None,
            gpu_error: None,
            client: crate::gpu3d::next_id(),
            gpu: Default::default(),
            gpu_adapter: None,
            gpu_frame: None,
            pointer_tag: None,
            picked_by: "cpu",
            field_frame: None,
            field_grid: None,
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

    /// Drops the selection box being dragged (its left drag ends with it).
    pub(crate) fn cancel_box(&mut self) {
        self.boxsel = None;
        self.left = None;
    }

    /// What lies under `pos` in the view drawn into `rect`.
    fn pick_at(&mut self, ctx: &egui::Context, rect: Rect, pos: egui::Pos2) -> Option<scene3d::Pick> {
        let at = pos - rect.min;
        // The GPU's tags under the pointer, when the frame shown kept them; else the CPU's rays.
        // (An imported part's see-through metal: the rays find what it holds behind it.)
        if let Some(key) = self.gpu_frame
            && let Some((any, solid)) = crate::gpu3d::pick(ctx, self.client, key, pixel(ctx, at))
            && !matches!(scene3d::Pick::from_tag(if solid != 0 { solid } else { any }), Some(scene3d::Pick::Part(_)))
        {
            self.picked_by = "gpu";
            return scene3d::Pick::from_tag(if solid != 0 { solid } else { any });
        }
        // The CPU's rays (through the scene's bounding volumes).
        self.picked_by = "cpu";
        let tag = self.shown().and_then(|b| {
            b.pick(
                &self.camera,
                emcraft_render::view3d::Pixel::new(f64::from(at.x), f64::from(at.y), f64::from(rect.width()), f64::from(rect.height())),
            )
        });
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
        let s = section_of(now);
        match view3d::section::section(&full.scene, &s) {
            Ok(mut scene) => {
                // Keep the cut open: generated caps otherwise overlap the mesh slice and
                // make vacuum look like solid matter. The existing cap outlines stay.
                let size = full.bounds.map_or(1.0, |(lo, hi)| (0..3).map(|k| hi[k] - lo[k]).fold(0.0, f64::max));
                let eps = size * 1e-9;
                scene.tris.retain(|t| !t.p.iter().all(|p| (p[now.axis] - now.at).abs() <= eps));
                let cut = if key.overlay == Overlay::Mesh {
                    scene3d::mesh_slice(model, now.axis, now.at, crate::theme::scene_colours().wire)
                } else {
                    scene3d::section_outline(model, now.axis, now.at, crate::theme::scene_colours().wire)
                };
                let mut gpu_scene = full.scene.clone();
                gpu_scene.lines.extend(cut.lines.iter().copied());
                scene.lines.extend(cut.lines);
                let labels = full.labels.iter().filter(|l| s.keeps(l.at)).cloned().collect();
                let built = Built {
                    scene,
                    gpu_scene: Some(gpu_scene),
                    arrows: full.arrows.clone(),
                    labels,
                    legend: full.legend.clone(),
                    colour_bar: full.colour_bar.clone(),
                    arrow_legend: full.arrow_legend.clone(),
                    bounds: full.bounds,
                    cache: Default::default(),
                };
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
    pub fn highlighted<'a>(&'a self, selected: &'a [scene3d::Pick]) -> &'a [scene3d::Pick] {
        self.hover.as_ref().map_or(selected, std::slice::from_ref)
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
    pub fn inspect(&self, ui: &crate::ui_state::UiState, l: emcraft_model::units::Length) -> serde_json::Value {
        let selected = ui.selection();
        let c = &self.camera;
        let b =
            self.built.as_ref().zip(self.shown()).map(|((k, _), b)| {
                (k.overlay.id(), b.scene.tris.len(), b.scene.lines.len(), b.scene.tris.iter().filter(|t| t.wire.is_some()).count())
            });
        serde_json::json!({
            "yaw": c.yaw.to_degrees(), "pitch": c.pitch.to_degrees(), "distance": c.distance,
            "target": c.target,
            "projection": match c.projection { Projection::Perspective => "perspective", Projection::Orthographic => "orthographic" },
            "overlay": b.map(|x| x.0), "triangles": b.map(|x| x.1), "lines": b.map(|x| x.2),
            // Triangles drawn with their edges (the mesh's wireframe).
            "wired": b.map(|x| x.3),
            "ms": self.last_ms, "size": self.last_size,
            "selected": selected.last().map(|p| format!("{p:?}")),
            "selection": selected.iter().map(|p| format!("{p:?}")).collect::<Vec<_>>(),
            "box": self.boxsel.map(|b| serde_json::json!({"crossing": b.crossing, "rect": [b.rect.left(), b.rect.top(), b.rect.right(), b.rect.bottom()]})),
            "hover": self.hover.map(|p| format!("{p:?}")),
            "gpu": self.drawn_on_gpu,
            "animating": self.animating(),
            "renderer": self.drawn_by.id(),
            "gpu_adapter": self.gpu_adapter,
            "gpu_arrows": self.shown().map_or(0, |b| b.arrows.len()),
            "gpu_clip": self.section.is_some(),
            "scene_generation": self.scene_gen(),
            "picked_by": self.picked_by,
            "gpu_field": self.field_frame.is_some(),
            "section": self.section.map(|s| s.inspect(l)),
            "section_dims": self.section_dims,
            "tools": self.tools.inspect(ui.tool(), l),
        })
    }
}

use crate::theme::{color32 as color, rgb};

static EMPTY: Built = Built {
    scene: view3d::Scene { tris: Vec::new(), lines: Vec::new(), tag: 0 },
    gpu_scene: None,
    arrows: Vec::new(),
    labels: Vec::new(),
    legend: Vec::new(),
    colour_bar: None,
    arrow_legend: Vec::new(),
    bounds: None,
    cache: scene3d::Cache::none(),
};

/// Draws the 3D view (its state, `st.view3d`) in `rect` for `overlay`, the field view's planes
/// (`st.field`) in it when the overlay shows them.
pub fn show(app: &EmcraftApp, st: &mut crate::viewport::Parts, ui: &mut egui::Ui, rect: Rect, overlay: Overlay, out: &mut Actions) {
    let (v, field) = (&mut st.view3d, &mut st.field);
    let t = &Tokens::get();
    // The scene, built again only when what it shows changed (the field's planes, for the
    // Results step: the field view's and those kept).
    // A geometry-inspection cut may hide the field plane entirely. Entering the field
    // overlay starts with the whole model as context; its plane remains independently movable.
    if overlay == Overlay::Field && v.built.as_ref().is_some_and(|(k, _)| k.overlay != Overlay::Field) {
        v.section = None;
    }
    let gpu_field = overlay == Overlay::Field && app.prefs.gpu_look.field && field.kept.is_empty() && drawer_of(ui) != Drawer::None;
    let key = scene_key(app, v, field, overlay, gpu_field);
    if !v.is_built(&key) {
        build(app, v, field, overlay, key, out);
    }
    v.update_cut(app.session.model(), out);
    v.field_frame = if gpu_field { field_frame(app, v, field) } else { None };
    let built = v.built_shallow();
    let Some(bounds) = built.bounds.or_else(|| built.scene.bounds()) else {
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            tr(t!("No geometry yet: open a sample or start at step 1, Geometry.")),
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
    let grip = crate::grips::nearest(app, v, rect, ui.input(|i| i.pointer.hover_pos())).filter(|_| !app.busy());
    let edit = navigate(app, v, ui, rect, frame, grip);
    keys(app, v, ui, rect, now);
    if let Some(d) = edit.plane_drag {
        move_plane(app, &v.camera, field, rect, bounds, d);
    }
    tools(app, v, ui, rect, &edit, out);
    if overlay == Overlay::Field {
        publish_plane(app, &v.camera, field, rect, bounds);
    }
    // A click or a box asks the selection (crate::ui_state); a double click's edit follows.
    out.extend(edit.select);
    let picked = picked_rows(app, v);
    let single = picked.first().filter(|_| app.ui_state.selection().single().is_some());
    // Moving pictures (a drag, a view change, a playing field).
    let moving = v.dragging || v.anim.is_some() || (overlay == Overlay::Field && field.player.playing);
    let drawer = drawer_of(ui);
    v.gpu_adapter = match drawer {
        Drawer::Window => crate::gpu3d::available(ui.ctx()).map(|a| a.adapter),
        Drawer::Offscreen => crate::gpu3d::headless::shared().ok().map(|o| o.adapter.clone()),
        Drawer::None => None,
    };
    let look = app.prefs.gpu_look;
    if drawer == Drawer::None || !look.pick {
        v.gpu_frame = None;
    }
    pointer_tag(v, ui, rect, look.outline && !moving);
    draw(v, app.ui_state.selection(), ui, rect, moving, look);
    let p = ui.painter().with_clip_rect(rect);
    labels(&p, rect, &v.camera, &built.labels);
    crate::section3d::annotate(app, v, &p, rect, bounds);
    legend(&p, rect, &built);
    if overlay == Overlay::Field {
        field.hover = hover_probe(app, v, field, ui, rect, bounds.0);
        if let Some(probe) = &field.hover {
            hover_card(&p, ui, probe, field.view.component, field.view.phase);
        }
    }
    crate::tools3d::draw(app, v, &p, rect);
    if let Some(b) = v.boxsel {
        paint_box(&p, b);
    }
    if let Some((_, row)) = single {
        let hint = if row.editable() && row.item.is_none() { tr(t!("  ·  double-click to edit")) } else { "" };
        // Its symbols set as mathematics; a port with its role.
        let text = match &row.role {
            Some(role) => format!("{}  ·  {}{hint}", row.tex(), crate::math::texify(&role.tip())),
            None => format!("{}{hint}", row.tex()),
        };
        let item = row.item.clone();
        let card =
            Card { tex: text, edit: item.clone().map(Action::EditItem), delete: item.map(|i| Action::DeleteItems(vec![i])), enabled: !app.busy() };
        let card = card.show(ui, rect, out);
        crate::tools3d::ref_field(app, v, ui, card.center_bottom(), out);
    } else if app.ui_state.selection().len() > 1 {
        // Several parts: how many, and Delete for those that are items of the model.
        let items: Vec<crate::browser::Item> = picked.iter().filter_map(|(_, r)| r.item.clone()).collect();
        let text = trf(t!("{n} selected"), args!(n = app.ui_state.selection().len()));
        let card = Card { tex: text, edit: None, delete: (!items.is_empty()).then_some(Action::DeleteItems(items)), enabled: !app.busy() };
        card.show(ui, rect, out);
    }
    let facing = v.facing.map(|f| facing_plane(app.session.display().length, bounds.0, f));
    match app.prefs.navigator() {
        Navigator::Cube => crate::view_cube::show(v, ui, rect, out),
        Navigator::Axes => navigator(&v.camera, ui, rect, out),
    }
    let bar = NavBar {
        facing,
        standalone: app.place.view() != crate::View::Field,
        playing: field.player.playing,
        ports: overlay == Overlay::Ports,
        tool: app.ui_state.tool(),
    };
    bar.show(v, ui, rect, bounds, out);
    // A tool on: the band saying so, over the picture.
    crate::tools3d::banner(app, ui, rect);
    if v.section.is_some() && app.place.view() != crate::View::Field {
        section_bar(v, ui, rect, bounds, app.session.display().length);
    }
}

/// What a fit frames: an open problem's structure (its bodies, ports and sources) with room
/// around it, not the air and the layer that fill most of its box; else the whole scene.
fn focus(m: &emcraft_model::Model, (lo, hi): ([f64; 3], [f64; 3])) -> ([f64; 3], [f64; 3]) {
    if m.domain.open.is_none() {
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

/// The fields where the pointer's ray meets the field plane (model `lo` its low corner), when
/// it does inside the mesh.
fn hover_probe(app: &EmcraftApp, v: &View3d, field: &FieldState, ui: &egui::Ui, rect: Rect, lo: [f64; 3]) -> Option<emcraft_engine::view::Probe> {
    let samples = field.samples.as_ref()?;
    let at = ui.ctx().pointer_hover_pos().filter(|p| rect.contains(*p))?;
    let (origin, dir) = v.camera.ray(emcraft_render::view3d::Pixel::new(
        f64::from(at.x - rect.left()),
        f64::from(at.y - rect.top()),
        f64::from(rect.width()),
        f64::from(rect.height()),
    ))?;
    let k = samples.normal_axis();
    let plane = lo[k] + samples.at;
    if dir[k].abs() < 1e-12 {
        return None;
    }
    let t = (plane - origin[k]) / dir[k];
    let mut p = [0, 1, 2].map(|i| origin[i] + t * dir[i]);
    p[k] = plane;
    let unit = app.session.display().length;
    let q = samples.plane_point(emcraft_model::Point3::new(p[0], p[1], p[2]), unit)?;
    samples.probe(q)
}

/// A card by the pointer with the fields under it: the coloured quantity at this phase, and E
/// and H's amplitudes, each component.
fn hover_card(p: &egui::Painter, ui: &egui::Ui, probe: &emcraft_engine::view::Probe, component: emcraft_engine::view::Component, phase: f64) {
    let Some(at) = ui.ctx().pointer_hover_pos() else { return };
    let t = &Tokens::get();
    let fmt = emcraft_render::axes::format;
    let unit = |magnetic: bool| if magnetic { "A/m" } else { "V/m" };
    let (now, _) = probe.value(component, phase);
    let mut lines = vec![format!("{} = {} {}", component.tex(), fmt(now), unit(component.is_magnetic()))];
    for (f, v, magnetic) in [("E", probe.e, false), ("H", probe.h, true)] {
        lines.push(format!("$|{f}_x|$ {}  $|{f}_y|$ {}  $|{f}_z|$ {} {}", fmt(v.x.norm()), fmt(v.y.norm()), fmt(v.z.norm()), unit(magnetic)));
    }
    let galleys: Vec<_> = lines.iter().map(|l| p.layout_job(crate::math::job(l, t.font_small, t.text))).collect();
    let w = galleys.iter().map(|g| g.size().x).fold(0.0, f32::max) + 16.0;
    let h = galleys.iter().map(|g| g.size().y + 2.0).sum::<f32>() + 10.0;
    let r = Rect::from_min_size(at + vec2(16.0, 16.0), vec2(w, h));
    p.rect_filled(r, 6.0, t.panel.gamma_multiply(0.95));
    p.rect_stroke(r, 6.0, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    let mut y = r.top() + 5.0;
    for g in galleys {
        let gh = g.size().y;
        p.galley(pos2(r.left() + 8.0, y), g, t.text);
        y += gh + 2.0;
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
fn scene_key(app: &EmcraftApp, v: &View3d, field: &mut FieldState, overlay: Overlay, gpu: bool) -> SceneKey {
    let planes = (overlay == Overlay::Field).then(|| {
        let l = app.session.display().length;
        field.view.at = field.at.map(|v| l.to_si(v));
        let mut view = field.view.clone();
        if gpu {
            view.phase = 0.0;
        }
        FieldKey { view, at: field.at, kept: field.kept.iter().map(|k| (k.plane, k.at)).collect(), gpu }
    });
    SceneKey {
        revision: app.revision,
        overlay,
        field: planes,
        highlighted: v.highlighted(app.ui_state.selection()).to_vec(),
        theme: crate::theme::key(),
        language: lang::current(),
    }
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
    let gpu = key.field.as_ref().is_some_and(|f| f.gpu);
    let fl: Vec<scene3d::FieldLayer> = layers.iter().map(|(plot, plane, at)| scene3d::FieldLayer { plot, plane: *plane, at: *at, gpu }).collect();
    v.facing = layers.first().map(|(plot, plane, at)| Facing { x: plot.x.span(), y: plot.y.span(), plane: *plane, at: *at });
    let mut built = scene3d::build_with(&app.session, overlay, &crate::theme::scene_colours(), &fl, &key.highlighted);
    if gpu {
        dynamic_field_arrows(app, field, &mut built);
    }
    v.built = Some((key, built));
    v.generation = v.generation.wrapping_add(1);
}

/// Upload complex vectors once. Their phase, lengths and glyph geometry are evaluated in WGSL.
fn dynamic_field_arrows(app: &EmcraftApp, field: &FieldState, built: &mut Built) {
    let (Some(samples), Some((lo, _))) = (field.samples.as_ref(), built.bounds) else { return };
    let (x, y) = samples.axes();
    let [c, u, v] = scene3d::field_frame_of(&app.session, samples.plane, samples.at, x.span(), y.span(), lo);
    let normal = scene3d::normal_axis(samples.plane);
    let du = [0, 1, 2].map(|k| u[k] - c[k]);
    let dv = [0, 1, 2].map(|k| v[k] - c[k]);
    let len = |d: [f64; 3]| d.iter().map(|v| v * v).sum::<f64>().sqrt();
    let max_length = len(du).max(len(dv)) / 18.0 * 0.85;
    for (shown, component, color) in [
        (field.view.show_e, emcraft_engine::view::Component::E, crate::theme::scene().port_input),
        (field.view.show_h, emcraft_engine::view::Component::H, crate::theme::scene().port_output),
    ] {
        if !shown {
            continue;
        }
        let emcraft_engine::view::Phasors { nx, ny, grid: mut data } = samples.phasors(component);
        if nx < 2 || ny < 2 {
            continue;
        }
        // Normalize against the full complex field, not its in-plane numerical residue.
        let peak = emcraft_engine::view::robust_peak(data.iter().flatten().map(|(re, im)| len(*re).hypot(len(*im))).collect());
        for (re, im) in data.iter_mut().flatten() {
            re[normal] = 0.0;
            im[normal] = 0.0;
        }
        if !(peak > 0.0 && peak.is_finite()) {
            continue;
        }
        // An arrow at full length is the peak (its length clamps at it).
        built.arrow_legend.push((scene3d::arrow_legend(component.is_magnetic(), peak), color));
        let stride = nx.max(ny).div_ceil(18).max(1);
        for j in (stride / 2..ny).step_by(stride) {
            for i in (stride / 2..nx).step_by(stride) {
                let Some(Some((re, im))) = data.get(j * nx + i) else { continue };
                if !re.iter().chain(im).all(|v| v.is_finite()) || len(*re).hypot(len(*im)) < peak * 0.05 {
                    continue;
                }
                let centre = [0, 1, 2].map(|k| c[k] + du[k] * i as f64 / (nx - 1) as f64 + dv[k] * j as f64 / (ny - 1) as f64);
                for side in [-1.0, 1.0] {
                    let mut start = centre;
                    start[normal] += side * max_length * 0.14;
                    built.arrows.push(crate::gpu3d::Arrow {
                        start,
                        end: start,
                        color,
                        section_clip: true,
                        motion: Some(crate::gpu3d::Motion {
                            re: re.map(|v| v * max_length / peak),
                            im: im.map(|v| v * max_length / peak),
                            max_length,
                        }),
                    });
                }
            }
        }
    }
}

/// The browser rows of what is selected in the view, in its order (looked up again only when the
/// selection or the session changed).
fn picked_rows(app: &EmcraftApp, v: &mut View3d) -> Vec<(String, crate::browser::Row)> {
    if app.ui_state.selection().is_empty() {
        return Vec::new();
    }
    let key = (app.revision, app.ui_state.selection().to_vec());
    if v.picked.as_ref().is_none_or(|(k, _)| *k != key) {
        let rows = crate::browser::rows_of(app, &key.1);
        v.picked = Some((key, rows));
    }
    v.picked.as_ref().map(|(_, rows)| rows.clone()).unwrap_or_default()
}

/// What the pointer asked of the view this frame.
struct Gesture {
    /// What a click or a box asks of the selection.
    select: Option<Action>,
    /// The pointer's move this frame while dragging the field view's plane.
    plane_drag: Option<egui::Vec2>,
    grip: GripDrag,
    /// A click with a tool on: the tool takes it.
    tool_click: bool,
}

/// A drag of the reference plane's grip this frame: none, it began (and moved it this much), it
/// moved it, or it let it go.
#[derive(Clone, Copy, Debug, PartialEq)]
enum GripDrag {
    Idle,
    Started(egui::Vec2),
    Moved(egui::Vec2),
    Released,
}

/// The tools' share of the frame: the reference plane's drag, what the pointer is over, a
/// tool's click and Escape.
fn tools(app: &EmcraftApp, v: &mut View3d, ui: &egui::Ui, rect: Rect, edit: &Gesture, out: &mut Actions) {
    crate::grips::keys(app, v, ui, out);
    match edit.grip {
        GripDrag::Idle => {}
        GripDrag::Started(d) => {
            // The move from the press, the drag's threshold included: a grip follows the
            // pointer from where it was taken, not from where the drag was first told.
            let (press, now) = ui.input(|i| (i.pointer.press_origin(), i.pointer.hover_pos()));
            let pressed = press.or_else(|| now.map(|p| p - d));
            crate::grips::start_any(app, v, rect, pressed);
            crate::grips::moved(app, v, rect, pressed.zip(now).map_or(d, |(a, b)| b - a));
        }
        GripDrag::Moved(d) => crate::grips::moved(app, v, rect, d),
        GripDrag::Released => crate::grips::released(app, v, out),
    }
    crate::tools3d::hover(app, v, rect, ui.input(|i| i.pointer.hover_pos()));
    if edit.tool_click && !app.busy() {
        // On empty space — nothing under the pointer for the tool, not even a snap — a click
        // ends the tool, as Escape does; else it is the tool's.
        if v.tools.hover.is_none() {
            out.push(Action::Tool(None));
        } else {
            crate::tools3d::click(app, v, out);
        }
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
