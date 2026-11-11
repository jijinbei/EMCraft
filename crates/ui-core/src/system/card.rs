//! What a node's card on a system's canvas holds, as data each window draws: its title, its rows
//! of settings (a number typed in place, a name chosen) and the command each runs when changed,
//! the chain's reading at its output; and a number field's text, shown and typed.

use emcraft_engine::Session;
use emcraft_model::system::{DataSource, Part, Stage};
use serde_json::{Value, json};

use super::gesture::Command;
use super::layout::{Kind, NodeBox};
use crate::lang::{tr, trf};

/// What a row edits.
#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    /// A number in `unit`, which may be left empty when `optional`.
    Number { value: Option<f64>, unit: String, optional: bool },
    /// A name among `options` (each its value and its text).
    Choice { options: Vec<(String, String)>, current: String },
}

/// A row of a card: its label, the parameter it sets and what it edits.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub label: String,
    pub key: &'static str,
    pub value: Edit,
}

fn number(label: &str, key: &'static str, value: Option<f64>, unit: &str, optional: bool) -> Row {
    Row { label: label.to_owned(), key, value: Edit::Number { value, unit: unit.to_owned(), optional } }
}

/// The measurements of `ports` ports, as choices.
fn measured(s: &Session, ports: usize) -> impl Iterator<Item = (String, String)> + '_ {
    s.views().measured.iter().filter(move |x| x.data.outputs() == ports).map(|x| (x.name.clone(), x.name.clone()))
}

/// A stage's rows: its own settings (the rest in its dialog).
fn stage_rows(s: &Session, stage: &Stage) -> Vec<Row> {
    let v = emcraft_engine::system::stage_params(s, &stage.part);
    let n = |label: &str, key: &'static str, unit: &str, optional: bool| number(label, key, v[key].as_f64(), unit, optional);
    let lu = s.display().length.symbol();
    match &stage.part {
        Part::Amplifier { .. } => {
            vec![n("G", "gain", "dB", false), n("NF", "noise_figure", "dB", false), n("OIP3", "oip3", "dBm", true), n("P1dB", "p1db", "dBm", true)]
        }
        Part::Attenuator { .. } => vec![n("L", "loss", "dB", false)],
        Part::Line { .. } => vec![n("Z₀", "impedance", "Ω", false), n("ℓ", "length", lu, false), n("εr", "eps_r", "", false)],
        Part::Element { .. } => vec![n("R", "resistance", "Ω", false), n("L", "inductance", "nH", false), n("C", "capacitance", "pF", false)],
        Part::Data { data, .. } => {
            let current = match data {
                DataSource::Sweep => "sweep".to_owned(),
                DataSource::Measurement { name } => name.clone(),
            };
            let options = std::iter::once(("sweep".to_owned(), tr(t!("the sweep")).to_owned())).chain(measured(s, 2)).collect();
            vec![Row { label: "S".into(), key: "data", value: Edit::Choice { options, current } }]
        }
    }
}

/// The rows `node`'s card shows.
///
/// ```
/// use emcraft_ui_core::geom::Point;
/// use emcraft_ui_core::system::{card, layout::Layout};
/// let s = emcraft_engine::Session::new();
/// let layout = Layout::of(&s.model().system, Point::ORIGIN, None);
/// // The source's antenna, temperature and signal.
/// let keys: Vec<&str> = card::rows(&s, &layout.nodes[0]).iter().map(|r| r.key).collect();
/// assert_eq!(keys, ["antenna", "temperature", "power"]);
/// ```
pub fn rows(s: &Session, node: &NodeBox) -> Vec<Row> {
    let m = s.model();
    let fu = s.display().frequency;
    match node.kind {
        Kind::Source => {
            let src = &m.system.source;
            let options = std::iter::once((String::new(), format!("{} Ω", m.system.receiver.impedance))).chain(measured(s, 1)).collect();
            let current = src.antenna.clone().unwrap_or_default();
            vec![
                Row { label: tr(t!("antenna")).to_owned(), key: "antenna", value: Edit::Choice { options, current } },
                number("T", "temperature", Some(src.temperature), "K", false),
                number(tr(t!("signal")), "power", src.power, "dBm", true),
            ]
        }
        Kind::Receiver => {
            let r = &m.system.receiver;
            vec![
                number("B", "bandwidth", r.bandwidth.map(|b| fu.from_si(b)), fu.symbol(), true),
                number("SNR", "snr", Some(r.snr), "dB", false),
                number("Z₀", "impedance", Some(r.impedance), "Ω", false),
            ]
        }
        Kind::Stage(i) => m.system.stages.get(i).map_or_else(Vec::new, |st| stage_rows(s, st)),
    }
}

