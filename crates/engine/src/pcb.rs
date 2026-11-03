//! PCB commands: the board kept in the project's history, geometric screening of its routes and
//! an explicitly approximate quasi-TEM estimate of a route's signal integrity. No volume mesh, no
//! field solution and no EMC verdict is implied.

use std::io::Read;

use emcraft_model::pcb::{Board, Copper, Layer, Segment};
use emcraft_model::units::{Frequency, Length};
use emcraft_theory::pcb::{Finding, Report, Section};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{EngineError, Params, Result, Session, Step};

pub use emcraft_theory::pcb::{Code, Pair, near};

emcraft_macros::named_enum! {
    /// PCB workspace stages, saved with the project by `view.pcb`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Stage {
        Board = "board" => "Prepare board",
        Net = "net" => "Select route",
        Settings = "settings" => "Set conditions",
        Inspect = "inspect" => "Run diagnosis",
        Results = "results" => "Review results",
    }
}
emcraft_macros::named_serde!(Stage);

emcraft_macros::named_enum! {
    /// What the coupling schematic between two nets shows.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
    pub enum CouplingMode {
        #[default]
        Electric = "electric" => "Electric coupling",
        Magnetic = "magnetic" => "Magnetic coupling",
        Return = "return" => "Return reference",
    }
}

emcraft_macros::named_serde!(CouplingMode);

/// The coupling schematic's settings, saved with the project by `view.pcb`.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Illustration {
    /// The receiving trace (the route is the noise source).
    pub victim: Option<String>,
    /// The mechanism the board's 3D view illustrates, none for the whole board.
    pub illustrated: Option<CouplingMode>,
    /// Explanatory motion of the illustration (not physical time).
    pub playing: bool,
}

/// Last verified PCB conditions; frequency is SI hertz, impedance is ohms.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Conditions {
    pub ground: String,
    pub reference: f64,
    pub stop_hz: f64,
}

/// The current PCB stage: a board just loaded starts on its stackup.
///
/// ```
/// use emcraft_engine::Session;
/// use emcraft_engine::pcb::{Stage, stage};
/// assert_eq!(stage(&Session::new()), Stage::Board);
/// ```
pub fn stage(s: &Session) -> Stage {
    match s.model.pcb {
        Some(_) => s.views.pcb_stage.unwrap_or(Stage::Board),
        None => Stage::Board,
    }
}

/// `view.pcb`: moves through the PCB workflow or selects a routed net, calculating nothing.
pub fn view(s: &mut Session, p: &Params) -> Result<Value> {
    let stage = p.pick::<Stage>("stage").optional()?.unwrap_or_else(|| stage(s));
    let net = p.string("net").optional()?;
    let board = s.model.pcb.as_ref();
    if stage != Stage::Board && board.is_none() {
        return Err(failed("load a PCB before continuing"));
    }
    if let Some(net) = net {
        if !board.is_some_and(|b| b.segments.iter().any(|seg| seg.net == net)) {
            return Err(p.error("net", "select a routed net in this board"));
        }
        s.views.pcb_net = Some(net.to_owned());
    }
    if matches!(stage, Stage::Settings | Stage::Inspect | Stage::Results) && s.views.pcb_net.is_none() {
        return Err(failed("select a routed net before continuing"));
    }
    let mut illustration = s.views.pcb_illustration.clone();
    if let Some(v) = p.string("victim").optional()? {
        if !board.is_some_and(|b| b.segments.iter().any(|t| t.net == v)) || s.views.pcb_net.as_deref() == Some(v) {
            return Err(p.error("victim", "choose a different routed net"));
        }
        illustration.victim = Some(v.to_owned());
    }
    if illustration.victim == s.views.pcb_net {
        illustration.victim = None;
    }
    // `mode` illustrates that mechanism; `coupling` false the whole board again, true the last
    // mechanism shown (the electric at first).
    let mode: Option<CouplingMode> = p.pick("mode").optional()?;
    illustration.illustrated = match (p.flag("coupling").optional()?, mode) {
        (Some(false), _) => None,
        (_, Some(m)) => Some(m),
        (Some(true), None) => Some(illustration.illustrated.unwrap_or_default()),
        (None, None) => illustration.illustrated,
    };
    if let Some(v) = p.flag("playing").optional()? {
        illustration.playing = v;
    }
    s.views.pcb_illustration = illustration;
    s.views.pcb_stage = Some(stage);
    Ok(json!({"stage": stage.id(), "net": s.views.pcb_net}))
}

/// An engine failure with this text.
fn failed(s: impl ToString) -> EngineError {
    EngineError::failed(s.to_string())
}

