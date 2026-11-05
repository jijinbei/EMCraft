//! The analysis history: each solve that succeeds kept with the model it solved and its results,
//! so a job's tries can be compared (`view.sparameters` `compare`, the window's chart) and gone
//! back to (`results.history.restore`). At most [`MAX_KEPT`] unpinned solves are kept, the
//! oldest let go first; pinned ones (at most [`MAX_PINNED`]) stay until unpinned or deleted.
//!
//! The history belongs to the project: its file keeps each solve's identity (id, when, command
//! and parameters, pin, label), the history of commands that built its model and what it found
//! as shown and compared ([`Kept`]: S-parameters, eigenmodes, the time domain's S) —
//! not its model, mesh or fields ([`file`], docs/project-file.md). A solve read back from a
//! file holds no model: restoring it replays its history (the mesh comes back bit for bit) and
//! solves it again, as long as its first solve took; `solve` false brings back only the model.

use std::sync::Arc;

use emcraft_model::Model;
use emcraft_model::units::Display;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::expr::Variables;
use crate::{EngineError, Params, Result, Results, Session, Step};

pub mod file;
pub mod kept;

pub use file::{StoredResults, StoredSweep};
pub use kept::{Kept, KeptSweep};

/// Most solves kept besides the pinned ones.
pub const MAX_KEPT: usize = 16;

/// Most solves pinned.
pub const MAX_PINNED: usize = 32;

/// Longest label of a kept solve (characters).
pub const MAX_LABEL: usize = 80;

/// Longest version text a file's provenance may give.
const MAX_VERSION: usize = 64;

/// Most threads a provenance may name.
const MAX_THREADS: usize = 1 << 16;

/// How a kept solve was made: by which EMCraft, on what, how large and how long. Not part of
/// what it found ([`Kept`]): the same solve made elsewhere is the same solve.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Provenance {
    /// EMCraft's version.
    pub version: String,
    /// Wall-clock seconds the solve took.
    pub seconds: f64,
    /// The mesh's tetrahedra, where it was meshed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tetrahedra: Option<usize>,
    /// The system's unknowns, where the solve has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unknowns: Option<usize>,
    /// The elements' order (1 or 2) of a frequency-domain solve.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<u8>,
    /// How a sweep's systems were solved (direct or iterative).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linear_solver: Option<crate::LinearSolver>,
    /// Where it ran, and the time domain's precision there.
    pub device: crate::Device,
    pub precision: crate::Precision,
    /// The threads it had.
    pub threads: usize,
}

impl Provenance {
    /// How solve `command` that just ran in `s` was made, in `seconds`, replying `reply`.
    pub(crate) fn of(s: &Session, command: &str, seconds: f64, reply: &Value) -> Provenance {
        let sweep = s.results.sweep.as_ref().filter(|_| matches!(command, "solve.frequency" | "solve.refine"));
        let count = |key: &str| reply.get(key).and_then(Value::as_u64).and_then(|n| usize::try_from(n).ok());
        Provenance {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            seconds: if seconds.is_finite() { seconds.max(0.0) } else { 0.0 },
            tetrahedra: s.model.mesh.as_ref().map(|m| m.tets.len()),
            unknowns: count("unknowns"),
            order: count("order").and_then(|n| u8::try_from(n).ok()),
            linear_solver: sweep.map(|sw| if sw.driven.iterative { crate::LinearSolver::Iterative } else { crate::LinearSolver::Direct }),
            device: s.compute.device(),
            precision: s.compute.precision,
            threads: emcraft_solver::threads(),
        }
    }

    /// Checked as read from a file (hostile).
    pub(crate) fn checked(self) -> std::result::Result<Provenance, String> {
        if self.version.chars().count() > MAX_VERSION || self.version.chars().any(char::is_control) {
            return Err(format!("its version is at most {MAX_VERSION} characters, none of them control characters"));
        }
        if !(self.seconds.is_finite() && self.seconds >= 0.0) || self.threads > MAX_THREADS || self.order.is_some_and(|o| !(1..=2).contains(&o)) {
            return Err(format!("its seconds are a finite number ≥ 0, its threads at most {MAX_THREADS}, its order 1 or 2"));
        }
        Ok(self)
    }
}

/// A solve kept: its command and parameters, the model it solved and what it found.
#[derive(Clone, Debug)]
pub struct Archived {
    pub id: u32,
    /// When it was solved (seconds since 1970, UTC).
    pub when: u64,
    pub command: String,
    /// Its parameters as given, in the display units `display`.
    pub params: Value,
    /// The display units, the history and the variables of the model it solved.
    pub display: Display,
    pub history: Arc<Vec<Step>>,
    pub variables: Arc<Variables>,
    /// What it found, as shown and compared (and as the project file keeps it).
    pub kept: Arc<Kept>,
    pub pinned: bool,
    pub label: Option<String>,
    /// How it was made.
    pub provenance: Provenance,
    /// The model and the whole results, while the session holds them: not for a solve read
    /// back from a project file until it is restored.
    pub(crate) solved: Option<Arc<Solved>>,
}

