//! The 3D view's tools, worked by pointing at the model, whatever window draws them:
//! - **Face port**: the face under the pointer lit; a click puts a port there or opens its
//!   dialog ([`port`]).
//! - **Gap port**: two clicks, each snapped ([`snap`]), give a lumped port's `from` and `to`
//!   (`model.port.lumped`).
//! - **Measure**: two clicks, snapped the same way, measured by `geometry.measure` (a face brings
//!   its normal, an edge its direction): the distance, Δx, Δy, Δz and the gap or angle.
//! - A selected port's **reference plane**, dragged along its line by its grip ([`reference`]).
//!
//! Every change goes through a command ([`Outcome`]); a tool never edits the model itself.

pub mod port;
pub mod reference;
pub mod snap;

use emcraft_model::units::Length;
use serde_json::{Value, json};

use crate::glyph::Glyph;
use crate::pick::Pick;
use crate::scene3d::faces::Face;
pub use port::PortTarget;
pub use reference::{GRIP_PX, RefDrag, RefPlane};
pub use snap::{FaceCache, Sight, screen};

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
emcraft_macros::named_serde!(Tool);

impl Tool {
    pub fn glyph(self) -> Glyph {
        match self {
            Tool::Port => Glyph::PickFace,
            Tool::Lumped => Glyph::PickGap,
            Tool::Measure => Glyph::Ruler,
        }
    }

