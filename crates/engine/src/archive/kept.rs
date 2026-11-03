//! What a kept solve found, as the history shows and compares it and the project file keeps it:
//! the sweep's S-parameters (entries, reference impedances, de-embedding), the eigenmodes'
//! frequencies and Q, the time domain's S11 and S21 — the S-parameters as [`SData`]. Never the
//! mesh, the system or the fields: those come back by solving again. The file's form of them,
//! read as hostile input, is [`super::file::StoredResults`].

use emcraft_fem::Order;
use serde::{Deserialize, Serialize};

use crate::{Results, SData};

/// Most entries (ports' modes, terminals) of a kept sweep's S.
pub const MAX_ENTRIES: usize = crate::sdata::MAX_ENTRIES;

/// Longest name of an entry.
pub(crate) const MAX_ENTRY_NAME: usize = 64;

/// What a kept solve found, as shown and compared.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Kept {
    pub sweep: Option<KeptSweep>,
    pub eigenmodes: Vec<KeptMode>,
    /// The time domain's S11 and S21 (entry 0 excited).
    pub time_domain: Option<SData>,
}

/// A driven sweep's S-parameters.
#[derive(Clone, Debug, PartialEq)]
pub struct KeptSweep {
    pub order: Order,
    /// S's entries, in its order.
    pub entries: Vec<KeptEntry>,
    /// Each port's de-embedding (m) the S-parameters are referred to.
    pub deembed: Vec<f64>,
    /// S (n × n, n the entries).
    pub s: SData,
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

/// `x` if it is finite.
fn kept_number(x: Option<f64>) -> Option<f64> {
    x.filter(|x| x.is_finite())
}

impl Kept {
    /// What `results` found, as kept. A part holding a number that is not finite (which a file
    /// cannot write), or more of them than a set may hold, is left out.
    pub fn of(results: &Results) -> Kept {
        let sweep = results.sweep.as_ref().and_then(|sw| {
            let entries: Vec<KeptEntry> = sw
                .entries()
                .into_iter()
                .map(|e| KeptEntry { name: e.name, port: e.port, terminal: e.terminal, reference: kept_number(e.reference) })
                .collect();
            let s = sw.sdata().ok()?;
            (s.outputs() == entries.len() && sw.deembed.iter().all(|d| d.is_finite())).then(|| KeptSweep {
                order: sw.order,
                entries,
                deembed: sw.deembed.clone(),
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
        let time_domain = results.time_domain.as_ref().and_then(|t| crate::sdata::of_time_domain(t).ok());
        Kept { sweep, eigenmodes, time_domain }
    }
}

impl KeptSweep {
    /// S's entries.
    pub fn ports(&self) -> usize {
        self.entries.len()
    }

    /// S\[p\]\[q\] at each frequency (Hz).
    pub fn curve(&self, p: usize, q: usize) -> Vec<(f64, emcraft_solver::Complex64)> {
        self.s.curve(p, q).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_solved_keeps_nothing() {
        assert_eq!(Kept::of(&Results::default()), Kept::default());
    }
}
