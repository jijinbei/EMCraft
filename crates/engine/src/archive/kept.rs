//! What a kept solve found, as the history shows and compares it and the project file keeps it:
//! the sweep's S-parameters (frequencies, matrices, entries, reference impedances,
//! de-embedding), the eigenmodes' frequencies and Q, the time domain's S11 and S21. Never the
//! mesh, the system or the fields: those come back by solving again.
//!
//! Complex numbers are written as pairs of plain numbers, real then imaginary: an S matrix of n
//! entries is 2n² numbers, row by row (S\[p\]\[q\] at 2(pn + q)).

use emcraft_solver::Complex64 as C;
use serde::{Deserialize, Serialize};

use crate::Results;

/// Most entries (ports' modes, terminals) of a kept sweep's S.
pub const MAX_ENTRIES: usize = 64;

/// Most numbers of one kept solve's S-parameters (frequencies × 2n²): 32 MB of them.
pub const MAX_NUMBERS: usize = 1 << 22;

/// Longest name of an entry.
const MAX_ENTRY_NAME: usize = 64;

/// What a kept solve found, as shown and compared.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Kept {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sweep: Option<KeptSweep>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub eigenmodes: Vec<KeptMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_domain: Option<KeptTimeDomain>,
}

/// A driven sweep's S-parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KeptSweep {
    /// The elements' order (1 or 2).
    pub order: u8,
    /// S's entries, in its order.
    pub entries: Vec<KeptEntry>,
    /// Each port's de-embedding (m) the S-parameters are referred to.
    #[serde(default)]
    pub deembed: Vec<f64>,
    /// Ascending (Hz).
    pub frequencies: Vec<f64>,
    /// Per frequency, S as 2n² numbers (row by row, real then imaginary).
    pub s: Vec<Vec<f64>>,
}

/// One entry of S: a port read in its first mode, or a terminal of a terminal port.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KeptEntry {
    pub name: String,
    pub port: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal: Option<usize>,
    /// The impedance (Ω) its waves are referred to; none for a mode's own power waves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<f64>,
}

/// An eigenmode: its frequency (Hz) and its Q of the walls, dielectrics and magnetic losses.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KeptMode {
    pub frequency: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub q_walls: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub q_dielectric: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub q_magnetic: Option<f64>,
}

/// The time domain's S11 and S21 at its frequencies (Hz), each 2 numbers a frequency.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KeptTimeDomain {
    pub frequencies: Vec<f64>,
    pub s11: Vec<f64>,
    pub s21: Vec<f64>,
}

fn pairs(v: impl IntoIterator<Item = C>) -> Vec<f64> {
    v.into_iter().flat_map(|c| [c.re, c.im]).collect()
}

fn finite(v: &[f64]) -> bool {
    v.iter().all(|x| x.is_finite())
}

/// `x` if it is finite.
fn kept_number(x: Option<f64>) -> Option<f64> {
    x.filter(|x| x.is_finite())
}

impl Kept {
    /// What `results` found, as kept. A part holding a number that is not finite (which a file
    /// cannot write) is left out, as is a time domain of more frequencies than a sweep may hold.
    pub fn of(results: &Results) -> Kept {
        let sweep = results.sweep.as_ref().and_then(|sw| {
            let entries: Vec<KeptEntry> = sw
                .entries()
                .into_iter()
                .map(|e| KeptEntry { name: e.name, port: e.port, terminal: e.terminal, reference: kept_number(e.reference) })
                .collect();
            let n = entries.len();
            let s: Vec<Vec<f64>> = sw.results.iter().map(|r| pairs(r.s.rows().flatten().copied())).collect();
            let fits = s.iter().all(|row| row.len() == 2 * n * n && finite(row));
            let frequencies: Vec<f64> = sw.results.iter().map(|r| r.frequency).collect();
            (fits && finite(&frequencies) && finite(&sw.deembed)).then(|| KeptSweep {
                order: sw.order.as_u8(),
                entries,
                deembed: sw.deembed.clone(),
                frequencies,
                s,
            })
        });
        let eigenmodes = results.eigen.as_ref().map_or_else(Vec::new, |e| {
            e.modes
                .iter()
                .filter(|m| m.frequency.is_finite())
                .map(|m| KeptMode {
                    frequency: m.frequency,
                    q_walls: kept_number(m.q_walls),
                    q_dielectric: kept_number(m.q_dielectric),
                    q_magnetic: kept_number(m.q_magnetic),
                })
                .collect()
        });
        let time_domain = results.time_domain.as_ref().and_then(|t| {
            let kept = KeptTimeDomain { frequencies: t.frequencies.clone(), s11: pairs(t.s11.iter().copied()), s21: pairs(t.s21.iter().copied()) };
            kept.checked().ok()
        });
        Kept { sweep, eigenmodes, time_domain }
    }

    /// Checked as read from a file (hostile): sizes within their caps and agreeing, numbers
    /// finite, frequencies positive and ascending.
    pub fn checked(self) -> Result<Kept, String> {
        let sweep = self.sweep.map(KeptSweep::checked).transpose()?;
        if self.eigenmodes.len() > crate::EIGEN_MAX {
            return Err(format!("more than {} eigenmodes", crate::EIGEN_MAX));
        }
        for m in &self.eigenmodes {
            let qs = [m.q_walls, m.q_dielectric, m.q_magnetic];
            if !(m.frequency.is_finite() && m.frequency > 0.0) || qs.iter().flatten().any(|q| !q.is_finite()) {
                return Err("an eigenmode's frequency and Q must be finite numbers".into());
            }
        }
        let time_domain = self.time_domain.map(KeptTimeDomain::checked).transpose()?;
        Ok(Kept { sweep, eigenmodes: self.eigenmodes, time_domain })
    }
}

