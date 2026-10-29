//! A parameter study's chart: a sweep's S-parameters or eigenmodes as the variable varies, an
//! optimization's progress. A sweep over a grid of several variables is charted a slice at a
//! time: along one variable (its values the curves, or the x axis), every other held at one of
//! its values ([`Slice`]).

use emcraft_render::Rgb;
use emcraft_render::axes::Axis;
use emcraft_render::chart::{Chart, Series, Style};

use super::chart::{DB_FLOOR, db_axis, padded_axis, span};
use crate::study::{Kind, Row, Study};
use crate::{EngineError, Result};

/// A colour along a ramp from blue to red, `t` from 0 to 1: a sweep's values in order.
fn ramp(t: f64) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    let mix = |a: f64, b: f64| (a + (b - a) * t).round() as u8;
    Rgb(mix(30.0, 200.0), mix(80.0, 50.0), mix(200.0, 40.0))
}

fn nothing_to_chart() -> EngineError {
    EngineError::failed("the study has nothing to chart: no S-parameters, eigenmodes or goal values")
}

/// Which part of a grid sweep a chart shows: the variable it runs along (`free`, an index into
/// the study's variables) and, for every other, the index of the value it is held at (the first
/// where missing). A one-variable sweep has one slice, the whole sweep.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Slice {
    pub free: usize,
    pub at: Vec<usize>,
}

impl Slice {
    /// The slice of `study` that `at` names (variable → value held, the nearest of its values;
    /// the first variable not named is the one charted along); `None` holds every variable but
    /// the first at its first value.
    pub fn named(study: &Study, at: Option<&serde_json::Map<String, serde_json::Value>>) -> Result<Slice> {
        let mut slice = Slice { free: 0, at: vec![0; study.variables.len()] };
        let Some(at) = at else { return Ok(slice) };
        let bad = |why: String| EngineError::Params(format!("`at` {why}"));
        for (name, v) in at {
            let k = study.variables.iter().position(|n| n == name).ok_or_else(|| bad(format!("names no variable of the sweep: `{name}`")))?;
            let v = v.as_f64().filter(|v| v.is_finite()).ok_or_else(|| bad(format!("`{name}` must be a number")))?;
            let axis = study.grid.get(k).map(Vec::as_slice).unwrap_or_default();
            let i = axis.iter().enumerate().min_by(|a, b| (a.1 - v).abs().total_cmp(&(b.1 - v).abs()).then(a.0.cmp(&b.0))).map(|(i, _)| i);
            if let Some(slot) = slice.at.get_mut(k) {
                *slot = i.ok_or_else(|| bad(format!("`{name}`: the sweep has no values")))?;
            }
        }
        if let Some(k) = study.variables.iter().position(|n| !at.contains_key(n)) {
            slice.free = k;
        }
        Ok(slice)
    }

