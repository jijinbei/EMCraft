//! The analysis history: each solve that succeeds kept with the model it solved and its results,
//! so a job's tries can be compared (`view.sparameters` `compare`, the window's chart) and gone
//! back to (`results.history.restore`). It belongs to the session, not the project file: a
//! history's models and fields are what a file leaves out. At most [`MAX_KEPT`] unpinned solves
//! are kept, the oldest let go first; pinned ones stay until unpinned or deleted.

use serde_json::{Value, json};

use crate::project::Snapshot;
use crate::{EngineError, Params, Result, Results, Session};

/// Most solves kept besides the pinned ones.
pub const MAX_KEPT: usize = 16;

/// A solve kept: its command and parameters, the model it solved and what it found.
#[derive(Clone, Debug)]
pub struct Archived {
    pub id: u32,
    pub command: String,
    pub params: Value,
    pub state: Snapshot,
    pub results: Results,
    pub pinned: bool,
}

/// Keeps the solve that just succeeded in `s`.
pub(crate) fn keep(s: &mut Session, command: &str, params: &Value) {
    s.archive_next = s.archive_next.saturating_add(1);
    let state = s.snapshot(s.history.clone(), None);
    s.archive.push(Archived {
        id: s.archive_next,
        command: command.to_owned(),
        params: params.clone(),
        state,
        results: s.results.clone(),
        pinned: false,
    });
    // The oldest unpinned ones go past the cap.
    while s.archive.iter().filter(|a| !a.pinned).count() > MAX_KEPT {
        let Some(k) = s.archive.iter().position(|a| !a.pinned) else { break };
        s.archive.remove(k);
    }
}

/// What changed in the model between two kept solves: the variables whose values differ
/// (`d 3 → 4`), else whether the model's history differs at all.
fn changes(before: Option<&Archived>, now: &Archived) -> Vec<String> {
    let Some(b) = before else { return Vec::new() };
    let mut out: Vec<String> = now
        .state
        .variables
        .iter()
        .filter_map(|(name, v)| match b.state.variables.get(name) {
            Some(w) if w != v => Some(format!("{name} {w} → {v}")),
            None => Some(format!("{name} = {v}")),
            _ => None,
        })
        .collect();
    if out.is_empty() && b.state.history != now.state.history {
        out.push("model changed".to_owned());
    }
    out
}

/// A kept solve in brief: its id, command, how many frequencies, what changed since the one
/// before it, whether it is pinned and whether the session's results are its own.
fn row(s: &Session, before: Option<&Archived>, a: &Archived) -> Value {
    let fu = s.display.frequency;
    let points = a.results.sweep.as_ref().map(|sw| sw.results.len());
    let band = a.results.sweep.as_ref().and_then(|sw| Some((sw.results.first()?.frequency, sw.results.last()?.frequency)));
    // The deepest |S11| over the sweep, the usual figure of merit of a matched part.
    let best = a.results.sweep.as_ref().and_then(|sw| {
        sw.results
            .iter()
            .filter_map(|r| r.s.get(0, 0).map(|c| (r.frequency, emcraft_model::units::decibels(c.norm()))))
            .min_by(|x, y| x.1.total_cmp(&y.1))
    });
    let eigen = a.results.eigen.as_ref().and_then(|e| e.modes.first().map(|m| fu.from_si(m.frequency)));
    json!({
        "id": a.id,
        "command": a.command,
        "points": points,
        "band": band.map(|(x, y)| [fu.from_si(x), fu.from_si(y)]),
        "best_s11_db": best.map(|b| b.1),
        "best_at": best.map(|b| fu.from_si(b.0)),
        "first_mode": eigen,
        "changes": changes(before, a),
        "pinned": a.pinned,
        "current": s.archive_current == Some(a.id),
    })
}

fn find(s: &Session, p: &Params) -> Result<usize> {
    let id = p.integer("id", 1..=u32::MAX as usize).required()? as u32;
    s.archive.iter().position(|a| a.id == id).ok_or_else(|| p.error("id", format!("names no kept solve: {id} (results.history lists them)")))
}

/// The solves kept, the oldest first, as `results.history` lists them.
pub fn rows(s: &Session) -> Vec<Value> {
    let before = std::iter::once(None).chain(s.archive.iter().map(Some));
    before.zip(&s.archive).map(|(b, a)| row(s, b, a)).collect()
}

/// `results.history`: the solves kept, the oldest first.
pub fn list(s: &mut Session, _: &Params) -> Result<Value> {
    Ok(json!({"history": rows(s), "kept": MAX_KEPT}))
}

/// `results.history.restore`: the model, units, variables and results of a kept solve, current
/// again (undo goes back).
pub fn restore(s: &mut Session, p: &Params) -> Result<Value> {
    let k = find(s, p)?;
    let a = &s.archive[k];
    let (id, reply, state, results) = (a.id, json!({"id": a.id, "command": a.command}), a.state.clone(), a.results.clone());
    s.model = state.model;
    s.display = state.display;
    s.history = state.history;
    s.variables = state.variables;
    s.results = results;
    // Its results are the model's: current after the restore.
    s.solved_revision = Some(s.revision);
    s.archive_current = Some(id);
    Ok(reply)
}

/// `results.history.pin`: keeps a solve past the cap (or lets it go with the others).
pub fn pin(s: &mut Session, p: &Params) -> Result<Value> {
    let k = find(s, p)?;
    let pinned = p.flag("pinned").or(true)?;
    s.archive[k].pinned = pinned;
    Ok(json!({"id": s.archive[k].id, "pinned": pinned}))
}

/// `results.history.delete`: lets a kept solve go.
pub fn delete(s: &mut Session, p: &Params) -> Result<Value> {
    let k = find(s, p)?;
    let a = s.archive.remove(k);
    if s.archive_current == Some(a.id) {
        s.archive_current = None;
    }
    Ok(json!({"id": a.id, "kept": s.archive.len()}))
}

/// The kept solves named by `ids`, in the order given; an error naming one not kept.
pub fn picked<'s>(s: &'s Session, ids: &[u32]) -> std::result::Result<Vec<&'s Archived>, EngineError> {
    ids.iter()
        .map(|id| s.archive.iter().find(|a| a.id == *id).ok_or_else(|| EngineError::Params(format!("compare: {id} names no kept solve"))))
        .collect()
}