/// A kept solve's model and results in full.
#[derive(Debug)]
pub(crate) struct Solved {
    model: Model,
    results: Results,
}

impl Archived {
    /// Whether the session holds its model and results (else restoring it solves again).
    pub fn in_memory(&self) -> bool {
        self.solved.is_some()
    }
}

/// What a project's unsaved state reads of a kept solve: its id, pin and label.
pub(crate) type Mark = (u32, bool, Option<String>);

/// The marks of `archive`, in order.
pub(crate) fn marks(archive: &[Archived]) -> Vec<Mark> {
    archive.iter().map(|a| (a.id, a.pinned, a.label.clone())).collect()
}

/// Seconds since 1970 (0 on a clock before it).
fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Keeps the solve that just succeeded in `s`; its id. The same solve again (its command,
/// parameters, units and model, and the same results) is the one kept before, not another: a
/// run repeated leaves the history, and the project, as they were.
pub(crate) fn keep(s: &mut Session, command: &str, params: &Value, provenance: Provenance) -> u32 {
    // Only finite values: a file writes no other (a variable of 1/0 is left out).
    let variables: Variables = s.variables.iter().filter(|(_, v)| v.is_finite()).map(|(k, v)| (k.clone(), *v)).collect();
    let kept = Kept::of(&s.results);
    let same = |a: &Archived| {
        a.command == command
            && a.params == *params
            && a.display == s.display
            && a.history == s.history
            && *a.variables == variables
            && *a.kept == kept
    };
    if let Some(a) = s.archive.iter_mut().find(|a| same(a)) {
        a.solved = Some(Arc::new(Solved { model: s.model.clone(), results: s.results.clone() }));
        a.provenance = provenance;
        return a.id;
    }
    s.archive_next = s.archive_next.saturating_add(1);
    // The same model as the solve before: its history shared.
    let history = match s.archive.last() {
        Some(a) if a.history == s.history => a.history.clone(),
        _ => s.history.clone(),
    };
    s.archive.push(Archived {
        id: s.archive_next,
        when: now(),
        command: command.to_owned(),
        params: params.clone(),
        display: s.display,
        history,
        variables: Arc::new(variables),
        kept: Arc::new(kept),
        pinned: false,
        label: None,
        provenance,
        solved: Some(Arc::new(Solved { model: s.model.clone(), results: s.results.clone() })),
    });
    // The oldest unpinned ones go past the cap.
    while s.archive.iter().filter(|a| !a.pinned).count() > MAX_KEPT {
        let Some(k) = s.archive.iter().position(|a| !a.pinned) else { break };
        s.archive.remove(k);
    }
    s.archive_next
}

/// The session's model and results held as kept solve `id`'s (a project just opened with its
/// results): restoring it needs no solve.
pub(crate) fn attach(s: &mut Session, id: u32) {
    let solved = Arc::new(Solved { model: s.model.clone(), results: s.results.clone() });
    if let Some(a) = s.archive.iter_mut().find(|a| a.id == id) {
        a.solved = Some(solved);
    }
}

/// What changed in the model between two kept solves: the variables whose values differ
/// (`d 3 → 4`), else whether the model's history differs at all.
fn changes(before: Option<&Archived>, now: &Archived) -> Vec<String> {
    let Some(b) = before else { return Vec::new() };
    let mut out: Vec<String> = now
        .variables
        .iter()
        .filter_map(|(name, v)| match b.variables.get(name) {
            Some(w) if w != v => Some(format!("{name} {w} → {v}")),
            None => Some(format!("{name} = {v}")),
            _ => None,
        })
        .collect();
    if out.is_empty() && b.history != now.history {
        out.push("model changed".to_owned());
    }
    out
}

/// A kept solve in brief, as `results.history` lists it: its id, when, command, label, how many
/// frequencies and its band, its deepest |S11| and where, its first eigenmode (display units),
/// what changed in the model since the one before, whether it is pinned and whether the
/// session's results are its own, and how it was made.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HistoryRow {
    pub id: u32,
    pub when: u64,
    pub command: String,
    pub label: Option<String>,
    pub points: Option<usize>,
    pub band: Option<[f64; 2]>,
    pub best_s11_db: Option<f64>,
    pub best_at: Option<f64>,
    pub first_mode: Option<f64>,
    pub changes: Vec<String>,
    pub pinned: bool,
    pub current: bool,
    pub provenance: Provenance,
}

