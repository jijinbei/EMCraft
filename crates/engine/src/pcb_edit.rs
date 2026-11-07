//! Editing a board: its routed segments, vias and filled copper added, changed and removed, each
//! a command recorded in the history (so undone like any other), the board checked after each.
//! Lengths in the session's display units; a board's region (`pcb.region`) stays, its mesh goes.

use emcraft_model::pcb::{Board, Copper, Segment, Via};
use serde_json::{Value, json};

use crate::{EngineError, Params, Result, Session};

fn failed(why: impl std::fmt::Display) -> EngineError {
    EngineError::failed(why)
}

/// Applies `change` to a copy of the board, checks it and keeps it (the mesh dropped).
fn edit(s: &mut Session, change: impl FnOnce(&mut Board) -> Result<Value>) -> Result<Value> {
    let mut board = s.model.pcb().cloned().ok_or_else(|| failed("load a PCB first: pcb.import, pcb.load or pcb.sample"))?;
    let reply = change(&mut board)?;
    board.check()?;
    s.model.set_pcb(Some(board));
    s.model.mesh = None;
    Ok(reply)
}

/// A point `[x, y]` in display units, in metres.
fn point(p: &Params, name: &str) -> Result<Option<[f64; 2]>> {
    match p.lengths(name).optional()? {
        Some(v) => Ok(Some(<[f64; 2]>::try_from(v.as_slice()).map_err(|_| p.error(name, "give two lengths [x, y]"))?)),
        None => Ok(None),
    }
}

/// `pcb.segment`: a routed segment added, or the one named `id` changed (only what is given).
pub fn segment(s: &mut Session, p: &Params) -> Result<Value> {
    let id = p.string("id").optional()?.map(str::to_owned);
    let net = p.string("net").optional()?.map(str::to_owned);
    let layer = p.string("layer").optional()?.map(str::to_owned);
    let (start, end) = (point(p, "start")?, point(p, "end")?);
    let width = p.positive_length("width").optional()?;
    edit(s, |b| {
        if let Some(g) = id.as_deref().and_then(|id| b.segments.iter_mut().find(|g| g.id == id)) {
            if let Some(n) = net {
                g.net = n;
            }
            if let Some(l) = layer {
                g.layer = l;
            }
            g.start = start.unwrap_or(g.start);
            g.end = end.unwrap_or(g.end);
            g.width = width.unwrap_or(g.width);
            return Ok(json!({"id": g.id, "changed": true}));
        }
        let missing = |what: &str| p.error(what, "a new segment needs it");
        let id = id.unwrap_or_else(|| {
            (b.segments.len()..).map(|k| format!("edit:{k}")).find(|n| !b.segments.iter().any(|g| &g.id == n)).unwrap_or_default()
        });
        let g = Segment {
            id: id.clone(),
            net: net.ok_or_else(|| missing("net"))?,
            layer: layer.ok_or_else(|| missing("layer"))?,
            start: start.ok_or_else(|| missing("start"))?,
            end: end.ok_or_else(|| missing("end"))?,
            width: width.ok_or_else(|| missing("width"))?,
        };
        b.segments.push(g);
        Ok(json!({"id": id, "changed": false, "segments": b.segments.len()}))
    })
}

/// `pcb.segment.delete`: the segment named `id` removed.
pub fn segment_delete(s: &mut Session, p: &Params) -> Result<Value> {
    let id = p.string("id").required()?.to_owned();
    edit(s, |b| {
        let i = b.segments.iter().position(|g| g.id == id).ok_or_else(|| p.error("id", "name a segment of the board"))?;
        b.segments.remove(i);
        Ok(json!({"segments": b.segments.len()}))
    })
}

/// `pcb.via`: a via added between two layers.
pub fn via(s: &mut Session, p: &Params) -> Result<Value> {
    let net = p.string("net").required()?.to_owned();
    let at = point(p, "at")?.ok_or_else(|| p.error("at", "give the via's position [x, y]"))?;
    let layers: Vec<String> = p.list("layers", 2).required()?.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect();
    let [from, to] = <[String; 2]>::try_from(layers).map_err(|_| p.error("layers", "name the two layers it joins"))?;
    let diameter = p.positive_length("diameter").required()?;
    let drill = p.positive_length("drill").optional()?;
    edit(s, |b| {
        b.vias.push(Via { net, at, layers: [from, to], diameter, drill });
        Ok(json!({"vias": b.vias.len()}))
    })
}

/// `pcb.via.delete`: via `index` (1 the first) removed.
pub fn via_delete(s: &mut Session, p: &Params) -> Result<Value> {
    let i = p.integer("index", 1..=emcraft_model::pcb::MAX_SEGMENTS).required()?;
    edit(s, |b| {
        if i > b.vias.len() {
            return Err(p.error("index", "name a via of the board (1 the first)"));
        }
        b.vias.remove(i - 1);
        Ok(json!({"vias": b.vias.len()}))
    })
}

/// `pcb.pour`: filled copper of a net added on a layer.
pub fn pour(s: &mut Session, p: &Params) -> Result<Value> {
    let net = p.string("net").required()?.to_owned();
    let layer = p.string("layer").required()?.to_owned();
    let polygon = p.pairs("polygon", 4096).required()?;
    edit(s, |b| {
        b.copper.push(Copper { net, layer, polygon });
        Ok(json!({"copper": b.copper.len()}))
    })
}

/// `pcb.pour.delete`: filled copper `index` (1 the first) removed.
pub fn pour_delete(s: &mut Session, p: &Params) -> Result<Value> {
    let i = p.integer("index", 1..=emcraft_model::pcb::MAX_COPPER).required()?;
    edit(s, |b| {
        if i > b.copper.len() {
            return Err(p.error("index", "name filled copper of the board (1 the first)"));
        }
        b.copper.remove(i - 1);
        Ok(json!({"copper": b.copper.len()}))
    })
}
