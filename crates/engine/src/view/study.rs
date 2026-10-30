//! A parameter study's chart: a sweep's S-parameters or eigenmodes as the variable varies, an
//! optimization's progress. A sweep over a grid of several variables is charted a slice at a
//! time: along one variable (its values the curves, or the x axis), every other held at one of
//! its values ([`Slice`]).

use emcraft_render::Pen;
use emcraft_render::axes::Axis;
use emcraft_render::chart::{Chart, Series, Style};

use super::chart::{DB_FLOOR, db_axis, padded_axis, span};
use crate::study::{Kind, Row, Study};
use crate::{EngineError, Quantity, Result, Session};

/// A colour along the ramp from the first series colour (blue) to the second (red), `t` from
/// 0 to 1: a sweep's values in order.
fn ramp(t: f64) -> Pen {
    Pen::Ramp(t.clamp(0.0, 1.0))
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

    /// The held variables in words: `b = 5 mm, c = 2`.
    fn held(&self, study: &Study, units: &[Option<String>]) -> String {
        study
            .variables
            .iter()
            .enumerate()
            .filter(|(k, _)| *k != self.free)
            .filter_map(|(k, _)| {
                let axis = study.grid.get(k)?;
                Some(valued(study, units, k, *axis.get(self.at.get(k).copied().unwrap_or(0)).or(axis.first())?))
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
    chart_with_units(study, parameter, slice, &[])
}

/// [`chart`] with each variable's display unit (`units`, in the study's variables' order; none
/// where missing), on its axis and beside its values: `l = 15 mm`.
pub fn chart_with_units(study: &Study, parameter: Option<&str>, slice: &Slice, units: &[Option<String>]) -> Result<Chart> {
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
        Some(p) if one_frequency && study.is_grid() => return Ok(one_frequency_grid_chart(study, &all, &p, &slice, units)),
        Some(p) if one_frequency => one_frequency_chart(study, &ok, &p, free, units),
        Some(p) => sweep_chart(study, &ok, &p, free, units)?,
        None => modes_chart(study, &ok, free, units)?,
    };
    if study.is_grid() {
        chart.title = format!("{} at {}", chart.title, slice.held(study, units));
    }
    Ok(chart)
}

/// The name of the sweep's variable `free`, or `otherwise`.
fn variable(study: &Study, free: usize, otherwise: &str) -> String {
    study.variables.get(free).map_or(otherwise.to_owned(), |n| n.clone())
}

/// Variable `k`'s unit, when known.
fn unit_of(units: &[Option<String>], k: usize) -> Option<&str> {
    units.get(k).and_then(Option::as_deref).filter(|u| !u.is_empty())
}

/// An axis along variable `k`: its name and unit, `l (mm)`.
fn axis_name(study: &Study, units: &[Option<String>], k: usize) -> String {
    let name = variable(study, k, "value");
    match unit_of(units, k) {
        Some(u) => format!("{name} ({u})"),
        None => name,
    }
}

/// Variable `k` at value `v` in words: `l = 15 mm`.
fn valued(study: &Study, units: &[Option<String>], k: usize, v: f64) -> String {
    let name = variable(study, k, "");
    match unit_of(units, k) {
        Some(u) => format!("{name} = {v} {u}"),
        None => format!("{name} = {v}"),
    }
}

/// The unit of an optimization's goal, from what it reads: dB for an S-parameter's, the
/// frequency unit for an eigenfrequency's, none for a number the solve gives.
pub fn goal_unit(study: &Study) -> Option<&str> {
    let goal = study.goal.as_deref()?;
    if goal.contains("(dB)") || goal.ends_with(" dB") {
        Some("dB")
    } else if goal.starts_with("mode ") {
        Some(&study.frequency_unit)
    } else {
        None
    }
}

/// Each of the study's variables' display unit (see [`variable_unit`]).
pub fn variable_units(s: &Session, study: &Study) -> Vec<Option<String>> {
    study.variables.iter().map(|n| variable_unit(s, n)).collect()
}

/// The display unit variable `name` stands for, from where the history writes it: a length's
/// or a frequency's when every number it goes into is one (`"=l"`, `"=l/2 + 1"` in a length);
/// none when it goes into plain numbers, both kinds, or nowhere.
///
/// ```
/// use emcraft_engine::view::variable_unit;
///
/// let mut s = emcraft_engine::Session::new();
/// s.run_script(&serde_json::json!([
///     {"command": "project.variable", "params": {"name": "l", "value": 20}},
///     {"command": "project.variable", "params": {"name": "er", "value": 2}},
///     {"command": "model.material.create", "params": {"name": "fill", "eps_r": "=er"}},
///     {"command": "geometry.waveguide", "params": {"width": 22.86, "height": 10.16,
///         "sections": [{"length": "=l", "material": "fill"}]}}
/// ])).unwrap();
/// assert_eq!(variable_unit(&s, "l").as_deref(), Some("mm"));
/// assert_eq!(variable_unit(&s, "er"), None);
/// assert_eq!(variable_unit(&s, "nobody"), None);
/// ```
pub fn variable_unit(s: &Session, name: &str) -> Option<String> {
    let mut found: Option<Quantity> = None;
    for step in s.history.iter().filter(|st| !st.command.starts_with("project.variable")) {
        let Some(spec) = crate::find_command(&step.command) else { continue };
        let mut keys = Vec::new();
        uses(&step.params, "", name, 0, &mut keys);
        for key in keys {
            let q = quantity_of(spec, key);
            match found {
                None => found = Some(q),
                Some(f) if f != q => return None,
                Some(_) => {}
            }
        }
    }
    match found? {
        Quantity::Length => Some(s.display.length.symbol().to_owned()),
        Quantity::Frequency => Some(s.display.frequency.symbol().to_owned()),
        Quantity::None => None,
    }
}

/// The keys under which `v` holds an expression naming variable `name` (nested no deeper than
/// a few levels: a table's rows).
fn uses<'a>(v: &'a serde_json::Value, key: &'a str, name: &str, depth: usize, out: &mut Vec<&'a str>) {
    match v {
        serde_json::Value::String(e) if e.starts_with('=') => {
            if e.split(|c: char| !(c.is_alphanumeric() || c == '_')).any(|w| w == name) {
                out.push(key);
            }
        }
        serde_json::Value::Array(items) if depth < 4 => items.iter().for_each(|x| uses(x, key, name, depth + 1, out)),
        serde_json::Value::Object(map) if depth < 4 => map.iter().for_each(|(k, x)| uses(x, k, name, depth + 1, out)),
        _ => {}
    }
}

/// What a number under `key` of command `spec` is: its parameter's quantity, or for a key
/// inside a table (a section's `length`) the quantity a parameter of that name has elsewhere.
fn quantity_of(spec: &crate::CommandSpec, key: &str) -> Quantity {
    if let Some(p) = spec.params.iter().find(|p| p.name == key) {
        return p.quantity;
    }
    crate::COMMANDS
        .iter()
        .flat_map(|c| c.params.iter())
        .find(|p| p.name == key && p.quantity != Quantity::None)
        .map_or(Quantity::None, |p| p.quantity)
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
    let label = match goal_unit(study) {
        Some(u) => format!("goal ({u})"),
        None => "goal".to_owned(),
    };
    let y = padded_axis(&label, points.iter().map(|p| p[1]));
    Ok(Chart::new(
        format!("Optimization: {}", study.goal.clone().unwrap_or_default()),
        Axis::new("evaluation", 0.5, study.rows.len().max(2) as f64 + 0.5),
        y,
        vec![
            Series::new("each", "each solve", Pen::Muted, Style::Markers, points),
            Series::new("best", "best so far", Pen::Series(0), Style::Line, so_far),
        ],
    ))
}

/// A grid sweep that solved one frequency: |S_p| in dB against the free variable, one curve per
/// value of the next variable, any others held.
fn one_frequency_grid_chart(study: &Study, all: &[&Row], p: &str, slice: &Slice, units: &[Option<String>]) -> Chart {
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
        let key = format!("value{}", i + 1);
        series.push(Series::new(&key, valued(study, units, other, *v), ramp(t), Style::Line, points.clone()));
        series.push(Series::new(&key, String::new(), ramp(t), Style::Markers, points));
    }
    let y = padded_axis(&format!("{label} (dB)"), series.iter().flat_map(|s| s.points.iter().map(|q| q[1])));
    let x = padded_axis(&axis_name(study, units, free), all.iter().filter_map(|r| value(r, free)));
    let mut title = format!("{label} at {f} {} as {} varies", study.frequency_unit, variable(study, free, ""));
    let held: Vec<String> = (0..study.variables.len())
        .filter(|k| *k != free && *k != other)
        .filter_map(|k| Some(valued(study, units, k, *study.grid.get(k)?.get(slice.at.get(k).copied().unwrap_or(0))?)))
        .collect();
    if !held.is_empty() {
        title = format!("{title} at {}", held.join(", "));
    }
    Chart::new(title, x, y, series)
}

