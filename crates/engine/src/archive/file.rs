//! The analysis history as a project file keeps it (`analysis_history`, docs/project-file.md):
//! the solves' identities, how they were made and what they found ([`Kept`], the S-parameters
//! as array parts), and the histories that built their models, each written once however many
//! solves share it.
//!
//! A file is hostile: on reading, the solves are capped ([`MAX_KEPT`] unpinned + [`MAX_PINNED`]),
//! ids ascend, every command is a solve command with parameters it knows, every step of a
//! history is a command a history holds (one that changes the model), and every number is
//! finite and every size agrees ([`StoredResults::checked`]); anything else refuses the file.

use std::sync::Arc;

use emcraft_model::units::Display;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use emcraft_io::package::{Reader, Writer};

use super::kept::{KeptEntry, KeptMode, KeptSweep, MAX_ENTRIES, MAX_ENTRY_NAME};
use super::{Archived, Kept, MAX_KEPT, MAX_PINNED, Provenance, checked_label};
use crate::expr::{self, Variables};
use crate::store::StoredS;
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
    pub when: u64,
    pub command: String,
    pub params: Value,
    /// The display units its parameters were read in.
    pub units: Display,
    /// Its model's history: an index into `histories`.
    pub history: usize,
    pub variables: Variables,
    pub pinned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub results: StoredResults,
    pub provenance: Provenance,
}

/// What a kept solve found, as a file writes it ([`Kept`] once checked), the S-parameters as
/// array parts.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StoredResults {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sweep: Option<StoredSweep>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub eigenmodes: Vec<KeptMode>,
    /// The time domain's S11 and S21 (2 rows, entry 0 excited).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_domain: Option<StoredS>,
}

/// A driven sweep's S-parameters in a file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoredSweep {
    /// The elements' order (1 or 2).
    pub order: u8,
    pub entries: Vec<KeptEntry>,
    pub deembed: Vec<f64>,
    /// S (n × n, n the entries).
    pub s: StoredS,
}

impl StoredResults {
    /// `kept` as a file writes it, its arrays as parts under `prefix`.
    pub fn of(kept: &Kept, prefix: &str, w: &mut Writer) -> Result<StoredResults, String> {
        let sweep = match &kept.sweep {
            Some(sw) => Some(StoredSweep {
                order: sw.order.as_u8(),
                entries: sw.entries.clone(),
                deembed: sw.deembed.clone(),
                s: StoredS::write(&sw.s, &format!("{prefix}/s.c64"), w)?,
            }),
            None => None,
        };
        let time_domain = match &kept.time_domain {
            Some(t) => Some(StoredS::write(t, &format!("{prefix}/time_domain.c64"), w)?),
            None => None,
        };
        Ok(StoredResults { sweep, eigenmodes: kept.eigenmodes.clone(), time_domain })
    }

    /// Checked as read from a file (hostile): sizes within their caps and agreeing, numbers
    /// finite, frequencies positive and ascending.
    pub fn checked(self, r: &mut Reader) -> Result<Kept, String> {
        let sweep = self.sweep.map(|sw| sw.checked(r)).transpose()?;
        if self.eigenmodes.len() > crate::EIGEN_MAX {
            return Err(format!("more than {} eigenmodes", crate::EIGEN_MAX));
        }
        for m in &self.eigenmodes {
            let qs = [m.q_walls, m.q_dielectric, m.q_magnetic];
            if !(m.frequency.is_finite() && m.frequency > 0.0) || qs.iter().flatten().any(|q| !q.is_finite()) {
                return Err("an eigenmode's frequency and Q must be finite numbers".into());
            }
        }
        let time_domain = match self.time_domain {
            Some(t) if t.outputs != 2 || t.inputs != [0] => return Err("the time domain's S is S11 and S21 (2 rows, entry 0 excited)".into()),
            Some(t) => Some(t.read(r)?),
            None => None,
        };
        Ok(Kept { sweep, eigenmodes: self.eigenmodes, time_domain })
    }
}

impl StoredSweep {
    fn checked(self, r: &mut Reader) -> Result<KeptSweep, String> {
        let order = emcraft_fem::Order::try_from(self.order).map_err(|n| format!("a sweep's order is 1 or 2, not {n}"))?;
        let n = self.entries.len();
        if n == 0 || n > MAX_ENTRIES || self.deembed.len() > MAX_ENTRIES {
            return Err(format!("a sweep has from 1 to {MAX_ENTRIES} entries of S"));
        }
        for e in &self.entries {
            if e.name.is_empty() || e.name.chars().count() > MAX_ENTRY_NAME || e.port >= MAX_ENTRIES || e.terminal.is_some_and(|t| t >= MAX_ENTRIES) {
                return Err(format!("an entry of S has a name of 1 to {MAX_ENTRY_NAME} characters, a port and a terminal below {MAX_ENTRIES}"));
            }
            if e.reference.is_some_and(|z| !(z.is_finite() && z > 0.0)) {
                return Err("an entry's reference impedance must be a finite number > 0".into());
            }
        }
        if !self.deembed.iter().all(|d| d.is_finite()) {
            return Err("de-embedding must be finite numbers".into());
        }
        if self.s.outputs != n || self.s.inputs != (0..n).collect::<Vec<_>>() {
            return Err(format!("a sweep's S is {n} × {n}, its entries'"));
        }
        let s = self.s.read(r)?;
        Ok(KeptSweep { order, entries: self.entries, deembed: self.deembed, s })
    }
}

fn no_params() -> Value {
    json!({})
}

/// `archive` as a file keeps it, `next` the last id given.
pub(crate) fn stored(archive: &[Archived], next: u32, w: &mut Writer) -> Result<StoredHistory, String> {
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
            results: StoredResults::of(&a.kept, &format!("history/{}", a.id), w)?,
            provenance: a.provenance.clone(),
        });
    }
    Ok(StoredHistory { next, histories: histories.into_iter().map(|h| (**h).clone()).collect(), solves })
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
pub(crate) fn read(st: StoredHistory, r: &mut Reader) -> Result<(Vec<Archived>, u32), String> {
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
        let kept = s.results.checked(r).map_err(|e| format!("{what}: {e}"))?;
        let provenance = s.provenance.checked().map_err(|e| format!("{what}: its provenance: {e}"))?;
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
            provenance,
            solved: None,
        });
    }
    Ok((out, next))
}