/// A kept solve's row. Read from what it keeps ([`Kept`]), so a solve read back from a file
/// reads the same.
fn row(s: &Session, before: Option<&Archived>, a: &Archived) -> HistoryRow {
    let fu = s.display.frequency;
    let sweep = a.kept.sweep.as_ref();
    let band = sweep.and_then(|sw| Some((*sw.s.frequencies().first()?, *sw.s.frequencies().last()?)));
    // The deepest |S11| over the sweep, the usual figure of merit of a matched part.
    let best = sweep
        .and_then(|sw| sw.curve(0, 0).into_iter().map(|(f, c)| (f, emcraft_model::units::decibels(c.norm()))).min_by(|x, y| x.1.total_cmp(&y.1)));
    HistoryRow {
        id: a.id,
        when: a.when,
        command: a.command.clone(),
        label: a.label.clone(),
        points: sweep.map(|sw| sw.s.frequencies().len()),
        band: band.map(|(x, y)| [fu.from_si(x), fu.from_si(y)]),
        best_s11_db: best.map(|b| b.1),
        best_at: best.map(|b| fu.from_si(b.0)),
        first_mode: a.kept.eigenmodes.first().map(|m| fu.from_si(m.frequency)),
        changes: changes(before, a),
        pinned: a.pinned,
        current: s.archive_current == Some(a.id),
        provenance: a.provenance.clone(),
    }
}

fn find(s: &Session, p: &Params) -> Result<usize> {
    let id = p.integer("id", 1..=u32::MAX as usize).required()? as u32;
    s.archive.iter().position(|a| a.id == id).ok_or_else(|| p.error("id", format!("names no kept solve: {id} (results.history lists them)")))
}

/// The solves kept, the oldest first, as `results.history` lists them.
pub fn rows(s: &Session) -> Vec<HistoryRow> {
    let before = std::iter::once(None).chain(s.archive.iter().map(Some));
    before.zip(&s.archive).map(|(b, a)| row(s, b, a)).collect()
}

/// `results.history`: the solves kept, the oldest first.
pub fn list(s: &mut Session, _: &Params) -> Result<Value> {
    Ok(json!({"history": rows(s), "kept": MAX_KEPT, "pinned": MAX_PINNED}))
}

/// `results.history.restore`: the model, units, variables and results of a kept solve, current
/// again (undo goes back). One read back from a project file is rebuilt from its history and
/// solved again (with `solve` false, only its model comes back).
pub fn restore(s: &mut Session, p: &Params) -> Result<Value> {
    let k = find(s, p)?;
    let solve = p.flag("solve").or(true)?;
    let a = s.archive[k].clone();
    let mut reply = json!({"id": a.id, "command": a.command});
    let solved = match &a.solved {
        Some(sv) => {
            s.model = sv.model.clone();
            s.display = a.display;
            s.history = a.history.clone();
            s.variables = (*a.variables).clone();
            s.results = sv.results.clone();
            true
        }
        None => {
            let mut fresh = s.fresh();
            fresh.run_steps(&a.history)?;
            if solve {
                let spec = crate::find_command(&a.command).ok_or_else(|| EngineError::failed(format!("`{}` is no command", a.command)))?;
                let params = crate::runs::in_units(spec, &a.params, a.display, fresh.display, &fresh.variables)?;
                fresh.execute(&a.command, &params)?;
                let kept = Kept::of(&fresh.results);
                reply["solved_again"] = json!(true);
                reply["same_as_saved"] = json!(kept == *a.kept);
                s.archive[k] = Archived {
                    kept: Arc::new(kept),
                    solved: Some(Arc::new(Solved { model: fresh.model.clone(), results: fresh.results.clone() })),
                    ..a.clone()
                };
            }
            s.model = fresh.model;
            s.display = fresh.display;
            s.history = fresh.history;
            s.variables = fresh.variables;
            s.results = if solve { fresh.results } else { Results::default() };
            solve
        }
    };
    // Its results are the model's: current after the restore.
    s.archive_current = solved.then_some(a.id);
    Ok(reply)
}

/// `results.history.pin`: keeps a solve past the cap (or lets it go with the others).
pub fn pin(s: &mut Session, p: &Params) -> Result<Value> {
    let k = find(s, p)?;
    let pinned = p.flag("pinned").or(true)?;
    if pinned && !s.archive[k].pinned && s.archive.iter().filter(|a| a.pinned).count() >= MAX_PINNED {
        return Err(p.error("id", format!("at most {MAX_PINNED} solves are pinned: unpin one first")));
    }
    s.archive[k].pinned = pinned;
    Ok(json!({"id": s.archive[k].id, "pinned": pinned}))
}

/// `results.history.label`: names a kept solve (an empty label takes its name away).
pub fn label(s: &mut Session, p: &Params) -> Result<Value> {
    let k = find(s, p)?;
    let label = checked_label(p.string("label").required()?).map_err(|e| p.error("label", e))?;
    s.archive[k].label.clone_from(&label);
    Ok(json!({"id": s.archive[k].id, "label": label}))
}

/// A label as kept: trimmed, none when empty; an error past [`MAX_LABEL`] characters or holding
/// a control character.
pub(crate) fn checked_label(text: &str) -> std::result::Result<Option<String>, String> {
    let text = text.trim();
    if text.chars().count() > MAX_LABEL || text.chars().any(char::is_control) {
        return Err(format!("must be at most {MAX_LABEL} characters, none of them control characters"));
    }
    Ok((!text.is_empty()).then(|| text.to_owned()))
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
