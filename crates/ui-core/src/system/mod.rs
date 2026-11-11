//! What the window shows of a system's chain wherever it shows it — the System view's canvas,
//! the browser and the ribbon: the kinds of stage and the settings a new one starts from, a
//! stage's kind and numbers in a line, and the chain's reading at a stage's output.

pub mod band;
pub mod budget;
pub mod card;
pub mod gesture;
pub mod layout;

use emcraft_model::system::{DataSource, Part, Stage};
use serde_json::{Value, json};

use crate::lang::Text;

/// The kinds of stage, each with its name and what it is.
pub const KINDS: [(&str, Text, Text); 6] = [
    ("amplifier", t!("Amplifier"), t!("An ideal amplifier: its gain, noise figure, OIP3 and P1dB")),
    ("attenuator", t!("Attenuator"), t!("A matched attenuator: its loss")),
    ("data", t!("S-parameters"), t!("A two-port's S: the sweep or an imported Touchstone file (a filter, a measured LNA)")),
    ("line", t!("Line"), t!("A transmission line: its impedance, length and losses")),
    ("series", t!("Series element"), t!("R, L and C in series along the line")),
    ("shunt", t!("Shunt element"), t!("R, L and C across the line")),
];

/// The settings a new stage of `kind` starts from.
pub fn starting(kind: &str) -> Value {
    match kind {
        "amplifier" => json!({"type": kind, "gain": 20, "noise_figure": 1}),
        "attenuator" => json!({"type": kind, "loss": 3}),
        "data" => json!({"type": kind, "data": "sweep"}),
        "line" => json!({"type": kind, "impedance": 50, "length": 10}),
        _ => json!({"type": kind, "capacitance": 1}),
    }
}

/// A part's kind as the window names it.
pub fn kind_name(part: &Part) -> Text {
    let id = part.kind();
    KINDS.iter().find(|(k, _, _)| *k == id).map_or(t!("Amplifier"), |(_, name, _)| *name)
}

/// A stage's numbers in a line: an amplifier's gain and noise figure, an attenuator's loss, a
/// data stage's source, a line's impedance, an element's parts.
pub fn summary(stage: &Stage) -> String {
    let g = |x: f64| format!("{x:.1} dB");
    match &stage.part {
        Part::Amplifier { gain, noise_figure, .. } => format!("G {}  NF {}", g(*gain), g(*noise_figure)),
        Part::Attenuator { loss } => format!("−{}", g(*loss)),
        Part::Data { data: DataSource::Sweep, .. } => "S: sweep".to_owned(),
        Part::Data { data: DataSource::Measurement { name }, .. } => format!("S: {}", emcraft_engine::text::clip(name, 16)),
        Part::Line { impedance, .. } => format!("{impedance:.0} Ω"),
        Part::Element { element, .. } => {
            let parts = [(element.resistance, 1.0, "Ω"), (element.inductance, 1e9, "nH"), (element.capacitance, 1e12, "pF")];
            parts.iter().filter(|(v, _, _)| *v > 0.0).map(|(v, scale, unit)| format!("{:.3} {unit}", v * scale)).collect::<Vec<_>>().join(" ")
        }
    }
}

/// A new stage of `kind` put at `at` on the canvas, off the chain until linked.
///
/// ```
/// let v = emcraft_ui_core::system::placed("attenuator", [40.0, 80.0]);
/// assert_eq!((v["type"].as_str(), v["linked"].as_bool(), v["at"][1].as_f64()), (Some("attenuator"), Some(false), Some(80.0)));
/// ```
pub fn placed(kind: &str, at: [f64; 2]) -> Value {
    let base = starting(kind).as_object().cloned().unwrap_or_default();
    Value::Object(base.into_iter().chain([("at".to_owned(), json!(at)), ("linked".to_owned(), json!(false))]).collect())
}

/// The stage of the chain named `name`, as the budget at its frequency reads it: the chain's
/// gain and noise figure at its output (dB).
pub fn reading(s: &emcraft_engine::Session, name: &str) -> Option<(Option<f64>, Option<f64>)> {
    let r = s.results().system.as_deref()?;
    let k = r.stages.iter().position(|(n, _)| n == name)?;
    let row = r.nearest(r.at)?.rows.get(k)?;
    Some((row.gain_db, row.nf_db))
}
