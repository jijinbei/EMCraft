//! The Setup step's band: the one proposed before any is set (the ports' guide's), the
//! frequencies the solve will work at as the form stands, and the small diagram of them over
//! the band with the ports' and sections' cut-offs near it ([`diagram`]), for each window to
//! draw.

use serde_json::{Value, json};

use super::Solver;
use super::form::{Fields, integer, positive};
use crate::lang::{args, trf};

/// The band (Hz) the model's first port with a rectangular guide asks for: a standard guide's
/// (air), else its TE10 band in its filling.
pub fn proposed(m: &emcraft_model::Model) -> Option<(f64, f64)> {
    m.ports.iter().find_map(|p| {
        let (a, _) = m.port_shape_of(p).ok()?.sides()?;
        let n = m.port_material(p.end).map_or(1.0, |x| (x.eps_r.get() * x.mu_r.get()).sqrt());
        let band = if (n - 1.0).abs() < 1e-9 { emcraft_engine::guides::standard(a) } else { None };
        band.or_else(|| emcraft_engine::guides::te10_band(a, n)).map(|b| (b.start, b.stop))
    })
}

/// The form with the proposed band filled in (display units), when nothing is set yet.
pub fn with_proposal(f: Fields, s: &emcraft_engine::Session) -> Fields {
    let empty = f.start.trim().is_empty() && f.stop.trim().is_empty() && s.model().job.range().is_none();
    match proposed(s.model()).filter(|_| empty) {
        Some((a, z)) => {
            let unit = s.display().frequency;
            Fields { start: format!("{}", unit.from_si(a)), stop: format!("{}", unit.from_si(z)), ..f }
        }
        None => f,
    }
}

/// The frequencies (display units) the solve will work at as the form stands: the sweep's
/// points over the band, or the one frequency of an eigenmode or radiation solve.
///
/// ```
/// use emcraft_ui_core::steps::{Solver, band::solves, form::Fields};
/// let f = Fields { start: "6".into(), stop: "9".into(), points: "4".into(), ..Fields::default() };
/// assert_eq!(solves(Solver::Frequency, &f, 1000), vec![6.0, 7.0, 8.0, 9.0]);
/// ```
pub fn solves(solver: Solver, f: &Fields, limit: u64) -> Vec<f64> {
    match solver {
        Solver::Frequency | Solver::TimeDomain | Solver::System => {
            let (Some((a, z)), Ok(n)) = (f.band(), integer(&f.points, 1, limit)) else { return Vec::new() };
            (0..n).map(|k| if n == 1 { a } else { a + (z - a) * k as f64 / (n - 1) as f64 }).collect()
        }
        Solver::Pcb => Vec::new(),
        Solver::Eigenmodes | Solver::Radiation => positive(&f.frequency).map_or_default(|v| vec![v]),
    }
}

/// A cut-off near the band: where (display units), its label, and whether it is a port's first
/// mode (below it no power enters).
#[derive(Clone, Debug, PartialEq)]
pub struct Cut {
    pub at: f64,
    pub label: String,
    pub first: bool,
}

/// The band's diagram: the axis's span, the band, the solve's frequencies, the cut-offs near it,
/// the axis's ticks, and its tip in words (display units throughout).
#[derive(Clone, Debug, PartialEq)]
pub struct Diagram {
    pub span: (f64, f64),
    pub band: (f64, f64),
    pub solves: Vec<f64>,
    pub cuts: Vec<Cut>,
    pub ticks: Vec<f64>,
    pub tip: String,
}

impl Diagram {
    /// What it shows, for `ui.inspect`.
    pub fn inspect(&self) -> Value {
        json!({"cutoffs": self.cuts.iter().map(|c| c.label.clone()).collect::<Vec<_>>(), "solves": self.solves.len()})
    }
}

/// The diagram of `solves` over the form's band (else the one frequency) in session `s`'s
/// units, with the cut-offs within a third of the band beyond it.
pub fn diagram(s: &emcraft_engine::Session, f: &Fields, solves: Vec<f64>) -> Option<Diagram> {
    let unit = s.display().frequency;
    let (a, z) = f.band().or_else(|| solves.first().map(|x| (*x, *x)))?;
    let width = (z - a).max(z.abs() * 1e-3).max(1e-12);
    let cuts: Vec<Cut> = emcraft_engine::workflow::cutoffs(s)
        .into_iter()
        .map(|c| Cut { at: unit.from_si(c.frequency), label: crate::chart_words::translate(&c.label), first: c.first })
        .filter(|c| c.at >= a - 0.35 * width && c.at <= z + 0.35 * width)
        .collect();
    let lo = cuts.iter().map(|c| c.at).fold(a, f64::min);
    let hi = cuts.iter().map(|c| c.at).fold(z, f64::max);
    let pad = 0.04 * (hi - lo).max(width);
    let (lo, hi) = (lo - pad, hi + pad);
    let raw = (hi - lo) / 6.0;
    let mag = 10f64.powf(raw.log10().floor());
    let step = [1.0, 2.0, 5.0, 10.0].into_iter().map(|k| k * mag).find(|s| *s >= raw).unwrap_or(10.0 * mag);
    let first = (lo / step).ceil() * step;
    let ticks = (0..12).map(|k| ((first + step * f64::from(k)) / step).round() * step).take_while(|v| v.is_finite() && *v <= hi).collect();
    let fmt = crate::dialogs::short_number;
    let n = solves.len();
    let template = if n == 1 {
        t!("band {a} to {z} {unit}; the solve works at {n} frequency")
    } else {
        t!("band {a} to {z} {unit}; the solve works at {n} frequencies")
    };
    let lines = std::iter::once(trf(template, args!(a = fmt(a), z = fmt(z), unit = unit.symbol(), n = n))).chain(
        cuts.iter().map(|c| trf(t!("cut-off {mode}: {f} {unit}"), args!(mode = c.label, f = fmt((c.at * 1e4).round() / 1e4), unit = unit.symbol()))),
    );
    let tip = lines.collect::<Vec<_>>().join("\n");
    Some(Diagram { span: (lo, hi), band: (a, z), solves, cuts, ticks, tip })
}
