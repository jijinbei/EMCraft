//! The field view's side column, beside the picture it is about: where the plane cuts the
//! model (a small drawing that a drag moves the plane in), where the frequency sits on the
//! S-parameters, the fields at the pointer and the pinned point, and the picture's and the
//! period's export.

use emcraft_model::{Aabb, Point3};

use crate::scene3d::{BOX_EDGES, box_corner, normal_axis};

use super::field_params::FieldParams;

use super::*;

/// The side column's width.
pub(super) const SIDE: f32 = 230.0;

/// A small heading.
fn heading(ui: &mut egui::Ui, text: &str) {
    let t = Tokens::get();
    ui.label(egui::RichText::new(text).size(t.font_small).color(t.text_dim).strong());
}

pub(super) fn side(app: &EmcraftApp, field: &mut FieldState, ui: &mut egui::Ui, out: &mut Actions) {
    let t = Tokens::get();
    heading(ui, "WHERE");
    locator(app, field, ui);
    ui.add_space(t.space_8);
    if field.view.mode.is_none() && app.session.results().sweep.is_some() {
        heading(ui, "FREQUENCY");
        spark(app, field, ui);
        ui.add_space(t.space_8);
    }
    readout(app, field, ui);
    ui.add_space(t.space_8);
    heading(ui, "SAVE");
    ui.horizontal(|ui| {
        let s = &app.session;
        let picture = || Action::dialog_with("view.field", FieldParams::picture(field, s, "field.png").to_params());
        ui.action_button(Fixed::FieldSavePicture.into(), "Picture…", "This picture as a PNG (view.field)", picture, out);
        let video = || Action::dialog_with("view.animation", FieldParams::animation(field, s, "field-animation.png").to_params());
        ui.action_button(Fixed::FieldSaveVideo.into(), "Video…", "A period of this view as a looping animated PNG (view.animation)", video, out);
    });
}

/// What fills the model along the plane's normal through its middle: runs of (from, to) as
/// fractions of the span, with a material's colour, or `None` where there is no material
/// (conductor, outside).
pub(super) fn bands(app: &EmcraftApp, plane: Plane) -> Vec<(f64, f64, Option<Color32>)> {
    let m = app.session.model();
    let Some(Aabb { min: lo, max: hi }) = m.bounds() else { return Vec::new() };
    let colour = |i: usize| {
        let c = crate::scene3d::material_rgb(m, i);
        Color32::from_rgb(c.0, c.1, c.2)
    };
    let axis = normal_axis(plane);
    // A waveguide's sections lie along z; across it, its first section's material.
    if let Some(w) = &m.waveguide {
        let index = |name: &str| m.material(name).ok().map(|(i, _)| colour(i));
        if axis != 2 {
            return vec![(0.0, 1.0, w.sections.first().and_then(|s| index(&s.material)))];
        }
        let total = w.length().max(1e-12);
        let mut at = 0.0;
        return w
            .sections
            .iter()
            .map(|s| {
                let run = (at / total, (at + s.length) / total, index(&s.material));
                at += s.length;
                run
            })
            .collect();
    }
    // A solid: sampled along the line through the middle.
    const SAMPLES: usize = 160;
    let mid = Point3::new(0.5 * (lo.x + hi.x), 0.5 * (lo.y + hi.y), 0.5 * (lo.z + hi.z));
    let mut runs: Vec<(f64, f64, Option<Color32>)> = Vec::new();
    for k in 0..SAMPLES {
        let f = (k as f64 + 0.5) / SAMPLES as f64;
        let p = mid.with(axis, lo[axis] + f * (hi[axis] - lo[axis]));
        let c = m.material_at(p).map(colour);
        let (a, b) = (k as f64 / SAMPLES as f64, (k + 1) as f64 / SAMPLES as f64);
        match runs.last_mut() {
            Some(last) if last.2 == c => last.1 = b,
            _ => runs.push((a, b, c)),
        }
    }
    runs
}