/// The command a row's new value runs: the source's, the receiver's or the stage's settings.
pub fn command(node: &NodeBox, key: &str, value: Value) -> Command {
    match node.kind {
        Kind::Source => Command { id: "system.source", params: json!({ key: value }) },
        Kind::Receiver => Command { id: "system.receiver", params: json!({ key: value }) },
        Kind::Stage(_) => Command { id: "system.stage.edit", params: json!({"stage": node.name, key: value}) },
    }
}

/// What the chain reads at `node`'s output: a stage's gain and noise figure, the receiver's
/// sensitivity.
pub fn reading(s: &Session, node: &NodeBox) -> Option<String> {
    let f = |x: Option<f64>| x.map_or_else(|| "—".to_owned(), |v| format!("{v:.2}"));
    match node.kind {
        Kind::Source => None,
        Kind::Receiver => {
            let r = s.results().system.as_deref()?;
            let sens = r.nearest(r.at)?.sensitivity_dbm?;
            Some(trf(t!("sensitivity {s} dBm"), crate::args!(s = format!("{sens:.1}"))))
        }
        Kind::Stage(_) => {
            let (g, nf) = super::reading(s, &node.name)?;
            Some(trf(t!("total {gain} dB · NF {nf} dB"), crate::args!(gain = f(g), nf = f(nf))))
        }
    }
}

/// The card's title: the source, the receiver, or a stage's name and kind.
pub fn title(s: &Session, node: &NodeBox) -> String {
    match (node.kind, node.kind_stage(s)) {
        (Kind::Source, _) => tr(t!("Source")).to_owned(),
        (Kind::Receiver, _) => tr(t!("Receiver")).to_owned(),
        (Kind::Stage(_), Some(st)) => format!("{}  ·  {}", emcraft_engine::text::clip(&st.name, 18), tr(super::kind_name(&st.part))),
        (Kind::Stage(_), None) => node.name.clone(),
    }
}

impl NodeBox {
    /// The stage it is, in `s`'s chain.
    fn kind_stage<'a>(&self, s: &'a Session) -> Option<&'a Stage> {
        match self.kind {
            Kind::Stage(i) => s.model().system.stages.get(i),
            Kind::Source | Kind::Receiver => None,
        }
    }
}

/// A number field's text as the model has it.
pub fn shown(value: Option<f64>) -> String {
    value.map_or_else(String::new, |v| format!("{}", (v * 1e6).round() / 1e6))
}

/// The value a number field typed as `typed` commits, when it changed from `value`: the number,
/// or none for an empty field when `optional`; nothing when unchanged or not a number.
///
/// ```
/// use emcraft_ui_core::system::card::committed;
/// assert_eq!(committed(" 2.5 ", Some(1.0), false), Some(Some(2.5)));
/// assert_eq!(committed("", Some(1.0), true), Some(None));
/// assert_eq!((committed("1", Some(1.0), false), committed("abc", None, false), committed("inf", None, false)), (None, None, None));
/// ```
pub fn committed(typed: &str, value: Option<f64>, optional: bool) -> Option<Option<f64>> {
    let typed = typed.trim();
    let parsed = if typed.is_empty() && optional { Some(None) } else { typed.parse::<f64>().ok().filter(|v| v.is_finite()).map(Some) };
    parsed.filter(|_| typed != shown(value))
}
