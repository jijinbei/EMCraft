//! A selected body's grips in the 3D view (docs/adr/0005-geometry-ux.md), whatever window
//! draws them: a box's six faces, a cylinder's radius and length, a sphere's radius, an arrow
//! along each axis that moves the whole body, and (for a body that may turn) a ring at its middle
//! that turns its own axis to the next one, keeping its middle. Dragging one moves it along its
//! direction on screen, its value snapping to the planes of the other bodies' faces, the model's
//! box and the ports' faces when it comes within a few points of one, else to a grid of the
//! display unit; digits typed while it is held give the value exactly. Letting go gives one
//! `geometry.body.edit` with the new dimensions, origin or axis ([`let_go`]), so undo, the
//! history and scripts see the edit the drag made. Points are on the view, from its corner.

use emcraft_model::units::Length;
use emcraft_model::{Axis, Point3, Shape, Vec3};
use emcraft_render::view3d::Camera;
use serde_json::{Value, json};

use super::GRIP_PX;
use super::snap::screen;
use crate::geom::{Point, Vector};
use crate::pick::Pick;

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
pub struct Held {
    pub name: String,
    pub shape: Shape,
    pub at: Point3,
    pub axis: Axis,
    pub of: emcraft_engine::bodies::BodyRef,
}

impl Held {
    /// The body's box in the model's frame (m).
    pub fn bounds(&self) -> emcraft_model::Aabb {
        let own = self.shape.bounds();
        let (a, b) = (self.axis.turn(own.min.to_vec()), self.axis.turn(own.max.to_vec()));
        emcraft_model::Aabb::new(self.at + a.min(b), self.at + a.max(b))
    }

    /// Its middle in its own frame.
    pub fn middle_local(&self) -> Vec3 {
        let b = self.shape.bounds();
        (b.min.to_vec() + b.max.to_vec()) * 0.5
    }
}

