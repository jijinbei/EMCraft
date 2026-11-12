//! The 3D view's tools in the egui window: what the shared rules find and decide
//! ([`emcraft_ui_core::tools`]: the face and snap under the pointer, where a port goes, what a
//! click runs, the reference plane's drag) drawn over the picture — the face lit, the snap's
//! marker, the points picked, the measurement's dimension and card, the reference plane and its
//! grip — and the reference plane's field under the selection card.

use egui::{FontId, Pos2, Rect, Stroke, pos2, vec2};
use emcraft_model::End;
use emcraft_render::view3d::Camera;
use emcraft_ui_core::tools::{self, Ink, Outcome, Sight, reference};
use serde_json::{Value, json};

use crate::handle::{Fixed, Handle};
use crate::lang::{Raw, tr};
use crate::scene3d::quad_centre;
use crate::theme::Tokens;
use crate::view3d::View3d;
use crate::{Action, Actions, EmcraftApp, Hint, UiExt, scenario};

pub use emcraft_ui_core::tools::reference::rounded;
pub use emcraft_ui_core::tools::{GRIP_PX, Hover, Measured, PortTarget, RefPlane, Snap, SnapKind, Tool, Tools, measure_params, trimmed};

/// The reference plane's field under the selection card.
#[derive(Clone, Debug, PartialEq)]
pub struct RefText {
    port: usize,
    text: String,
    /// The value (display units) it was filled from.
    seen: f64,
}

/// Where `p` lands on screen in `rect`.
pub fn screen(cam: &Camera, rect: Rect, p: [f64; 3]) -> Option<Pos2> {
    tools::screen(cam, crate::geom::vector(rect.size()), p).map(|q| rect.min + vec2(q.x, q.y))
}

/// Finds what the pointer (`at`, in `rect`) is over while a tool is on.
pub fn hover(app: &EmcraftApp, v: &mut View3d, rect: Rect, at: Option<Pos2>) {
    let at = at.filter(|p| rect.contains(*p)).filter(|_| !v.dragging).map(|p| crate::geom::point(p - rect.min.to_vec2()));
    let generation = v.scene_gen();
    let camera = v.camera;
    let mut tools = std::mem::take(&mut v.tools);
    let sight = v.shown().map(|built| Sight { built, generation, camera: &camera, size: crate::geom::vector(rect.size()) });
    tools.hovered(app.session.model(), sight.as_ref(), app.ui_state.tool(), at);
    v.tools = tools;
}

/// While a tool is on, a band over the 3D view says so: what it does and how to leave it.
pub fn banner(app: &EmcraftApp, ui: &egui::Ui, rect: Rect) {
    let Some(tool) = app.ui_state.tool() else { return };
    let t = &crate::theme::Tokens::get();
    let text = format!("{}  ·  {}", tr(tool.hint()), tr(t!("Escape or a click on empty space ends it")));
    let p = ui.painter_at(rect);
    let g = p.layout_no_wrap(text, FontId::proportional(t.font_small), t.on_accent);
    let r = Rect::from_center_size(pos2(rect.center().x, rect.bottom() - 62.0), g.size() + vec2(20.0, 10.0));
    p.rect_filled(r, 10.0, t.accent.gamma_multiply(0.92));
    p.galley(r.min + vec2(10.0, 5.0), g, t.on_accent);
    crate::scenario::publish_target(crate::handle::Fixed::View3dToolBanner, r);
}

/// A click with a tool on: what it asks for.
pub fn click(app: &EmcraftApp, v: &mut View3d, out: &mut Actions) {
    let Some(tool) = app.ui_state.tool() else { return };
    let action = match v.tools.clicked(tool, app.session.model(), app.session.display().length) {
        Some(Outcome::Select(pick)) => Action::Select { row: crate::browser::row_of(app, pick).map(|(k, _)| k), picks: vec![pick] },
        Some(Outcome::Run(id, params)) => Action::run(id, params),
        Some(Outcome::Dialog(id, params)) => Action::dialog_with(id, params),
        Some(Outcome::Refused(why)) => Action::error(tr(why)),
        Some(Outcome::Measure(picks)) => Action::Measure(picks),
        None => return,
    };
    out.push(action);
}

/// The reference plane of the selected port (as dragged, while it is).
pub fn ref_plane(app: &EmcraftApp, v: &View3d) -> Option<RefPlane> {
    reference::ref_plane(app.session.model(), app.ui_state.selection(), v.tools.drag.as_ref())
}

/// The reference plane's grip on screen.
pub fn grip(app: &EmcraftApp, v: &View3d, rect: Rect) -> Option<Pos2> {
    ref_plane(app, v).and_then(|r| screen(&v.camera, rect, r.centre()))
}

