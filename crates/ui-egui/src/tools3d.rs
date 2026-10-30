//! The 3D view's tools, worked by pointing at the model:
//! - **Face port**: hovering lights the face under the pointer; a face across z at an end of the
//!   model takes a waveguide port at a click (`model.port.create`, the next free name, the
//!   default modes), any other flat face across x, y or z of a solid (a bend's or a tee's arm
//!   ending on a side, a guide's end inside a body) opens the port dialog on that rectangle and
//!   axis (the side the guide lies on to choose), a port's own face selects it.
//! - **Gap port**: two clicks, each snapped to a corner, an edge's middle, a face's centre, an
//!   edge or a face, give a lumped port's `from` and `to` (`model.port.lumped`).
//! - **Measure**: two clicks, snapped the same way, measured by `geometry.measure` (a face
//!   brings its normal, an edge its direction): the distance drawn as a dimension, with Δx,
//!   Δy, Δz and the gap or angle the picks define on a card. Escape clears.
//! - A selected port's **reference plane** (its de-embedding) is drawn inside the model and
//!   dragged along the port's line by its grip; the drag ends in `model.port.edit {deembed}`.
//!
//! What the pointer is over is found on the drawn scene: the nearest tagged triangle under it
//! ([`emcraft_render::view3d::hits`]) and its face ([`crate::scene3d::faces::Face`]). Every
//! change goes through a command; a tool never edits the model itself.

use egui::{FontId, Pos2, Rect, Stroke, pos2, vec2};
use emcraft_model::End;
use emcraft_render::view3d::{self, Camera};
use serde_json::{Value, json};

use crate::glyphs::Glyph;
use crate::handle::{Fixed, Handle};
use crate::lang::{Raw, args, tr, trf};
use crate::scene3d::Pick;
use crate::scene3d::faces::{Face, dot, lerp, norm, sub};
use crate::theme::Tokens;
use crate::view3d::View3d;
use crate::{Action, Actions, EmcraftApp, Hint, UiExt, scenario};

emcraft_macros::named_enum! {
    /// A tool of the 3D view.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Tool {
        /// A waveguide port on the face clicked.
        Port = "port" => "Face port",
        /// A lumped port across two points clicked.
        Lumped = "lumped" => "Gap port",
        /// The distance between two things clicked.
        Measure = "measure" => "Measure",
    }
}

impl Tool {
    pub fn glyph(self) -> Glyph {
        match self {
            Tool::Port => Glyph::PickFace,
            Tool::Lumped => Glyph::PickGap,
            Tool::Measure => Glyph::Ruler,
        }
    }

    /// What it does, on hover.
    pub fn hint(self) -> &'static str {
        match self {
            Tool::Port => "Click a flat face: a port there",
            Tool::Lumped => "Click two points: a lumped port between them",
            Tool::Measure => "Click two points, edges or faces: the distance (Esc clears)",
        }
    }

    /// Whether it makes ports (the Excitation step's tools).
    pub fn ports(self) -> bool {
        matches!(self, Tool::Port | Tool::Lumped)
    }
}

emcraft_macros::named_enum! {
    /// What a pick snapped to.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum SnapKind {
        Vertex = "vertex",
        Midpoint = "midpoint",
        Centre = "centre",
        /// A point on a straight edge, with its direction.
        Edge = "edge",
        /// A point on a face, with its normal.
        Face = "face",
    }
}

/// A picked point (m), what it snapped to and the edge's direction or the face's normal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snap {
    pub at: [f64; 3],
    pub kind: SnapKind,
    pub dir: Option<[f64; 3]>,
}

impl Snap {
    /// For `ui.inspect`, lengths in `l`.
    fn inspect(&self, l: emcraft_model::units::Length) -> Value {
        json!({"kind": self.kind.id(), "at": shown(l, self.at)})
    }
}

/// What a click of the port tool does on the face under the pointer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PortTarget {
    /// A port's own face: selects it.
    Existing(usize),
    /// A free end of the model: a port there at once.
    End(End),
    /// A face across `axis` of a solid, not a free end: the port dialog on its rectangle (m:
    /// `face` as `model.port.create` takes it), the guide's side guessed from where the body
    /// lies.
    Inside { axis: emcraft_model::Axis, face: [f64; 5], end: End },
    /// No port goes here: why.
    No(&'static str),
}

