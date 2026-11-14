//! The particle run's page as every window shows it (docs/adr/0012-particles.md, stage B):
//! the energy in the field and in the particles over time, each conducting wall's current, the
//! particles' phase space along the run's axis, each probe's field, and a table of what the run
//! gave. The charts are drawn from the result the session keeps.

use emcraft_engine::Session;
use emcraft_engine::particles::PicResult;
use emcraft_model::particles::{Face, Wall};
use emcraft_render::Pen;
use emcraft_render::axes::Axis;
use emcraft_render::chart::{Chart, Series, Style};

use super::Table;
use crate::lang::{args, tr, trf};

/// The axes' names.
const AXES: [&str; 3] = ["x", "y", "z"];

/// The axis the particles run along: the first emitter's normal, else a plasma's wave's, else
/// the held voltage's, else z.
pub fn along(s: &Session) -> usize {
    let m = &s.model().particles;
    m.emitters
        .first()
        .map(|e| e.face.axis)
        .or_else(|| m.plasmas.iter().find_map(|p| p.wave.map(|w| w.axis)))
        .or_else(|| m.voltage.map(|v| v.axis))
        .unwrap_or(2)
        .min(2)
}

/// The span of `values` with a little room, never empty.
fn span(values: impl Iterator<Item = f64>) -> (f64, f64) {
    let (lo, hi) = values.filter(|v| v.is_finite()).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(v), b.max(v)));
    if !(lo.is_finite() && hi.is_finite()) {
        return (0.0, 1.0);
    }
    let pad = 0.05 * (hi - lo).max(1e-12 * hi.abs().max(lo.abs()).max(1e-30));
    (lo - pad, hi + pad)
}

/// The prefix that puts the largest of `values` between 1 and 1000: its factor and symbol.
fn prefix(values: impl Iterator<Item = f64>) -> (f64, &'static str) {
    let most = values.filter(|v| v.is_finite()).fold(0.0, |m: f64, v| m.max(v.abs()));
    const PREFIXES: [(f64, &str); 7] = [(1e3, "k"), (1.0, ""), (1e-3, "m"), (1e-6, "µ"), (1e-9, "n"), (1e-12, "p"), (1e-15, "f")];
    PREFIXES.iter().copied().find(|(f, _)| most >= *f).unwrap_or((1e-15, "f"))
}

/// `series` with their values scaled to the prefix their largest takes, and its symbol.
fn scaled(series: Vec<Series>) -> (Vec<Series>, &'static str) {
    let (factor, symbol) = prefix(series.iter().flat_map(|s| s.points.iter().map(|p| p[1])));
    let series = series.into_iter().map(|s| Series { points: s.points.iter().map(|p| [p[0], p[1] / factor]).collect(), ..s }).collect();
    (series, symbol)
}

/// A chart of `series` over the run's time (ns), its words translated.
fn over_time(title: &str, y_label: &str, series: Vec<Series>, r: &PicResult) -> Chart {
    let end = r.steps as f64 * r.dt * 1e9;
    let (lo, hi) = span(series.iter().flat_map(|s| s.points.iter().map(|p| p[1])));
    let mut chart = Chart::new(title, Axis::new("time (ns)", 0.0, end.max(1e-12)), Axis::new(y_label, lo, hi), series);
    crate::chart_words::translate_chart(&mut chart);
    chart
}

/// The energy in the field and in the particles (J) over time.
pub fn energy(r: &PicResult) -> Chart {
    let at = |f: fn(&emcraft_engine::particles::Record) -> f64| r.history.iter().map(|h| [h.t * 1e9, f(h)]).collect();
    let series = vec![
        Series::new("field", "field", Pen::Series(1), Style::Line, at(|h| h.field)),
        Series::new("kinetic", "particles", Pen::Series(2), Style::Line, at(|h| h.kinetic)),
    ];
    let (series, p) = scaled(series);
    over_time("Energy", &format!("energy ({p}J)"), series, r)
}

/// Each conducting wall's current (A) over time, those that collect any.
pub fn currents(r: &PicResult) -> Option<Chart> {
    let series: Vec<Series> = (0..3)
        .filter(|a| r.walls[*a] == Wall::Conductor)
        .flat_map(|axis| [false, true].map(|high| Face { axis, high }))
        .enumerate()
        .filter_map(|(k, face)| {
            let points: Vec<[f64; 2]> = r.history.iter().map(|h| [h.t * 1e9, h.collected[face.axis][usize::from(face.high)]]).collect();
            points
                .iter()
                .any(|p| p[1] != 0.0)
                .then(|| Series::new(&face.name(), face.name(), Pen::Series(u8::try_from(k % 8 + 1).unwrap_or(1)), Style::Line, points))
        })
        .collect();
    if series.is_empty() {
        return None;
    }
    let (series, p) = scaled(series);
    Some(over_time("Current on the walls", &format!("current ({p}A)"), series, r))
}

