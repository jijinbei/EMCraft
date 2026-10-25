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

use crate::scene3d::{self, Built, Overlay};
use crate::theme::Tokens;
use crate::{EmcraftApp, Picture, scenario};

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
    gizmo(&p, rect, &cam, &t);
    toolbar(app, ui, rect);
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

/// The axes' directions in the corner: x red, y green, z blue.
fn gizmo(p: &egui::Painter, rect: Rect, cam: &Camera, t: &Tokens) {
    let c = pos2(rect.left() + 46.0, rect.bottom() - 46.0);
    p.circle_filled(c, 36.0, t.panel.gamma_multiply(0.7));
    let axes = view3d::axes_on_screen(cam);
    let colors = [Color32::from_rgb(214, 60, 60), Color32::from_rgb(60, 160, 70), Color32::from_rgb(60, 110, 220)];
    // Farthest first, so the one towards the eye is on top.
    let b = cam.basis();
    let mut order = [0usize, 1, 2];
    order.sort_by(|&i, &j| b.forward[j].total_cmp(&b.forward[i]));
    for k in order {
        let e = c + vec2(axes[k][0] as f32, axes[k][1] as f32) * 26.0;
        p.line_segment([c, e], Stroke::new(2.0, colors[k]));
        p.text(c + (e - c) * 1.25, Align2::CENTER_CENTER, ["X", "Y", "Z"][k], FontId::proportional(t.font_small), colors[k]);
    }
}

/// Fit, the standard views and the projection, at the top right.
fn toolbar(app: &mut EmcraftApp, ui: &mut egui::Ui, rect: Rect) {
    let t = Tokens::get();
    let ortho = app.view3d.camera.projection == Projection::Orthographic;
    let mut items: Vec<(String, String)> = vec![("Fit".into(), "view3d:fit".into())];
    for v in StandardView::ALL {
        let name = v.name();
        let mut label = name.to_owned();
        if let Some(f) = label.get_mut(0..1) {
            f.make_ascii_uppercase();
        }
        items.push((label, format!("view3d:std:{name}")));
    }
    items.push((if ortho { "Orthographic" } else { "Perspective" }.into(), "view3d:projection".into()));
    let font = FontId::proportional(t.font_small);
    let widths: Vec<f32> = items.iter().map(|(s, _)| ui.painter().layout_no_wrap(s.clone(), font.clone(), t.text).size().x + 14.0).collect();
    let total: f32 = widths.iter().sum::<f32>() + 2.0 * items.len() as f32;
    let mut x = rect.right() - total - 10.0;
    let y = rect.top() + 10.0;
    let bar = Rect::from_min_size(pos2(x - 4.0, y - 3.0), vec2(total + 6.0, 26.0));
    ui.painter().rect_filled(bar, 6.0, t.panel.gamma_multiply(0.9));
    ui.painter().rect_stroke(bar, 6.0, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    for ((label, handle), w) in items.iter().zip(widths) {
        let r = Rect::from_min_size(pos2(x, y), vec2(w, 20.0));
        let resp = ui.interact(r, ui.id().with(handle.as_str()), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
        if resp.hovered() {
            ui.painter().rect_filled(r, 4.0, t.hover);
        }
        ui.painter().text(r.center(), Align2::CENTER_CENTER, label, font.clone(), t.text);
        scenario::publish_target(handle, r);
        if resp.clicked() {
            match handle.strip_prefix("view3d:std:").and_then(StandardView::parse) {
                Some(v) => app.view3d.standard(v),
                None if handle == "view3d:fit" => app.view3d.fit_next = true,
                None => app.view3d.toggle_projection(),
            }
        }
        x += w + 2.0;
    }
}