/// What the pointer is over while a tool is on.
#[derive(Clone, Debug, PartialEq)]
pub struct Hover {
    pub face: Face,
    pub snap: Snap,
    pub port: Option<PortTarget>,
}

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

/// The tools' state.
#[derive(Clone, Debug, Default)]
pub struct Tools {
    pub tool: Option<Tool>,
    pub hover: Option<Hover>,
    /// The faces last found (the latest last), by the scene generation and triangle each was
    /// found from.
    faces: Vec<((u64, usize), Face)>,
    /// The points clicked so far (a gap's first end, a measurement's first pick).
    pub picks: Vec<Snap>,
    /// The last measurement: its picks and `geometry.measure`'s reply.
    pub measured: Option<([Snap; 2], Value)>,
    pub drag: Option<RefDrag>,
    /// The reference plane's field under the selection card: the port, its text, and the value
    /// (display units) it was filled from.
    pub ref_text: Option<(usize, String, f64)>,
}

impl Tools {
    /// Turns a tool on (or all off), dropping what the last one had picked.
    pub fn set(&mut self, tool: Option<Tool>) {
        self.tool = tool;
        self.picks.clear();
        self.hover = None;
        if tool != Some(Tool::Measure) {
            self.measured = None;
        }
    }

    /// Escape: what was picked goes; with nothing picked, the tool goes off.
    pub fn escape(&mut self) {
        if self.picks.is_empty() && self.measured.is_none() {
            self.tool = None;
        }
        self.picks.clear();
        self.measured = None;
    }

    /// The state for `ui.inspect`.
    pub fn inspect(&self, l: emcraft_model::units::Length) -> Value {
        json!({
            "tool": self.tool.map(Tool::id),
            "pointer": self.hover.as_ref().map(|h| {
                let mut v = h.snap.inspect(l);
                v["port"] = h.port.map_or(Value::Null, |p| json!(match p {
                    PortTarget::Existing(_) => "existing",
                    PortTarget::End(End::Start) => "start",
                    PortTarget::End(End::End) => "end",
                    PortTarget::Inside { .. } => "inside",
                    PortTarget::No(_) => "none",
                }));
                v
            }),
            "picks": self.picks.iter().map(|p| p.inspect(l)).collect::<Vec<_>>(),
            "measure": self.measured.as_ref().map(|(_, r)| r.clone()),
            "drag": self.drag.as_ref().map(|d| json!({"port": d.name, "deembed": l.from_si(d.deembed)})),
        })
    }
}

/// `v` with up to `digits` decimals, trailing zeros dropped.
pub fn trimmed(v: f64, digits: usize) -> String {
    let s = format!("{v:.digits$}");
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_owned() } else { s }
}

/// Pixels from a snap point within which the pointer snaps to it.
const SNAP_PX: f32 = 10.0;
/// Pixels from an edge within which the pointer picks it.
const EDGE_PX: f32 = 5.0;
/// Pixels from the reference plane's grip a drag takes it.
pub const GRIP_PX: f32 = 12.0;

/// Where `p` lands on screen in `rect`.
pub fn screen(cam: &Camera, rect: Rect, p: [f64; 3]) -> Option<Pos2> {
    let q = cam.project(p, f64::from(rect.width()), f64::from(rect.height()))?;
    q.iter().all(|c| c.is_finite()).then(|| rect.min + vec2(q[0] as f32, q[1] as f32))
}