/// The last particles' phase space: their position along `axis` (display units) against γβ
/// along it.
pub fn phase_space(s: &Session, r: &PicResult, axis: usize) -> Option<Chart> {
    let unit = s.display().length;
    let c = emcraft_model::units::C0;
    let points: Vec<[f64; 2]> = r.snapshot.iter().map(|p| [unit.from_si(p.x[axis]), p.u[axis] / c]).collect();
    if points.is_empty() {
        return None;
    }
    let length = r.cells[axis] as f64 * r.h[axis];
    let x = Axis::new(&format!("{} ({})", AXES[axis], unit.symbol()), unit.from_si(r.origin[axis]), unit.from_si(r.origin[axis] + length));
    let (lo, hi) = span(points.iter().map(|p| p[1]));
    let mut chart = Chart::new(
        format!("Phase space along {}", AXES[axis]),
        x,
        Axis::new("γβ", lo, hi),
        vec![Series::new("particles", "particles", Pen::Series(1), Style::Markers, points)],
    );
    chart.look.legend = emcraft_render::chart::Legend::None;
    crate::chart_words::translate_chart(&mut chart);
    Some(chart)
}

/// Each probe's strongest component of E (V/m) over time.
pub fn probes(r: &PicResult) -> Option<Chart> {
    let series: Vec<Series> = r
        .probes
        .iter()
        .enumerate()
        .map(|(k, p)| {
            let spread = |c: usize| p.e.iter().map(|e| e[c] * e[c]).sum::<f64>();
            let c = (0..3).max_by(|a, b| spread(*a).total_cmp(&spread(*b))).unwrap_or(0);
            let points = p.e.iter().enumerate().map(|(n, e)| [(n + 1) as f64 * p.every as f64 * r.dt * 1e9, e[c]]).collect();
            Series::new(
                &format!("probe{k}"),
                format!("E{} {}", AXES[c], k + 1),
                Pen::Series(u8::try_from(k % 8 + 1).unwrap_or(1)),
                Style::Line,
                points,
            )
        })
        .collect();
    (!series.is_empty()).then(|| over_time("Field at the probes", "E (V/m)", series, r))
}

/// The page's charts, in order.
pub fn charts(s: &Session) -> Vec<Chart> {
    let Some(r) = s.results().particles.as_deref() else { return Vec::new() };
    let axis = along(s);
    std::iter::once(energy(r)).chain(currents(r)).chain(phase_space(s, r, axis)).chain(probes(r)).collect()
}

/// What the run gave: its grid and time, the particles at the end, each conducting wall's
/// current over the second half, each probe's frequency.
pub fn summary(s: &Session) -> Option<Table> {
    let r = s.results().particles.as_deref()?;
    let lu = s.display().length;
    let fu = s.display().frequency;
    let mut rows = vec![
        vec![tr(t!("Grid")).to_owned(), format!("{} × {} × {}", r.cells[0], r.cells[1], r.cells[2])],
        vec![tr(t!("Cell")).to_owned(), format!("{} {}", crate::number::value(lu.from_si(r.h.iter().copied().fold(0.0, f64::max))), lu.symbol())],
        vec![tr(t!("Time")).to_owned(), format!("{} ns ({} steps)", crate::number::value(r.steps as f64 * r.dt * 1e9), r.steps)],
        vec![tr(t!("Particles")).to_owned(), format!("{}", r.particles)],
    ];
    for axis in (0..3).filter(|a| r.walls[*a] == Wall::Conductor) {
        for high in [false, true] {
            let current = r.collected[axis][usize::from(high)];
            if current != 0.0 {
                let face = Face { axis, high }.name();
                rows.push(vec![trf(t!("Current on {face}"), args!(face = face)), format!("{} A", crate::number::value(current))]);
            }
        }
    }
    if r.metal != 0.0 {
        rows.push(vec![tr(t!("Current into the bodies")).to_owned(), format!("{} A", crate::number::value(r.metal))]);
    }
    if let Some(k) = r.loss_factor {
        rows.push(vec![tr(t!("Loss factor")).to_owned(), format!("{} V/pC", crate::number::value(k * 1e-12))]);
    }
    for (k, p) in r.probes.iter().enumerate() {
        let f = p.frequency.map_or_else(|| "–".to_owned(), |f| format!("{} {}", crate::number::frequency(fu.from_si(f)), fu.symbol()));
        rows.push(vec![trf(t!("Probe {n} rings at"), args!(n = (k + 1))), f]);
    }
    Some(Table { caption: None, headers: vec![String::new(), String::new()], rows, marks: None })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_plasma_run_is_charted_and_summed_up() {
        let mut s = Session::new();
        for (c, p) in [
            ("geometry.solid", json!({"shape": "box", "size": [40, 5, 5]})),
            ("particles.walls", json!({"x": "periodic", "y": "periodic", "z": "periodic"})),
            ("particles.plasma", json!({"density": 1e16, "per_cell": [4, 1, 1], "wave": 1e4})),
            ("particles.probe", json!({"at": [10, 0, 0]})),
            ("solve.particles", json!({"time": 4, "cell": 5})),
        ] {
            s.execute(c, &p).unwrap();
        }
        let charts = charts(&s);
        // Energy, phase space, the probe; no wall collects.
        assert_eq!(charts.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(), ["Energy", "Phase space along x", "Field at the probes"]);
        assert_eq!(charts[1].series[0].points.len(), 32);
        // The energy in a unit that keeps its ticks short.
        assert!(charts[0].y.label.ends_with("fJ)"), "{}", charts[0].y.label);
        let t = summary(&s).unwrap();
        assert_eq!(t.rows[0][1], "8 × 1 × 1");
        assert!(t.rows.last().is_some_and(|r| r[1].ends_with("GHz")), "{t:?}");
    }
}
