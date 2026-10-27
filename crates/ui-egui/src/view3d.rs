//! The 3D view in the viewport: the session's scene ([`crate::scene3d`]) drawn by
//! `emcraft-render`'s CPU renderer into a texture, with orbit (left drag), pan (middle or shift
//! drag), zoom (wheel), fit (double click or Fit), the standard views, perspective or
//! orthographic projection, labels, a legend and an orientation indicator. While dragging the
//! picture is drawn at half resolution; at rest at full resolution, supersampled when small
//! enough.

use std::time::Instant;

use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, pos2, vec2};
use emcraft_render::Rgb;
use emcraft_render::view3d::{self, Camera, Projection, StandardView};

use emcraft_engine::view::Plane;

use crate::scene3d::{self, Built, Overlay};
use crate::theme::Tokens;
use crate::{EmcraftApp, Picture, glyphs, scenario};

/// The most pixels the view is drawn with (beyond, it is drawn smaller and scaled up).
const MAX_PIXELS: f64 = 2.4e6;
/// Supersampled at rest up to this many output pixels.
const SUPERSAMPLE_PIXELS: f64 = 1.3e6;

/// What the picture was drawn from.
#[derive(Clone, Debug, PartialEq)]
struct Key {
    scene: (u64, Overlay, String),
    camera: Camera,
    size: [usize; 2],
    supersample: usize,
    dark: bool,
}

/// The 3D view's state.
pub struct View3d {
    pub camera: Camera,
    /// The box last fitted to: a new geometry is framed once.
    fitted: Option<([f64; 3], [f64; 3])>,
    built: Option<((u64, Overlay, String), Built)>,
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
    /// The scene for `overlay` (built once per revision).
    pub fn built(&mut self, app_revision: u64, session: &emcraft_engine::Session, overlay: Overlay) -> &Built {
        self.built_with(app_revision, session, overlay, String::new(), &[])
    }