/// The distance from `p` to the segment `a`–`b` on screen, and how far along it.
fn to_segment(p: Pos2, a: Pos2, b: Pos2) -> (f32, f32) {
    let d = b - a;
    let t = if d.length_sq() > 0.0 { ((p - a).dot(d) / d.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
    ((a + d * t).distance(p), t)
}

/// The point of segment `a`–`b` nearest the ray through the pointer (`t` on screen when the
/// ray runs along it).
fn on_segment(ray: Option<([f64; 3], [f64; 3])>, a: [f64; 3], b: [f64; 3], t: f32) -> [f64; 3] {
    let Some((o, d)) = ray else { return lerp(a, b, f64::from(t)) };
    let u = sub(b, a);
    let w = sub(a, o);
    let (uu, ud, dd) = (dot(u, u), dot(u, d), dot(d, d));
    let den = uu * dd - ud * ud;
    if den.abs() <= 1e-12 * uu * dd {
        return lerp(a, b, f64::from(t));
    }
    let s = ((ud * dot(w, d) - dd * dot(w, u)) / den).clamp(0.0, 1.0);
    lerp(a, b, s)
}

fn unit(v: [f64; 3]) -> Option<[f64; 3]> {
    let l = norm(v);
    (l > 0.0 && l.is_finite()).then(|| v.map(|c| c / l))
}

/// What the pointer at `at` snaps to on `face`, hit at `hit`: a corner, an edge's middle or the
/// face's centre within [`SNAP_PX`], else an edge within [`EDGE_PX`], else the face where it
/// was hit (just off the face, past its outline, only its corners and edges).
fn snap(face: &Face, hit: Option<[f64; 3]>, cam: &Camera, rect: Rect, at: Pos2) -> Option<Snap> {
    let mut best: Option<(f32, Snap)> = None;
    let mut consider = |p: [f64; 3], kind: SnapKind, dir: Option<[f64; 3]>| {
        if let Some(s) = screen(cam, rect, p) {
            let d = s.distance(at);
            if d <= SNAP_PX && best.as_ref().is_none_or(|(b, _)| d < *b) {
                best = Some((d, Snap { at: p, kind, dir }));
            }
        }
    };
    for c in &face.corners {
        consider(*c, SnapKind::Vertex, None);
    }
    for (a, b) in &face.edges {
        consider(lerp(*a, *b, 0.5), SnapKind::Midpoint, None);
    }
    consider(face.centre, SnapKind::Centre, Some(face.normal));
    if let Some((_, s)) = best {
        return Some(s);
    }
    let ray = cam.ray(f64::from(at.x - rect.left()), f64::from(at.y - rect.top()), f64::from(rect.width()), f64::from(rect.height()));
    let mut edge: Option<(f32, Snap)> = None;
    for (a, b) in &face.edges {
        let (Some(sa), Some(sb)) = (screen(cam, rect, *a), screen(cam, rect, *b)) else { continue };
        let (d, t) = to_segment(at, sa, sb);
        if d <= EDGE_PX && edge.as_ref().is_none_or(|(e, _)| d < *e) {
            edge = Some((d, Snap { at: on_segment(ray, *a, *b, t), kind: SnapKind::Edge, dir: unit(sub(*b, *a)) }));
        }
    }
    edge.map(|(_, s)| s).or_else(|| hit.map(|at| Snap { at, kind: SnapKind::Face, dir: Some(face.normal) }))
}

/// What a port goes on at `face`.
fn port_target(m: &emcraft_model::Model, face: &Face) -> PortTarget {
    if let Some(Pick::Port(i)) = Pick::from_tag(face.tag) {
        return PortTarget::Existing(i);
    }
    let Some(axis) = face.axis() else { return PortTarget::No("Ports lie on flat faces across x, y or z") };
    let Some(b) = m.bounds() else { return PortTarget::No("Ports lie on flat faces across x, y or z") };
    let z = face.centre[2];
    let tol = 1e-6 * (b.max.z - b.min.z).abs().max(1e-9);
    if m.open.is_none() && axis == emcraft_model::Axis::Z {
        let end = if (z - b.min.z.min(0.0)).abs() <= tol {
            Some(End::Start)
        } else if (z - b.max.z).abs() <= tol {
            Some(End::End)
        } else {
            None
        };
        if let Some(end) = end {
            return match m.ports.iter().position(|p| p.face.is_none() && p.end == end) {
                Some(i) => PortTarget::Existing(i),
                None => PortTarget::End(end),
            };
        }
    }
    if m.solid.is_none() {
        return PortTarget::No("A port inside the model needs a solid");
    }
    // A port's own face, where the body's face is found in front of the port's.
    let k = axis.index();
    let centre = emcraft_model::Point3::new(face.centre[0], face.centre[1], face.centre[2]);
    let near = 1e-6 * (0..3).map(|d| (b.max[d] - b.min[d]).abs()).fold(1e-9, f64::max);
    if let Some(i) = m.ports.iter().position(|p| p.face.is_some_and(|f| f.axis == axis && f.holds(centre, near))) {
        return PortTarget::Existing(i);
    }
    // The guide lies where the body is: above the face along its axis, or below it.
    let (u, v, _) = emcraft_model::Frame::new(axis).axes();
    let h = 1e-4 * (b.max[k] - b.min[k]).abs().max(1e-9);
    let mut ahead = emcraft_model::Point3::new(face.centre[0], face.centre[1], face.centre[2]);
    ahead[k] += h;
    let end = if m.material_at(ahead).is_some() { End::Start } else { End::End };
    let face = [face.centre[k], face.lo[u], face.lo[v], face.hi[u] - face.lo[u], face.hi[v] - face.lo[v]];
    PortTarget::Inside { axis, face, end }
}

/// Finds what the pointer (`at`, in `rect`) is over while a tool is on.
pub fn hover(app: &EmcraftApp, v: &mut View3d, rect: Rect, at: Option<Pos2>) {
    let mut faces = std::mem::take(&mut v.tools.faces);
    v.tools.hover = v.tools.tool.and_then(|tool| over(app, v, tool, rect, at, &mut faces));
    v.tools.faces = faces;
}

/// What the pointer is over for `tool`, the faces found kept in `faces`.
fn over(app: &EmcraftApp, v: &View3d, tool: Tool, rect: Rect, at: Option<Pos2>, faces: &mut Vec<((u64, usize), Face)>) -> Option<Hover> {
    let p = at.filter(|p| rect.contains(*p)).filter(|_| !v.dragging)?;
    let scene = v.shown()?;
    let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
    let m = app.session.model();
    let now = v.scene_gen();
    // Near a corner, an edge's middle or the centre of the face last under the pointer, the
    // snap holds (on a body's outline the pointer may as well hit what lies behind).
    if tool != Tool::Port
        && let Some((_, last)) = faces.last().filter(|((g, _), _)| *g == now)
        && let Some(s) = snap(last, None, &v.camera, rect, p).filter(|s| !matches!(s.kind, SnapKind::Edge | SnapKind::Face))
    {
        return Some(Hover { face: last.clone(), snap: s, port: None });
    }
    let hits = view3d::hits(&scene.scene, &v.camera, f64::from(p.x - rect.left()), f64::from(p.y - rect.top()), w, h);
    let mut face_of = |hit: &view3d::Hit| -> Option<Face> {
        let key = (now, hit.index);
        if let Some(i) = faces.iter().position(|(k, _)| *k == key) {
            // The latest last.
            let found = faces.remove(i);
            let f = found.1.clone();
            faces.push(found);
            return Some(f);
        }
        let f = Face::of(&scene.scene, hit.index)?;
        faces.push((key, f.clone()));
        if faces.len() > 4 {
            faces.remove(0);
        }
        Some(f)
    };
    // Points are picked through see-through walls, on what they show inside, as a click selects.
    // A port goes on the face in front when one may go there (a free end, a port's face), else
    // on what shows through it (a body's face inside an open box).
    let hit = match hits {
        None => None,
        Some((near, solid)) if tool == Tool::Port => {
            let air = m.open.is_some() && Pick::from_tag(near.tag) == Some(Pick::Solid);
            let usable = !air && face_of(&near).is_some_and(|f| !matches!(port_target(m, &f), PortTarget::No(_)));
            Some(if usable { near } else { solid.unwrap_or(near) })
        }
        Some((near, solid)) => Some(solid.unwrap_or(near)),
    };
    let face = match hit {
        Some(hit) => face_of(&hit)?,
        // Just off the face last under the pointer, its corners and edges still snap.
        None if tool != Tool::Port => faces.last().filter(|((g, _), _)| *g == now).map(|(_, f)| f.clone())?,
        None => return None,
    };
    let s = snap(&face, hit.map(|h| h.at), &v.camera, rect, p)?;
    let port = (tool == Tool::Port).then(|| port_target(m, &face));
    Some(Hover { face, snap: s, port })
}

/// The smallest free port name: 1, 2, …
fn next_name(m: &emcraft_model::Model) -> String {
    (1..=10_000usize).map(|n| n.to_string()).find(|n| !m.has_port_named(n)).unwrap_or_else(|| "new".to_owned())
}

/// A length (m) in display units, to ten significant digits: what a pick's rounding leaves
/// (23.000000000000004 mm) reads as it was meant (23).
fn neat(l: emcraft_model::units::Length, x: f64) -> f64 {
    let v = l.from_si(x);
    format!("{v:.9e}").parse().unwrap_or(v)
}

/// A point (m) as `[x, y, z]` in display units.
fn shown(l: emcraft_model::units::Length, p: [f64; 3]) -> Value {
    json!(p.map(|c| neat(l, c)))
}

/// A click with a tool on: what it asks for.
pub fn click(app: &EmcraftApp, v: &mut View3d, out: &mut Actions) {
    let (Some(tool), Some(hover)) = (v.tools.tool, v.tools.hover.clone()) else { return };
    let m = app.session.model();
    let l = app.session.display().length;
    match tool {
        Tool::Port => match hover.port {
            Some(PortTarget::Existing(i)) => {
                let row = crate::browser::row_of(app, Pick::Port(i)).map(|(k, _)| k);
                out.push(Action::Select { row, pick: Some(Pick::Port(i)) });
            }
            Some(PortTarget::End(end)) => out.push(Action::run("model.port.create", json!({"name": next_name(m), "end": end.id()}))),
            Some(PortTarget::Inside { axis, face, end }) => {
                let face = face.map(|c| neat(l, c));
                out.push(Action::dialog_with("model.port.create", json!({"name": next_name(m), "end": end.id(), "face": face, "axis": axis.id()})));
            }
            Some(PortTarget::No(why)) => out.push(Action::error(tr(why))),
            None => {}
        },
        Tool::Lumped => {
            v.tools.picks.push(hover.snap);
            if let [a, b] = v.tools.picks[..] {
                v.tools.picks.clear();
                out.push(Action::run("model.port.lumped", json!({"name": next_name(m), "from": shown(l, a.at), "to": shown(l, b.at)})));
            }
        }
        Tool::Measure => {
            if v.tools.measured.is_some() {
                v.tools.measured = None;
            }
            v.tools.picks.push(hover.snap);
            if let [a, b] = v.tools.picks[..] {
                v.tools.picks.clear();
                out.push(Action::Measure([a, b]));
            }
        }
    }
}

/// `geometry.measure`'s parameters for two picks, lengths in `l`.
pub fn measure_params(l: emcraft_model::units::Length, [a, b]: [Snap; 2]) -> Value {
    let mut p = json!({"from": shown(l, a.at), "to": shown(l, b.at)});
    for (s, name) in [(a, "from"), (b, "to")] {
        match (s.kind, s.dir) {
            (SnapKind::Face, Some(n)) => p[format!("{name}_normal")] = json!(n),
            (SnapKind::Edge, Some(d)) => p[format!("{name}_along")] = json!(d),
            _ => {}
        }
    }
    p
}

/// A selected port's reference plane: the port, its face moved by its de-embedding along its
/// line, and the line's direction (a unit vector into the model along the port's axis).
pub struct RefPlane {
    pub port: usize,
    pub name: String,
    pub deembed: f64,
    pub face: [[f64; 3]; 4],
    pub inward: [f64; 3],
}

impl RefPlane {
    fn at(&self) -> [[f64; 3]; 4] {
        self.face.map(|p| [0, 1, 2].map(|k| p[k] + self.inward[k] * self.deembed))
    }

    fn centre(&self) -> [f64; 3] {
        let q = self.at();
        [0, 1, 2].map(|k| 0.25 * (q[0][k] + q[1][k] + q[2][k] + q[3][k]))
    }
}

/// The reference plane of the selected port (as dragged, while it is).
pub fn ref_plane(app: &EmcraftApp, v: &View3d) -> Option<RefPlane> {
    let Some(Pick::Port(i)) = v.selected else { return None };
    let m = app.session.model();
    let p = m.ports.get(i)?;
    let face = crate::scene3d::port_quad(m, p)?;
    let deembed = v.tools.drag.as_ref().filter(|d| d.port == i).map_or(p.deembed, |d| d.deembed);
    Some(RefPlane { port: i, name: p.name.clone(), deembed, face, inward: crate::scene3d::port_normal(p) })
}

/// The reference plane's grip on screen.
pub fn grip(app: &EmcraftApp, v: &View3d, rect: Rect) -> Option<Pos2> {
    ref_plane(app, v).and_then(|r| screen(&v.camera, rect, r.centre()))
}

/// Starts dragging the reference plane.
pub fn start_drag(app: &EmcraftApp, v: &mut View3d) {
    v.tools.drag = ref_plane(app, v).map(|r| RefDrag { port: r.port, name: r.name, deembed: r.deembed, raw: r.deembed });
}

/// Moves the dragged reference plane by the pointer's move `d`: the move's share of the port's
/// line on screen, over the model's length.
pub fn drag(app: &EmcraftApp, v: &mut View3d, rect: Rect, d: egui::Vec2) {
    let (Some(r), Some(b)) = (ref_plane(app, v), app.session.model().bounds()) else { return };
    let span = (0..3).map(|k| (b.max[k] - b.min[k]).abs() * r.inward[k].abs()).sum::<f64>().max(1e-9);
    let c = r.centre();
    let (Some(a), Some(e)) = (screen(&v.camera, rect, c), screen(&v.camera, rect, [0, 1, 2].map(|k| c[k] + r.inward[k] * span))) else { return };
    let along = e - a;
    if along.length_sq() < 4.0 {
        return;
    }
    if let Some(drag) = v.tools.drag.as_mut() {
        drag.raw = (drag.raw + f64::from(d.dot(along) / along.length_sq()) * span).clamp(-span, span);
        // The port's face holds it within a snap's pixels.
        let near = f64::from(SNAP_PX / along.length()) * span;
        drag.deembed = if drag.raw.abs() < near { 0.0 } else { drag.raw };
    }
}

/// The drag let go: the port's de-embedding set where it was left (rounded in display units).
pub fn end_drag(app: &EmcraftApp, v: &mut View3d, out: &mut Actions) {
    let Some(d) = v.tools.drag.take() else { return };
    let l = app.session.display().length;
    let at = (l.from_si(d.deembed) * 1e3).round() / 1e3;
    let now = app.session.model().ports.get(d.port).map(|p| l.from_si(p.deembed));
    if now.is_some_and(|n| (n - at).abs() > 0.0) {
        out.push(Action::run("model.port.edit", json!({"name": d.name, "deembed": at})));
    }
}

/// Draws the tools over the picture: the face under the pointer, its snap, the points picked,
/// the measurement and the selected port's reference plane.
pub fn draw(app: &EmcraftApp, v: &View3d, p: &egui::Painter, rect: Rect) {
    let t = &Tokens::get();
    let cam = &v.camera;
    let at = |q: [f64; 3]| screen(cam, rect, q);
    let l = app.session.display().length;
    arrows(app.session.model(), p, cam, rect);
    if let Some(r) = ref_plane(app, v) {
        reference(p, &r, cam, rect, l, t);
    }
    if let (Some(tool), Some(h)) = (v.tools.tool, &v.tools.hover) {
        let ok = !matches!(h.port, Some(PortTarget::No(_)));
        let ink = if ok { t.accent } else { t.text_dim };
        if let Some(scene) = v.shown()
            && h.face.tris.len() <= 20_000
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
                tip(p, q, &port_text(app, h.port), ok, t);
            }
        } else if let Some(q) = at(h.snap.at) {
            marker(p, q, h.snap.kind, t);
            scenario::publish_handle(Fixed::View3dSnap, q);
        }
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
    if let Some((picks, reply)) = &v.tools.measured {
        dimension(p, cam, rect, picks, reply, t);
    }
    // The end faces a port may take, named for scenarios and agents.
    if v.tools.tool == Some(Tool::Port) {
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

/// What a port-tool click does here, in a few words.
fn port_text(app: &EmcraftApp, target: Option<PortTarget>) -> String {
    let m = app.session.model();
    match target {
        Some(PortTarget::Existing(i)) => m.ports.get(i).map_or_else(String::new, |p| trf("Port {name}", args!(name = p.name))),
        Some(PortTarget::End(_)) => format!("+ {}", trf("Port {name}", args!(name = next_name(m)))),
        Some(PortTarget::Inside { .. }) => format!("+ {}…", trf("Port {name}", args!(name = next_name(m)))),
        Some(PortTarget::No(why)) => tr(why).to_owned(),
        None => String::new(),
    }
}

/// A short text on a pill at `at`.
fn tip(p: &egui::Painter, at: Pos2, text: &str, ok: bool, t: &Tokens) {
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
fn marker(p: &egui::Painter, q: Pos2, kind: SnapKind, t: &Tokens) {
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
fn dimension(p: &egui::Painter, cam: &Camera, rect: Rect, [a, b]: &[Snap; 2], reply: &Value, t: &Tokens) {
    let at = |q: [f64; 3]| screen(cam, rect, q);
    let unit = reply.get("unit").and_then(Value::as_str).unwrap_or("");
    let num = |k: &str| reply.get(k).and_then(Value::as_f64);
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
    let distance = num("distance").map_or_else(|| "—".to_owned(), |x| format!("{} {unit}", trimmed(x, 4)));
    tip(p, sa + d * 0.5 + vec2(0.0, -14.0), &distance, true, t);
    // The card: the distance, its components, the gap and the angle.
    let delta = reply.get("delta").and_then(Value::as_array);
    let comp = |k: usize| delta.and_then(|v| v.get(k)).and_then(Value::as_f64).map_or_else(|| "—".to_owned(), |x| trimmed(x, 4));
    let mut lines =
        vec![(distance, t.text), (format!("Δx {}", comp(0)), t.axis_x), (format!("Δy {}", comp(1)), t.axis_y), (format!("Δz {}", comp(2)), t.axis_z)];
    if let Some(g) = num("gap") {
        lines.push((trf("gap {gap} {unit}", args!(gap = trimmed(g, 4), unit = unit)), t.text));
    }
    if let Some(angle) = num("angle") {
        lines.push((format!("∠ {}°", trimmed(angle, 2)), t.text));
    }
    let font = FontId::proportional(t.font_small);
    let galleys: Vec<_> = lines.into_iter().map(|(s, c)| p.layout_no_wrap(s, font.clone(), c)).collect();
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
fn reference(p: &egui::Painter, r: &RefPlane, cam: &Camera, rect: Rect, l: emcraft_model::units::Length, t: &Tokens) {
    let at = |q: [f64; 3]| screen(cam, rect, q);
    let q = r.at();
    let pts: Vec<Pos2> = q.iter().filter_map(|c| at(*c)).collect();
    if pts.len() == 4 {
        p.add(egui::Shape::convex_polygon(pts.clone(), t.accent.gamma_multiply(0.18), Stroke::NONE));
        for k in 0..4 {
            dashed(p, pts[k], pts[(k + 1) % 4], Stroke::new(1.6, t.accent));
        }
    }
    let face = r.face;
    let fc = [0, 1, 2].map(|k| 0.25 * (face[0][k] + face[1][k] + face[2][k] + face[3][k]));
    let (Some(a), Some(g)) = (at(fc), at(r.centre())) else { return };
    p.line_segment([a, g], Stroke::new(1.2, t.accent));
    p.circle_filled(g, 6.0, t.panel);
    p.circle_stroke(g, 6.0, Stroke::new(2.0, t.accent));
    p.circle_filled(g, 2.5, t.accent);
    scenario::publish_handle(Fixed::View3dRefPlane, g);
    let text = format!("{} {}", trimmed(l.from_si(r.deembed), 3), l.symbol());
    let gal = p.layout_no_wrap(text, FontId::proportional(t.font_small), t.text);
    let box_ = Rect::from_min_size(g + vec2(10.0, -gal.size().y - 8.0), gal.size() + vec2(10.0, 4.0));
    p.rect_filled(box_, 4.0, t.panel.gamma_multiply(0.9));
    p.galley(box_.min + vec2(5.0, 2.0), gal, t.text);
}

/// An arrow (`a` to `b` on screen) with its head at `b`.
fn arrow(p: &egui::Painter, a: Pos2, b: Pos2, s: Stroke) {
    let d = b - a;
    let len = d.length();
    if len < 4.0 {
        return;
    }
    let u = d / len;
    let n = vec2(-u.y, u.x);
    let head = (len * 0.35).min(14.0);
    // A dark rim keeps it off a background of its own colour (a port's face, a field plot).
    let rim = Stroke::new(s.width + 3.0, crate::theme::color32(crate::theme::scene().edge));
    let tip = [b, b - u * head + n * head * 0.5, b - u * head - n * head * 0.5];
    let shaft = b - u * head * 0.8;
    p.line_segment([a, shaft], rim);
    p.add(egui::Shape::convex_polygon(tip.to_vec(), rim.color, rim));
    p.line_segment([a, shaft], s);
    p.add(egui::Shape::convex_polygon(tip.to_vec(), s.color, Stroke::NONE));
}

/// Which way each port sends its wave: from a waveguide port's face into the model, along a
/// lumped port's gap from `from` to `to`.
fn arrows(m: &emcraft_model::Model, p: &egui::Painter, cam: &Camera, rect: Rect) {
    let ink = crate::theme::color32(crate::theme::scene().port);
    let s = Stroke::new(3.0, ink);
    let span = m.bounds().map_or(0.0, |b| (b.max.z - b.min.z).abs());
    for port in &m.ports {
        let Some(q) = crate::scene3d::port_quad(m, port) else { continue };
        let c = [0, 1, 2].map(|k| 0.25 * (q[0][k] + q[1][k] + q[2][k] + q[3][k]));
        let size = (0..3).map(|k| (q[2][k] - q[0][k]).abs()).fold(0.0, f64::max);
        let len = (0.6 * size).min(0.3 * span).max(1e-9);
        let n = crate::scene3d::port_normal(port);
        let tip = [0, 1, 2].map(|k| c[k] + n[k] * len);
        if let (Some(a), Some(b)) = (screen(cam, rect, c), screen(cam, rect, tip)) {
            arrow(p, a, b, s);
        }
    }
    for lp in &m.lumped {
        let (f, t) = ([lp.from.x, lp.from.y, lp.from.z], [lp.to.x, lp.to.y, lp.to.z]);
        if let (Some(a), Some(b)) = (screen(cam, rect, f), screen(cam, rect, t)) {
            arrow(p, a, b, s);
        }
    }
}

/// The selected waveguide port's reference plane as a number under the selection card: Enter
/// sets it, 0 puts it back on the port's face (`model.port.edit {deembed}`).
pub fn ref_field(app: &EmcraftApp, v: &mut View3d, ui: &mut egui::Ui, at: Pos2, out: &mut Actions) {
    let Some(r) = ref_plane(app, v) else {
        v.tools.ref_text = None;
        return;
    };
    let t = &Tokens::get();
    let l = app.session.display().length;
    let now = (l.from_si(r.deembed) * 1e3).round() / 1e3;
    // The text follows the port (and a drag) until it is edited.
    let fresh = match &v.tools.ref_text {
        Some((port, _, seen)) => *port != r.port || *seen != now,
        None => true,
    };
    if fresh {
        v.tools.ref_text = Some((r.port, trimmed(now, 3), now));
    }
    let Some((_, text, _)) = v.tools.ref_text.as_mut() else { return };
    let busy = app.busy();
    let area = Rect::from_center_size(at + vec2(0.0, 14.0), vec2(220.0, 26.0));
    ui.painter().rect_filled(area, 6.0, t.panel.gamma_multiply(0.92));
    let mut child =
        ui.new_child(egui::UiBuilder::new().max_rect(area.shrink2(vec2(8.0, 2.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
    crate::ui_kit::small(&mut child, "Ref. plane", t.text_dim);
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
    if zero.hint("Back to the port's face").clicked() {
        set(0.0, out);
    }
}