/// Frequencies (Hz) within the cap of a sweep, finite, positive and ascending.
fn check_frequencies(f: &[f64]) -> Result<(), String> {
    let max = emcraft_solver::driven::MAX_FREQUENCIES;
    if f.len() > max {
        return Err(format!("more than {max} frequencies"));
    }
    if f.iter().any(|x| !(x.is_finite() && *x > 0.0)) || f.windows(2).any(|w| w[1] < w[0]) {
        return Err("frequencies must be finite, positive and ascending".into());
    }
    Ok(())
}

impl KeptSweep {
    /// S's entries.
    pub fn ports(&self) -> usize {
        self.entries.len()
    }

    /// S\[p\]\[q\] at each frequency (Hz).
    pub fn curve(&self, p: usize, q: usize) -> Vec<(f64, C)> {
        let n = self.ports();
        if p >= n || q >= n {
            return Vec::new();
        }
        let k = 2 * (p * n + q);
        self.frequencies.iter().zip(&self.s).filter_map(|(f, row)| Some((*f, C::new(*row.get(k)?, *row.get(k + 1)?)))).collect()
    }

    fn checked(self) -> Result<KeptSweep, String> {
        let n = self.entries.len();
        if !(1..=2).contains(&self.order) {
            return Err(format!("a sweep's order is 1 or 2, not {}", self.order));
        }
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
        if !finite(&self.deembed) {
            return Err("de-embedding must be finite numbers".into());
        }
        check_frequencies(&self.frequencies)?;
        if self.s.len() != self.frequencies.len() {
            return Err(format!("{} frequencies but {} S matrices", self.frequencies.len(), self.s.len()));
        }
        if self.frequencies.len().saturating_mul(2 * n * n) > MAX_NUMBERS {
            return Err(format!("S-parameters of more than {MAX_NUMBERS} numbers"));
        }
        if let Some(row) = self.s.iter().find(|row| row.len() != 2 * n * n) {
            return Err(format!("an S matrix of {n} entries is {} numbers, not {}", 2 * n * n, row.len()));
        }
        if !self.s.iter().all(|row| finite(row)) {
            return Err("S-parameters must be finite numbers".into());
        }
        Ok(self)
    }
}

impl KeptTimeDomain {
    fn checked(self) -> Result<KeptTimeDomain, String> {
        check_frequencies(&self.frequencies)?;
        let n = 2 * self.frequencies.len();
        if self.s11.len() != n || self.s21.len() != n {
            return Err(format!("the time domain's S11 and S21 are 2 numbers a frequency: {n}"));
        }
        if !(finite(&self.s11) && finite(&self.s21)) {
            return Err("the time domain's S-parameters must be finite numbers".into());
        }
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sweep() -> KeptSweep {
        let entries = ["1", "2"].map(|n| KeptEntry { name: n.into(), port: 0, terminal: None, reference: None }).to_vec();
        KeptSweep { order: 1, entries, deembed: vec![0.0, 0.0], frequencies: vec![7e9, 8e9], s: vec![vec![0.5; 8], vec![0.25; 8]] }
    }

    #[test]
    fn a_kept_sweep_reads_its_entries_and_refuses_what_does_not_fit() {
        let ok = sweep().checked().unwrap();
        assert_eq!(ok.curve(1, 0), vec![(7e9, C::new(0.5, 0.5)), (8e9, C::new(0.25, 0.25))]);
        assert!(ok.curve(2, 0).is_empty());
        let bad: [(fn(&mut KeptSweep), &str); 8] = [
            (|s| s.order = 3, "order"),
            (|s| s.entries.clear(), "entries"),
            (|s| s.frequencies.reverse(), "ascending"),
            (|s| s.frequencies[0] = -1.0, "positive"),
            (|s| drop(s.s.pop()), "S matrices"),
            (|s| s.s[0].push(1.0), "numbers, not"),
            (|s| s.s[1][3] = f64::INFINITY, "finite"),
            (|s| s.entries[0].reference = Some(0.0), "reference"),
        ];
        for (spoil, why) in bad {
            let mut s = sweep();
            spoil(&mut s);
            assert!(matches!(s.checked(), Err(m) if m.contains(why)), "{why}");
        }
        let mut big = sweep();
        big.frequencies = vec![1.0; emcraft_solver::driven::MAX_FREQUENCIES + 1];
        assert!(big.checked().is_err());
    }

    #[test]
    fn kept_eigenmodes_and_time_domain_are_checked() {
        let td = KeptTimeDomain { frequencies: vec![1e9], s11: vec![0.0, 1.0], s21: vec![1.0] };
        assert!(Kept { time_domain: Some(td), ..Kept::default() }.checked().is_err());
        let mode = KeptMode { frequency: 0.0, q_walls: None, q_dielectric: None, q_magnetic: None };
        assert!(Kept { eigenmodes: vec![mode], ..Kept::default() }.checked().is_err());
        assert_eq!(Kept::of(&Results::default()), Kept::default());
    }
}
