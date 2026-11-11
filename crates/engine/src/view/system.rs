//! A system's charts from `solve.system`'s result: its gain and noise figure along the chain at
//! the budget's frequency (each stage's output, stage by stage), or the chain's noise figure or
//! gain over the band.

use emcraft_render::chart::{Chart, Series, Style};
use emcraft_render::{Axis, Pen};

use crate::system::{Point, SystemResult};
use crate::{EngineError, Result, Session};

emcraft_macros::named_enum! {
    /// What a system's chart shows.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
    pub enum SystemChart {
        /// The chain's gain and noise figure at each stage's output, at the budget's frequency.
        #[default]
        Along = "along" => "Along the chain",
        /// The chain's noise figure over the band.
        Figure = "figure" => "Noise figure",
        /// The chain's available gain over the band.
        Gain = "gain" => "Gain",
    }
}

/// A y axis over `values`, padded by a tenth (a unit either side when flat).
fn y_axis(label: &str, values: impl Iterator<Item = f64>) -> Axis {
    let (lo, hi) = values.filter(|v| v.is_finite()).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), y| (a.min(y), b.max(y)));
    if !(lo.is_finite() && hi.is_finite()) {
        return Axis::new(label, 0.0, 1.0);
    }
    let pad = if hi - lo < 1e-9 { 1.0 } else { 0.1 * (hi - lo) };
    Axis::new(label, lo - pad, hi + pad)
}

/// The system's chart `what`, frequencies in the session's display unit.
pub fn system_chart(s: &Session, what: SystemChart) -> Result<Chart> {
    let r = s.results.system.as_deref().ok_or(EngineError::NotSolved { what: "system", run: "solve.system" })?;
    let unit = s.display.frequency;
    Ok(match what {
        SystemChart::Along => along(r, s.show_frequency(r.at), unit.symbol()),
        SystemChart::Figure => over_band(r, unit, "nf", "NF", "noise figure (dB)", |p| p.rows.last().and_then(|x| x.nf_db)),
        SystemChart::Gain => over_band(r, unit, "gain", "gain", "gain (dB)", |p| p.rows.last().and_then(|x| x.gain_db)),
    })
}

/// The gain and noise figure at each stage's output (stage k at x = k, the source at 0), the
/// stages marked by name.
fn along(r: &SystemResult, f: f64, unit: &str) -> Chart {
    let empty = Point { frequency: r.at, s: [[0.0; 2]; 4], rows: Vec::new(), system_temperature: None, sensitivity_dbm: None, why: None };
    let p = r.nearest(r.at).unwrap_or(&empty);
    let at = |pick: fn(&crate::system::Row) -> Option<f64>, start: f64| -> Vec<[f64; 2]> {
        std::iter::once([0.0, start]).chain(p.rows.iter().enumerate().filter_map(|(k, row)| Some([(k + 1) as f64, pick(row)?]))).collect()
    };
    let series = vec![
        Series::new("gain", "gain", Pen::Series(0), Style::Joined, at(|x| x.gain_db, 0.0)),
        Series::new("nf", "NF", Pen::Series(1), Style::Joined, at(|x| x.nf_db, 0.0)),
    ];
    let y = y_axis("dB", series.iter().flat_map(|c| c.points.iter().map(|q| q[1])));
    let n = r.stages.len().max(1) as f64;
    // Room past the last stage for its mark's name.
    let mut chart = Chart::new(format!("System budget at {f} {unit}"), Axis::new("stage", 0.0, n + 0.6), y, series);
    chart.marks = r.stages.iter().enumerate().map(|(k, (name, _))| ((k + 1) as f64, name.clone())).collect();
    chart
}

/// One quantity of the chain against frequency.
fn over_band(r: &SystemResult, unit: emcraft_model::units::Frequency, key: &str, label: &str, axis: &str, pick: fn(&Point) -> Option<f64>) -> Chart {
    let points: Vec<[f64; 2]> = r.points.iter().filter_map(|p| Some([unit.from_si(p.frequency), pick(p)?])).collect();
    let (lo, hi) = r.points.first().zip(r.points.last()).map_or((0.0, 1.0), |(a, b)| (unit.from_si(a.frequency), unit.from_si(b.frequency)));
    let along = format!("frequency ({})", unit.symbol());
    let x = if hi > lo { Axis::new(&along, lo, hi) } else { Axis::new(&along, lo - 1.0, lo + 1.0) };
    let y = y_axis(axis, points.iter().map(|q| q[1]));
    let mut chart = Chart::new("System budget", x, y, vec![Series::new(key, label, Pen::Series(0), Style::Joined, points)]);
    chart.marks = vec![(unit.from_si(r.at), "budget".to_owned())];
    chart
}
