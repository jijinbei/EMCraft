//! A selected body's grips in the 3D view (docs/adr/0005-geometry-ux.md): a box's six faces, a
//! cylinder's radius and length, a sphere's radius. Dragging one moves it along its direction on
//! screen, its value snapping to a grid of the display unit and shown beside it; letting go runs
//! one `geometry.body.edit` with the new dimensions (and, for a box's near face, its new origin),
//! so undo, the history and scripts see the edit the drag made.

use egui::{FontId, Pos2, Rect, Stroke, vec2};
use emcraft_model::{Axis, Point3, Shape, Vec3};
use serde_json::{Value, json};

use crate::handle::Handle;
use crate::scene3d::Pick;
use crate::theme::Tokens;
use crate::tools3d::{screen, trimmed};
use crate::view3d::View3d;
use crate::{Action, Actions, EmcraftApp, scenario};

/// What a grip changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grip {
    /// A box's face across its own `axis` (0, 1, 2): the far one (at its size) or the near one (at
    /// its origin); dragging it out grows the box that way.
    Side {
        axis: usize,
        far: bool,
    },
    Radius,
    Length,
}

impl Grip {
    /// Its name for scenarios: `size.x+`, `size.z-`, `radius`, `length`.
    pub fn id(self) -> String {
        match self {
            Grip::Side { axis, far } => format!("size.{}{}", ["x", "y", "z"][axis.min(2)], if far { "+" } else { "-" }),
            Grip::Radius => "radius".into(),
            Grip::Length => "length".into(),
        }
    }
}

/// A body as its grips see it: its name, shape, origin and axis.
#[derive(Clone, Debug, PartialEq)]
struct Held {
    name: String,
    shape: Shape,
    at: Point3,
    axis: Axis,
}

/// The single body selected, when it is a primitive (box, cylinder, sphere).
fn held(app: &EmcraftApp) -> Option<Held> {
    let m = app.session.model();
    let (shape, at, axis, at_ref) = match app.ui_state.selection()[..] {
        [Pick::Feature(i)] => m.features().get(i).map(|f| (f.shape.clone(), f.at, f.axis, emcraft_engine::bodies::BodyRef::Feature(i)))?,
        [Pick::Inclusion(i)] => m.inclusions().get(i).map(|f| (f.shape.clone(), f.at, Axis::Z, emcraft_engine::bodies::BodyRef::Inclusion(i)))?,
        _ => return None,
    };
    if matches!(shape, Shape::Surface { .. }) {
        return None;
    }
    let name = emcraft_engine::bodies::bodies(m).into_iter().find(|b| b.at == at_ref)?.name;
    Some(Held { name, shape, at, axis })
}

/// Each grip of `h` (with the drag `d` applied when it is held): where it is and its outward
/// direction, in the model's frame (m).
fn grips_of(h: &Held) -> Vec<(Grip, [f64; 3], [f64; 3])> {
    let world = |local: Vec3| h.at + h.axis.turn(local);
    let dir = |local: Vec3| {
        let w = h.axis.turn(local);
        [w.x, w.y, w.z]
    };
    let p = |q: Point3| [q.x, q.y, q.z];
    match &h.shape {
        Shape::Box { size } => {
            let s = size.get();
            let c = s * 0.5;
            let mut out = Vec::with_capacity(6);
            for axis in 0..3 {
                for far in [false, true] {
                    let mut q = c;
                    q[axis] = if far { s[axis] } else { 0.0 };
                    let mut d = Vec3::ZERO;
                    d[axis] = if far { 1.0 } else { -1.0 };
                    out.push((Grip::Side { axis, far }, p(world(q)), dir(d)));
                }
            }
            out
        }
        Shape::Cylinder { radius, length } => vec![
            (Grip::Radius, p(world(Vec3::new(radius.get(), 0.0, 0.5 * length.get()))), dir(Vec3::new(1.0, 0.0, 0.0))),
            (Grip::Length, p(world(Vec3::new(0.0, 0.0, length.get()))), dir(Vec3::new(0.0, 0.0, 1.0))),
        ],
        Shape::Sphere { radius } => vec![(Grip::Radius, p(world(Vec3::new(radius.get(), 0.0, 0.0))), dir(Vec3::new(1.0, 0.0, 0.0)))],
        Shape::Surface { .. } => Vec::new(),
    }
}

