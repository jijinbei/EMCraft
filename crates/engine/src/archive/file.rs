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

use super::kept::{KeptEntry, KeptMode, KeptSweep, MAX_ENTRIES, MAX_ENTRY_NAME};
use super::{Archived, Kept, MAX_KEPT, MAX_PINNED, checked_label};
use crate::SData;
use crate::expr::{self, Variables};
use crate::{Effect, Step, find_command};
use emcraft_solver::Complex64 as C;

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
    pub results: StoredResults,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<super::Provenance>,
}

/// What a kept solve found, as a file writes it ([`Kept`] once checked). Complex numbers are
/// pairs of plain numbers, real then imaginary: an S matrix of n entries is 2n² numbers, row by
/// row (S\[p\]\[q\] at 2(pn + q)).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StoredResults {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sweep: Option<StoredSweep>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub eigenmodes: Vec<KeptMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_domain: Option<StoredTimeDomain>,
}

/// A driven sweep's S-parameters in a file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoredSweep {
    /// The elements' order (1 or 2).
    pub order: u8,
    pub entries: Vec<KeptEntry>,
    #[serde(default)]
    pub deembed: Vec<f64>,
    /// Ascending (Hz).
    pub frequencies: Vec<f64>,
    /// Per frequency, S as 2n² numbers (row by row, real then imaginary).
    pub s: Vec<Vec<f64>>,
}

/// The time domain's S11 and S21 at its frequencies (Hz), each 2 numbers a frequency.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoredTimeDomain {
    pub frequencies: Vec<f64>,
    pub s11: Vec<f64>,
    pub s21: Vec<f64>,
}

fn pairs(v: impl IntoIterator<Item = C>) -> Vec<f64> {
    v.into_iter().flat_map(|c| [c.re, c.im]).collect()
}

/// Pairs of numbers as complex values (an odd one out is an error).
fn complex(v: &[f64], what: &str) -> Result<Vec<C>, String> {
    let (pairs, rest) = v.as_chunks::<2>();
    if !rest.is_empty() {
        return Err(format!("{what}: complex values are 2 numbers each"));
    }
    Ok(pairs.iter().map(|&[re, im]| C::new(re, im)).collect())
}

impl StoredResults {
    /// `kept` as a file writes it.
    pub fn of(kept: &Kept) -> StoredResults {
        let sweep = kept.sweep.as_ref().map(|sw| StoredSweep {
            order: sw.order.as_u8(),
            entries: sw.entries.clone(),
            deembed: sw.deembed.clone(),
            frequencies: sw.s.frequencies().to_vec(),
            s: (0..sw.s.frequencies().len()).map(|k| pairs(sw.s.at(k).iter().copied())).collect(),
        });
        let time_domain = kept.time_domain.as_ref().map(|t| StoredTimeDomain {
            frequencies: t.frequencies().to_vec(),
            s11: pairs(t.curve(0, 0).map(|(_, c)| c)),
            s21: pairs(t.curve(1, 0).map(|(_, c)| c)),
        });
        StoredResults { sweep, eigenmodes: kept.eigenmodes.clone(), time_domain }
    }

    /// Checked as read from a file (hostile): sizes within their caps and agreeing, numbers
    /// finite, frequencies positive and ascending.
    pub fn checked(self) -> Result<Kept, String> {
        let sweep = self.sweep.map(StoredSweep::checked).transpose()?;
        if self.eigenmodes.len() > crate::EIGEN_MAX {
            return Err(format!("more than {} eigenmodes", crate::EIGEN_MAX));
        }
        for m in &self.eigenmodes {
            let qs = [m.q_walls, m.q_dielectric, m.q_magnetic];
            if !(m.frequency.is_finite() && m.frequency > 0.0) || qs.iter().flatten().any(|q| !q.is_finite()) {
                return Err("an eigenmode's frequency and Q must be finite numbers".into());
            }
        }
        let time_domain = self.time_domain.map(StoredTimeDomain::checked).transpose()?;
        Ok(Kept { sweep, eigenmodes: self.eigenmodes, time_domain })
    }
}

impl StoredSweep {
    fn checked(self) -> Result<KeptSweep, String> {
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
        if self.s.len() != self.frequencies.len() {
            return Err(format!("{} frequencies but {} S matrices", self.frequencies.len(), self.s.len()));
        }
        if let Some(row) = self.s.iter().find(|row| row.len() != 2 * n * n) {
            return Err(format!("an S matrix of {n} entries is {} numbers, not {}", 2 * n * n, row.len()));
        }
        // Within the cap before a value is made of them.
        if self.frequencies.len().saturating_mul(n * n) > crate::sdata::MAX_VALUES {
            return Err(format!("S-parameters of more than {} values", crate::sdata::MAX_VALUES));
        }
        let mut s = Vec::with_capacity(self.frequencies.len() * n * n);
        for row in &self.s {
            s.extend(complex(row, "an S matrix")?);
        }
        let s = SData::square(self.frequencies, n, s)?;
        Ok(KeptSweep { order, entries: self.entries, deembed: self.deembed, s })
    }
}

