//! The S-parameter chart: |S11| and |S21| (and the rest of the first column with more ports or
//! terminals) against frequency — in dB, as a magnitude or as a phase in degrees — the solved
//! points over the reference's curves, with the sections' cut-offs marked.

use emcraft_model::units::decibels;
use emcraft_render::chart::{Chart, Series, Style};
use emcraft_render::{Axis, Pen};
use emcraft_solver::Complex64 as C;

use super::chart::{DB_FLOOR, db_axis};
use super::reference::{reference, section_cutoffs};
use crate::{EngineError, Result, Session};

/// The most entries of S's first column the chart draws.
const MAX_CHART_ENTRIES: usize = 8;

/// The series colours of the first column's entries past S₂₁ (green, orange, purple).
const MORE_PENS: [u8; 3] = [2, 4, 3];

emcraft_macros::named_enum! {
    /// What the S-parameter chart shows of each entry: |S| in dB (the default), |S| as a
    /// magnitude from 0 to 1, or its phase in degrees.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
    pub enum SDisplay {
        #[default]
        Db = "db" => "|S| (dB)",
        Magnitude = "magnitude" => "|S| (linear)",
        Phase = "phase" => "phase (°)",
    }
}

impl SDisplay {
    /// What the chart plots of `s`: dB no lower than the chart reaches, the magnitude, or the
    /// phase in degrees (−180 to 180).
    pub fn of(self, s: C) -> f64 {
        match self {
            SDisplay::Db => decibels(s.norm()).max(DB_FLOOR),
            SDisplay::Magnitude => s.norm(),
            SDisplay::Phase => s.arg().to_degrees(),
        }
    }

    /// Entry `pq`'s name in a legend: `$|S_{21}|$`, `arg $S_{21}$`.
    fn entry(self, pq: &str) -> String {
        match self {
            SDisplay::Phase => format!("arg $S_{{{pq}}}$"),
            _ => format!("$|S_{{{pq}}}|$"),
        }
    }

    /// The y axis over `series`.
    fn axis(self, series: &[Series]) -> Axis {
        match self {
            SDisplay::Db => db_axis("$|S|$ (dB)", series),
            SDisplay::Magnitude => {
                let top = series.iter().flat_map(|s| s.points.iter().map(|p| p[1])).filter(|v| v.is_finite()).fold(1.0f64, f64::max);
                Axis::new("$|S|$ (linear, 0 to 1)", 0.0, (top * 1.05).min(10.0))
            }
            SDisplay::Phase => Axis::new("phase (°)", -190.0, 190.0),
        }
    }
}

/// A curve as series: one, or for a phase one per stretch between its wraps from +180° to
/// −180° (the first labelled), so no line crosses the chart at a wrap.
fn curve(label: String, pen: Pen, points: Vec<[f64; 2]>, display: SDisplay) -> Vec<Series> {
    if display != SDisplay::Phase {
        return vec![Series { label, pen, style: Style::Line, points }];
    }
    let mut out: Vec<Series> = Vec::new();
    let mut run: Vec<[f64; 2]> = Vec::new();
    for p in points {
        if run.last().is_some_and(|q| (p[1] - q[1]).abs() > 180.0) {
            let label = if out.is_empty() { label.clone() } else { String::new() };
            out.push(Series { label, pen, style: Style::Line, points: std::mem::take(&mut run) });
        }
        run.push(p);
    }
    let label = if out.is_empty() { label } else { String::new() };
    out.push(Series { label, pen, style: Style::Line, points: run });
    out
}

/// |S11| and |S21| in dB against frequency: the solved points and the reference curves.
pub fn sparameter_chart(session: &Session) -> Result<Chart> {
    sparameter_chart_as(session, SDisplay::Db)
}

