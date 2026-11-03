//! PCB commands: canonical embedded model history, geometric routing screening, and an
//! explicitly approximate quasi-TEM SI calculation. No volume mesh/EMC verdict is implied.
use crate::{EngineError, Params, Result, Session, Step};
use emcraft_model::pcb::{Board, Copper, Layer, Segment};
use emcraft_model::units::Length;
pub use emcraft_theory::pcb::near;
use serde_json::{Value, json};
use std::io::Read;
fn failed(s: impl ToString) -> EngineError {
    EngineError::failed(s.to_string())
}
fn install(s: &mut Session, board: Board) -> Result<Value> {
    board.check()?;
    let reply = json!({"board":board.name,"segments":board.segments.len(),"layers":board.layers.len(),"notes":board.notes});
    s.model = emcraft_model::Model { pcb: Some(board), ..Default::default() };
    s.views.pcb_net = None;
    Ok(reply)
}
/// Native board JSON uses the session's display length unit, including layer z coordinates.
pub fn load(s: &mut Session, p: &Params) -> Result<Value> {
    let mut board: Board = serde_json::from_value(p.raw("data").cloned().ok_or_else(|| failed("give data"))?).map_err(failed)?;
    board.map_lengths(|v| s.display.length.to_si(v));
    install(s, board)
}
fn embedded(s: &mut Session, board: Board) -> Result<Value> {
    let mut data = board.clone();
    data.map_lengths(|v| s.display.length.from_si(v));
    let reply = install(s, board)?;
    s.history.push(Step::new("pcb.load", json!({"data":data})));
    Ok(reply)
}
pub fn import(s: &mut Session, p: &Params) -> Result<Value> {
    let path = p.string("path").required()?;
    let file = std::fs::File::open(path).map_err(failed)?;
    let mut text = String::new();
    file.take(8 * 1024 * 1024 + 1).read_to_string(&mut text).map_err(failed)?;
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
/// An original two-layer example, with/without a split in its return plane (not third-party data).
pub fn sample(s: &mut Session, p: &Params) -> Result<Value> {
    let split = p.flag("split").or(true)?;
    let neighbour = p.flag("neighbour").or(true)?;
    let l = Length::Millimetre.from_si(p.positive_length("length").or(0.04)?);
    let w = Length::Millimetre.from_si(p.positive_length("width").or(0.0003)?);
    let h = Length::Millimetre.from_si(p.positive_length("height").or(0.00018)?);
    let er = p.number("er").or(4.2)?;
    if !(1.0..=1000.0).contains(&l) || !(0.001..=10.0).contains(&w) || !(0.001..=10.0).contains(&h) || !(1.0..=128.0).contains(&er) {
        return Err(failed("PCB example requires length 1..1000 mm, width/height 0.001..10 mm, er 1..128"));
    }
    let poly = |x0, x1| Copper { net: "GND".into(), layer: "B.Cu".into(), polygon: vec![[x0, 0.0], [x1, 0.0], [x1, 20.0], [x0, 20.0]] };
    let mut board = Board {
        name: "PCB return-path example".into(),
        layers: vec![Layer { name: "F.Cu".into(), z: 0.0, eps_below: er }, Layer { name: "B.Cu".into(), z: h, eps_below: 1.0 }],
        segments: vec![
            Segment { id: "signal:0".into(), net: "DATA".into(), layer: "F.Cu".into(), start: [5.0, 10.0], end: [5.0 + l / 2.0, 10.0], width: w },
            Segment { id: "signal:1".into(), net: "DATA".into(), layer: "F.Cu".into(), start: [5.0 + l / 2.0, 10.0], end: [5.0 + l, 10.0], width: w },
            Segment {
                id: "clock:0".into(),
                net: "CLK".into(),
                layer: "F.Cu".into(),
                start: [5.0, 10.0 + w + 0.25],
                end: [5.0 + l, 10.0 + w + 0.25],
                width: w,
            },
        ],
        vias: vec![],
        copper: match split {
            true => vec![poly(0.0, 5.0 + 0.375 * l), poly(5.0 + 0.425 * l, l + 10.0)],
            false => vec![poly(0.0, l + 10.0)],
        },
        notes: vec!["Original routing example; lossless, zero-thickness conductors and no pads.".into()],
    };
    if !neighbour {
        board.segments.retain(|s| s.net != "CLK");
    }
    board.map_lengths(|v| Length::Millimetre.to_si(v));
    install(s, board)
}
pub fn inspect(s: &mut Session, p: &Params) -> Result<Value> {
    let ground = p.string("ground").or("GND")?;
    let board = s.model.pcb.as_ref().ok_or_else(|| failed("load a PCB with pcb.import, pcb.load or pcb.sample"))?;
    let net = p
        .string("net")
        .optional()?
        .or_else(|| s.views.pcb_net.as_deref().filter(|n| board.segments.iter().any(|seg| seg.net == *n)))
        .or_else(|| board.segments.iter().find(|seg| seg.net != ground).map(|seg| seg.net.as_str()))
        .ok_or_else(|| failed("load a PCB and select a signal net"))?;
    if net.chars().count() > 128 || ground.chars().count() > 128 {
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
    let report = emcraft_theory::pcb::inspect(board, net, ground).map_err(failed)?;
    let length = s.display.length;
    let frequency = s.display.frequency;
    let sections:Vec<_>=report.sections.iter().map(|v|json!({"id":v.id,"reference_layer":v.reference,"length":length.from_si(v.length),"height":length.from_si(v.h),"ideal_impedance":v.impedance,"ideal_delay_s":v.delay,"uncovered":v.uncovered})).collect();
    let findings: Vec<_> =
        report.findings.iter().map(|f| json!({"code":f.code,"method":"geometry_screening","segment":f.segment,"detail":f.detail})).collect();
    let mut reply = json!({"net":net,"ground":ground,"board":board.name,"length":length.from_si(report.length),"length_unit":length.symbol(),"model_available":report.model_available,"sections":sections,"findings":findings,"import_notes":board.notes,
        "model":"lossless quasi-static transmission-line approximation; zero-thickness outer microstrip, infinite continuous ground; excludes bends, pads, vias, losses and coupling","return_current":"Geometry screening only: filled copper support is not a solved return-current density.","reference":reference,"stop":frequency.from_si(stop),"stop_hz":stop,"frequency_unit":frequency.symbol()});
    if report.model_available {
        let freqs: Vec<_> = (1..=points).map(|i| stop * i as f64 / points as f64).collect();
        let mut waves = Vec::new();
        let mut rows = Vec::new();
        for &f in &freqs {
            let (r, t) = emcraft_theory::pcb::s_parameters(&report, f, reference).map_err(failed)?;
            waves.push(r);
            rows.push(json!({"frequency":frequency.from_si(f),"s11":crate::reply::complex(r),"s21":crate::reply::complex(t)}));
        }
        let tdr = crate::tdr::transform(&freqs, &waves, crate::tdr::Mode::LowPass)?;
        let tdr_rows: Vec<_> = tdr
            .times
            .iter()
            .zip(&tdr.values)
            .map(|(t, r)| json!({"time_s":t,"reflection":r,"impedance":if (1.0-r).abs()>1e-9{Some(reference*(1.0+r)/(1.0-r))}else{None}}))
            .collect();
        reply["si"] = json!({"method":"analytical_transmission_line","delay_s":report.sections.iter().map(|v|v.delay).sum::<f64>(),"frequency_unit":frequency.symbol(),"points":rows,"tdr":tdr_rows,"resolution_s":tdr.resolution});
    } else {
        reply["si"] = Value::Null;
    }
    s.views.pcb_net = Some(net.to_owned());
    Ok(reply)
}