    /// What it does, on hover.
    pub fn hint(self) -> crate::lang::Text {
        match self {
            Tool::Port => t!("Click a flat face: a port there"),
            Tool::Lumped => t!("Click two points: a lumped port between them"),
            Tool::Measure => t!("Click two points, edges or faces: the distance (Esc clears)"),
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
emcraft_macros::named_serde!(SnapKind);

/// A picked point (m), what it snapped to and the edge's direction or the face's normal.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Snap {
    pub at: [f64; 3],
    pub kind: SnapKind,
    pub dir: Option<[f64; 3]>,
}

/// What the pointer is over while a tool is on.
#[derive(Clone, Debug, PartialEq)]
pub struct Hover {
    pub face: Face,
    pub snap: Snap,
    pub port: Option<PortTarget>,
}

/// A measurement: its picks and `geometry.measure`'s reply.
#[derive(Clone, Debug, PartialEq)]
pub struct Measured {
    pub picks: [Snap; 2],
    pub reply: Value,
}

/// What a tool's click asks the window to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// Select a part (a port's own face).
    Select(Pick),
    /// Run a command with its parameters.
    Run(&'static str, Value),
    /// Open a command's dialog filled with these parameters.
    Dialog(&'static str, Value),
    /// Say why nothing happens.
    Refused(crate::lang::Text),
    /// Measure between two picks (`geometry.measure` with [`measure_params`]).
    Measure([Snap; 2]),
}

/// The tools' state: what the pointer is over, the points clicked so far (a gap's first end, a
/// measurement's first pick), the last measurement and the reference plane's drag.
#[derive(Clone, Debug, Default)]
pub struct Tools {
    pub hover: Option<Hover>,
    faces: FaceCache,
    pub picks: Vec<Snap>,
    pub measured: Option<Measured>,
    pub drag: Option<RefDrag>,
}

impl Tools {
    /// What the tool had picked dropped, `tool` on next.
    pub fn reset(&mut self, tool: Option<Tool>) {
        self.picks.clear();
        self.hover = None;
        if tool != Some(Tool::Measure) {
            self.measured = None;
        }
    }

    /// Escape: what was picked goes. Whether nothing was picked (the tool goes off then).
    pub fn escape(&mut self) -> bool {
        let nothing = self.picks.is_empty() && self.measured.is_none();
        self.picks.clear();
        self.measured = None;
        nothing
    }

    /// What the pointer at `at` (none: away, or the view moving) is over for `tool` on `m`.
    pub fn hovered(&mut self, m: &emcraft_model::Model, sight: Option<&Sight>, tool: Option<Tool>, at: Option<crate::geom::Point>) {
        let mut faces = std::mem::take(&mut self.faces);
        self.hover = match (tool, sight, at) {
            (Some(tool), Some(sight), Some(at)) => snap::over(m, sight, tool, at, &mut faces),
            _ => None,
        };
        self.faces = faces;
    }

    /// A click with `tool` on, over what [`Tools::hover`] holds: what it asks for (a point picked
    /// is kept until its pair comes), lengths in `l`.
    pub fn clicked(&mut self, tool: Tool, m: &emcraft_model::Model, l: Length) -> Option<Outcome> {
        let hover = self.hover.clone()?;
        let port = |end: emcraft_model::End| json!({"name": next_name(m), "end": end.id()});
        let on_face = |end, axis: emcraft_model::Axis, face: [f64; 5]| {
            let mut p = port(end);
            p["face"] = json!(face.map(|c| neat(l, c)));
            p["axis"] = json!(axis.id());
            p
        };
        match tool {
            Tool::Port => hover.port.map(|target| match target {
                PortTarget::Existing(i) => Outcome::Select(Pick::Port(i)),
                PortTarget::End(end) => Outcome::Run("model.port.create", port(end)),
                PortTarget::Inside { axis, face, end } => Outcome::Dialog("model.port.create", on_face(end, axis, face)),
                PortTarget::Opening { axis, face, end } => Outcome::Run("model.port.create", on_face(end, axis, face)),
                PortTarget::No(why) => Outcome::Refused(why),
            }),
            Tool::Lumped => {
                self.picks.push(hover.snap);
                let [a, b] = self.picks[..] else { return None };
                self.picks.clear();
                Some(Outcome::Run("model.port.lumped", json!({"name": next_name(m), "from": shown(l, a.at), "to": shown(l, b.at)})))
            }
            Tool::Measure => {
                self.measured = None;
                self.picks.push(hover.snap);
                let [a, b] = self.picks[..] else { return None };
                self.picks.clear();
                Some(Outcome::Measure([a, b]))
            }
        }
    }

    /// The state for `ui.inspect`, lengths in `l`.
    pub fn inspect(&self, tool: Option<Tool>, l: Length) -> Value {
        json!({
            "tool": tool.map(Tool::id),
            "pointer": self.hover.as_ref().map(|h| {
                let mut v = snap_inspect(&h.snap, l);
                v["port"] = h.port.map_or(Value::Null, |p| json!(p.id()));
                v
            }),
            "picks": self.picks.iter().map(|p| snap_inspect(p, l)).collect::<Vec<_>>(),
            "measure": self.measured.as_ref().map(|m| m.reply.clone()),
            "drag": self.drag.as_ref().map(|d| d.inspect(l)),
        })
    }
}

/// A snap as `ui.inspect` gives it, lengths in `l`.
fn snap_inspect(s: &Snap, l: Length) -> Value {
    json!({"kind": s.kind.id(), "at": shown(l, s.at)})
}

/// `v` with up to `digits` decimals, trailing zeros dropped.
///
/// ```
/// assert_eq!(emcraft_ui_core::tools::trimmed(2.50, 3), "2.5");
/// assert_eq!(emcraft_ui_core::tools::trimmed(3.0, 2), "3");
/// ```
pub fn trimmed(v: f64, digits: usize) -> String {
    let s = format!("{v:.digits$}");
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_owned() } else { s }
}

/// The smallest free port name: 1, 2, …
pub fn next_name(m: &emcraft_model::Model) -> String {
    (1..=10_000usize).map(|n| n.to_string()).find(|n| !m.has_port_named(n)).unwrap_or_else(|| "new".to_owned())
}

/// A length (m) in display units, to ten significant digits: what a pick's rounding leaves
/// (23.000000000000004 mm) reads as it was meant (23).
fn neat(l: Length, x: f64) -> f64 {
    let v = l.from_si(x);
    format!("{v:.9e}").parse().unwrap_or(v)
}

/// A point (m) as `[x, y, z]` in display units.
fn shown(l: Length, p: [f64; 3]) -> Value {
    json!(p.map(|c| neat(l, c)))
}

/// `geometry.measure`'s parameters for two picks, lengths in `l`: a face brings its normal, an
/// edge its direction.
pub fn measure_params(l: Length, [a, b]: [Snap; 2]) -> Value {
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

/// What a port-tool click does on `target`, in a few words.
pub fn port_text(m: &emcraft_model::Model, target: Option<PortTarget>) -> String {
    use crate::lang::{args, tr, trf};
    match target {
        Some(PortTarget::Existing(i)) => m.ports.get(i).map_or_else(String::new, |p| trf(t!("Port {name}"), args!(name = p.name))),
        Some(PortTarget::End(_) | PortTarget::Opening { .. }) => format!("+ {}", trf(t!("Port {name}"), args!(name = next_name(m)))),
        Some(PortTarget::Inside { .. }) => format!("+ {}…", trf(t!("Port {name}"), args!(name = next_name(m)))),
        Some(PortTarget::No(why)) => tr(why).to_owned(),
        None => String::new(),
    }
}

/// What a line of the measurement's card is drawn in: the text's colour or an axis's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ink {
    Text,
    Axis(usize),
}

/// The measurement's card from `geometry.measure`'s reply: the distance, Δx, Δy, Δz (each in its
/// axis's colour), the gap and the angle when the picks define them.
pub fn measure_card(reply: &Value) -> Vec<(String, Ink)> {
    use crate::lang::{args, trf};
    let unit = reply.get("unit").and_then(Value::as_str).unwrap_or("");
    let num = |k: &str| reply.get(k).and_then(Value::as_f64);
    let delta = reply.get("delta").and_then(Value::as_array);
    let comp = |k: usize| delta.and_then(|v| v.get(k)).and_then(Value::as_f64).map_or_else(|| "—".to_owned(), |x| trimmed(x, 4));
    let distance = (measure_distance(reply), Ink::Text);
    let axes = ["Δx", "Δy", "Δz"].into_iter().enumerate().map(|(k, d)| (format!("{d} {}", comp(k)), Ink::Axis(k)));
    let gap = num("gap").map(|g| (trf(t!("gap {gap} {unit}"), args!(gap = trimmed(g, 4), unit = unit)), Ink::Text));
    let angle = num("angle").map(|a| (format!("∠ {}°", trimmed(a, 2)), Ink::Text));
    std::iter::once(distance).chain(axes).chain(gap).chain(angle).collect()
}

/// The measured distance with its unit, as written on the dimension.
pub fn measure_distance(reply: &Value) -> String {
    let unit = reply.get("unit").and_then(Value::as_str).unwrap_or("");
    reply.get("distance").and_then(Value::as_f64).map_or_else(|| "—".to_owned(), |x| format!("{} {unit}", trimmed(x, 4)))
}