/// Starts dragging the reference plane.
pub fn start_drag(app: &EmcraftApp, v: &mut View3d) {
    v.tools.drag = ref_plane(app, v).map(reference::start);
}

/// Moves the dragged reference plane by the pointer's move `d`.
pub fn drag(app: &EmcraftApp, v: &mut View3d, rect: Rect, d: egui::Vec2) {
    let Some(r) = ref_plane(app, v) else { return };
    let size = crate::geom::vector(rect.size());
    v.tools.drag = v.tools.drag.take().map(|drag| reference::dragged(drag, &r, app.session.model(), &v.camera, size, crate::geom::vector(d)));
}

/// The drag let go: the port's de-embedding set where it was left (rounded in display units).
pub fn end_drag(app: &EmcraftApp, v: &mut View3d, out: &mut Actions) {
    let Some(d) = v.tools.drag.take() else { return };
    if let Some(params) = reference::let_go(&d, app.session.model(), app.session.display().length) {
        out.push(Action::run("model.port.edit", params));
    }
}

/// Draws the tools over the picture: the face under the pointer, its snap, the points picked,
/// the measurement and the selected port's reference plane.
pub fn draw(app: &EmcraftApp, v: &View3d, p: &egui::Painter, rect: Rect) {
    let t = &Tokens::get();
    let cam = &v.camera;
    let at = |q: [f64; 3]| screen(cam, rect, q);
    let l = app.session.display().length;
    if let Some(r) = ref_plane(app, v) {
        reference(p, &r, cam, rect, l);
    }
    crate::grips::draw(app, v, p, rect);
    if let (Some(tool), Some(h)) = (app.ui_state.tool(), &v.tools.hover) {
        hovered(app, v, tool, h, p, rect);
    }
    // The points picked, and a line from the last to the pointer.
    for s in &v.tools.picks {
        if let Some(q) = at(s.at) {
            p.circle_filled(q, 4.0, t.accent);
        }
    }
    if let (Some(last), Some(h)) = (v.tools.picks.last(), &v.tools.hover)
        && let (Some(a), Some(b)) = (at(last.at), at(h.snap.at))
    {
        dashed(p, a, b, Stroke::new(1.5, t.accent));
    }
    if let Some(m) = &v.tools.measured {
        dimension(p, cam, rect, &m.picks, &m.reply);
    }
    // The end faces a port may take, named for scenarios and agents.
    if app.ui_state.tool() == Some(Tool::Port) {
        let m = app.session.model();
        if let Some(b) = m.bounds() {
            let (c, lo, hi) = (b.center(), b.min.z.min(0.0), b.max.z);
            for (end, z) in [(End::Start, lo), (End::End, hi)] {
                if let Some(q) = at([c.x, c.y, z]) {
                    scenario::publish_handle(Handle::PortEnd(end), q);
                }
            }
        }
    }
}

/// Most triangles of the face under the pointer lit.
const MAX_LIT: usize = 20_000;

/// The face under the pointer lit, its outline and corners, and the port tool's tip or the snap's
/// marker.
fn hovered(app: &EmcraftApp, v: &View3d, tool: Tool, h: &Hover, p: &egui::Painter, rect: Rect) {
    let t = &Tokens::get();
    let at = |q: [f64; 3]| screen(&v.camera, rect, q);
    let ok = !matches!(h.port, Some(PortTarget::No(_)));
    let ink = if ok { t.accent } else { t.text_dim };
    if let Some(scene) = v.shown()
        && h.face.tris.len() <= MAX_LIT
    {
        let mut mesh = egui::Mesh::default();
        for tri in h.face.tris.iter().filter_map(|i| scene.scene.tris.get(*i)) {
            let (Some(a), Some(b), Some(c)) = (at(tri.p[0]), at(tri.p[1]), at(tri.p[2])) else { continue };
            let base = mesh.vertices.len() as u32;
            for q in [a, b, c] {
                mesh.colored_vertex(q, ink.gamma_multiply(0.25));
            }
            mesh.add_triangle(base, base + 1, base + 2);
        }
        p.add(egui::Shape::mesh(mesh));
    }
    for (a, b) in &h.face.outline {
        if let (Some(a), Some(b)) = (at(*a), at(*b)) {
            p.line_segment([a, b], Stroke::new(2.0, ink));
        }
    }
    // The corners, to aim at.
    for (i, c) in h.face.corners.iter().enumerate().take(64) {
        if let Some(q) = at(*c) {
            p.circle_filled(q, 2.5, ink);
            scenario::publish_handle(Handle::Corner(i), q);
        }
    }
    if tool == Tool::Port {
        if let Some(q) = at(h.face.centre) {
            tip(p, q, &tools::port_text(app.session.model(), h.port), ok);
        }
    } else if let Some(q) = at(h.snap.at) {
        marker(p, q, h.snap.kind);
        scenario::publish_handle(Fixed::View3dSnap, q);
    }
}