/// Replaces the model by `board` (checked), the PCB views starting afresh.
fn install(s: &mut Session, board: Board) -> Result<Value> {
    board.check()?;
    let reply = json!({"board": board.name, "segments": board.segments.len(), "layers": board.layers.len(), "notes": board.notes});
    s.model = emcraft_model::Model { pcb: Some(board), ..Default::default() };
    s.views.pcb_net = None;
    s.views.pcb_illustration = Illustration::default();
    s.views.pcb_stage = None;
    s.views.pcb_conditions = None;
    Ok(reply)
}

/// `pcb.load`: a board from its JSON, lengths (layers' z too) in the session's display unit.
pub fn load(s: &mut Session, p: &Params) -> Result<Value> {
    let mut board: Board = serde_json::from_value(p.raw("data").cloned().ok_or_else(|| failed("give data"))?).map_err(failed)?;
    board.map_lengths(|v| s.display.length.to_si(v));
    install(s, board)
}

/// Installs `board` and records it in the history as `pcb.load` with its data, so reopening the
/// project needs no source file.
fn embedded(s: &mut Session, board: Board) -> Result<Value> {
    let mut data = board.clone();
    data.map_lengths(|v| s.display.length.from_si(v));
    let reply = install(s, board)?;
    s.history.push(Step::new("pcb.load", json!({"data": data})));
    Ok(reply)
}

/// `pcb.import`: a KiCad board read from `path` (at most 64 MiB).
pub fn import(s: &mut Session, p: &Params) -> Result<Value> {
    let path = p.string("path").required()?;
    let file = std::fs::File::open(path).map_err(failed)?;
    let mut text = String::new();
    file.take(64 * 1024 * 1024 + 1).read_to_string(&mut text).map_err(failed)?;
    let layers = match p.raw("stackup") {
        None => None,
        Some(v) => {
            let mut ls: Vec<Layer> = serde_json::from_value(v.clone()).map_err(failed)?;
            for l in &mut ls {
                l.z = s.display.length.to_si(l.z);
            }
            Some(ls)
        }
    };
    let mut board = emcraft_io::pcb::read(&text, layers).map_err(failed)?;
    board.name = std::path::Path::new(path).file_name().and_then(|n| n.to_str()).unwrap_or("PCB").chars().take(128).collect();
    embedded(s, board)
}

/// `pcb.sample`: an original two-layer example, its return plane split or whole (no third-party
/// data).
pub fn sample(s: &mut Session, p: &Params) -> Result<Value> {
    let split = p.flag("split").or(true)?;
    let neighbour = p.flag("neighbour").or(true)?;
    let l = Length::Millimetre.from_si(p.positive_length("length").or(0.04)?);
    let w = Length::Millimetre.from_si(p.positive_length("width").or(0.0003)?);
    let gap = Length::Millimetre.from_si(p.positive_length("gap").or(0.00025)?);
    if gap > 5.0 {
        return Err(p.error("gap", "must be at most 5 mm"));
    }
    let h = Length::Millimetre.from_si(p.positive_length("height").or(0.00018)?);
    let er = p.number("er").or(4.2)?;
    if !(1.0..=1000.0).contains(&l) || !(0.001..=10.0).contains(&w) || !(0.001..=10.0).contains(&h) || !(1.0..=128.0).contains(&er) {
        return Err(failed("PCB example requires length 1..1000 mm, width/height 0.001..10 mm, er 1..128"));
    }
    let poly = |x0, x1| Copper { net: "GND".into(), layer: "B.Cu".into(), polygon: vec![[x0, 0.0], [x1, 0.0], [x1, 20.0], [x0, 20.0]] };
    let mut board = Board {
        name: "PCB return-path example".into(),
        layers: vec![
            Layer { name: "F.Cu".into(), z: 0.0, eps_below: er, loss_below: 0.0 },
            Layer { name: "B.Cu".into(), z: h, eps_below: 1.0, loss_below: 0.0 },
        ],
        segments: vec![
            Segment { id: "signal:0".into(), net: "DATA".into(), layer: "F.Cu".into(), start: [5.0, 10.0], end: [5.0 + l / 2.0, 10.0], width: w },
            Segment { id: "signal:1".into(), net: "DATA".into(), layer: "F.Cu".into(), start: [5.0 + l / 2.0, 10.0], end: [5.0 + l, 10.0], width: w },
            Segment {
                id: "clock:0".into(),
                net: "CLK".into(),
                layer: "F.Cu".into(),
                start: [5.0, 10.0 + w + gap],
                end: [5.0 + l, 10.0 + w + gap],
                width: w,
            },
        ],
        vias: vec![],
        copper: match split {
            true => vec![poly(0.0, 5.0 + 0.375 * l), poly(5.0 + 0.425 * l, l + 10.0)],
            false => vec![poly(0.0, l + 10.0)],
        },
        notes: vec!["Original routing example; lossless, zero-thickness conductors and no pads.".into()],
        region: None,
    };
    if !neighbour {
        board.segments.retain(|s| s.net != "CLK");
    }
    board.map_lengths(|v| Length::Millimetre.to_si(v));
    install(s, board)
}