impl StoredTimeDomain {
    fn checked(self) -> Result<SData, String> {
        let n = 2 * self.frequencies.len();
        if self.s11.len() != n || self.s21.len() != n {
            return Err(format!("the time domain's S11 and S21 are 2 numbers a frequency: {n}"));
        }
        if self.frequencies.len() > crate::sdata::MAX_FREQUENCIES {
            return Err(format!("more than {} frequencies", crate::sdata::MAX_FREQUENCIES));
        }
        let (s11, s21) = (complex(&self.s11, "S11")?, complex(&self.s21, "S21")?);
        SData::new(self.frequencies, 2, vec![0], s11.into_iter().zip(s21).flat_map(|(a, b)| [a, b]).collect())
    }
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
            results: StoredResults::of(&a.kept),
            provenance: a.provenance.clone(),
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
        let provenance = s.provenance.map(super::Provenance::checked).transpose().map_err(|e| format!("{what}: its provenance: {e}"))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sweep() -> StoredSweep {
        let entries = ["1", "2"].map(|n| KeptEntry { name: n.into(), port: 0, terminal: None, reference: None }).to_vec();
        StoredSweep { order: 1, entries, deembed: vec![0.0, 0.0], frequencies: vec![7e9, 8e9], s: vec![vec![0.5; 8], vec![0.25; 8]] }
    }

    #[test]
    fn a_stored_sweep_reads_back_as_written() {
        let kept = sweep().checked().unwrap();
        assert_eq!(kept.curve(1, 0), vec![(7e9, C::new(0.5, 0.5)), (8e9, C::new(0.25, 0.25))]);
        assert!(kept.curve(2, 0).is_empty());
        let all = Kept { sweep: Some(kept), ..Kept::default() };
        assert_eq!(StoredResults::of(&all).checked(), Ok(all.clone()));
        assert_eq!(StoredResults::of(&all).sweep, Some(sweep()));
    }

    #[rstest::rstest]
    #[case::order(|s: &mut StoredSweep| s.order = 3, "order")]
    #[case::entries(|s: &mut StoredSweep| s.entries.clear(), "entries")]
    #[case::ascending(|s: &mut StoredSweep| s.frequencies.reverse(), "ascending")]
    #[case::positive(|s: &mut StoredSweep| s.frequencies[0] = -1.0, "positive")]
    #[case::matrices(|s: &mut StoredSweep| drop(s.s.pop()), "S matrices")]
    #[case::numbers(|s: &mut StoredSweep| s.s[0].push(1.0), "numbers, not")]
    #[case::finite(|s: &mut StoredSweep| s.s[1][3] = f64::INFINITY, "finite")]
    #[case::reference(|s: &mut StoredSweep| s.entries[0].reference = Some(0.0), "reference")]
    fn a_stored_sweep_that_does_not_fit_is_refused(#[case] spoil: fn(&mut StoredSweep), #[case] why: &str) {
        let mut s = sweep();
        spoil(&mut s);
        assert!(matches!(s.checked(), Err(m) if m.contains(why)), "{why}");
    }

    #[test]
    fn too_many_frequencies_are_refused() {
        let mut big = sweep();
        big.frequencies = vec![1.0; crate::sdata::MAX_FREQUENCIES + 1];
        big.s = vec![vec![0.0; 8]; big.frequencies.len()];
        assert!(big.checked().is_err());
    }

    #[test]
    fn stored_eigenmodes_and_time_domain_are_checked() {
        let td = StoredTimeDomain { frequencies: vec![1e9], s11: vec![0.0, 1.0], s21: vec![1.0] };
        assert!(StoredResults { time_domain: Some(td), ..StoredResults::default() }.checked().is_err());
        let td = StoredTimeDomain { frequencies: vec![1e9], s11: vec![0.1, 0.0], s21: vec![0.9, 0.0] };
        let kept = StoredResults { time_domain: Some(td.clone()), ..StoredResults::default() }.checked().unwrap();
        assert_eq!(StoredResults::of(&kept).time_domain, Some(td));
        let mode = KeptMode { frequency: 0.0, q_walls: None, q_dielectric: None, q_magnetic: None };
        assert!(StoredResults { eigenmodes: vec![mode], ..StoredResults::default() }.checked().is_err());
    }
}
