//! A selected port's reference plane (its de-embedding): drawn inside the model and dragged
//! along the port's line by its grip; the drag ends in `model.port.edit {deembed}`, rounded in
//! display units, and holds at the port's face within a snap's points.

use emcraft_model::units::Length;
use serde_json::{Value, json};

use super::snap::{SNAP_PX, screen};
use crate::geom::{Point, Vector};
use crate::pick::Pick;
use crate::scene3d::{along, port_normal, port_quad, quad_centre};

/// Points from the reference plane's grip within which a drag takes it.
pub const GRIP_PX: f32 = 12.0;

/// A port's reference plane while its grip is dragged.
#[derive(Clone, Debug, PartialEq)]
pub struct RefDrag {
    pub port: usize,
    pub name: String,
    /// The de-embedding (m) it shows now: where the pointer put it, at 0 when that is near.
    pub deembed: f64,
    /// Where the pointer put it (m).
    pub raw: f64,
}

impl RefDrag {
    /// Its state for `ui.inspect`, lengths in `l`.
    pub fn inspect(&self, l: Length) -> Value {
        json!({"port": self.name, "deembed": l.from_si(self.deembed)})
    }
}

/// A selected port's reference plane: the port, its face moved by its de-embedding along its
/// line, and the line's direction (a unit vector into the model along the port's axis).
#[derive(Clone, Debug, PartialEq)]
pub struct RefPlane {
    pub port: usize,
    pub name: String,
    pub deembed: f64,
    pub face: [[f64; 3]; 4],
    pub inward: [f64; 3],
}

impl RefPlane {
    /// Its corners: the port's face's, moved by the de-embedding.
    pub fn at(&self) -> [[f64; 3]; 4] {
        self.face.map(|p| along(p, self.inward, self.deembed))
    }

    /// Where its grip is.
    pub fn centre(&self) -> [f64; 3] {
        quad_centre(&self.at())
    }

    /// The distance from the port's face as shown by the grip, in `l`.
    pub fn label(&self, l: Length) -> String {
        format!("{} {}", super::trimmed(l.from_si(self.deembed), 3), l.symbol())
    }
}

/// The reference plane of the port `selected` alone (as dragged, while `drag` is on it).
pub fn ref_plane(m: &emcraft_model::Model, selected: &[Pick], drag: Option<&RefDrag>) -> Option<RefPlane> {
    let [Pick::Port(i)] = selected[..] else { return None };
    let p = m.ports.get(i)?;
    let face = port_quad(m, p)?;
    let deembed = drag.filter(|d| d.port == i).map_or(p.deembed, |d| d.deembed);
    Some(RefPlane { port: i, name: p.name.clone(), deembed, face, inward: port_normal(p) })
}

/// The drag begun on `r`.
pub fn start(r: RefPlane) -> RefDrag {
    RefDrag { port: r.port, name: r.name, deembed: r.deembed, raw: r.deembed }
}

/// `drag` after the pointer moved by `d` (points): the move's share of the port's line on a view
/// of `size` through `cam`, over the model's length along it; within a snap's points of the
/// port's face it holds there.
pub fn dragged(drag: RefDrag, r: &RefPlane, m: &emcraft_model::Model, cam: &emcraft_render::view3d::Camera, size: Vector, d: Vector) -> RefDrag {
    let Some(b) = m.bounds() else { return drag };
    let span = (0..3).map(|k| (b.max[k] - b.min[k]).abs() * r.inward[k].abs()).sum::<f64>().max(1e-9);
    let c = r.centre();
    let (Some(a), Some(e)) = (screen(cam, size, c), screen(cam, size, along(c, r.inward, span))) else { return drag };
    let line = e - a;
    let length2 = line.x * line.x + line.y * line.y;
    if length2 < 4.0 || !(d.x.is_finite() && d.y.is_finite()) {
        return drag;
    }
    let raw = (drag.raw + f64::from((d.x * line.x + d.y * line.y) / length2) * span).clamp(-span, span);
    let near = f64::from(SNAP_PX / length2.sqrt()) * span;
    RefDrag { raw, deembed: if raw.abs() < near { 0.0 } else { raw }, ..drag }
}

/// A length (m) in display units, to a thousandth: a reference plane as set and shown.
pub fn rounded(l: Length, x: f64) -> f64 {
    (l.from_si(x) * 1e3).round() / 1e3
}

/// The drag let go: `model.port.edit`'s parameters setting the de-embedding where it was left
/// (rounded in `l`), when that moved it.
pub fn let_go(d: &RefDrag, m: &emcraft_model::Model, l: Length) -> Option<Value> {
    let at = rounded(l, d.deembed);
    let now = m.ports.get(d.port).map(|p| l.from_si(p.deembed))?;
    ((now - at).abs() > 0.0).then(|| json!({"name": d.name, "deembed": at}))
}

/// The grip of `r` on a view of `size` through `cam`.
pub fn grip(r: &RefPlane, cam: &emcraft_render::view3d::Camera, size: Vector) -> Option<Point> {
    screen(cam, size, r.centre())
}
