//! A parameter study's chart: a sweep's S-parameters or eigenmodes as the variable varies, an
//! optimization's progress.

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

/// The study as a chart: a sweep's |`parameter`| in dB against frequency, one curve per value
/// (against the value when it solved one frequency), or its eigenmodes against the value; an
/// optimization's goal at each evaluation with the best so far.
pub fn chart(study: &Study, parameter: Option<&str>) -> Result<Chart> {
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
    let ok: Vec<&Row> = study.rows.iter().filter(|r| r.error.is_none()).collect();
    match parameter {
        Some(p) if ok.iter().all(|r| r.frequencies.len() == 1) => Ok(one_frequency_chart(study, &ok, &p)),
        Some(p) => sweep_chart(study, &ok, &p),
        None => modes_chart(study, &ok),
    }
}

/// The variable a sweep varies, or `otherwise`.
fn variable(study: &Study, otherwise: &str) -> String {
    study.variables.first().map_or(otherwise.to_owned(), |n| n.clone())
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

/// A sweep that solved one frequency: |S_p| in dB against the variable.
fn one_frequency_chart(study: &Study, ok: &[&Row], p: &str) -> Chart {
    let label = tex(p);
    let pts: Vec<[f64; 2]> = ok.iter().filter_map(|r| Some([*r.values.first()?, *r.db(p)?.first()?])).collect();
    let f = ok.first().and_then(|r| r.frequencies.first()).copied().unwrap_or(f64::NAN);
    let series = vec![
        Series { label: format!("{label} at {f} {}", study.frequency_unit), color: Rgb(30, 90, 200), style: Style::Line, points: pts.clone() },
        Series { label: String::new(), color: Rgb(30, 90, 200), style: Style::Markers, points: pts },
    ];
    let y = padded_axis(&format!("{label} (dB)"), series.iter().flat_map(|s| s.points.iter().map(|q| q[1])));
    let x = padded_axis(&variable(study, "value"), ok.iter().filter_map(|r| r.values.first().copied()));
    Chart { title: format!("Sweep of {}", variable(study, "")), x, y, series, marks: Vec::new() }
}

/// A sweep's |S_p| in dB against frequency, one curve per value.
fn sweep_chart(study: &Study, ok: &[&Row], p: &str) -> Result<Chart> {
    let fu = &study.frequency_unit;
    let label = tex(p);
    let n = ok.len();
    let mut series = Vec::new();
    for (i, r) in ok.iter().enumerate() {
        let Some(db) = r.db(p) else { continue };
        let points: Vec<[f64; 2]> = r.frequencies.iter().zip(db).map(|(f, d)| [*f, d.max(DB_FLOOR)]).collect();
        let v = r.values.first().copied().unwrap_or(f64::NAN);
        let t = if n > 1 { i as f64 / (n - 1) as f64 } else { 0.0 };
        series.push(Series { label: format!("{} = {v}", variable(study, "")), color: ramp(t), style: Style::Line, points });
    }
    if series.is_empty() {
        return Err(nothing_to_chart());
    }
    let (f0, f1) = span(series.iter().flat_map(|s| s.points.iter().map(|q| q[0])));
    let x = if f1 > f0 { Axis::new(&format!("frequency ({fu})"), f0, f1) } else { padded_axis(&format!("frequency ({fu})"), [f0].into_iter()) };
    let y = db_axis(&format!("{label} (dB)"), &series);
    Ok(Chart { title: format!("{label} as {} varies", variable(study, "")), x, y, series, marks: Vec::new() })
}

/// A sweep's eigenmodes against the variable.
fn modes_chart(study: &Study, ok: &[&Row]) -> Result<Chart> {
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
            points: ok.iter().filter_map(|r| Some([*r.values.first()?, *r.modes.get(m)?])).collect(),
        })
        .collect();
    let x = padded_axis(&variable(study, "value"), ok.iter().filter_map(|r| r.values.first().copied()));
    let y = padded_axis(&format!("frequency ({fu})"), series.iter().flat_map(|s| s.points.iter().map(|q| q[1])));
    Ok(Chart { title: format!("Eigenmodes as {} varies", variable(study, "")), x, y, series, marks: Vec::new() })
}