/// `pcb.inspect`: screens a net's route and, for a simple outer microstrip, estimates its line.
pub fn inspect(s: &mut Session, p: &Params) -> Result<Value> {
    let ground = p.string("ground").or("GND")?;
    let board = s.model.pcb.as_ref().ok_or_else(|| failed("load a PCB with pcb.import, pcb.load or pcb.sample"))?;
    let net = p
        .string("net")
        .optional()?
        .or_else(|| s.views.pcb_net.as_deref().filter(|n| board.segments.iter().any(|seg| seg.net == *n)))
        .or_else(|| board.segments.iter().find(|seg| seg.net != ground).map(|seg| seg.net.as_str()))
        .ok_or_else(|| failed("load a PCB and select a signal net"))?;
    if net.chars().count() > 128 || ground.trim().is_empty() || ground.chars().count() > 128 {
        return Err(failed("net names must be at most 128 characters"));
    }
    let reference = p.number("reference").or(50.0)?;
    if !(0.1..=10000.0).contains(&reference) {
        return Err(p.error("reference", "must be 0.1..10000 ohms"));
    }
    let stop = p.frequency("stop").or(s.model.band.map_or(1e9, |b| b.stop.get()))?;
    if stop > 1e12 {
        return Err(p.error("stop", "must be at most 1 THz"));
    }
    let points = p.integer("points", 4..=128).or(64)?;
    let pairs = emcraft_theory::pcb::pairs(board, net, ground).map_err(failed)?;
    let report = emcraft_theory::pcb::inspect(board, net, ground).map_err(failed)?;
    let (length, frequency) = (s.display.length, s.display.frequency);
    let si = match report.model_available {
        true => Some(line(&report, stop, points, reference, frequency)?),
        false => None,
    };
    let reply = Inspection {
        net: net.to_owned(),
        ground: ground.to_owned(),
        board: board.name.clone(),
        length: length.from_si(report.length),
        length_unit: length.symbol().to_owned(),
        model_available: report.model_available,
        sections: report.sections.iter().map(|v| SectionRow::of(v, length)).collect(),
        findings: report.findings.iter().map(FindingRow::of).collect(),
        coupling_pairs: pairs.iter().map(|q| PairRow::of(q, length)).collect(),
        import_notes: board.notes.clone(),
        model: "lossless quasi-static transmission-line approximation; zero-thickness outer microstrip, infinite continuous ground; excludes bends, pads, vias, losses and coupling".to_owned(),
        return_current: "Geometry screening only: filled copper support is not a solved return-current density.".to_owned(),
        reference,
        stop: frequency.from_si(stop),
        stop_hz: stop,
        frequency_unit: frequency.symbol().to_owned(),
        si,
    };
    s.views.pcb_net = Some(net.to_owned());
    s.views.pcb_stage = Some(Stage::Results);
    s.views.pcb_conditions = Some(Conditions { ground: ground.to_owned(), reference, stop_hz: stop });
    serde_json::to_value(reply).map_err(|e| failed(e.to_string()))
}

/// `pcb.inspect`'s reply: the route's screening and, where the line model holds, its line.
/// Lengths in the session's display unit (`length_unit`), frequencies in its own
/// (`frequency_unit`), times in seconds. The window reads it back with `serde_json::from_value`.
///
/// ```
/// use emcraft_engine::{Session, pcb::Inspection};
/// use serde_json::json;
///
/// let mut s = Session::new();
/// s.execute("pcb.sample", &json!({})).unwrap();
/// let reply = s.execute("pcb.inspect", &json!({"net": "DATA", "stop": 5})).unwrap();
/// let report: Inspection = serde_json::from_value(reply).unwrap();
/// assert_eq!((report.net.as_str(), report.length_unit.as_str(), report.stop), ("DATA", "mm", 5.0));
/// // The line and its sweep only where the route meets the line model.
/// assert_eq!(report.si.is_some(), report.model_available);
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Inspection {
    pub net: String,
    pub ground: String,
    pub board: String,
    /// The route's length.
    pub length: f64,
    pub length_unit: String,
    /// Whether the route meets the line model (`si` is given).
    pub model_available: bool,
    pub sections: Vec<SectionRow>,
    pub findings: Vec<FindingRow>,
    pub coupling_pairs: Vec<PairRow>,
    pub import_notes: Vec<String>,
    /// What the estimate assumes.
    pub model: String,
    pub return_current: String,
    /// The reference impedance (Ω).
    pub reference: f64,
    /// The sweep's top frequency, in the display unit and in Hz.
    pub stop: f64,
    pub stop_hz: f64,
    pub frequency_unit: String,
    pub si: Option<Line>,
}

