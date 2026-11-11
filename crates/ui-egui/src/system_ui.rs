//! What the window shows of a system's chain wherever it shows it — the System view's canvas,
//! the browser and the ribbon: the kinds of stage and the settings a new one starts from, a
//! stage's kind and numbers in a line, the item a stage's row edits and deletes, and the chain's
//! reading at a stage's output.

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
            let mut parts = Vec::new();
            if element.resistance > 0.0 {
                parts.push(format!("{:.3} Ω", element.resistance));
            }
            if element.inductance > 0.0 {
                parts.push(format!("{:.3} nH", element.inductance * 1e9));
            }
            if element.capacitance > 0.0 {
                parts.push(format!("{:.3} pF", element.capacitance * 1e12));
            }
            parts.join(" ")
        }
    }
}

/// The stage `name` as an item of the model, which its row edits and deletes.
pub fn item(name: &str) -> crate::browser::Item {
    crate::browser::Item::new("system.stage.edit", "system.stage.delete", "stage", json!(name))
}

/// The stage of the chain named `name`, as the budget at its frequency reads it: the chain's
/// gain and noise figure at its output (dB).
pub fn reading(s: &emcraft_engine::Session, name: &str) -> Option<(Option<f64>, Option<f64>)> {
    let r = s.results().system.as_deref()?;
    let k = r.stages.iter().position(|(n, _)| n == name)?;
    let row = r.nearest(r.at)?.rows.get(k)?;
    Some((row.gain_db, row.nf_db))
}