/// The single body `selected`, when it is a primitive (box, cylinder, sphere).
pub fn held(m: &emcraft_model::Model, selected: &[Pick]) -> Option<Held> {
    let (shape, at, axis, at_ref) = match selected[..] {
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
pub fn grips_of(h: &Held) -> Vec<(Grip, [f64; 3], [f64; 3])> {
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
    pub held: Held,
    pub grip: Grip,
    raw: f64,
    snapped: f64,
    /// The plane (m, along the grip's axis) the move snapped to, `None` for the grid.
    pub plane: Option<f64>,
    pub typed: String,
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
    pub fn moved(&self, l: emcraft_model::units::Length) -> f64 {
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
pub fn value(h: &Held, g: Grip, d: f64) -> f64 {
    match (&h.shape, g) {
        (Shape::Box { size }, Grip::Side { axis, .. }) => size.get()[axis] + d,
        (Shape::Cylinder { radius, .. } | Shape::Sphere { radius }, Grip::Radius) => radius.get() + d,
        (Shape::Cylinder { length, .. }, Grip::Length) => length.get() + d,
        (_, Grip::Move { .. } | Grip::Turn) => d,
        _ => 0.0,
    }
}

/// The axis after `a`: z → x → y → z.
pub fn next_axis(a: Axis) -> Axis {
    match a {
        Axis::Z => Axis::X,
        Axis::X => Axis::Y,
        Axis::Y => Axis::Z,
    }
}

/// The step the grips snap to (m): a round number of the display unit near a fiftieth of the
/// model's size.
fn step(m: &emcraft_model::Model, l: Length) -> f64 {
    let span = m.bounds().map_or(1.0, |b| l.from_si((b.max - b.min).norm()));
    let raw = (span / 50.0).max(1e-9);
    let p = 10f64.powf(raw.log10().floor());
    let nice = [1.0, 2.0, 5.0, 10.0].into_iter().map(|k| k * p).find(|s| *s >= raw).unwrap_or(10.0 * p);
    l.to_si(nice)
}

/// How near on screen (points) a dragged face comes to a plane before it snaps to it.
const SNAP_PX: f64 = 8.0;

/// The planes across the model's `axis` a dragged face snaps to (m): the faces of the bodies but
/// `h`, the model's box and the ports' faces across that axis (a guide's ends across z).
fn planes(m: &emcraft_model::Model, h: &Held, axis: usize) -> Vec<f64> {
    use emcraft_engine::bodies::BodyRef;
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

/// The grip of the body `selected` under the pointer `at` on a view of `size` through `cam`
/// (within [`GRIP_PX`]), nearest first, with where it is.
pub fn under(m: &emcraft_model::Model, selected: &[Pick], cam: &Camera, size: Vector, at: Point) -> Option<(Grip, Point)> {
    let h = held(m, selected)?;
    grips_of(&h)
        .into_iter()
        .filter_map(|(g, q, _)| screen(cam, size, q).map(|s| (g, s)))
        .filter(|(_, s)| s.distance(at) <= GRIP_PX)
        .min_by(|a, b| a.1.distance(at).total_cmp(&b.1.distance(at)))
}

/// The drag begun on the grip under `at`, if one is.
pub fn start(m: &emcraft_model::Model, selected: &[Pick], cam: &Camera, size: Vector, at: Point) -> Option<BodyDrag> {
    let (grip, _) = under(m, selected, cam, size, at)?;
    Some(BodyDrag { held: held(m, selected)?, grip, raw: 0.0, snapped: 0.0, plane: None, typed: String::new() })
}

/// `bd` after the pointer moved by `d` (points) on a view of `size` through `cam`: its share
/// along the grip's direction on screen (a turn grip: the pointer's whole move), snapped to a
/// plane within [`SNAP_PX`] of a face it carries, else the dimension (or a move) to the grid of
/// `l`, a dimension kept above a twentieth of what it was.
pub fn dragged(bd: BodyDrag, m: &emcraft_model::Model, l: Length, cam: &Camera, size: Vector, d: Vector) -> BodyDrag {
    if !(d.x.is_finite() && d.y.is_finite()) {
        return bd;
    }
    if bd.grip == Grip::Turn {
        let raw = bd.raw + f64::from(d.x.hypot(d.y));
        return BodyDrag { raw, snapped: raw, ..bd };
    }
    let step = step(m, l);
    let Some(&(_, q, dir)) = grips_of(&bd.held).iter().find(|g| g.0 == bd.grip) else { return bd };
    let span = 20.0 * step;
    let (Some(a), Some(e)) = (screen(cam, size, q), screen(cam, size, [q[0] + dir[0] * span, q[1] + dir[1] * span, q[2] + dir[2] * span])) else {
        return bd;
    };
    let along = e - a;
    let length2 = along.x * along.x + along.y * along.y;
    if length2 < 4.0 {
        return bd;
    }
    // Points on screen per metre along the grip.
    let px = f64::from(length2.sqrt()) / span;
    let carried = carried(&bd.held, bd.grip, q, dir);
    let candidates = carried.first().map(|c| planes(m, &bd.held, c.0)).unwrap_or_default();
    let raw = bd.raw + f64::from((d.x * along.x + d.y * along.y) / length2) * span;
    let was = value(&bd.held, bd.grip, 0.0);
    // The nearest plane a carried face reaches within the snap's reach: the move that puts it there.
    let plane = carried
        .iter()
        .flat_map(|&(_, x, sign)| candidates.iter().map(move |&c| (sign * (c - x), c)))
        .filter(|(mv, _)| (mv - raw).abs() * px <= SNAP_PX)
        .min_by(|a, b| (a.0 - raw).abs().total_cmp(&(b.0 - raw).abs()));
    let floor = |mv: f64| if matches!(bd.grip, Grip::Move { .. }) { mv } else { mv.max(0.05 * was - was) };
    match plane {
        Some((mv, c)) if floor(mv) == mv => BodyDrag { raw, snapped: mv, plane: Some(c), ..bd },
        _ => {
            let wanted = ((was + raw) / step).round() * step;
            BodyDrag { raw, snapped: floor(wanted - was), plane: None, ..bd }
        }
    }
}

/// A key typed while a grip is held: a digit, `.`, `-` or `e` added (up to 24), `None`
/// (Backspace) one taken back; anything else leaves it.
pub fn typed(bd: BodyDrag, key: Option<char>) -> BodyDrag {
    let mut typed = bd.typed;
    match key {
        Some(c) if (c.is_ascii_digit() || matches!(c, '.' | '-' | 'e')) && typed.len() < 24 => typed.push(c),
        Some(_) => {}
        None => {
            typed.pop();
        }
    }
    BodyDrag { typed, ..bd }
}

/// How far (points) a turn grip must be dragged to turn the body.
const TURN_PX: f64 = 24.0;

/// The drag let go: `geometry.body.edit`'s parameters with the dimension (and a box's near face
/// its origin), the origin moved, or the axis turned, where it was left; none when it moved
/// nothing.
pub fn let_go(bd: &BodyDrag, l: Length) -> Option<Value> {
    let moved = bd.moved(l);
    if moved == 0.0 || (bd.grip == Grip::Turn && moved < TURN_PX) {
        return None;
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
        _ => return None,
    }
    Some(params)
}

/// A grip as drawn: which, where (m; the dragged one where the drag has taken it), the middle
/// a move grip's arrow starts from, and the dragged one's value beside it.
#[derive(Clone, Debug, PartialEq)]
pub struct Mark {
    pub grip: Grip,
    pub at: [f64; 3],
    pub from: Option<[f64; 3]>,
    pub label: Option<String>,
}

/// The body `selected`'s grips to draw, `drag` applied, lengths in `l`.
pub fn marks(m: &emcraft_model::Model, selected: &[Pick], drag: Option<&BodyDrag>, l: Length) -> Vec<Mark> {
    let Some(h) = held(m, selected) else { return Vec::new() };
    let dragging = drag.filter(|d| d.held.name == h.name);
    let middle = h.at + h.axis.turn(h.middle_local());
    let shifted = |q: [f64; 3], dir: [f64; 3], by: f64| [q[0] + dir[0] * by, q[1] + dir[1] * by, q[2] + dir[2] * by];
    grips_of(&h)
        .into_iter()
        .map(|(g, q, dir)| {
            let held = dragging.filter(|d| d.grip == g);
            let moved = held.map_or(0.0, |d| if g == Grip::Turn { 0.0 } else { d.moved(l) });
            let from = matches!(g, Grip::Move { .. }).then(|| shifted([middle.x, middle.y, middle.z], dir, moved));
            let label = held.map(|d| {
                let text = match g {
                    Grip::Turn => format!("{} → {}", h.axis.id(), next_axis(h.axis).id()),
                    Grip::Move { axis } => format!("Δ{} {} {}", ["x", "y", "z"][axis.min(2)], super::trimmed(l.from_si(d.moved(l)), 3), l.symbol()),
                    _ => format!("{} {}", super::trimmed(l.from_si(value(&h, g, d.moved(l))), 3), l.symbol()),
                };
                // Typed digits show as typed, a snap to a plane with a mark.
                match (d.typed.is_empty(), d.plane.is_some()) {
                    (false, _) => format!("{text} ⌨ {}", d.typed),
                    (true, true) => format!("{text}  ⊣"),
                    (true, false) => text,
                }
            });
            Mark { grip: g, at: shifted(q, dir, moved), from, label }
        })
        .collect()
}
