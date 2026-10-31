//! The analysis history as a project file keeps it (`analysis_history`, since version 4;
//! docs/project-file.md): the solves' identities and what they found ([`Kept`]), and the
//! histories that built their models, each written once however many solves share it.
//!
//! A file is hostile: on reading, the solves are capped ([`MAX_KEPT`] unpinned + [`MAX_PINNED`]),
//! ids ascend, every command is a solve command with parameters it knows, every step of a
//! history is a command a history holds (one that changes the model), and every number is
//! finite and every size agrees ([`Kept::checked`]); anything else refuses the file.

use std::sync::Arc;

use emcraft_model::units::Display;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Archived, Kept, MAX_KEPT, MAX_PINNED, checked_label};
use crate::expr::{self, Variables};
use crate::{Effect, Step, find_command};

/// Most steps of one kept solve's history.
pub const MAX_STEPS: usize = 20_000;

/// The analysis history in a project file.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StoredHistory {
    /// The id the next solve kept takes after the last: ids are not given twice.
    pub next: u32,
    /// The histories of commands that built the solves' models.
    pub histories: Vec<Vec<Step>>,
    /// The solves, the oldest first.
    pub solves: Vec<StoredSolve>,
}

/// A kept solve in a project file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoredSolve {
    pub id: u32,
    /// Seconds since 1970, UTC.
    #[serde(default)]
    pub when: u64,
    pub command: String,
    #[serde(default = "no_params")]
    pub params: Value,
    /// The display units its parameters were read in.
    pub units: Display,
    /// Its model's history: an index into `histories`.
    pub history: usize,
    #[serde(default)]
    pub variables: Variables,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default)]
    pub results: Kept,
}

fn no_params() -> Value {
    json!({})
}

/// `archive` as a file keeps it, `next` the last id given.
pub(crate) fn stored(archive: &[Archived], next: u32) -> StoredHistory {
    let mut histories: Vec<&Arc<Vec<Step>>> = Vec::new();
    let mut solves = Vec::with_capacity(archive.len());
    for a in archive {
        let history = match histories.iter().position(|h| Arc::ptr_eq(h, &a.history) || **h == a.history) {
            Some(k) => k,
            None => {
                histories.push(&a.history);
                histories.len() - 1
            }
        };
        solves.push(StoredSolve {
            id: a.id,
            when: a.when,
            command: a.command.clone(),
            params: a.params.clone(),
            units: a.display,
            history,
            variables: (*a.variables).clone(),
            pinned: a.pinned,
            label: a.label.clone(),
            results: (*a.kept).clone(),
        });
    }
    StoredHistory { next, histories: histories.into_iter().map(|h| (**h).clone()).collect(), solves }
}

/// A command's parameters from a file: an object (null reads as none) with only the keys it
/// knows.
fn checked_params(command: &str, params: &mut Value, effects: &[Effect], what: &str) -> Result<(), String> {
    let spec = find_command(command).filter(|c| effects.contains(&c.effect)).ok_or_else(|| {
        let kind = if effects.contains(&Effect::Solve) { "a solve command" } else { "a command a history holds" };
        format!("{what}: `{}` is not {kind}", crate::text::clip(command, 64))
    })?;
    if params.is_null() {
        *params = no_params();
    }
    if !params.is_object() {
        return Err(format!("{what}: its parameters must be a JSON object"));
    }
    crate::cmd::check_keys(spec, params).map_err(|e| format!("{what}: {e}"))
}

/// The kept solves of a file's analysis history, and the last id given; an error saying what
/// is wrong with it.
pub(crate) fn read(st: StoredHistory) -> Result<(Vec<Archived>, u32), String> {
    let StoredHistory { next, histories, solves } = st;
    if solves.len() > MAX_KEPT + MAX_PINNED || solves.iter().filter(|s| s.pinned).count() > MAX_PINNED {
        return Err(format!("the analysis history keeps at most {MAX_KEPT} solves and {MAX_PINNED} pinned ones"));
    }
    if histories.len() > solves.len() {
        return Err("the analysis history holds more histories than solves".into());
    }
    let mut models = Vec::with_capacity(histories.len());
    for (h, mut steps) in histories.into_iter().enumerate() {
        if steps.len() > MAX_STEPS {
            return Err(format!("history {}: more than {MAX_STEPS} steps", h + 1));
        }
        for (i, step) in steps.iter_mut().enumerate() {
            let what = format!("history {}, step {}", h + 1, i + 1);
            checked_params(&step.command, &mut step.params, &[Effect::Record, Effect::Adjust, Effect::Rebuild], &what)?;
        }
        models.push(Arc::new(steps));
    }
    let mut out: Vec<Archived> = Vec::with_capacity(solves.len());
    for (i, mut s) in solves.into_iter().enumerate() {
        let what = format!("kept solve {}", i + 1);
        if s.id == 0 || out.last().is_some_and(|b| b.id >= s.id) || s.id > next {
            return Err(format!("{what}: ids must ascend from 1 up to `next` ({next})"));
        }
        checked_params(&s.command, &mut s.params, &[Effect::Solve], &what)?;
        let history = models.get(s.history).cloned().ok_or_else(|| format!("{what}: names no history ({})", s.history))?;
        if s.variables.len() > crate::study::MAX_VARIABLES || s.variables.iter().any(|(name, v)| !(expr::valid_name(name) && v.is_finite())) {
            return Err(format!("{what}: at most {} variables, each a valid name of a finite value", crate::study::MAX_VARIABLES));
        }
        let label = match s.label.as_deref() {
            Some(l) => checked_label(l).map_err(|e| format!("{what}: its label {e}"))?,
            None => None,
        };
        let kept = s.results.checked().map_err(|e| format!("{what}: {e}"))?;
        out.push(Archived {
            id: s.id,
            when: s.when,
            command: s.command,
            params: s.params,
            display: s.units,
            history,
            variables: Arc::new(s.variables),
            kept: Arc::new(kept),
            pinned: s.pinned,
            label,
            solved: None,
        });
    }
    Ok((out, next))
}