/// The model's box drawn obliquely (z to the right, y up, x receding), its sections along z,
/// and the plane where it cuts, filled. A drag moves the plane along its normal; a click puts
/// it there.
fn locator(app: &EmcraftApp, field: &mut FieldState, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let Some(Aabb { min: lo, max: hi }) = app.session.model().bounds() else { return };
    let Some((span, axis_name)) = plane_span_of(app, field.view.plane) else { return };
    let size = [hi.x - lo.x, hi.y - lo.y, hi.z - lo.z].map(|s| s.max(1e-12));
    let (rect, resp) = ui.allocate_exact_size(vec2(SIDE - 8.0, 120.0), Sense::click_and_drag());
    scenario::publish_target(Fixed::FieldLocator, rect);
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, t.radius_sm, t.panel);
    // Oblique projection: z → right, y → up, x → up-right at a third.
    let (dx, dy) = (0.45f64, 0.35f64);
    let raw = |p: [f64; 3]| [p[2] + dx * p[0], p[1] + dy * p[0]];
    let w = size[2] + dx * size[0];
    let h = size[1] + dy * size[0];
    let room = rect.shrink2(vec2(14.0, 22.0));
    let scale = (room.width() as f64 / w).min(room.height() as f64 / h);
    let origin = pos2(room.center().x - (w * scale) as f32 / 2.0, room.center().y + (h * scale) as f32 / 2.0);
    let to = |p: [f64; 3]| {
        let [u, v] = raw(p);
        pos2(origin.x + (u * scale) as f32, origin.y - (v * scale) as f32)
    };
    let [sx, sy, sz] = size;
    let corner = |i: usize| box_corner([0.0; 3], size, i);
    let edge = Stroke::new(1.0, t.text_dim);
    for (a, b) in BOX_EDGES {
        painter.line_segment([to(corner(a)), to(corner(b))], edge);
    }
    // The sections (or materials) along z as faint frames at their boundaries.
    if normal_axis(field.view.plane) == 2 {
        for (from, _, _) in bands(app, Plane::Cross).iter().skip(1) {
            let z = from * sz;
            let ring = [[0.0, 0.0, z], [sx, 0.0, z], [sx, sy, z], [0.0, sy, z]].map(to);
            painter.add(egui::Shape::closed_line(ring.to_vec(), Stroke::new(1.0, t.border)));
        }
    }
    // The plane.
    let axis = normal_axis(field.view.plane);
    let at = field.at.unwrap_or(span / 2.0).clamp(0.0, span);
    let frac = if span > 0.0 { at / span } else { 0.5 };
    let quad: Vec<Pos2> = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]
        .iter()
        .map(|[a, b]| {
            let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
            let mut p = [0.0; 3];
            p[axis] = frac * size[axis];
            p[u] = a * size[u];
            p[v] = b * size[v];
            to(p)
        })
        .collect();
    painter.add(egui::Shape::convex_polygon(quad, t.accent.gamma_multiply(0.35), Stroke::new(1.6, t.accent)));
    let unit = app.session.display().length.symbol();
    painter.text(
        rect.left_top() + vec2(8.0, 6.0),
        Align2::LEFT_TOP,
        format!("{axis_name} = {} {unit}", emcraft_render::axes::format(at)),
        FontId::proportional(t.font_small),
        t.text,
    );
    painter.text(rect.right_bottom() - vec2(8.0, 6.0), Align2::RIGHT_BOTTOM, "drag to move", FontId::proportional(t.font_small), t.text_dim);
    // Dragging along the normal's direction on the drawing moves the plane.
    let mut unit_axis = [0.0; 3];
    unit_axis[axis] = size[axis];
    let along = to(unit_axis) - to([0.0; 3]);
    let len2 = along.length_sq();
    let mut moved = None;
    if resp.dragged() && len2 > 1.0 {
        moved = Some(at + (resp.drag_delta().dot(along) / len2) as f64 * span);
    } else if resp.clicked()
        && let Some(pos) = resp.interact_pointer_pos()
        && len2 > 1.0
    {
        // The click's place along the normal, measured from the box's middle line.
        let mut mid = [0.5 * sx, 0.5 * sy, 0.5 * sz];
        mid[axis] = 0.0;
        moved = Some(((pos - to(mid)).dot(along) / len2) as f64 * span);
    }
    if let Some(v) = moved {
        field.at = Some(v.clamp(0.0, span));
        field.pinned = None;
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
}