/// A sweep that solved one frequency: |S_p| in dB against the variable.
fn one_frequency_chart(study: &Study, ok: &[&Row], p: &str, free: usize, units: &[Option<String>]) -> Chart {
    let label = tex(p);
    let pts: Vec<[f64; 2]> = ok.iter().filter_map(|r| Some([value(r, free)?, *r.db(p)?.first()?])).collect();
    let f = ok.first().and_then(|r| r.frequencies.first()).copied().unwrap_or(f64::NAN);
    let series = vec![
        Series::new("s", format!("{label} at {f} {}", study.frequency_unit), Pen::Series(0), Style::Line, pts.clone()),
        Series::new("s", String::new(), Pen::Series(0), Style::Markers, pts),
    ];
    let y = padded_axis(&format!("{label} (dB)"), series.iter().flat_map(|s| s.points.iter().map(|q| q[1])));
    let x = padded_axis(&axis_name(study, units, free), ok.iter().filter_map(|r| value(r, free)));
    Chart::new(format!("Sweep of {}", variable(study, free, "")), x, y, series)
}

/// A sweep's |S_p| in dB against frequency, one curve per value.
fn sweep_chart(study: &Study, ok: &[&Row], p: &str, free: usize, units: &[Option<String>]) -> Result<Chart> {
    let fu = &study.frequency_unit;
    let label = tex(p);
    let n = ok.len();
    let mut series = Vec::new();
    for (i, r) in ok.iter().enumerate() {
        let Some(db) = r.db(p) else { continue };
        let points: Vec<[f64; 2]> = r.frequencies.iter().zip(db).map(|(f, d)| [*f, d.max(DB_FLOOR)]).collect();
        let v = value(r, free).unwrap_or(f64::NAN);
        let t = if n > 1 { i as f64 / (n - 1) as f64 } else { 0.0 };
        series.push(Series::new(&format!("value{}", i + 1), valued(study, units, free, v), ramp(t), Style::Line, points));
    }
    if series.is_empty() {
        return Err(nothing_to_chart());
    }
    let (f0, f1) = span(series.iter().flat_map(|s| s.points.iter().map(|q| q[0])));
    let x = if f1 > f0 { Axis::new(&format!("frequency ({fu})"), f0, f1) } else { padded_axis(&format!("frequency ({fu})"), [f0].into_iter()) };
    let y = db_axis(&format!("{label} (dB)"), &series);
    Ok(Chart::new(format!("{label} as {} varies", variable(study, free, "")), x, y, series))
}

/// A sweep's eigenmodes against the variable.
fn modes_chart(study: &Study, ok: &[&Row], free: usize, units: &[Option<String>]) -> Result<Chart> {
    let fu = &study.frequency_unit;
    let modes = ok.iter().map(|r| r.modes.len()).max().unwrap_or(0);
    if modes == 0 {
        return Err(nothing_to_chart());
    }
    let series: Vec<Series> = (0..modes.min(12))
        .map(|m| {
            let points = ok.iter().filter_map(|r| Some([value(r, free)?, *r.modes.get(m)?])).collect();
            Series::new(&format!("mode{}", m + 1), format!("mode {}", m + 1), ramp(m as f64 / (modes.max(2) - 1) as f64), Style::Line, points)
        })
        .collect();
    let x = padded_axis(&axis_name(study, units, free), ok.iter().filter_map(|r| value(r, free)));
    let y = padded_axis(&format!("frequency ({fu})"), series.iter().flat_map(|s| s.points.iter().map(|q| q[1])));
    Ok(Chart::new(format!("Eigenmodes as {} varies", variable(study, free, "")), x, y, series))
}