    /// The scene with the field's planes (`field_key` names the field's state).
    pub fn built_with(
        &mut self,
        app_revision: u64,
        session: &emcraft_engine::Session,
        overlay: Overlay,
        field_key: String,
        fields: &[scene3d::FieldLayer],
    ) -> &Built {
        let key = (app_revision, overlay, format!("{field_key}|{:?}", self.highlighted()));
        if self.built.as_ref().is_none_or(|(k, _)| *k != key) {
            self.built = Some((key, scene3d::build_with(session, overlay, fields, self.highlighted())));
        }
        match &self.built {
            Some((_, b)) => b,
            None => &EMPTY,
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

    /// The state for `ui.inspect`.
    pub fn inspect(&self) -> serde_json::Value {
        let c = &self.camera;
        let b = self.built.as_ref().map(|(k, b)| (k.1.id(), b.scene.tris.len(), b.scene.lines.len()));
        serde_json::json!({
            "yaw": c.yaw.to_degrees(), "pitch": c.pitch.to_degrees(), "distance": c.distance,
            "target": c.target,
            "projection": match c.projection { Projection::Perspective => "perspective", Projection::Orthographic => "orthographic" },
            "overlay": b.map(|x| x.0), "triangles": b.map(|x| x.1), "lines": b.map(|x| x.2),
            "ms": self.last_ms, "size": self.last_size,
            "selected": self.selected.map(|p| format!("{p:?}")),
            "hover": self.hover.map(|p| format!("{p:?}")),
            "gpu": self.drawn_on_gpu,
        })
    }
}

static EMPTY: Built =
    Built { scene: view3d::Scene { tris: Vec::new(), lines: Vec::new(), tag: 0 }, labels: Vec::new(), legend: Vec::new(), bounds: None };

fn rgb(c: Color32) -> Rgb {
    Rgb(c.r(), c.g(), c.b())
}

fn color(c: Rgb) -> Color32 {
    Color32::from_rgb(c.0, c.1, c.2)
}

/// Draws the 3D view in `rect` for `overlay`.
pub fn show(app: &mut EmcraftApp, ui: &mut egui::Ui, rect: Rect, overlay: Overlay) {
    let t = Tokens::get();
    let revision = app.revision;
    // The field's planes, for the Results step: the field view's and those kept.
    let (field_key, layers) = field_layers(app, overlay);
    let fl: Vec<scene3d::FieldLayer> = layers.iter().map(|(plot, plane, at)| scene3d::FieldLayer { plot, plane: *plane, at: *at }).collect();
    let built = app.view3d.built_with(revision, &app.session, overlay, field_key.clone(), &fl).clone_shallow();
    let Some(bounds) = built.bounds.or_else(|| built.scene.bounds()) else {
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            "No geometry yet: open a sample or start at step 1, Geometry.",
            FontId::proportional(t.font_heading),
            t.text_dim,
        );
        return;
    };
    let frame = focus(&app.session.model, bounds);
    let v = &mut app.view3d;
    let aspect = (rect.width() / rect.height().max(1.0)) as f64;
    if v.fitted != Some(bounds) || v.fit_next {
        v.camera.fit(frame.0, frame.1, aspect);
        v.fitted = Some(bounds);
        v.fit_next = false;
    }
    let edit = navigate(v, ui, rect, frame);
    if let Some(d) = edit.plane_drag {
        move_plane(app, rect, bounds, d);
    }
    if overlay == Overlay::Field {
        publish_plane(app, rect, bounds);
    }
    let picked = app.view3d.selected.and_then(|p| crate::browser::row_of(app, p));
    if edit.clicked {
        app.browser.selected = picked.as_ref().map(|(k, _)| k.clone());
    }
    if edit.double
        && let Some((_, row)) = &picked
        && let crate::browser::Act::Edit(id) = row.act
        && !app.busy()
        && let Err(e) = app.open_dialog(id)
    {
        app.message(e, true);
    }
    // Moving pictures (an orbit, a playing field) draw on the GPU.
    let gpu = app.prefs.gpu3d || app.view3d.dragging || (overlay == Overlay::Field && app.field.player.playing);
    let key = format!("{field_key}|{:?}", app.view3d.highlighted());
    let p = ui.painter().with_clip_rect(rect);
    draw(&mut app.view3d, ui, &p, rect, (revision, overlay, key), gpu, &t);
    labels(&p, rect, &app.view3d.camera, &built.labels, &t);
    legend(&p, rect, &built.legend, &t);
    if let Some((_, row)) = &picked {
        selection_card(&p, rect, row, &t);
    }
    let facing = layers.first().map(|(plot, plane, at)| facing_plane(app.session.display.length, bounds.0, plot, *plane, *at));
    navigator(app, ui, rect);
    nav_bar(app, ui, rect, facing);
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
fn publish_plane(app: &EmcraftApp, rect: Rect, (lo, hi): ([f64; 3], [f64; 3])) {
    let Some((span, _)) = crate::viewport::plane_span_of(app, app.field.view.plane) else { return };
    let axis = normal(app.field.view.plane);
    let frac = if span > 0.0 { app.field.at.unwrap_or(span / 2.0) / span } else { 0.5 };
    let mut c = [0, 1, 2].map(|i| 0.5 * (lo[i] + hi[i]));
    c[axis] = lo[axis] + frac * (hi[axis] - lo[axis]);
    if let Some(p) = app.view3d.camera.project(c, f64::from(rect.width()), f64::from(rect.height())) {
        scenario::publish_handle("view3d:plane", rect.min + vec2(p[0] as f32, p[1] as f32));
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
fn move_plane(app: &mut EmcraftApp, rect: Rect, (lo, hi): ([f64; 3], [f64; 3]), d: egui::Vec2) {
    let Some((span, _)) = crate::viewport::plane_span_of(app, app.field.view.plane) else { return };
    let axis = normal(app.field.view.plane);
    let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
    let mid = [0, 1, 2].map(|i| 0.5 * (lo[i] + hi[i]));
    let (mut a, mut b) = (mid, mid);
    a[axis] = lo[axis];
    b[axis] = hi[axis];
    let cam = &app.view3d.camera;
    let (Some(pa), Some(pb)) = (cam.project(a, w, h), cam.project(b, w, h)) else { return };
    let along = vec2((pb[0] - pa[0]) as f32, (pb[1] - pa[1]) as f32);
    let len2 = along.length_sq();
    if len2 < 4.0 {
        return;
    }
    let at = app.field.at.unwrap_or(span / 2.0);
    app.field.at = Some((at + f64::from(d.dot(along) / len2) * span).clamp(0.0, span));
    app.field.pinned = None;
}

/// The field plots for the 3D view in the Results step, and a key naming their state.
fn field_layers(app: &mut EmcraftApp, overlay: Overlay) -> (String, Vec<(emcraft_render::plot::FieldPlot, Plane, f64)>) {
    if overlay != Overlay::Field {
        return (String::new(), Vec::new());
    }
    match crate::viewport::field_plots(app) {
        Ok(list) => (format!("{:?}|{:?}|{:?}", app.field.view, app.field.at, app.field.kept), list),
        Err(e) => {
            app.message(e, true);
            (String::new(), Vec::new())
        }
    }
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
    let resp = ui.interact(rect, ui.id().with("view3d"), Sense::click_and_drag());
    scenario::publish_target("view3d", rect);
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
            let tag = v.built.as_ref().and_then(|(_, b)| {
                view3d::pick(&b.scene, &v.camera, f64::from(at.x), f64::from(at.y), f64::from(rect.width()), f64::from(rect.height()))
            });
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
        let tag = v.built.as_ref().and_then(|(_, b)| {
            view3d::pick(&b.scene, &v.camera, f64::from(at.x), f64::from(at.y), f64::from(rect.width()), f64::from(rect.height()))
        });
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
fn draw(v: &mut View3d, ui: &egui::Ui, p: &egui::Painter, rect: Rect, scene_key: (u64, Overlay, String), gpu: bool, t: &Tokens) {
    v.drawn_on_gpu = gpu;
    if gpu {
        let began = Instant::now();
        p.add(gradient(rect, t.viewport_top, t.viewport_bottom));
        if let Some((_, b)) = &v.built {
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
    let key = Key { scene: scene_key, camera: v.camera, size, supersample, dark: crate::theme::is_dark() };
    if v.picture.as_ref().is_none_or(|(k, _)| *k != key) {
        let began = Instant::now();
        let opt = view3d::Options {
            width: size[0],
            height: size[1],
            supersample,
            background: (rgb(t.viewport_top), rgb(t.viewport_bottom)),
            line_width: (scale / ppp).clamp(0.5, 2.0) * ppp.max(1.0),
        };
        let canvas = match &v.built {
            Some((_, b)) => view3d::render(&b.scene, &v.camera, &opt),
            None => emcraft_render::Canvas::new(size[0], size[1], rgb(t.viewport_top)),
        };
        v.last_ms = began.elapsed().as_secs_f64() * 1e3;
        v.last_size = size;
        v.picture = Some((key, Picture::new("view3d".into(), &canvas)));
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
    let hint = if matches!(row.act, crate::browser::Act::Edit(_)) { "  ·  double-click to edit" } else { "" };
    let g = p.layout_no_wrap(format!("{}{hint}", row.text.trim()), FontId::proportional(t.font_small), t.text);
    let r = Rect::from_min_size(pos2(rect.center().x - g.size().x / 2.0 - 10.0, rect.top() + 10.0), g.size() + vec2(20.0, 10.0));
    p.rect_filled(r, 6.0, t.panel.gamma_multiply(0.92));
    p.rect_stroke(r, 6.0, Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
    p.galley(r.min + vec2(10.0, 5.0), g, t.text);
    scenario::publish_target("view3d:selected", r);
}

/// A field plane's box in the model (m) and the standard view that faces it.
fn facing_plane(
    l: emcraft_model::units::Length,
    lo: [f64; 3],
    plot: &emcraft_render::plot::FieldPlot,
    plane: Plane,
    at: f64,
) -> ([f64; 3], [f64; 3], StandardView) {
    let ((h0, h1), (v0, v1)) = (plot.x.span(), plot.y.span());
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

/// The axes' colours: x red, y green, z blue.
const AXIS_COLORS: [Color32; 3] = [Color32::from_rgb(214, 60, 60), Color32::from_rgb(60, 160, 70), Color32::from_rgb(60, 110, 220)];

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
fn navigator(app: &mut EmcraftApp, ui: &mut egui::Ui, rect: Rect) {
    let t = Tokens::get();
    let cam = app.view3d.camera;
    let c = pos2(rect.left() + 52.0, rect.bottom() - 52.0);
    let p = ui.painter().with_clip_rect(rect);
    p.circle_filled(c, 44.0, t.panel.gamma_multiply(0.75));
    let axes = view3d::axes_on_screen(&cam);
    let forward = cam.basis().forward;
    // Farthest first, so the tips towards the eye are drawn (and hit) on top.
    let mut tips: Vec<(usize, f64)> = (0..3).flat_map(|k| [(k, 1.0), (k, -1.0)]).collect();
    tips.sort_by(|a, b| (forward[b.0] * b.1).total_cmp(&(forward[a.0] * a.1)));
    let mut clicked = None;
    for (k, sign) in tips {
        let dir = vec2(axes[k][0] as f32, axes[k][1] as f32) * sign as f32;
        let tip = c + dir * 30.0;
        let color = AXIS_COLORS[k];
        if sign > 0.0 {
            p.line_segment([c, tip], Stroke::new(2.0, color));
        }
        let r = Rect::from_center_size(tip, vec2(18.0, 18.0));
        let Some(view) = view_from(k, sign) else { continue };
        let resp = ui.interact(r, ui.id().with(("navigator", k, sign as i32)), Sense::click());
        let name = format!("{}{}", if sign > 0.0 { "+" } else { "−" }, ["X", "Y", "Z"][k]);
        let hover = resp.hovered();
        if sign > 0.0 {
            p.circle_filled(tip, if hover { 10.0 } else { 9.0 }, color);
            p.text(tip, Align2::CENTER_CENTER, &name[1..], FontId::proportional(t.font_small), Color32::WHITE);
        } else {
            p.circle_filled(tip, 7.0, t.panel);
            p.circle_stroke(tip, if hover { 8.0 } else { 7.0 }, Stroke::new(2.0, color));
        }
        scenario::publish_target(&format!("view3d:std:{}", view.name()), r);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, view.name()));
        if resp.on_hover_text(format!("View from {name} ({})", view.name())).clicked() {
            clicked = Some(view);
        }
    }
    if let Some(v) = clicked {
        app.view3d.standard(v);
    }
}

/// The view's tools as icons in a bar at the bottom centre: fit, the home (isometric) view,
/// facing the field's plane (in Results), perspective or orthographic.
fn nav_bar(app: &mut EmcraftApp, ui: &mut egui::Ui, rect: Rect, facing: Option<([f64; 3], [f64; 3], StandardView)>) {
    let t = Tokens::get();
    let ortho = app.view3d.camera.projection == Projection::Orthographic;
    let mut items: Vec<(&str, &str, &str)> =
        vec![("fit", "view3d:fit", "Fit the model to the view (double-click the view)"), ("home", "view3d:std:iso", "Home view (isometric)")];
    if facing.is_some() {
        items.push(("lookat", "view3d:face", "Face the field's plane"));
        // The field's phase, played like a video.
        items.push(if app.field.player.playing {
            ("pause", "view3d:play", "Pause the field's animation")
        } else {
            ("play", "view3d:play", "Play the field's animation")
        });
    }
    items.push(if ortho {
        ("orthographic", "view3d:projection", "Orthographic — click for perspective")
    } else {
        ("perspective", "view3d:projection", "Perspective — click for orthographic")
    });
    let (size, pitch) = (28.0, 32.0);
    let width = pitch * items.len() as f32 + 6.0;
    let bar = Rect::from_center_size(pos2(rect.center().x, rect.bottom() - 26.0), vec2(width, size + 8.0));
    let p = ui.painter().with_clip_rect(rect);
    let near = ui.input(|i| i.pointer.hover_pos()).is_some_and(|q| bar.expand(24.0).contains(q));
    p.rect_filled(bar, 8.0, t.panel.gamma_multiply(if near { 0.95 } else { 0.6 }));
    p.rect_stroke(bar, 8.0, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    for (i, (glyph, handle, tip)) in items.into_iter().enumerate() {
        let r = Rect::from_center_size(pos2(bar.left() + 3.0 + pitch * (i as f32 + 0.5), bar.center().y), vec2(size, size));
        let resp = ui.interact(r, ui.id().with(handle), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tip));
        if resp.hovered() {
            p.rect_filled(r, 5.0, t.hover);
        }
        let ink = if near || resp.hovered() { t.text } else { t.text.gamma_multiply(0.7) };
        glyphs::paint(&p, r.shrink(4.0), glyph, ink, t.accent);
        scenario::publish_target(handle, r);
        if resp.on_hover_text(tip).clicked() {
            match handle {
                "view3d:fit" => app.view3d.fit_next = true,
                "view3d:std:iso" => app.view3d.standard(StandardView::Iso),
                "view3d:play" => app.field.player.playing = !app.field.player.playing,
                "view3d:face" => {
                    if let Some((lo, hi, v)) = facing {
                        app.view3d.camera.standard(v);
                        app.view3d.camera.fit(lo, hi, (rect.width() / rect.height().max(1.0)) as f64);
                    }
                }
                _ => app.view3d.toggle_projection(),
            }
        }
    }
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
