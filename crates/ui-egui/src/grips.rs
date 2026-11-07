//! A selected body's grips in the 3D view (docs/adr/0005-geometry-ux.md): a box's six faces, a
//! cylinder's radius and length, a sphere's radius, an arrow along each axis that moves the whole
//! body, and (for a body that may turn) a ring at its middle that turns its own axis to the next
//! one, keeping its middle. Dragging one moves it along its direction on screen, its value
//! snapping to the planes of the other bodies' faces, the model's box and the ports' faces when
//! it comes within a few pixels of one, else to a grid of the display unit, and shown beside it;
//! digits typed while it is held give the value exactly (Enter sets it, Escape lets the drag go
//! with nothing changed). Letting go runs one `geometry.body.edit` with the new dimensions,
//! origin or axis, so undo, the history and scripts see the edit the drag made.

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
    /// The whole body along the model's `axis` (0, 1, 2).
    Move {
        axis: usize,
    },
    /// The body's own axis turned to the next (z → x → y → z), its middle kept.
    Turn,
}

impl Grip {
    /// Its name for scenarios: `size.x+`, `size.z-`, `radius`, `length`.
    pub fn id(self) -> String {
        match self {
            Grip::Side { axis, far } => format!("size.{}{}", ["x", "y", "z"][axis.min(2)], if far { "+" } else { "-" }),
            Grip::Radius => "radius".into(),
            Grip::Length => "length".into(),
            Grip::Move { axis } => format!("move.{}", ["x", "y", "z"][axis.min(2)]),
            Grip::Turn => "turn".into(),
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
    of: emcraft_engine::bodies::BodyRef,
}

impl Held {
    /// The body's box in the model's frame (m).
    fn bounds(&self) -> emcraft_model::Aabb {
        let own = self.shape.bounds();
        let (a, b) = (self.axis.turn(own.min.to_vec()), self.axis.turn(own.max.to_vec()));
        emcraft_model::Aabb::new(self.at + a.min(b), self.at + a.max(b))
    }

    /// Its middle in its own frame.
    fn middle_local(&self) -> Vec3 {
        let b = self.shape.bounds();
        (b.min.to_vec() + b.max.to_vec()) * 0.5
    }
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
    Some(Held { name, shape, at, axis, of: at_ref })
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
    let mut out = match &h.shape {
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
        Shape::Surface { .. } | Shape::Sheet { .. } | Shape::Board(_) => Vec::new(),
    };
    if out.is_empty() {
        return out;
    }
    // The arrows that move it, a quarter of its extent out from its middle along each axis, and
    // the ring at its middle that turns it (a feature's: an inclusion keeps the z axis).
    let b = h.bounds();
    let middle = world(h.middle_local());
    for axis in 0..3 {
        let mut d = [0.0; 3];
        d[axis] = 1.0;
        let mut q = p(middle);
        q[axis] += 0.25 * (b.max[axis] - b.min[axis]);
        out.push((Grip::Move { axis }, q, d));
    }
    if matches!(h.of, emcraft_engine::bodies::BodyRef::Feature(_)) && !matches!(h.shape, Shape::Sphere { .. }) {
        out.push((Grip::Turn, p(middle), [0.0; 3]));
    }
    out
}

/// A grip being dragged: the body as it was, the grip, its outward move so far (m) — raw, and
/// snapped to a plane or the grid — what that snap was, and the digits typed while it is held.
#[derive(Clone, Debug, PartialEq)]
pub struct BodyDrag {
    held: Held,
    grip: Grip,
    raw: f64,
    snapped: f64,
    /// The plane (m, along the grip's axis) the move snapped to, `None` for the grid.
    plane: Option<f64>,
    typed: String,
}

impl BodyDrag {
    /// The state for `ui.inspect`: the body, the grip, the value it gives (display units) and
    /// what it snapped to.
    pub fn inspect(&self, l: emcraft_model::units::Length) -> Value {
        json!({
            "body": self.held.name,
            "grip": self.grip.id(),
            "value": l.from_si(value(&self.held, self.grip, self.moved(l))),
            "snap": if self.plane.is_some() { "plane" } else { "grid" },
            "typed": self.typed,
        })
    }

    /// The move the drag gives (m): the typed value's when the digits make a usable one, else
    /// the pointer's, snapped.
    fn moved(&self, l: emcraft_model::units::Length) -> f64 {
        let Some(t) = self.typed.parse::<f64>().ok().filter(|t| t.is_finite()) else { return self.snapped };
        let t = l.to_si(t);
        match self.grip {
            Grip::Move { .. } => t,
            Grip::Turn => self.snapped,
            _ if t > 0.0 => t - value(&self.held, self.grip, 0.0),
            _ => self.snapped,
        }
    }
}

/// The value a grip shows after an outward move `d` (m): the dimension it changes, how far a
/// move grip has taken the body, a turn grip's move (it turns the body once it is long enough).
fn value(h: &Held, g: Grip, d: f64) -> f64 {
    match (&h.shape, g) {
        (Shape::Box { size }, Grip::Side { axis, .. }) => size.get()[axis] + d,
        (Shape::Cylinder { radius, .. } | Shape::Sphere { radius }, Grip::Radius) => radius.get() + d,
        (Shape::Cylinder { length, .. }, Grip::Length) => length.get() + d,
        (_, Grip::Move { .. } | Grip::Turn) => d,
        _ => 0.0,
    }
}

/// The axis after `a`: z → x → y → z.
fn next_axis(a: Axis) -> Axis {
    match a {
        Axis::Z => Axis::X,
        Axis::X => Axis::Y,
        Axis::Y => Axis::Z,
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

/// How near on screen (points) a dragged face comes to a plane before it snaps to it.
const SNAP_PX: f64 = 8.0;

/// The planes across the model's `axis` a dragged face snaps to (m): the faces of the bodies but
/// `h`, the model's box and the ports' faces across that axis (a guide's ends across z).
fn planes(app: &EmcraftApp, h: &Held, axis: usize) -> Vec<f64> {
    use emcraft_engine::bodies::BodyRef;
    let m = app.session.model();
    let mut out = Vec::new();
    let mut add = |b: emcraft_model::Aabb| out.extend([b.min[axis], b.max[axis]]);
    for (i, f) in m.features().iter().enumerate() {
        if h.of != BodyRef::Feature(i) {
            add(f.bounds());
        }
    }
    for (i, f) in m.inclusions().iter().enumerate() {
        if h.of != BodyRef::Inclusion(i) {
            add(f.bounds());
        }
    }
    if let Some(b) = m.bounds() {
        add(b);
    }
    for p in &m.ports {
        match p.face {
            Some(f) if f.axis.index() == axis => out.push(f.at),
            None if axis == 2 => {
                if let Some(b) = m.bounds() {
                    out.push(if p.end == emcraft_model::End::Start { b.min.z } else { b.max.z });
                }
            }
            _ => {}
        }
    }
    out.retain(|x| x.is_finite());
    out
}

/// The faces a grip carries along its axis as it moves: (where each is, m; +1 when it moves with
/// the outward move, −1 against it).
fn carried(h: &Held, g: Grip, q: [f64; 3], dir: [f64; 3]) -> Vec<(usize, f64, f64)> {
    let Some(axis) = (0..3).find(|&k| dir[k].abs() > 0.5) else { return Vec::new() };
    match g {
        Grip::Move { .. } => {
            let b = h.bounds();
            vec![(axis, b.min[axis], 1.0), (axis, b.max[axis], 1.0)]
        }
        Grip::Turn => Vec::new(),
        _ => vec![(axis, q[axis], dir[axis].signum())],
    }
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
    v.tools.body_drag = Some(BodyDrag { held, grip, raw: 0.0, snapped: 0.0, plane: None, typed: String::new() });
    true
}

/// Moves the dragged grip by the pointer's move `d`: its share along the grip's direction on
/// screen (a turn grip: the pointer's whole move), snapped to a plane within [`SNAP_PX`] of a
/// face it carries, else the dimension (or a move) to the grid, a dimension kept above a
/// twentieth of what it was.
pub fn drag(app: &EmcraftApp, v: &mut View3d, rect: Rect, d: egui::Vec2) {
    let step = step(app);
    let Some(bd) = v.tools.body_drag.as_ref() else { return };
    if bd.grip == Grip::Turn {
        if let Some(bd) = v.tools.body_drag.as_mut() {
            bd.raw += f64::from(d.length());
            bd.snapped = bd.raw;
        }
        return;
    }
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
    // Points on screen per metre along the grip.
    let px = f64::from(along.length()) / span;
    let carried = carried(&bd.held, bd.grip, q, dir);
    let candidates = carried.first().map(|c| planes(app, &bd.held, c.0)).unwrap_or_default();
    let Some(bd) = v.tools.body_drag.as_mut() else { return };
    bd.raw += f64::from(d.dot(along) / along.length_sq()) * span;
    let was = value(&bd.held, bd.grip, 0.0);
    // The nearest plane a carried face reaches within the snap's reach: the move that puts it there.
    let plane = carried
        .iter()
        .flat_map(|&(_, x, sign)| candidates.iter().map(move |&c| (sign * (c - x), c)))
        .filter(|(m, _)| (m - bd.raw).abs() * px <= SNAP_PX)
        .min_by(|a, b| (a.0 - bd.raw).abs().total_cmp(&(b.0 - bd.raw).abs()));
    let floor = |m: f64| if matches!(bd.grip, Grip::Move { .. }) { m } else { m.max(0.05 * was - was) };
    match plane {
        Some((m, c)) if floor(m) == m => {
            bd.snapped = m;
            bd.plane = Some(c);
        }
        _ => {
            let wanted = ((was + bd.raw) / step).round() * step;
            bd.snapped = floor(wanted - was);
            bd.plane = None;
        }
    }
}

/// How far (points) a turn grip must be dragged to turn the body.
const TURN_PX: f64 = 24.0;

/// The drag let go: one `geometry.body.edit` with the dimension (and a box's near face its
/// origin), the origin moved, or the axis turned, where it was left.
pub fn end(app: &EmcraftApp, v: &mut View3d, out: &mut Actions) {
    let Some(bd) = v.tools.body_drag.take() else { return };
    let l = app.session.display().length;
    let moved = bd.moved(l);
    if moved == 0.0 || (bd.grip == Grip::Turn && moved < TURN_PX) {
        return;
    }
    let neat = |x: f64| (l.from_si(x) * 1e6).round() / 1e6;
    let point = |q: Point3| json!([neat(q.x), neat(q.y), neat(q.z)]);
    let h = &bd.held;
    let mut params = json!({"body": h.name});
    match (&h.shape, bd.grip) {
        (Shape::Box { size }, Grip::Side { axis, far }) => {
            let mut s = size.get();
            s[axis] += moved;
            params["size"] = json!([neat(s.x), neat(s.y), neat(s.z)]);
            if !far {
                let mut back = Vec3::ZERO;
                back[axis] = -moved;
                params["at"] = point(h.at + h.axis.turn(back));
            }
        }
        (Shape::Cylinder { .. } | Shape::Sphere { .. }, Grip::Radius) => params["radius"] = json!(neat(value(h, bd.grip, moved))),
        (Shape::Cylinder { .. }, Grip::Length) => params["length"] = json!(neat(value(h, bd.grip, moved))),
        (_, Grip::Move { axis }) => {
            let mut d = Vec3::ZERO;
            d[axis] = moved;
            params["at"] = point(h.at + d);
        }
        (_, Grip::Turn) => {
            // The same middle, the body's own frame turned onto the next axis.
            let next = next_axis(h.axis);
            let middle = h.at + h.axis.turn(h.middle_local());
            params["axis"] = json!(next.id());
            params["at"] = point(middle - next.turn(h.middle_local()));
        }
        _ => return,
    }
    out.push(Action::run("geometry.body.edit", params));
}

/// Digits typed while a grip is held: the value it sets, exactly (display units; a move grip's
/// distance, signed). Enter lets go with it, Escape drops the drag, Backspace takes a digit back.
/// The keys are taken from the frame, so nothing else reads them.
pub fn keys(app: &EmcraftApp, v: &mut View3d, ui: &egui::Ui, out: &mut Actions) {
    if v.tools.body_drag.is_none() {
        return;
    }
    let (mut enter, mut escape) = (false, false);
    let mut typed = Vec::new();
    ui.input_mut(|i| {
        i.events.retain(|e| match e {
            egui::Event::Text(t) => {
                typed.extend(t.chars().filter(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | 'e')).map(Some));
                false
            }
            egui::Event::Key { key: egui::Key::Backspace, pressed: true, .. } => {
                typed.push(None);
                false
            }
            egui::Event::Key { key: egui::Key::Enter, pressed: true, .. } => {
                enter = true;
                false
            }
            egui::Event::Key { key: egui::Key::Escape, pressed: true, .. } => {
                escape = true;
                false
            }
            _ => true,
        });
    });
    let Some(bd) = v.tools.body_drag.as_mut() else { return };
    for c in typed {
        match c {
            Some(c) if bd.typed.len() < 24 => bd.typed.push(c),
            Some(_) => {}
            None => {
                bd.typed.pop();
            }
        }
    }
    if escape {
        v.tools.body_drag = None;
    } else if enter {
        end(app, v, out);
    }
}

/// Draws the selected body's grips (the dragged one where it is, with its value): squares on its
/// faces, arrows from its middle that move it, a ring that turns it.
pub fn draw(app: &EmcraftApp, v: &View3d, p: &egui::Painter, rect: Rect) {
    let Some(h) = held(app) else { return };
    let t = &Tokens::get();
    let l = app.session.display().length;
    let dragging = v.tools.body_drag.as_ref().filter(|d| d.held.name == h.name);
    let middle = h.at + h.axis.turn(h.middle_local());
    let axis_colour = |k: usize| [t.axis_x, t.axis_y, t.axis_z][k.min(2)];
    for (g, q, dir) in grips_of(&h) {
        let moved = dragging.filter(|d| d.grip == g).map_or(0.0, |d| if g == Grip::Turn { 0.0 } else { d.moved(l) });
        let q = [q[0] + dir[0] * moved, q[1] + dir[1] * moved, q[2] + dir[2] * moved];
        let Some(s) = screen(&v.camera, rect, q) else { continue };
        match g {
            Grip::Move { axis } => {
                let c = axis_colour(axis);
                // From the middle, carried as far as the arrow.
                if let Some(m) = screen(&v.camera, rect, [middle.x + dir[0] * moved, middle.y + dir[1] * moved, middle.z + dir[2] * moved]) {
                    p.line_segment([m, s], Stroke::new(2.0, c));
                }
                p.circle_filled(s, 5.5, c);
                p.circle_stroke(s, 5.5, Stroke::new(1.5, t.panel));
            }
            Grip::Turn => {
                p.circle_stroke(s, 9.0, Stroke::new(2.0, t.accent));
                p.circle_filled(s, 3.0, t.accent);
            }
            _ => {
                let r = Rect::from_center_size(s, vec2(9.0, 9.0));
                p.rect_filled(r, 2.0, t.panel);
                p.rect_stroke(r, 2.0, Stroke::new(2.0, t.accent), egui::StrokeKind::Middle);
            }
        }
        scenario::publish_handle(Handle::BodyGrip(&g.id()), s);
        if let Some(d) = dragging.filter(|d| d.grip == g) {
            let text = match g {
                Grip::Turn => format!("{} → {}", h.axis.id(), next_axis(h.axis).id()),
                Grip::Move { axis } => format!("Δ{} {} {}", ["x", "y", "z"][axis.min(2)], trimmed(l.from_si(d.moved(l)), 3), l.symbol()),
                _ => format!("{} {}", trimmed(l.from_si(value(&h, g, d.moved(l))), 3), l.symbol()),
            };
            // Typed digits show as typed, a snap to a plane with a mark.
            let text = if d.typed.is_empty() { text } else { format!("{} ⌨ {}", text, d.typed) };
            let text = if d.plane.is_some() && d.typed.is_empty() { format!("{text}  ⊣") } else { text };
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
