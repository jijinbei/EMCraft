//! The S-parameter chart: |S11| and |S21| (and the rest of the first column with more ports or
//! terminals) against frequency — in dB, as a magnitude or as a phase in degrees — the solved
//! points over the reference's curves, with the sections' cut-offs marked. A fast sweep's curves
//! are lines marked only at the frequencies solved in full.

use emcraft_model::units::decibels;
use emcraft_render::chart::{Chart, Series, Style};
use emcraft_render::{Axis, Pen};
use emcraft_solver::Complex64 as C;

use super::chart::{DB_FLOOR, db_axis};
use super::reference::{reference, section_cutoffs};
use crate::{EngineError, Result, Session};

/// The most entries of S's first column the chart draws.
const MAX_CHART_ENTRIES: usize = 8;

/// The key of the legend's entry for a fast sweep's marks: the frequencies solved in full (the
/// rest were solved in its reduced basis, the line between them).
pub const FULL_SOLVES: &str = "full";

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
fn curve(key: &str, label: String, pen: Pen, points: Vec<[f64; 2]>, display: SDisplay) -> Vec<Series> {
    if display != SDisplay::Phase {
        return vec![Series::new(key, label, pen, Style::Line, points)];
    }
    let mut out: Vec<Series> = Vec::new();
    let mut run: Vec<[f64; 2]> = Vec::new();
    for p in points {
        if run.last().is_some_and(|q| (p[1] - q[1]).abs() > 180.0) {
            let label = if out.is_empty() { label.clone() } else { String::new() };
            out.push(Series::new(key, label, pen, Style::Line, std::mem::take(&mut run)));
        }
        run.push(p);
    }
    let label = if out.is_empty() { label } else { String::new() };
    out.push(Series::new(key, label, pen, Style::Line, run));
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
    sparameter_chart_compared(session, display, &[])
}

/// The pens the `k`-th kept solve drawn over the chart takes: its S21's, its S11's.
pub fn compare_pens(k: usize) -> [Pen; 2] {
    [0, 1].map(|j| Pen::Series(MORE_PENS[(2 * k + j) % MORE_PENS.len()]))
}

/// The S-parameter chart with the sweeps of kept solves `compare` ([`crate::archive`]) drawn
/// over it: each one's |S11| and |S21| as lines through its points, named by its id (`#3`).
/// With nothing solved now (a project just opened), the kept solves' alone.
pub fn sparameter_chart_compared(session: &Session, display: SDisplay, compare: &[u32]) -> Result<Chart> {
    let fu = session.display.frequency;
    let (mut chart, own) = match sparameter_chart_now(session, display) {
        Ok(chart) => (chart, true),
        Err(e) if compare.is_empty() => return Err(e),
        Err(_) => {
            let x = Axis::new(&format!("frequency ({})", fu.symbol()), f64::INFINITY, f64::NEG_INFINITY);
            (Chart::new("S-parameters", x, display.axis(&[]), Vec::new()), false)
        }
    };
    let mut added = false;
    for (k, a) in crate::archive::picked(session, compare)?.into_iter().enumerate() {
        let Some(sw) = a.kept.sweep.as_ref() else { continue };
        added = true;
        let pts = |p: usize, q: usize| sw.curve(p, q).into_iter().map(|(f, s)| [fu.from_si(f), display.of(s)]).collect();
        // Its solved points, as the session's own are drawn: data, not curves through it.
        let [p21, p11] = compare_pens(k);
        chart.series.push(Series::new(&format!("run{}.s21", a.id), format!("#{} {}", a.id, display.entry("21")), p21, Style::Joined, pts(1, 0)));
        chart.series.push(Series::new(&format!("run{}.s11", a.id), format!("#{} {}", a.id, display.entry("11")), p11, Style::Joined, pts(0, 0)));
    }
    if !own && !added {
        return Err(EngineError::NotSolved { what: "results", run: "solve.frequency or solve.time_domain" });
    }
    // The axes take in what was added.
    if added {
        for s in chart.series.iter().flat_map(|s| &s.points) {
            chart.x.min = chart.x.min.min(s[0]);
            chart.x.max = chart.x.max.max(s[0]);
        }
    }
    // One frequency alone: a little either side of it, as the session's own chart.
    if !own && !(chart.x.min.is_finite() && chart.x.max.is_finite()) {
        (chart.x.min, chart.x.max) = (0.0, 1.0);
    } else if !own && chart.x.max <= chart.x.min {
        let pad = 0.05 * chart.x.min.abs().max(f64::MIN_POSITIVE);
        (chart.x.min, chart.x.max) = (chart.x.min - pad, chart.x.max + pad);
    }
    if !compare.is_empty() {
        chart.y = display.axis(&chart.series);
    }
    Ok(chart)
}