/// A grip being dragged: the body as it was, the grip and its outward move so far (m), raw and
/// snapped.
#[derive(Clone, Debug, PartialEq)]
pub struct BodyDrag {
    held: Held,
    grip: Grip,
    raw: f64,
    snapped: f64,
}

impl BodyDrag {
    /// The state for `ui.inspect`: the body, the grip and the value it gives (display units).
    pub fn inspect(&self, l: emcraft_model::units::Length) -> Value {
        json!({"body": self.held.name, "grip": self.grip.id(), "value": l.from_si(value(&self.held, self.grip, self.snapped))})
    }
}

/// The dimension a grip changes, after an outward move `d` (m).
fn value(h: &Held, g: Grip, d: f64) -> f64 {
    match (&h.shape, g) {
        (Shape::Box { size }, Grip::Side { axis, .. }) => size.get()[axis] + d,
        (Shape::Cylinder { radius, .. } | Shape::Sphere { radius }, Grip::Radius) => radius.get() + d,
        (Shape::Cylinder { length, .. }, Grip::Length) => length.get() + d,
        _ => 0.0,
    }
}

/// The step the grips snap to (m): a round number of the display unit near a fiftieth of the
/// model's size.
fn step(app: &EmcraftApp) -> f64 {
    let l = app.session.display().length;
    let span = app.session.model().bounds().map_or(1.0, |b| l.from_si((b.max - b.min).norm()));
    let raw = (span / 50.0).max(1e-9);
    let p = 10f64.powf(raw.log10().floor());
    let nice = [1.0, 2.0, 5.0, 10.0].into_iter().map(|k| k * p).find(|s| *s >= raw).unwrap_or(10.0 * p);
    l.to_si(nice)
}

/// The grip under the pointer `at` (within [`crate::tools3d::GRIP_PX`]), nearest first.
pub fn under(app: &EmcraftApp, v: &View3d, rect: Rect, at: Pos2) -> Option<(Grip, Pos2)> {
    let h = held(app)?;
    grips_of(&h)
        .into_iter()
        .filter_map(|(g, q, _)| screen(&v.camera, rect, q).map(|s| (g, s)))
        .filter(|(_, s)| s.distance(at) <= crate::tools3d::GRIP_PX)
        .min_by(|a, b| a.1.distance(at).total_cmp(&b.1.distance(at)))
}

/// Starts dragging the grip under `at`.
pub fn start(app: &EmcraftApp, v: &mut View3d, rect: Rect, at: Pos2) -> bool {
    let Some((grip, _)) = under(app, v, rect, at) else { return false };
    let Some(held) = held(app) else { return false };
    v.tools.body_drag = Some(BodyDrag { held, grip, raw: 0.0, snapped: 0.0 });
    true
}

/// Moves the dragged grip by the pointer's move `d`: its share along the grip's direction on
/// screen, the dimension snapped to the grid and kept above a twentieth of what it was.
pub fn drag(app: &EmcraftApp, v: &mut View3d, rect: Rect, d: egui::Vec2) {
    let step = step(app);
    let Some(bd) = v.tools.body_drag.as_ref() else { return };
    let Some(&(_, q, dir)) = grips_of(&bd.held).iter().find(|g| g.0 == bd.grip) else { return };
    let span = 20.0 * step;
    let (Some(a), Some(e)) =
        (screen(&v.camera, rect, q), screen(&v.camera, rect, [q[0] + dir[0] * span, q[1] + dir[1] * span, q[2] + dir[2] * span]))
    else {
        return;
    };
    let along = e - a;
    if along.length_sq() < 4.0 {
        return;
    }
    let Some(bd) = v.tools.body_drag.as_mut() else { return };
    bd.raw += f64::from(d.dot(along) / along.length_sq()) * span;
    let was = value(&bd.held, bd.grip, 0.0);
    let wanted = ((was + bd.raw) / step).round() * step;
    bd.snapped = wanted.max(0.05 * was) - was;
}

