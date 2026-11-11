//! A system's budget as a table: its columns' names, a stage's cells at the budget's frequency,
//! and what the receiver reads under it.

use emcraft_engine::Session;
use emcraft_engine::system::{Point, Row, SystemResult};

use crate::lang::{Text, trf};

/// The budget's columns.
pub const HEADERS: [Text; 11] = [
    t!("stage"),
    t!("gain (dB)"),
    t!("total gain (dB)"),
    t!("NF (dB)"),
    t!("Te (K)"),
    t!("signal (dBm)"),
    t!("noise (dBm)"),
    t!("SNR (dB)"),
    t!("IIP3 (dBm)"),
    t!("OIP3 (dBm)"),
    t!("OP1dB (dBm)"),
];

/// Stage `name`'s cells, a column each ("—" for what is not known).
pub fn cells(name: &str, x: &Row) -> Vec<String> {
    let num = |v: Option<f64>, digits: usize| v.map_or_else(|| "—".to_owned(), |v| format!("{v:.digits$}"));
    std::iter::once(name.to_owned())
        .chain([
            num(x.stage_gain_db, 2),
            num(x.gain_db, 2),
            num(x.nf_db, 3),
            num(x.noise_temperature, 1),
            num(x.signal_dbm, 2),
            num(x.noise_dbm, 2),
            num(x.snr_db, 2),
            num(x.iip3_dbm, 2),
            num(x.oip3_dbm, 2),
            num(x.op1db_dbm, 2),
        ])
        .collect()
}

/// The budget at its frequency: the point read there, and each stage's cells.
pub fn table(r: &SystemResult) -> Option<(&Point, Vec<Vec<String>>)> {
    let p = r.nearest(r.at)?;
    Some((p, r.stages.iter().zip(&p.rows).map(|((name, _), x)| cells(name, x)).collect()))
}

/// What the receiver reads at `p` (its system temperature and sensitivity), or what it lacks
/// for it (a bandwidth); and why the chain could not be read further, when it could not.
pub fn summary(s: &Session, p: &Point) -> Vec<(String, bool)> {
    let reading = match (p.system_temperature, p.sensitivity_dbm) {
        (Some(t), Some(sens)) => {
            Some((trf(t!("System temperature {t} K; sensitivity {s} dBm"), crate::args!(t = format!("{t:.1}"), s = format!("{sens:.2}"))), false))
        }
        _ if s.model().system.receiver.bandwidth.is_none() => {
            Some((crate::lang::tr(t!("Give the receiver a bandwidth (on its node) for its noise, SNR and sensitivity.")).to_owned(), true))
        }
        _ => None,
    };
    reading.into_iter().chain(p.why.clone().map(|w| (w, true))).collect()
}