/// The session's own S-parameter chart: its sweep's and time domain's, with the closed form
/// when the chart's style adds it.
fn sparameter_chart_now(session: &Session, display: SDisplay) -> Result<Chart> {
    let sweep = session.results.sweep.as_ref();
    let td = session.results.time_domain.as_ref();
    let fu = session.display.frequency;
    let span = sweep
        .and_then(|sw| Some((sw.results.first()?.frequency, sw.results.last()?.frequency)))
        .or_else(|| td.and_then(|t| Some((*t.frequencies.first()?, *t.frequencies.last()?))));
    let Some((f0, f1)) = span else {
        return Err(EngineError::NotSolved { what: "results", run: "solve.frequency or solve.time_domain" });
    };
    let pad = if f1 > f0 { 0.0 } else { 0.05 * f0 };
    let (f0, f1) = (f0 - pad, f1 + pad);
    // The data only, and a closed form when the chart's style adds it.
    let wanted = |key: &str| crate::views::added(session, crate::views::ChartId::SParameters, key);
    let mut series: Vec<Series> = if wanted("s21.ref") || wanted("s11.ref") {
        reference_curves(session, f0, f1, display).into_iter().filter(|c| wanted(&c.key)).collect()
    } else {
        Vec::new()
    };
    if let Some(sweep) = sweep {
        let pts = |p: usize, q: usize, all: bool| {
            sweep.results.iter().filter(|r| all || r.full).filter_map(|r| r.s.get(p, q).map(|s| [fu.from_si(r.frequency), display.of(s)])).collect()
        };
        // A fast sweep's curves: lines through every point, marked where solved in full.
        let fast = sweep.results.iter().any(|r| !r.full);
        let mut fem = |key: &str, label: String, pen: Pen, p: usize| {
            if fast {
                series.push(Series::new(key, label, pen, Style::Line, pts(p, 0, true)));
                series.push(Series::new(key, "", pen, Style::Markers, pts(p, 0, false)));
            } else {
                series.push(Series::new(key, label, pen, Style::Joined, pts(p, 0, true)));
            }
        };
        fem("s21.fem", format!("{} FEM (order {})", display.entry("21"), sweep.order.as_u8()), Pen::Strong(0), 1);
        fem("s11.fem", format!("{} FEM", display.entry("11")), Pen::Strong(1), 0);
        // More entries (a terminal port's conductors, more ports): the rest of the first column.
        let n = sweep.results.first().map_or(0, |r| r.s.ports()).min(MAX_CHART_ENTRIES);
        for (p, k) in (2..n).zip(MORE_PENS.iter().cycle()) {
            fem(&format!("s{}1.fem", p + 1), format!("{} FEM", display.entry(&format!("{}1", p + 1))), Pen::Strong(*k), p);
        }
        if fast {
            series.push(Series::new(FULL_SOLVES, "solved in full", Pen::Ink, Style::Markers, Vec::new()));
        }
    }
    if let Some(td) = td {
        let pts = |v: &[C]| td.frequencies.iter().zip(v).map(|(f, s)| [fu.from_si(*f), display.of(*s)]).collect();
        series.extend(curve("s21.td", format!("{} time domain", display.entry("21")), Pen::Series(2), pts(&td.s21), display));
        series.extend(curve("s11.td", format!("{} time domain", display.entry("11")), Pen::Series(4), pts(&td.s11), display));
    }
    let marks = section_cutoffs(session).into_iter().map(|(fc, name)| (fu.from_si(fc), format!("$f_c$ {name}"))).collect();
    let y = display.axis(&series);
    let mut chart = Chart::new("S-parameters", Axis::new(&format!("frequency ({})", fu.symbol()), fu.from_si(f0), fu.from_si(f1)), y, series);
    chart.marks = marks;
    Ok(chart)
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
    // The reference is at the ports' faces: moved to the sweep's reference planes as the solver
    // moves its own (results.compare does the same).
    let sweep = session.results.sweep.as_deref().filter(|sw| sw.deembed.iter().any(|d| *d != 0.0));
    let one = C::new(1.0, 0.0);
    let moved = |port: usize, f: f64| sweep.and_then(|sw| crate::results::deembedding(sw, port, f).ok()).unwrap_or(one);
    for f in crate::numbers::evenly_spaced(f0, f1, n + 1) {
        if let Ok((r, t)) = reference.s(f, modes) {
            let (a, b) = (moved(0, f), moved(1, f));
            let (r, t) = (r * a * a, t * a * b);
            s11.push([fu.from_si(f), display.of(r)]);
            s21.push([fu.from_si(f), display.of(t)]);
        }
    }
    let label = reference.label();
    let mut out = curve("s21.ref", format!("{} {label}", display.entry("21")), Pen::Series(0), s21, display);
    out.extend(curve("s11.ref", format!("{} {label}", display.entry("11")), Pen::Series(1), s11, display));
    out
}