/// The drag let go: one `geometry.body.edit` with the dimension (and a box's near face its
/// origin) where it was left.
pub fn end(app: &EmcraftApp, v: &mut View3d, out: &mut Actions) {
    let Some(bd) = v.tools.body_drag.take() else { return };
    if bd.snapped == 0.0 {
        return;
    }
    let l = app.session.display().length;
    let neat = |x: f64| (l.from_si(x) * 1e6).round() / 1e6;
    let h = &bd.held;
    let mut params = json!({"body": h.name});
    match (&h.shape, bd.grip) {
        (Shape::Box { size }, Grip::Side { axis, far }) => {
            let mut s = size.get();
            s[axis] += bd.snapped;
            params["size"] = json!([neat(s.x), neat(s.y), neat(s.z)]);
            if !far {
                let mut back = Vec3::ZERO;
                back[axis] = -bd.snapped;
                let at = h.at + h.axis.turn(back);
                params["at"] = json!([neat(at.x), neat(at.y), neat(at.z)]);
            }
        }
        (Shape::Cylinder { .. } | Shape::Sphere { .. }, Grip::Radius) => params["radius"] = json!(neat(value(h, bd.grip, bd.snapped))),
        (Shape::Cylinder { .. }, Grip::Length) => params["length"] = json!(neat(value(h, bd.grip, bd.snapped))),
        _ => return,
    }
    out.push(Action::run("geometry.body.edit", params));
}

/// Draws the selected body's grips (the dragged one where it is, with its value).
pub fn draw(app: &EmcraftApp, v: &View3d, p: &egui::Painter, rect: Rect) {
    let Some(h) = held(app) else { return };
    let t = &Tokens::get();
    let l = app.session.display().length;
    let dragging = v.tools.body_drag.as_ref().filter(|d| d.held.name == h.name);
    for (g, q, dir) in grips_of(&h) {
        let moved = dragging.filter(|d| d.grip == g).map_or(0.0, |d| d.snapped);
        let q = [q[0] + dir[0] * moved, q[1] + dir[1] * moved, q[2] + dir[2] * moved];
        let Some(s) = screen(&v.camera, rect, q) else { continue };
        let r = Rect::from_center_size(s, vec2(9.0, 9.0));
        p.rect_filled(r, 2.0, t.panel);
        p.rect_stroke(r, 2.0, Stroke::new(2.0, t.accent), egui::StrokeKind::Middle);
        scenario::publish_handle(Handle::BodyGrip(&g.id()), s);
        if let Some(d) = dragging.filter(|d| d.grip == g) {
            let text = format!("{} {}", trimmed(l.from_si(value(&h, g, d.snapped)), 3), l.symbol());
            let gal = p.layout_no_wrap(text, FontId::proportional(t.font_small), t.text);
            let at = Rect::from_min_size(s + vec2(10.0, -gal.size().y - 8.0), gal.size() + vec2(10.0, 4.0));
            p.rect_filled(at, 4.0, t.panel.gamma_multiply(0.9));
            p.galley(at.min + vec2(5.0, 2.0), gal, t.text);
        }
    }
}

/// Starts a drag on whatever grip is under `at`: a body's, else the port's reference plane.
pub fn start_any(app: &EmcraftApp, v: &mut View3d, rect: Rect, at: Option<Pos2>) {
    if let Some(at) = at
        && start(app, v, rect, at)
    {
        return;
    }
    crate::tools3d::start_drag(app, v);
}

/// The nearest grip to the pointer: a body's, or the reference plane's.
pub fn nearest(app: &EmcraftApp, v: &View3d, rect: Rect, at: Option<Pos2>) -> Option<Pos2> {
    let body = at.and_then(|a| under(app, v, rect, a)).map(|(_, s)| s);
    body.or_else(|| crate::tools3d::grip(app, v, rect))
}

/// Whether a body's grip is being dragged.
pub fn dragging(v: &View3d) -> bool {
    v.tools.body_drag.is_some()
}

/// The actions a body's drag ends in.
pub fn released(app: &EmcraftApp, v: &mut View3d, out: &mut Actions) {
    if dragging(v) {
        end(app, v, out);
    } else {
        crate::tools3d::end_drag(app, v, out);
    }
}

/// Moves whichever grip is dragged.
pub fn moved(app: &EmcraftApp, v: &mut View3d, rect: Rect, d: egui::Vec2) {
    if dragging(v) {
        drag(app, v, rect, d);
    } else {
        crate::tools3d::drag(app, v, rect, d);
    }
}