/// A cross-section of the route.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SectionRow {
    pub id: String,
    pub reference_layer: String,
    pub length: f64,
    pub height: f64,
    /// The ideal line's impedance (Ω) and delay (s).
    pub ideal_impedance: f64,
    pub ideal_delay_s: f64,
    /// The stretches (fractions along the segment, start and end) with no reference beneath.
    pub uncovered: Vec<[f64; 2]>,
}

impl SectionRow {
    fn of(v: &Section, length: Length) -> SectionRow {
        SectionRow {
            id: v.id.clone(),
            reference_layer: v.reference.clone(),
            length: length.from_si(v.length),
            height: length.from_si(v.h),
            ideal_impedance: v.impedance,
            ideal_delay_s: v.delay,
            uncovered: v.uncovered.clone(),
        }
    }
}

/// A screening finding: geometry only, never a solved field.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FindingRow {
    pub code: Code,
    /// Always `geometry_screening`.
    pub method: String,
    pub segment: Option<String>,
    pub detail: String,
}

impl FindingRow {
    fn of(f: &Finding) -> FindingRow {
        FindingRow { code: f.code, method: "geometry_screening".to_owned(), segment: f.segment.clone(), detail: f.detail.clone() }
    }
}

/// A pair of parallel segments of two nets: geometry only.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PairRow {
    pub source: String,
    pub target: String,
    pub source_segment: String,
    pub target_segment: String,
    pub layer: String,
    /// The parallel stretch's ends on each segment.
    pub source_points: [[f64; 2]; 2],
    pub target_points: [[f64; 2]; 2],
    pub gap: f64,
    pub overlap: f64,
    /// Always `geometry_only`.
    pub method: String,
}

impl PairRow {
    fn of(p: &Pair, length: Length) -> PairRow {
        let points = |ends: [[f64; 2]; 2]| ends.map(|v| v.map(|x| length.from_si(x)));
        PairRow {
            source: p.source.clone(),
            target: p.target.clone(),
            source_segment: p.source_segment.clone(),
            target_segment: p.target_segment.clone(),
            layer: p.layer.clone(),
            source_points: points(p.a),
            target_points: points(p.b),
            gap: length.from_si(p.gap),
            overlap: length.from_si(p.length),
            method: "geometry_only".to_owned(),
        }
    }
}

/// The ideal line's S11 and S21 and its low-pass TDR.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Line {
    /// Always `analytical_transmission_line`.
    pub method: String,
    pub delay_s: f64,
    pub frequency_unit: String,
    pub points: Vec<LinePoint>,
    pub tdr: Vec<TdrPoint>,
    pub resolution_s: f64,
}

/// A complex wave: parts, magnitude, dB and degrees.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Wave {
    pub re: f64,
    pub im: f64,
    pub mag: f64,
    pub db: f64,
    pub deg: f64,
}

impl Wave {
    fn of(c: emcraft_solver::Complex64) -> Wave {
        Wave { re: c.re, im: c.im, mag: c.norm(), db: emcraft_model::units::decibels(c.norm()), deg: c.arg().to_degrees() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LinePoint {
    pub frequency: f64,
    pub s11: Wave,
    pub s21: Wave,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TdrPoint {
    pub time_s: f64,
    pub reflection: f64,
    /// The impedance the reflection means (Ω); none where it is total.
    pub impedance: Option<f64>,
}

/// The ideal line at `points` frequencies up to `stop` (Hz) and its low-pass TDR.
fn line(report: &Report, stop: f64, points: usize, reference: f64, frequency: Frequency) -> Result<Line> {
    let freqs: Vec<f64> = (1..=points).map(|i| stop * i as f64 / points as f64).collect();
    let mut waves = Vec::with_capacity(points);
    let mut rows = Vec::with_capacity(points);
    for &f in &freqs {
        let (r, t) = emcraft_theory::pcb::s_parameters(report, f, reference).map_err(failed)?;
        waves.push(r);
        rows.push(LinePoint { frequency: frequency.from_si(f), s11: Wave::of(r), s21: Wave::of(t) });
    }
    let tdr = crate::tdr::transform(&freqs, &waves, crate::tdr::Mode::LowPass)?;
    let impedance = |r: f64| ((1.0 - r).abs() > 1e-9).then(|| reference * (1.0 + r) / (1.0 - r));
    Ok(Line {
        method: "analytical_transmission_line".to_owned(),
        delay_s: report.sections.iter().map(|v| v.delay).sum::<f64>(),
        frequency_unit: frequency.symbol().to_owned(),
        points: rows,
        tdr: tdr.times.iter().zip(&tdr.values).map(|(&t, &r)| TdrPoint { time_s: t, reflection: r, impedance: impedance(r) }).collect(),
        resolution_s: tdr.resolution,
    })
}
