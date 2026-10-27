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
        }
    }
}

impl View3d {
    /// The scene for `overlay` (built once per revision).
    pub fn built(&mut self, app_revision: u64, session: &emcraft_engine::Session, overlay: Overlay) -> &Built {
        self.built_with(app_revision, session, overlay, None)
    }

    /// The scene with the field's plane (`extra` names the field's state).
    pub fn built_with(
        &mut self,
        app_revision: u64,
        session: &emcraft_engine::Session,
        overlay: Overlay,
        field: Option<(String, &scene3d::FieldLayer)>,
    ) -> &Built {
        let key = (app_revision, overlay, field.as_ref().map(|f| f.0.clone()).unwrap_or_default());
        if self.built.as_ref().is_none_or(|(k, _)| *k != key) {
            self.built = Some((key, scene3d::build_with(session, overlay, field.map(|f| f.1))));
        }
        match &self.built {
            Some((_, b)) => b,
            None => &EMPTY,
        }
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
        })
    }
}

static EMPTY: Built = Built { scene: view3d::Scene { tris: Vec::new(), lines: Vec::new() }, labels: Vec::new(), legend: Vec::new(), bounds: None };

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
    // The field's plane, for the Results step.
    let mut field_key = String::new();
    let mut layer = None;
    if overlay == Overlay::Field {
        match crate::viewport::field_plot(app) {
            Ok(plot) => {
                field_key = format!("{:?}|{:?}", app.field.view, app.field.at);
                let (plane, at) = app.field.samples.as_ref().map_or((app.field.view.plane, 0.0), |s| (s.plane, s.at));
                layer = Some((plot, plane, at));
            }
            Err(e) => app.message(e, true),
        }
    }
    let v = &mut app.view3d;
    let fl = layer.as_ref().map(|(plot, plane, at)| scene3d::FieldLayer { plot, plane: *plane, at: *at });
    let built = v.built_with(revision, &app.session, overlay, fl.as_ref().map(|f| (field_key.clone(), f))).clone_shallow();
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
    let v = &mut app.view3d;
    let aspect = (rect.width() / rect.height().max(1.0)) as f64;
    if v.fitted != Some(bounds) || v.fit_next {
        v.camera.fit(bounds.0, bounds.1, aspect);
        v.fitted = Some(bounds);
        v.fit_next = false;
    }
    // Input.
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
    if resp.dragged() && (d.x != 0.0 || d.y != 0.0) {
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
    if resp.double_clicked() {
        v.camera.fit(bounds.0, bounds.1, aspect);
    }
    // The picture.
    let ppp = ui.ctx().pixels_per_point() as f64;
    let mut scale = ppp;
    let full = rect.width() as f64 * rect.height() as f64 * ppp * ppp;
    if full > MAX_PIXELS {
        scale *= (MAX_PIXELS / full).sqrt();
    }
    if v.dragging {
        scale *= 0.5;
    }
    let size = [((rect.width() as f64 * scale).round() as usize).max(1), ((rect.height() as f64 * scale).round() as usize).max(1)];
    let supersample = if !v.dragging && (size[0] * size[1]) as f64 <= SUPERSAMPLE_PIXELS { 2 } else { 1 };
    let key = Key { scene: (revision, overlay, field_key), camera: v.camera, size, supersample, dark: crate::theme::is_dark() };
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
    let p = ui.painter().with_clip_rect(rect);
    if let Some((_, pic)) = v.picture.as_mut() {
        let id = pic.texture(ui.ctx());
        p.image(id, rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    }
    // Labels.
    let cam = v.camera;
    let font = FontId::proportional(t.font_small);
    for l in &built.labels {
        if let Some(q) = cam.project(l.at, rect.width() as f64, rect.height() as f64) {
            let at = rect.min + vec2(q[0] as f32, q[1] as f32);
            if rect.contains(at) {
                // A label on a face turned away is faint.
                let b = cam.basis();
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
    }
    legend(&p, rect, &built.legend, &t);
    // The field's plane: its box in the model and the view that faces it.
    let facing = layer.as_ref().map(|(plot, plane, at)| {
        let l = app.session.display.length;
        let (lo, _) = bounds;
        let ((h0, h1), (v0, v1)) = (plot.x.span(), plot.y.span());
        let (h0, h1, v0, v1) = (l.to_si(h0), l.to_si(h1), l.to_si(v0), l.to_si(v1));
        match plane {
            Plane::H => ([v0, lo[1] + at, h0], [v1, lo[1] + at, h1], StandardView::Top),
            Plane::E => ([lo[0] + at, v0, h0], [lo[0] + at, v1, h1], StandardView::Right),
            Plane::Cross => ([h0, v0, lo[2] + at], [h1, v1, lo[2] + at], StandardView::Front),
        }
    });
    navigator(app, ui, rect);
    nav_bar(app, ui, rect, facing);
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