/// |S11| and |S21| over the sweep, small, with the view's frequency marked; a click picks the
/// nearest solved frequency.
fn spark(app: &EmcraftApp, field: &mut FieldState, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let Some(sw) = app.session.results().sweep.as_ref() else { return };
    let points: Vec<(f64, f64, f64)> = sw
        .results
        .iter()
        .map(|r| {
            let s = |i: usize| r.s.get(i, 0).map_or(0.0, |c| c.norm());
            (r.frequency, s(0), s(1))
        })
        .collect();
    let (Some(f0), Some(f1)) = (points.first().map(|p| p.0), points.last().map(|p| p.0)) else { return };
    let (rect, resp) = ui.allocate_exact_size(vec2(SIDE - 8.0, 64.0), Sense::click());
    scenario::publish_target(Fixed::FieldSpark, rect);
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, t.radius_sm, t.panel);
    let plot = rect.shrink2(vec2(8.0, 8.0));
    let fx = |f: f64| if f1 > f0 { plot.left() + ((f - f0) / (f1 - f0)) as f32 * plot.width() } else { plot.center().x };
    let sy = |m: f64| plot.bottom() - (m.clamp(0.0, 1.0) as f32) * plot.height();
    let s11 = Color32::from_rgb(200, 60, 50);
    let s21 = Color32::from_rgb(40, 90, 200);
    for (pick, colour) in [(1usize, s11), (2, s21)] {
        let line: Vec<Pos2> = points.iter().map(|p| pos2(fx(p.0), sy(if pick == 1 { p.1 } else { p.2 }))).collect();
        if line.len() > 1 {
            painter.add(egui::Shape::line(line.clone(), Stroke::new(1.4, colour)));
        }
        for q in line {
            painter.circle_filled(q, 1.8, colour);
        }
    }
    let f = field.view.frequency;
    let x = fx(f);
    painter.line_segment([pos2(x, rect.top() + 2.0), pos2(x, rect.bottom() - 2.0)], Stroke::new(1.6, t.accent));
    let fu = app.session.display().frequency;
    painter.text(
        pos2((x + 4.0).min(rect.right() - 70.0), rect.top() + 3.0),
        Align2::LEFT_TOP,
        format!("{:.4} {}", fu.from_si(f), fu.symbol()),
        FontId::proportional(t.font_small),
        t.text,
    );
    painter.text(rect.left_bottom() + vec2(4.0, -2.0), Align2::LEFT_BOTTOM, "|S11|", FontId::proportional(t.font_small * 0.9), s11);
    painter.text(rect.left_bottom() + vec2(40.0, -2.0), Align2::LEFT_BOTTOM, "|S21|", FontId::proportional(t.font_small * 0.9), s21);
    if resp.clicked()
        && let Some(pos) = resp.interact_pointer_pos()
        && plot.width() > 0.0
    {
        let target = f0 + ((pos.x - plot.left()) / plot.width()).clamp(0.0, 1.0) as f64 * (f1 - f0);
        if let Some(f) = crate::nearest(&app.frequencies(), target) {
            field.view.frequency = f;
        }
    }
    resp.on_hover_text("Click to show the nearest solved frequency");
}

/// The fields at the pointer and at the pinned point, one quantity a line.
fn readout(app: &EmcraftApp, field: &FieldState, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let Some(samples) = field.samples.as_ref() else { return };
    let (xa, ya) = samples.axes();
    let name = |a: &emcraft_render::Axis| a.label.split(' ').next().unwrap_or("").to_owned();
    let (hx, vy) = (name(xa), name(ya));
    let lu = app.session.display().length.symbol();
    let comp = field.view.component;
    let phase = field.view.phase;
    let unit = if comp.is_magnetic() { "A/m" } else { "V/m" };
    let fmt = emcraft_render::axes::format;
    let line = |ui: &mut egui::Ui, s: String, strong: bool| {
        let r = egui::RichText::new(s).size(t.font_small).color(t.text);
        ui.label(if strong { r.strong() } else { r });
    };
    for (what, probe, hint) in [
        ("POINTER", field.hover, "hover the picture to read the fields"),
        ("PINNED", field.pinned.and_then(|q| samples.probe(q)), "click the picture to pin a point (right click lets it go)"),
    ] {
        heading(ui, what);
        match probe {
            None => {
                crate::ui_kit::small(ui, hint, t.text_dim);
            }
            Some(p) => {
                let (now, amp) = p.value(comp, phase);
                line(ui, format!("{hx} {} {lu}, {vy} {} {lu}", fmt(p.at[0]), fmt(p.at[1])), false);
                line(ui, format!("{} = {} {unit} (amplitude {})", comp.label(), fmt(now), fmt(amp)), true);
                if what == "PINNED" {
                    for (f, v, u) in [("E", p.e, "V/m"), ("H", p.h, "A/m")] {
                        line(ui, format!("{f}x {}", phasor(v.x)), false);
                        line(ui, format!("{f}y {}", phasor(v.y)), false);
                        line(ui, format!("{f}z {}  {u}", phasor(v.z)), false);
                    }
                }
            }
        }
        ui.add_space(t.space_4);
    }
}
