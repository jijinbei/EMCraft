//! The field view's side column, beside the picture it is about: where the plane cuts the
//! model (a small drawing that a drag moves the plane in), where the frequency sits on the
//! S-parameters, the fields at the pointer and the pinned point, and the picture's and the
//! period's export.

use emcraft_model::{Aabb, Point3};

use crate::lang::args;
use crate::scene3d::{BOX_EDGES, box_corner, normal_axis};
use crate::ui_kit;

use super::field_params::FieldParams;

use super::*;

/// The side column's width.
pub(super) const SIDE: f32 = 230.0;

/// A small heading.
fn heading(ui: &mut egui::Ui, text: &str) {
    let t = &Tokens::get();
    ui.label(egui::RichText::new(tr(text)).size(t.font_small).color(t.text_dim).strong());
}

pub(super) fn side(app: &EmcraftApp, field: &mut FieldState, ui: &mut egui::Ui, out: &mut Actions) {
    let t = &Tokens::get();
    if !field.beside_3d {
        heading(ui, "WHERE");
        locator(app, field, ui);
        ui.add_space(t.space_8);
    }
    if field.view.mode.is_none() && app.session.results().sweep.is_some() {
        heading(ui, "FREQUENCY");
        spark(app, field, ui);
        power_readout(app, field, ui);
        ui.add_space(t.space_8);
    }
    probes(app, field, ui, out);
    ui.add_space(t.space_8);
    if field.beside_3d {
        ui.text(Style::SmallDim, "Move the field plane in the model. Use 2D plot for point probes.");
    } else {
        readout(app, field, ui);
    }
    ui.add_space(t.space_8);
    heading(ui, "SAVE");
    ui.horizontal(|ui| {
        let s = &app.session;
        let picture = || Action::dialog_with("view.field", FieldParams::picture(field, s, "field.png").to_params());
        ui.action_button(Fixed::FieldSavePicture.into(), "Picture…", "This picture as a PNG (view.field)", picture, out);
        let video = || Action::dialog_with("view.animation", FieldParams::animation(field, s, "field-animation.png").to_params());
        ui.action_button(Fixed::FieldSaveVideo.into(), "Animation…", "A period of this view as a looping animated PNG (view.animation)", video, out);
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
    let t = &Tokens::get();
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
    painter.text(rect.right_bottom() - vec2(8.0, 6.0), Align2::RIGHT_BOTTOM, tr("drag to move"), FontId::proportional(t.font_small), t.text_dim);
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

/// Every destination of the selected excitation, with the current frequency marked.
pub(super) fn spark(app: &EmcraftApp, field: &mut FieldState, ui: &mut egui::Ui) {
    let t = &Tokens::get();
    let Some(sw) = app.session.results().sweep.as_ref() else { return };
    let (Some(first), Some(last)) = (sw.results.first(), sw.results.last()) else { return };
    let (f0, f1) = (first.frequency, last.frequency);
    let input = field.view.excitation;
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width().min(SIDE - 8.0), 64.0), Sense::click());
    scenario::publish_target(Fixed::FieldSpark, rect);
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, t.radius_sm, t.panel);
    let plot = rect.shrink2(vec2(8.0, 8.0));
    let fx = |f: f64| if f1 > f0 { plot.left() + ((f - f0) / (f1 - f0)) as f32 * plot.width() } else { plot.center().x };
    let sy = |m: f64| plot.bottom() - (m.clamp(0.0, 1.0) as f32) * plot.height();
    let palette = crate::theme::palette();
    for row in 0..first.s.ports() {
        let colour = crate::theme::color32(palette.pen(view::sparameter_pen(row)));
        let line: Vec<Pos2> = sw.results.iter().filter_map(|r| r.s.get(row, input).map(|v| pos2(fx(r.frequency), sy(v.norm())))).collect();
        if line.len() > 1 {
            painter.add(egui::Shape::line(line.clone(), Stroke::new(1.4, colour)));
        }
        for q in line {
            painter.circle_filled(q, 1.8, colour);
        }
    }
    let x = fx(field.view.frequency);
    painter.line_segment([pos2(x, rect.top() + 2.0), pos2(x, rect.bottom() - 2.0)], Stroke::new(1.6, t.accent));
    let fu = app.session.display().frequency;
    painter.text(
        pos2((x + 4.0).min(rect.right() - 70.0), rect.top() + 3.0),
        Align2::LEFT_TOP,
        format!("{:.4} {}", fu.from_si(field.view.frequency), fu.symbol()),
        FontId::proportional(t.font_small),
        t.text,
    );
    if resp.clicked()
        && let Some(pos) = resp.interact_pointer_pos()
        && plot.width() > 0.0
    {
        let target = f0 + ((pos.x - plot.left()) / plot.width()).clamp(0.0, 1.0) as f64 * (f1 - f0);
        if let Some(f) = crate::nearest(&app.frequencies(), target) {
            field.view.frequency = f;
        }
    }
    resp.hint("Click to show the nearest solved frequency");
    ui.horizontal_wrapped(|ui| {
        for row in 0..first.s.ports() {
            let colour = crate::theme::color32(palette.pen(view::sparameter_pen(row)));
            ui.label(egui::RichText::new(format!("|S{}{}|", row + 1, input + 1)).size(t.font_small).color(colour));
        }
    });
}

