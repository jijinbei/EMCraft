//! The S-parameter chart: |S11| and |S21| (and the rest of the first column with more ports or
//! terminals) against frequency — in dB, as a magnitude or as a phase in degrees — the solved
//! points over the reference's curves, with the sections' cut-offs marked. A fast sweep's curves
//! are lines marked only at the frequencies solved in full. Measurements (`results.import`)
//! are drawn as lines behind them; the same S can be shown in mixed mode (ports paired in
//! order) or as each port's reflection on a Smith chart.

use emcraft_model::units::decibels;
use emcraft_render::chart::{Chart, Series, Style};
use emcraft_render::{Axis, Pen};
use emcraft_solver::Complex64 as C;

use super::chart::{DB_FLOOR, db_axis};
use super::reference::{reference, section_cutoffs};
use crate::{EngineError, Result, Session};

/// The most entries of S's first column the chart draws.
const MAX_CHART_ENTRIES: usize = 64;

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
        /// The phase followed through its wraps: its slope against frequency is the delay.
        Unwrapped = "phase_unwrapped" => "phase, unwrapped (°)",
        /// The group delay τ = −dφ/dω (ns): how long the signal takes through.
        GroupDelay = "group_delay" => "group delay (ns)",
        /// Each port's reflection Γ on a Smith chart (the impedance plane's circles of
        /// constant resistance and reactance mapped onto it).
        Smith = "smith" => "Smith chart",
    }
}

impl SDisplay {
    /// What the chart plots of `s`: dB no lower than the chart reaches, the magnitude, or the
    /// phase in degrees (−180 to 180).
    pub fn of(self, s: C) -> f64 {
        match self {
            SDisplay::Db => decibels(s.norm()).max(DB_FLOOR),
            SDisplay::Magnitude | SDisplay::Smith => s.norm(),
            SDisplay::Phase | SDisplay::Unwrapped | SDisplay::GroupDelay => s.arg().to_degrees(),
        }
    }

    /// Whether a curve's points are its phase still to be followed through its wraps (and
    /// differentiated for the group delay) once the curve is whole ([`SDisplay::derived`]).
    const fn along(self) -> bool {
        matches!(self, SDisplay::Unwrapped | SDisplay::GroupDelay)
    }

    /// A whole curve's points (frequency in display units `fu`, phase in degrees as
    /// [`SDisplay::of`] gives it) as the display shows them: the phase unwrapped, or the group
    /// delay τ = −(1/360) dφ/df in ns by central differences (one-sided at the ends); others
    /// as they are.
    ///
    /// ```
    /// use emcraft_engine::view::SDisplay;
    /// use emcraft_model::units::Frequency;
    /// // A line 1 ns long: φ = −360° f τ, wrapped; f in GHz.
    /// let pts: Vec<[f64; 2]> = (0..=20).map(|k| { let f = 8.0 + 0.2 * f64::from(k); [f, (-360.0 * f * 1.0 + 180.0).rem_euclid(360.0) - 180.0] }).collect();
    /// let tau = SDisplay::GroupDelay.derived(pts.clone(), Frequency::Gigahertz);
    /// assert!(tau.iter().all(|p| (p[1] - 1.0).abs() < 1e-9), "{tau:?}");
    /// let phase = SDisplay::Unwrapped.derived(pts, Frequency::Gigahertz);
    /// assert!(phase.windows(2).all(|w| (w[1][1] - w[0][1] + 72.0).abs() < 1e-9));
    /// ```
    pub fn derived(self, mut points: Vec<[f64; 2]>, fu: emcraft_model::units::Frequency) -> Vec<[f64; 2]> {
        if !self.along() || points.len() < 2 {
            return if self == SDisplay::GroupDelay { Vec::new() } else { points };
        }
        points.sort_by(|a, b| a[0].total_cmp(&b[0]));
        let mut offset = 0.0;
        let mut previous: Option<f64> = None;
        for p in &mut points {
            if let Some(q) = previous {
                let step = p[1] + offset - q;
                offset -= 360.0 * (step / 360.0).round();
            }
            p[1] += offset;
            previous = Some(p[1]);
        }
        if self == SDisplay::Unwrapped {
            return points;
        }
        let n = points.len();
        (0..n)
            .map(|i| {
                let (a, b) = (points[i.saturating_sub(1)], points[(i + 1).min(n - 1)]);
                let df = fu.to_si(b[0]) - fu.to_si(a[0]);
                let tau = if df > 0.0 { -(b[1] - a[1]) / (360.0 * df) * 1e9 } else { f64::NAN };
                [points[i][0], tau]
            })
            .collect()
    }