/// The first column of S against frequency, as `display` asks: the solved points and the
/// reference curves.
///
/// ```
/// use emcraft_engine::view::{SDisplay, sparameter_chart_as};
///
/// let mut s = emcraft_engine::Session::new();
/// // Nothing solved: no chart, and why.
/// assert!(sparameter_chart_as(&s, SDisplay::Phase).is_err());
/// s.execute("file.sample", &serde_json::json!({"name": "highpass", "solve": true})).unwrap();
/// let chart = sparameter_chart_as(&s, SDisplay::Phase).unwrap();
/// assert_eq!((chart.y.min, chart.y.max), (-190.0, 190.0));
/// assert!(chart.series.iter().flat_map(|c| &c.points).all(|p| p[1].abs() <= 180.0));
/// ```
pub fn sparameter_chart_as(session: &Session, display: SDisplay) -> Result<Chart> {
    let sweep = session.results.sweep.as_ref();
    let td = session.results.time_domain.as_ref();
    let fu = session.display.frequency;
    let (f0, f1) = match (sweep, td) {
        (Some(sw), _) if !sw.results.is_empty() => (sw.results[0].frequency, sw.results[sw.results.len() - 1].frequency),
        (_, Some(t)) if !t.frequencies.is_empty() => (t.frequencies[0], t.frequencies[t.frequencies.len() - 1]),
        _ => return Err(EngineError::NotSolved { what: "results", run: "solve.frequency or solve.time_domain" }),
    };
    let pad = if f1 > f0 { 0.0 } else { 0.05 * f0 };
    let (f0, f1) = (f0 - pad, f1 + pad);
    let mut series = reference_curves(session, f0, f1, display);
    if let Some(sweep) = sweep {
        let pts = |p: usize, q: usize| sweep.results.iter().filter_map(|r| r.s.get(p, q).map(|s| [fu.from_si(r.frequency), display.of(s)])).collect();
        series.push(Series {
            label: format!("{} FEM (order {})", display.entry("21"), sweep.order.as_u8()),
            pen: Pen::Strong(0),
            style: Style::Markers,
            points: pts(1, 0),
        });
        series.push(Series { label: format!("{} FEM", display.entry("11")), pen: Pen::Strong(1), style: Style::Markers, points: pts(0, 0) });
        // More entries (a terminal port's conductors, more ports): the rest of the first column.
        let n = sweep.results.first().map_or(0, |r| r.s.ports()).min(MAX_CHART_ENTRIES);
        for (p, k) in (2..n).zip(MORE_PENS.iter().cycle()) {
            let label = format!("{} FEM", display.entry(&format!("{}1", p + 1)));
            series.push(Series { label, pen: Pen::Strong(*k), style: Style::Markers, points: pts(p, 0) });
        }
    }
    if let Some(td) = td {
        let pts = |v: &[C]| td.frequencies.iter().zip(v).map(|(f, s)| [fu.from_si(*f), display.of(*s)]).collect();
        series.extend(curve(format!("{} time domain", display.entry("21")), Pen::Series(2), pts(&td.s21), display));
        series.extend(curve(format!("{} time domain", display.entry("11")), Pen::Series(4), pts(&td.s11), display));
    }
    let marks = section_cutoffs(session).into_iter().map(|(fc, name)| (fu.from_si(fc), format!("$f_c$ {name}"))).collect();
    Ok(Chart {
        title: "S-parameters".into(),
        x: Axis::new(&format!("frequency ({})", fu.symbol()), fu.from_si(f0), fu.from_si(f1)),
        y: display.axis(&series),
        series,
        marks,
    })
}

/// The reference's S21 and S11 from `f0` to `f1` (Hz), finely, as `display` asks; none without
/// a reference.
fn reference_curves(session: &Session, f0: f64, f1: f64, display: SDisplay) -> Vec<Series> {
    let Ok(reference) = reference(session) else { return Vec::new() };
    let fu = session.display.frequency;
    // The curve: fewer modes than the comparison takes (mode matching is dense linear algebra
    // per frequency); 40 keep the curve within 10⁻² dB.
    let (n, modes) = if reference.is_mode_matching() { (160, 40) } else { (400, 1) };
    let (mut s11, mut s21) = (Vec::new(), Vec::new());
    for f in crate::numbers::evenly_spaced(f0, f1, n + 1) {
        if let Ok((r, t)) = reference.s(f, modes) {
            s11.push([fu.from_si(f), display.of(r)]);
            s21.push([fu.from_si(f), display.of(t)]);
        }
    }
    let label = reference.label();
    let mut out = curve(format!("{} {label}", display.entry("21")), Pen::Series(0), s21, display);
    out.extend(curve(format!("{} {label}", display.entry("11")), Pen::Series(1), s11, display));
    out
}