/// A short text on a pill at `at`.
fn tip(p: &egui::Painter, at: Pos2, text: &str, ok: bool) {
    let t = &Tokens::get();
    if text.is_empty() {
        return;
    }
    let g = p.layout_no_wrap(text.to_owned(), FontId::proportional(t.font_small), if ok { t.on_accent } else { t.text });
    let r = Rect::from_center_size(at, g.size() + vec2(12.0, 6.0));
    p.rect_filled(r, 8.0, if ok { t.accent } else { t.panel.gamma_multiply(0.92) });
    p.galley(r.min + vec2(6.0, 3.0), g, t.text);
}

/// A snap's marker: a square on a corner, a triangle on an edge's middle, a ringed dot on a
/// centre, a diamond on an edge, a cross on a face.
fn marker(p: &egui::Painter, q: Pos2, kind: SnapKind) {
    let t = &Tokens::get();
    let s = Stroke::new(1.6, t.accent);
    let r = 5.5;
    match kind {
        SnapKind::Vertex => {
            p.rect_stroke(Rect::from_center_size(q, vec2(2.0 * r, 2.0 * r)), 0.0, s, egui::StrokeKind::Middle);
        }
        SnapKind::Midpoint => {
            let pts = vec![q + vec2(0.0, -r), q + vec2(r, r * 0.8), q + vec2(-r, r * 0.8)];
            p.add(egui::Shape::closed_line(pts, s));
        }
        SnapKind::Centre => {
            p.circle_stroke(q, r, s);
            p.circle_filled(q, 1.8, t.accent);
        }
        SnapKind::Edge => {
            let pts = vec![q + vec2(0.0, -r), q + vec2(r, 0.0), q + vec2(0.0, r), q + vec2(-r, 0.0)];
            p.add(egui::Shape::closed_line(pts, s));
        }
        SnapKind::Face => {
            p.line_segment([q + vec2(-r, 0.0), q + vec2(r, 0.0)], s);
            p.line_segment([q + vec2(0.0, -r), q + vec2(0.0, r)], s);
        }
    }
}

/// A dashed line from `a` to `b`.
fn dashed(p: &egui::Painter, a: Pos2, b: Pos2, s: Stroke) {
    p.extend(egui::Shape::dashed_line(&[a, b], s, 6.0, 4.0));
}