    /// Entry `pq`'s name in a legend: `$|S_{21}|$`, `arg $S_{21}$`.
    fn entry(self, pq: &str) -> String {
        match self {
            SDisplay::Phase | SDisplay::Unwrapped => format!("arg $S_{{{pq}}}$"),
            SDisplay::GroupDelay => format!("$\\tau_g$ of $S_{{{pq}}}$"),
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
            SDisplay::Smith => Axis::new("Im Γ", -SMITH_EDGE, SMITH_EDGE),
            SDisplay::Unwrapped | SDisplay::GroupDelay => {
                let values = series.iter().flat_map(|s| s.points.iter().map(|p| p[1])).filter(|v| v.is_finite());
                let (lo, hi) = values.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(v), b.max(v)));
                let (lo, hi) = if lo <= hi { (lo, hi) } else { (0.0, 1.0) };
                let pad = 0.05 * (hi - lo).max(1e-9 * hi.abs().max(1.0));
                let name = if self == SDisplay::Unwrapped { "phase, unwrapped (°)" } else { "group delay (ns)" };
                Axis::new(name, lo - pad, hi + pad)
            }
        }
    }

    /// Each curve of `series` as the display shows it whole ([`SDisplay::derived`]); a delay's
    /// display keeps the transmissions only (a reflection's phase is no time through, and a
    /// small one's is noise).
    fn derive_all(self, series: &mut Vec<Series>, fu: emcraft_model::units::Frequency) {
        if !self.along() {
            return;
        }
        series.retain(|s| !reflection(&s.key));
        for s in series.iter_mut().filter(|s| s.style != Style::Markers) {
            s.points = self.derived(std::mem::take(&mut s.points), fu);
        }
        // A fast sweep's marks of its full solves are too few to follow the phase through its
        // wraps: each takes its value from its curve's at its frequency.
        let curves: Vec<(String, Vec<[f64; 2]>)> =
            series.iter().filter(|s| s.style != Style::Markers).map(|s| (s.key.clone(), s.points.clone())).collect();
        for s in series.iter_mut().filter(|s| s.style == Style::Markers) {
            let Some((_, line)) = curves.iter().find(|(k, _)| *k == s.key) else { continue };
            for p in &mut s.points {
                p[1] = line.iter().find(|q| (q[0] - p[0]).abs() <= 1e-12 * p[0].abs().max(1.0)).map_or(f64::NAN, |q| q[1]);
            }
        }
    }
}

/// Whether a curve's key names a reflection, S_pp (`s11.fem`, `run2.s22`, `s11.ref`), or a
/// mixed-mode wave's back into its own pair (`sdd11.fem`, `scd11.m1`).
fn reflection(key: &str) -> bool {
    key.split('.').any(|part| {
        let b = part.as_bytes();
        (b.len() == 3 && b[0] == b's' && b[1].is_ascii_digit() && b[1] == b[2])
            || (b.len() == 5 && b[0] == b's' && b[1].is_ascii_alphabetic() && b[2].is_ascii_alphabetic() && b[3].is_ascii_digit() && b[3] == b[4])
    })
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
    sparameter_chart_with(session, display, compare, false)
}