fn power_readout(app: &EmcraftApp, field: &mut FieldState, ui: &mut egui::Ui) {
    let key = (app.revision, field.view.frequency.to_bits(), field.view.excitation);
    if field.power.as_ref().is_none_or(|p| p.key != key) {
        field.power = Some(crate::state::PowerReadout {
            key,
            value: emcraft_engine::power::at(&app.session, field.view.frequency, field.view.excitation).map_err(|e| e.to_string()),
        });
    }
    heading(ui, "POWER DISTRIBUTION");
    let Some(readout) = field.power.as_ref() else { return };
    let report = match &readout.value {
        Ok(v) => v,
        Err(e) => {
            ui_kit::caption(ui, Raw(e));
            return;
        }
    };
    egui::Grid::new("field:power").num_columns(3).spacing(vec2(8.0, 3.0)).show(ui, |ui| {
        for name in ["Port", "Incident power", "Output share"] {
            ui.small(tr(name));
        }
        ui.end_row();
        for output in report["outputs"].as_array().into_iter().flatten() {
            let label = format!("{} · {}", output["port"].as_str().unwrap_or(""), output["s"].as_str().unwrap_or(""));
            ui.small(label);
            ui.small(format!("{:.1}%", 100.0 * output["incident_fraction"].as_f64().unwrap_or(0.0)));
            ui.small(output["branch_fraction"].as_f64().map_or_else(|| "—".into(), |v| format!("{:.1}%", 100.0 * v)));
            ui.end_row();
        }
    });
    ui.small(trf("Unaccounted: {value}%", args!(value = format!("{:.2}", 100.0 * report["unaccounted"].as_f64().unwrap_or(0.0)))));
    ui_kit::caption(ui, "Power in reported S entries. The balance includes loss, radiation, higher modes and numerical error.");
}

/// The project's probes (`view.probe.*`): + adds one at the pinned point (the plane's middle
/// without one); each row its name and the coloured field's magnitude there (its point on
/// hover), ✕ removes it.
fn probes(app: &EmcraftApp, field: &mut FieldState, ui: &mut egui::Ui, out: &mut Actions) {
    let t = &Tokens::get();
    let probes = &app.session.views().probes;
    let unit = app.session.display().length;
    ui.horizontal(|ui| {
        heading(ui, "PROBES");
        let at = field.samples.as_ref().and_then(|s| {
            let q = field.pinned.or_else(|| {
                let (x, y) = s.axes();
                Some([0.5 * (x.min + x.max), 0.5 * (y.min + y.max)])
            })?;
            Some(s.model_point(q, unit).to_array().map(|v| unit.from_si(v)))
        });
        let hint = "A probe at the pinned point (the plane's middle without one)";
        let busy = app.busy();
        if let Some(at) = at {
            ui.act(
                !busy,
                egui::Button::new("+").small(),
                Fixed::ProbeAdd.into(),
                hint,
                || Action::run("view.probe.add", serde_json::json!({"at": at})),
                out,
            );
        }
    });
    if probes.is_empty() {
        return;
    }
    // Their fields for this view's solution, read again when it or they change.
    let key = crate::state::ProbeKey {
        revision: app.revision,
        frequency: field.view.frequency,
        mode: field.view.mode,
        excitation: field.view.excitation,
        points: probes.iter().map(|p| p.at).collect(),
    };
    if field.probe_values.as_ref().is_none_or(|(k, _)| *k != key) {
        let points: Vec<Point3> = probes.iter().map(|p| p.point()).collect();
        let values = view::fields_at(&app.session, &field.view, &points).map(|(_, v)| v).unwrap_or_default();
        field.probe_values = Some((key, values));
    }
    let magnetic = field.view.component.is_magnetic();
    let (name, unit_name) = if magnetic { ("|H|", "A/m") } else { ("|E|", "V/m") };
    let norm = |v: &emcraft_model::CVec3| v.iter().map(|c| c.norm_sqr()).sum::<f64>().sqrt();
    let values = field.probe_values.as_ref().map(|(_, v)| v.as_slice()).unwrap_or(&[]);
    for (i, p) in probes.iter().enumerate() {
        let value = match values.get(i).copied().flatten() {
            Some((e, h)) => format!("{name} {} {unit_name}", emcraft_render::axes::format(norm(if magnetic { &h } else { &e }))),
            None => "–".to_owned(),
        };
        ui.horizontal(|ui| {
            let at = p.at.map(|v| emcraft_render::axes::format(unit.from_si(v))).join(", ");
            let row = egui::Label::new(egui::RichText::new(format!("◆ {}  {value}", p.name)).size(t.font_small).color(t.text)).sense(Sense::hover());
            let r = ui.add(row).on_hover_text(format!("({at}) {}", unit.symbol()));
            scenario::publish_target(Handle::Probe(i), r.rect);
            let del = || Action::run("view.probe.delete", serde_json::json!({"name": p.name}));
            let (x, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
            let r = ui.hit(x, Handle::ProbeDelete(i), Sense::click());
            crate::ui_kit::close_mark(ui, x, r.hovered());
            if r.hint("Remove this probe").clicked() && !app.busy() {
                out.push(del());
            }
        });
    }
}

/// The fields at the pointer and at the pinned point, one quantity a line.
fn readout(app: &EmcraftApp, field: &FieldState, ui: &mut egui::Ui) {
    let t = &Tokens::get();
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
                line(
                    ui,
                    trf(
                        "{component} = {now} {unit} (amplitude {amplitude} {unit})",
                        args!(component = comp.label(), now = fmt(now), unit = unit, amplitude = fmt(amp)),
                    ),
                    true,
                );
                if what == "PINNED" {
                    // Each component a row with its unit.
                    for (f, v, u) in [("E", p.e, "V/m"), ("H", p.h, "A/m")] {
                        for (axis, c) in [("x", v.x), ("y", v.y), ("z", v.z)] {
                            line(ui, format!("{f}{axis} {}", phasor(c, u)), false);
                        }
                    }
                }
            }
        }
        ui.add_space(t.space_4);
    }
}