    /// The held variables in words: `b = 5, c = 2`.
    fn held(&self, study: &Study) -> String {
        study
            .variables
            .iter()
            .enumerate()
            .filter(|(k, _)| *k != self.free)
            .filter_map(|(k, n)| {
                let axis = study.grid.get(k)?;
                Some(format!("{n} = {}", axis.get(self.at.get(k).copied().unwrap_or(0)).or(axis.first())?))
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// The study as a chart: a sweep's |`parameter`| in dB against frequency, one curve per value
/// (against the value when it solved one frequency), or its eigenmodes against the value; an
/// optimization's goal at each evaluation with the best so far. A grid sweep shows `slice`:
/// curves along its free variable with the others held, or — solved at one frequency — |S|
/// against the free variable, one curve per value of the next variable.
pub fn chart(study: &Study, parameter: Option<&str>, slice: &Slice) -> Result<Chart> {
    if study.kind == Kind::Optimize {
        return optimization_chart(study);
    }
    let parameters = study.parameters();
    let parameter = match parameter {
        Some(p) => Some(
            parameters
                .iter()
                .find(|n| n.eq_ignore_ascii_case(p))
                .cloned()
                .ok_or_else(|| EngineError::Params(format!("`parameter` must be one the study holds: {}", parameters.join(", "))))?,
        ),
        None => parameters.first().cloned(),
    };
    let free = slice.free.min(study.variables.len().saturating_sub(1));
    let slice = Slice { free, at: slice.at.clone() };
    let all: Vec<&Row> = study.rows.iter().filter(|r| r.error.is_none()).collect();
    let one_frequency = all.iter().all(|r| r.frequencies.len() == 1);
    let ok: Vec<&Row> = if study.is_grid() { study.slice(free, &slice.at).into_iter().filter(|r| r.error.is_none()).collect() } else { all.clone() };
    let mut chart = match parameter {
        Some(p) if one_frequency && study.is_grid() => return Ok(one_frequency_grid_chart(study, &all, &p, &slice)),
        Some(p) if one_frequency => one_frequency_chart(study, &ok, &p, free),
        Some(p) => sweep_chart(study, &ok, &p, free)?,
        None => modes_chart(study, &ok, free)?,
    };
    if study.is_grid() {
        chart.title = format!("{} at {}", chart.title, slice.held(study));
    }
    Ok(chart)
}

/// The name of the sweep's variable `free`, or `otherwise`.
fn variable(study: &Study, free: usize, otherwise: &str) -> String {
    study.variables.get(free).map_or(otherwise.to_owned(), |n| n.clone())
}

/// A row's value of variable `free`.
fn value(r: &Row, free: usize) -> Option<f64> {
    r.values.get(free).copied()
}

/// |S_p|'s label: `$|S_{21}|$`.
fn tex(p: &str) -> String {
    format!("$|S_{{{}}}|$", p.trim_start_matches('s'))
}

/// An optimization's goal at each evaluation and the best so far.
fn optimization_chart(study: &Study) -> Result<Chart> {
    let mut best = f64::INFINITY;
    let (mut points, mut so_far) = (Vec::new(), Vec::new());
    for (i, r) in study.rows.iter().enumerate() {
        if let Some(o) = r.objective {
            points.push([(i + 1) as f64, o]);
            best = best.min(o);
        }
        if best.is_finite() {
            so_far.push([(i + 1) as f64, best]);
        }
    }
    if points.is_empty() {
        return Err(nothing_to_chart());
    }
    let y = padded_axis("goal", points.iter().map(|p| p[1]));
    Ok(Chart {
        title: format!("Optimization: {}", study.goal.clone().unwrap_or_default()),
        x: Axis::new("evaluation", 0.5, study.rows.len().max(2) as f64 + 0.5),
        y,
        series: vec![
            Series { label: "each solve".into(), color: Rgb(140, 140, 150), style: Style::Markers, points },
            Series { label: "best so far".into(), color: Rgb(30, 90, 200), style: Style::Line, points: so_far },
        ],
        marks: Vec::new(),
    })
}

/// A grid sweep that solved one frequency: |S_p| in dB against the free variable, one curve per
/// value of the next variable, any others held.
fn one_frequency_grid_chart(study: &Study, all: &[&Row], p: &str, slice: &Slice) -> Chart {
    let free = slice.free;
    let label = tex(p);
    let other = (0..study.variables.len()).find(|k| *k != free).unwrap_or(0);
    let axis = study.grid.get(other).cloned().unwrap_or_default();
    let n = axis.len();
    let f = all.first().and_then(|r| r.frequencies.first()).copied().unwrap_or(f64::NAN);
    let mut series = Vec::new();
    for (i, v) in axis.iter().enumerate() {
        let mut at = slice.at.clone();
        if let Some(slot) = at.get_mut(other) {
            *slot = i;
        }
        let rows = study.slice(free, &at);
        let points: Vec<[f64; 2]> = rows.iter().filter(|r| r.error.is_none()).filter_map(|r| Some([value(r, free)?, *r.db(p)?.first()?])).collect();
        let t = if n > 1 { i as f64 / (n - 1) as f64 } else { 0.0 };
        series.push(Series { label: format!("{} = {v}", variable(study, other, "")), color: ramp(t), style: Style::Line, points: points.clone() });
        series.push(Series { label: String::new(), color: ramp(t), style: Style::Markers, points });
    }
    let y = padded_axis(&format!("{label} (dB)"), series.iter().flat_map(|s| s.points.iter().map(|q| q[1])));
    let x = padded_axis(&variable(study, free, "value"), all.iter().filter_map(|r| value(r, free)));
    let mut title = format!("{label} at {f} {} as {} varies", study.frequency_unit, variable(study, free, ""));
    let held: Vec<String> = (0..study.variables.len())
        .filter(|k| *k != free && *k != other)
        .filter_map(|k| Some(format!("{} = {}", study.variables.get(k)?, study.grid.get(k)?.get(slice.at.get(k).copied().unwrap_or(0))?)))
        .collect();
    if !held.is_empty() {
        title = format!("{title} at {}", held.join(", "));
    }
    Chart { title, x, y, series, marks: Vec::new() }
}

/// A sweep that solved one frequency: |S_p| in dB against the variable.
fn one_frequency_chart(study: &Study, ok: &[&Row], p: &str, free: usize) -> Chart {
    let label = tex(p);
    let pts: Vec<[f64; 2]> = ok.iter().filter_map(|r| Some([value(r, free)?, *r.db(p)?.first()?])).collect();
    let f = ok.first().and_then(|r| r.frequencies.first()).copied().unwrap_or(f64::NAN);
    let series = vec![
        Series { label: format!("{label} at {f} {}", study.frequency_unit), color: Rgb(30, 90, 200), style: Style::Line, points: pts.clone() },
        Series { label: String::new(), color: Rgb(30, 90, 200), style: Style::Markers, points: pts },
    ];
    let y = padded_axis(&format!("{label} (dB)"), series.iter().flat_map(|s| s.points.iter().map(|q| q[1])));
    let x = padded_axis(&variable(study, free, "value"), ok.iter().filter_map(|r| value(r, free)));
    Chart { title: format!("Sweep of {}", variable(study, free, "")), x, y, series, marks: Vec::new() }
}

/// A sweep's |S_p| in dB against frequency, one curve per value.
fn sweep_chart(study: &Study, ok: &[&Row], p: &str, free: usize) -> Result<Chart> {
    let fu = &study.frequency_unit;
    let label = tex(p);
    let n = ok.len();
    let mut series = Vec::new();
    for (i, r) in ok.iter().enumerate() {
        let Some(db) = r.db(p) else { continue };
        let points: Vec<[f64; 2]> = r.frequencies.iter().zip(db).map(|(f, d)| [*f, d.max(DB_FLOOR)]).collect();
        let v = value(r, free).unwrap_or(f64::NAN);
        let t = if n > 1 { i as f64 / (n - 1) as f64 } else { 0.0 };
        series.push(Series { label: format!("{} = {v}", variable(study, free, "")), color: ramp(t), style: Style::Line, points });
    }
    if series.is_empty() {
        return Err(nothing_to_chart());
    }
    let (f0, f1) = span(series.iter().flat_map(|s| s.points.iter().map(|q| q[0])));
    let x = if f1 > f0 { Axis::new(&format!("frequency ({fu})"), f0, f1) } else { padded_axis(&format!("frequency ({fu})"), [f0].into_iter()) };
    let y = db_axis(&format!("{label} (dB)"), &series);
    Ok(Chart { title: format!("{label} as {} varies", variable(study, free, "")), x, y, series, marks: Vec::new() })
}

/// A sweep's eigenmodes against the variable.
fn modes_chart(study: &Study, ok: &[&Row], free: usize) -> Result<Chart> {
    let fu = &study.frequency_unit;
    let modes = ok.iter().map(|r| r.modes.len()).max().unwrap_or(0);
    if modes == 0 {
        return Err(nothing_to_chart());
    }
    let series: Vec<Series> = (0..modes.min(12))
        .map(|m| Series {
            label: format!("mode {}", m + 1),
            color: ramp(m as f64 / (modes.max(2) - 1) as f64),
            style: Style::Line,
            points: ok.iter().filter_map(|r| Some([value(r, free)?, *r.modes.get(m)?])).collect(),
        })
        .collect();
    let x = padded_axis(&variable(study, free, "value"), ok.iter().filter_map(|r| value(r, free)));
    let y = padded_axis(&format!("frequency ({fu})"), series.iter().flat_map(|s| s.points.iter().map(|q| q[1])));
    Ok(Chart { title: format!("Eigenmodes as {} varies", variable(study, free, "")), x, y, series, marks: Vec::new() })
}