/// The S-parameter chart as [`sparameter_chart_compared`] draws it, or in mixed mode when
/// `mixed` (ports paired in order: the first pair's differential column), or — for
/// [`SDisplay::Smith`] — each port's reflection on a Smith chart.
///
/// ```
/// use emcraft_engine::view::{SDisplay, sparameter_chart_with};
///
/// let mut s = emcraft_engine::Session::new();
/// s.execute("file.sample", &serde_json::json!({"name": "highpass", "solve": true})).unwrap();
/// let smith = sparameter_chart_with(&s, SDisplay::Smith, &[], false).unwrap();
/// assert!(smith.look.equal && smith.series.iter().any(|c| c.key == "s11.fem"));
/// // A passive port's reflection stays inside the unit circle.
/// let s11 = smith.series.iter().find(|c| c.key == "s11.fem").unwrap();
/// assert!(s11.points.iter().all(|p| p[0].hypot(p[1]) <= 1.0 + 1e-9));
/// // Two ports do not pair into mixed mode's two pairs… but they make one: Sdd11 alone.
/// let mixed = sparameter_chart_with(&s, SDisplay::Db, &[], true).unwrap();
/// assert!(mixed.series.iter().any(|c| c.key == "sdd11.fem"));
/// ```
pub fn sparameter_chart_with(session: &Session, display: SDisplay, compare: &[u32], mixed: bool) -> Result<Chart> {
    if display == SDisplay::Smith {
        return smith_chart(session, compare, mixed);
    }
    if mixed {
        return mixed_chart(session, display);
    }
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
        let mut kept = vec![
            Series::new(&format!("run{}.s21", a.id), format!("#{} {}", a.id, display.entry("21")), p21, Style::Joined, pts(1, 0)),
            Series::new(&format!("run{}.s11", a.id), format!("#{} {}", a.id, display.entry("11")), p11, Style::Joined, pts(0, 0)),
        ];
        display.derive_all(&mut kept, fu);
        chart.series.extend(kept);
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
    let bor = session.results.revolution.as_ref();
    let fu = session.display.frequency;
    let span = sweep
        .and_then(|sw| Some((sw.results.first()?.frequency, sw.results.last()?.frequency)))
        .or_else(|| td.and_then(|t| Some((*t.frequencies.first()?, *t.frequencies.last()?))))
        .or_else(|| bor.and_then(|b| Some((*b.data.frequencies().first()?, *b.data.frequencies().last()?))))
        .or_else(|| measured_span(session));
    let Some((f0, f1)) = span else {
        return Err(EngineError::NotSolved { what: "results", run: "solve.frequency or solve.time_domain" });
    };
    let pad = if f1 > f0 { 0.0 } else { 0.05 * f0 };
    let (f0, f1) = (f0 - pad, f1 + pad);
    // The data only, and a closed form when the chart's style adds it.
    let wanted = |key: &str| crate::views::added(session, crate::views::ChartId::SParameters, key);
    let mut series: Vec<Series> = if session.driven_entry() == 0 && (wanted("s21.ref") || wanted("s11.ref")) {
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
                series.push(Series::new(key, label, pen, Style::Line, pts(p, session.driven_entry(), true)));
                series.push(Series::new(key, "", pen, Style::Markers, pts(p, session.driven_entry(), false)));
            } else {
                series.push(Series::new(key, label, pen, Style::Joined, pts(p, session.driven_entry(), true)));
            }
        };
        let input = session.driven_entry();
        let n = sweep.results.first().map_or(0, |r| r.s.ports()).min(MAX_CHART_ENTRIES);
        for p in (0..n).filter(|p| *p != input).chain(std::iter::once(input)).filter(|p| *p < n) {
            let label = format!("{} FEM (order {})", display.entry(&format!("{}{}", p + 1, input + 1)), sweep.order.as_u8());
            fem(&format!("s{}{}.fem", p + 1, input + 1), label, sparameter_pen(p), p);
        }
        if fast {
            series.push(Series::new(FULL_SOLVES, "solved in full", Pen::Ink, Style::Markers, Vec::new()));
        }
    }
    if let Some((t, td)) = td.and_then(|t| Some((t, crate::sdata::of_time_domain(t).ok()?))) {
        let pts = |p: usize, q: usize| td.curve(p, q).map(|(f, s)| [fu.from_si(f), display.of(s)]).collect();
        // Port 1's column as the guide's run gives it; a solid's other ports and columns after.
        series.extend(curve("s21.td", format!("{} time domain", display.entry("21")), Pen::Series(2), pts(1, 0), display));
        series.extend(curve("s11.td", format!("{} time domain", display.entry("11")), Pen::Series(4), pts(0, 0), display));
        let n = t.ports.min(MAX_CHART_ENTRIES);
        for &q in t.inputs.iter().filter(|q| **q < n) {
            for p in (0..n).filter(|&p| q != 0 || p >= 2) {
                let entry = format!("{}{}", p + 1, q + 1);
                series.extend(curve(
                    &format!("s{entry}.td"),
                    format!("{} time domain", display.entry(&entry)),
                    sparameter_pen(p),
                    pts(p, q),
                    display,
                ));
            }
        }
    }
    // A body of revolution's: every entry of its S, in its azimuthal order.
    if let Some(b) = bor {
        let n = b.data.outputs().min(MAX_CHART_ENTRIES);
        for q in b.data.inputs().iter().copied().filter(|q| *q < n) {
            for p in 0..n {
                let entry = format!("{}{}", p + 1, q + 1);
                let pts = b.data.curve(p, q).map(|(f, s)| [fu.from_si(f), display.of(s)]).collect();
                series.extend(curve(
                    &format!("s{entry}.bor"),
                    format!("{} revolution (m = {})", display.entry(&entry), b.m),
                    sparameter_pen(p),
                    pts,
                    display,
                ));
            }
        }
    }
    series.extend(measured_series(session, display));
    let marks = section_cutoffs(session).into_iter().map(|(fc, name)| (fu.from_si(fc), format!("$f_c$ {name}"))).collect();
    display.derive_all(&mut series, fu);
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