/// The measurement: the line between the picks with arrow ends, its legs along x, y and z in the
/// axes' colours, the distance on it and a card with the components, the gap and the angle.
fn dimension(p: &egui::Painter, cam: &Camera, rect: Rect, [a, b]: &[Snap; 2], reply: &Value) {
    let t = &Tokens::get();
    let at = |q: [f64; 3]| screen(cam, rect, q);
    // The legs: x, then y, then z.
    let mut q = a.at;
    for (k, ink) in [t.axis_x, t.axis_y, t.axis_z].into_iter().enumerate() {
        let mut next = q;
        next[k] = b.at[k];
        if let (Some(s), Some(e)) = (at(q), at(next)) {
            dashed(p, s, e, Stroke::new(1.2, ink));
        }
        q = next;
    }
    let (Some(sa), Some(sb)) = (at(a.at), at(b.at)) else { return };
    let line = Stroke::new(2.0, t.accent);
    p.line_segment([sa, sb], line);
    let d = sb - sa;
    if d.length() > 1.0 {
        let u = d.normalized();
        let n = vec2(-u.y, u.x);
        for (tipp, dir) in [(sa, u), (sb, -u)] {
            p.add(egui::Shape::convex_polygon(vec![tipp, tipp + dir * 9.0 + n * 3.5, tipp + dir * 9.0 - n * 3.5], t.accent, Stroke::NONE));
        }
    }
    for s in [sa, sb] {
        p.circle_filled(s, 3.0, t.accent);
    }
    let distance = tools::measure_distance(reply);
    tip(p, sa + d * 0.5 + vec2(0.0, -14.0), &distance, true);
    // The card: the distance, its components, the gap and the angle.
    let axes = [t.axis_x, t.axis_y, t.axis_z];
    let lines = tools::measure_card(reply).into_iter().map(|(s, ink)| match ink {
        Ink::Axis(k) => (s, axes.get(k).copied().unwrap_or(t.text)),
        Ink::Text => (s, t.text),
    });
    let font = FontId::proportional(t.font_small);
    let galleys: Vec<_> = lines.map(|(s, c)| p.layout_no_wrap(s, font.clone(), c)).collect();
    let w = galleys.iter().map(|g| g.size().x).fold(0.0, f32::max) + 20.0;
    let h: f32 = galleys.iter().map(|g| g.size().y + 2.0).sum::<f32>() + 12.0;
    let card = Rect::from_min_size(pos2(rect.right() - w - 12.0, rect.top() + 12.0), vec2(w, h));
    p.rect_filled(card, 6.0, t.panel.gamma_multiply(0.94));
    p.rect_stroke(card, 6.0, Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
    let mut y = card.top() + 6.0;
    for g in galleys {
        let gh = g.size().y;
        p.galley(pos2(card.left() + 10.0, y), g, t.text);
        y += gh + 2.0;
    }
    scenario::publish_target(Fixed::View3dMeasure, card);
}

/// The reference plane: its outline dashed inside the model, a line from the port's face to it
/// and its grip with the distance.
fn reference(p: &egui::Painter, r: &RefPlane, cam: &Camera, rect: Rect, l: emcraft_model::units::Length) {
    let t = &Tokens::get();
    let at = |q: [f64; 3]| screen(cam, rect, q);
    let q = r.at();
    let pts: Vec<Pos2> = q.iter().filter_map(|c| at(*c)).collect();
    if pts.len() == 4 {
        p.add(egui::Shape::convex_polygon(pts.clone(), t.accent.gamma_multiply(0.18), Stroke::NONE));
        for k in 0..4 {
            dashed(p, pts[k], pts[(k + 1) % 4], Stroke::new(1.6, t.accent));
        }
    }
    let (Some(a), Some(g)) = (at(quad_centre(&r.face)), at(r.centre())) else { return };
    p.line_segment([a, g], Stroke::new(1.2, t.accent));
    p.circle_filled(g, 6.0, t.panel);
    p.circle_stroke(g, 6.0, Stroke::new(2.0, t.accent));
    p.circle_filled(g, 2.5, t.accent);
    scenario::publish_handle(Fixed::View3dRefPlane, g);
    let text = r.label(l);
    let gal = p.layout_no_wrap(text, FontId::proportional(t.font_small), t.text);
    let box_ = Rect::from_min_size(g + vec2(10.0, -gal.size().y - 8.0), gal.size() + vec2(10.0, 4.0));
    p.rect_filled(box_, 4.0, t.panel.gamma_multiply(0.9));
    p.galley(box_.min + vec2(5.0, 2.0), gal, t.text);
}

/// The selected waveguide port's reference plane as a number under the selection card: Enter
/// sets it, 0 puts it back on the port's face (`model.port.edit {deembed}`).
pub fn ref_field(app: &EmcraftApp, v: &mut View3d, ui: &mut egui::Ui, at: Pos2, out: &mut Actions) {
    let Some(r) = ref_plane(app, v) else {
        v.ref_text = None;
        return;
    };
    let t = &Tokens::get();
    let l = app.session.display().length;
    let now = rounded(l, r.deembed);
    // The text follows the port (and a drag) until it is edited.
    if v.ref_text.as_ref().is_none_or(|f| f.port != r.port || f.seen != now) {
        v.ref_text = Some(RefText { port: r.port, text: trimmed(now, 3), seen: now });
    }
    let Some(RefText { text, .. }) = v.ref_text.as_mut() else { return };
    let busy = app.busy();
    let area = Rect::from_center_size(at + vec2(0.0, 14.0), vec2(220.0, 26.0));
    ui.painter().rect_filled(area, 6.0, t.panel.gamma_multiply(0.92));
    let mut child =
        ui.new_child(egui::UiBuilder::new().max_rect(area.shrink2(vec2(8.0, 2.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
    crate::ui_kit::small(&mut child, t!("Ref. plane"), t.text_dim);
    let edit = egui::TextEdit::singleline(text).desired_width(64.0).char_limit(24);
    let resp = child.add_enabled_ui(!busy, |ui| ui.named(edit, Fixed::View3dRefField.into())).inner;
    crate::ui_kit::small(&mut child, Raw(l.symbol()), t.text_dim);
    let enter = resp.lost_focus() && child.input(|i| i.key_pressed(egui::Key::Enter));
    let set = |value: f64, out: &mut Actions| out.push(Action::run("model.port.edit", json!({"name": r.name, "deembed": value})));
    if enter {
        match text.trim().parse::<f64>() {
            Ok(x) if x.is_finite() && x != now => set(x, out),
            _ => *text = trimmed(now, 3),
        }
    }
    let zero = child.add_enabled_ui(!busy && now != 0.0, |ui| ui.named(egui::Button::new("0").small(), Fixed::View3dRefZero.into())).inner;
    if zero.hint(t!("Back to the port's face")).clicked() {
        set(0.0, out);
    }
}