/// Consistent colours for each destination in full charts and the field frequency chart.
pub fn sparameter_pen(row: usize) -> Pen {
    Pen::Strong(row_colour(row))
}

/// The series colour of destination `row`: S11's, S21's, then the rest's.
fn row_colour(row: usize) -> u8 {
    match row {
        0 => 1,
        1 => 0,
        _ => MORE_PENS[(row - 2) % MORE_PENS.len()],
    }
}

/// How far the Smith chart's axes reach either side of its centre (the unit circle within).
const SMITH_EDGE: f64 = 1.08;

/// The most ports whose reflections the Smith chart draws.
const MAX_SMITH_PORTS: usize = 8;

/// The band the measurements cover (Hz), when there are any.
fn measured_span(session: &Session) -> Option<(f64, f64)> {
    let fs = session.views.measured.iter().flat_map(|m| [m.data.frequencies().first().copied(), m.data.frequencies().last().copied()]).flatten();
    fs.fold(None, |acc: Option<(f64, f64)>, f| Some(acc.map_or((f, f), |(a, b)| (a.min(f), b.max(f)))))
}

/// The rows of column `q` in the order drawn: the others first, `q`'s own (its reflection)
/// last, at most [`MAX_CHART_ENTRIES`].
fn rows_of(n: usize, q: usize) -> impl Iterator<Item = usize> {
    let n = n.min(MAX_CHART_ENTRIES);
    (0..n).filter(move |p| *p != q).chain((q < n).then_some(q))
}

/// The measurements' column of S — the driven entry's when they have it, else their first —
/// as lines behind the solved points, named by the measurement.
fn measured_series(session: &Session, display: SDisplay) -> Vec<Series> {
    let fu = session.display.frequency;
    let mut out = Vec::new();
    for (k, m) in session.views.measured.iter().enumerate() {
        let n = m.data.outputs();
        let q = if session.driven_entry() < n { session.driven_entry() } else { 0 };
        for p in rows_of(n, q) {
            let entry = format!("{}{}", p + 1, q + 1);
            let pts = m.data.curve(p, q).map(|(f, s)| [fu.from_si(f), display.of(s)]).collect();
            out.extend(curve(
                &format!("s{entry}.m{}", k + 1),
                format!("{} {}", display.entry(&entry), m.name),
                Pen::Faded(row_colour(p)),
                pts,
                display,
            ));
        }
    }
    out
}

/// Mixed mode's S of `data` with its ports paired in order, its waves named (`d1`, `c1`, …);
/// none when they do not pair (an odd number) or not every port was excited.
fn mixed_of(data: &crate::SData) -> Option<(Vec<String>, crate::SData)> {
    let pairs = crate::network::default_pairs(data.outputs())?;
    let mixed = crate::network::mixed_data(data, &pairs)?;
    Some((crate::network::mixed_names(data.outputs(), &pairs), mixed))
}

/// Entry (r, c) of mixed mode by its waves' names: `dd21` for (d2, d1), `cd11` for (c1, d1);
/// `3,d1` for an unpaired port's.
fn mixed_entry(row: &str, column: &str) -> String {
    let split = |w: &str| w.chars().next().filter(char::is_ascii_alphabetic).map(|m| (m, w[1..].to_string()));
    match (split(row), split(column)) {
        (Some((a, i)), Some((b, j))) => format!("{a}{b}{i}{j}"),
        _ => format!("{row},{column}"),
    }
}

/// The S-parameter chart in mixed mode: the first pair's differential column (Sdd11, Sdd21,
/// …, Scd11, Scd21, …) of the sweep's points and the measurements' lines.
fn mixed_chart(session: &Session, display: SDisplay) -> Result<Chart> {
    let fu = session.display.frequency;
    let mut series = Vec::new();
    let mut sources: Vec<(String, String, Pen, Style, crate::SData)> = Vec::new();
    if let Some(sw) = session.results.sweep.as_ref() {
        sources.push(("fem".into(), format!("FEM (order {})", sw.order.as_u8()), Pen::Ink, Style::Joined, sw.sdata().map_err(EngineError::failed)?));
    }
    for (k, m) in session.views.measured.iter().enumerate() {
        sources.push((format!("m{}", k + 1), m.name.clone(), Pen::Muted, Style::Line, (*m.data).clone()));
    }
    if sources.is_empty() {
        return Err(EngineError::NotSolved { what: "results", run: "solve.frequency" });
    }
    let mut span: Option<(f64, f64)> = None;
    for (key, label, pen, style, data) in &sources {
        let Some((names, mixed)) = mixed_of(data) else { continue };
        for r in rows_of(names.len(), 0) {
            let entry = mixed_entry(&names[r], &names[0]);
            let pts: Vec<[f64; 2]> = mixed.curve(r, 0).map(|(f, s)| [fu.from_si(f), display.of(s)]).collect();
            let colour = row_colour(r);
            let pen = if *pen == Pen::Ink { Pen::Strong(colour) } else { Pen::Faded(colour) };
            let label = format!("{} {label}", display.entry(&entry));
            if *style == Style::Joined {
                series.push(Series::new(&format!("s{entry}.{key}"), label, pen, Style::Joined, pts));
            } else {
                series.extend(curve(&format!("s{entry}.{key}"), label, pen, pts, display));
            }
        }
        let (a, b) = (data.frequencies().first().copied(), data.frequencies().last().copied());
        if let (Some(a), Some(b)) = (a, b) {
            span = Some(span.map_or((a, b), |(x, y)| (x.min(a), y.max(b))));
        }
    }
    let Some((f0, f1)) = span else {
        return Err(EngineError::failed(
            "mixed mode pairs the ports in order (1 with 2, 3 with 4, …): an even number of them, every one excited (results.mixed_mode pairs them otherwise)",
        ));
    };
    let pad = if f1 > f0 { 0.0 } else { 0.05 * f0 };
    display.derive_all(&mut series, fu);
    let y = display.axis(&series);
    Ok(Chart::new(
        "Mixed-mode S-parameters",
        Axis::new(&format!("frequency ({})", fu.symbol()), fu.from_si(f0 - pad), fu.from_si(f1 + pad)),
        y,
        series,
    ))
}

/// The Smith chart's grid: circles of constant resistance (0, 0.2, 0.5, 1, 2, 5) and arcs of
/// constant reactance (±0.2, ±0.5, ±1, ±2, ±5), normalized, mapped by Γ = (z − 1)/(z + 1),
/// and the real axis.
fn smith_grid() -> Vec<Series> {
    const STEPS: [f64; 5] = [0.2, 0.5, 1.0, 2.0, 5.0];
    let one = C::new(1.0, 0.0);
    let gamma = |z: C| {
        let g = (z - one) / (z + one);
        [g.re, g.im]
    };
    let line = |points: Vec<[f64; 2]>| Series { width: Some(0.8), ..Series::new("grid", "", Pen::Muted, Style::Line, points) };
    let mut out = Vec::new();
    // Along a circle of resistance r: the reactance x = tan θ from −∞ to ∞.
    for r in std::iter::once(0.0).chain(STEPS) {
        let mut pts = vec![[1.0, 0.0]];
        pts.extend((1..360).map(|k| gamma(C::new(r, (std::f64::consts::PI * (f64::from(k) / 360.0 - 0.5)).tan()))));
        pts.push([1.0, 0.0]);
        out.push(line(pts));
    }
    // Along an arc of reactance x: the resistance r = tan φ from 0 to ∞.
    for x in STEPS.into_iter().flat_map(|x| [x, -x]) {
        let mut pts: Vec<[f64; 2]> = (0..180).map(|k| gamma(C::new((std::f64::consts::FRAC_PI_2 * f64::from(k) / 180.0).tan(), x))).collect();
        pts.push([1.0, 0.0]);
        out.push(line(pts));
    }
    out.push(line(vec![[-1.0, 0.0], [1.0, 0.0]]));
    out
}

/// Each port's reflection on a Smith chart: the sweep's points (mixed mode's waves' when
/// `mixed`), the time domain's and a body of revolution's lines, the measurements' and the
/// kept solves `compare`.
fn smith_chart(session: &Session, compare: &[u32], mixed: bool) -> Result<Chart> {
    let mut series = smith_grid();
    let grid = series.len();
    let gamma = |c: C| [c.re, c.im];
    // Reflections of `data`: each entry's own (p, p), or mixed mode's waves'.
    let reflections = |data: &crate::SData| -> Vec<(String, usize, Vec<[f64; 2]>)> {
        if mixed {
            let Some((names, m)) = mixed_of(data) else { return Vec::new() };
            (0..names.len().min(MAX_SMITH_PORTS))
                .map(|r| (mixed_entry(&names[r], &names[r]), r, m.curve(r, r).map(|(_, s)| gamma(s)).collect()))
                .collect()
        } else {
            let n = data.outputs().min(MAX_SMITH_PORTS);
            (0..n)
                .filter(|p| data.inputs().contains(p))
                .map(|p| (format!("{}{}", p + 1, p + 1), p, data.curve(p, p).map(|(_, s)| gamma(s)).collect()))
                .collect()
        }
    };
    let entry = |e: &str| format!("$S_{{{e}}}$");
    if let Some(sw) = session.results.sweep.as_ref() {
        let data = sw.sdata().map_err(EngineError::failed)?;
        for (e, p, pts) in reflections(&data) {
            series.push(Series::new(
                &format!("s{e}.fem"),
                format!("{} FEM (order {})", entry(&e), sw.order.as_u8()),
                sparameter_pen(p),
                Style::Joined,
                pts,
            ));
        }
    }
    if !mixed {
        if let Some(td) = session.results.time_domain.as_ref().and_then(|t| crate::sdata::of_time_domain(t).ok()) {
            for (e, p, pts) in reflections(&td) {
                series.push(Series::new(&format!("s{e}.td"), format!("{} time domain", entry(&e)), Pen::Series(row_colour(p)), Style::Line, pts));
            }
        }
        if let Some(b) = session.results.revolution.as_ref() {
            for (e, p, pts) in reflections(&b.data) {
                series.push(Series::new(
                    &format!("s{e}.bor"),
                    format!("{} revolution (m = {})", entry(&e), b.m),
                    Pen::Series(row_colour(p)),
                    Style::Line,
                    pts,
                ));
            }
        }
    }
    for (k, m) in session.views.measured.iter().enumerate() {
        for (e, p, pts) in reflections(&m.data) {
            series.push(Series::new(&format!("s{e}.m{}", k + 1), format!("{} {}", entry(&e), m.name), Pen::Faded(row_colour(p)), Style::Line, pts));
        }
    }
    if !mixed {
        for (k, a) in crate::archive::picked(session, compare)?.into_iter().enumerate() {
            let Some(sw) = a.kept.sweep.as_ref() else { continue };
            let pts = sw.curve(0, 0).into_iter().map(|(_, s)| gamma(s)).collect();
            series.push(Series::new(&format!("run{}.s11", a.id), format!("#{} {}", a.id, entry("11")), compare_pens(k)[1], Style::Joined, pts));
        }
    }
    if series.len() == grid {
        return Err(EngineError::NotSolved { what: "results", run: "solve.frequency, or a measurement (results.import)" });
    }
    let mut chart = Chart::new(
        if mixed { "Smith chart (mixed mode)" } else { "Smith chart" },
        Axis::new("Re Γ", -SMITH_EDGE, SMITH_EDGE),
        SDisplay::Smith.axis(&[]),
        series,
    );
    chart.look.grid = false;
    chart.look.equal = true;
    Ok(chart)
}
