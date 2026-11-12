//! The last parameter study (`study.sweep`, `study.optimize`, `study.tolerance`) as every
//! window shows it: a line on it (with its surrogate's account where it had one), the
//! S-parameter a sweep's chart shows, a grid sweep's slice (the variable charted along and the
//! value each other is held at), and the table of its points with each one's note (why it
//! failed, the best, a prediction and its σ).

use emcraft_engine::Session;
use emcraft_engine::study::{Kind, Slice, Study};

use super::Table;
use crate::lang::{args, tr, trf};

/// The most values offered for one held variable, and the most rows the table shows.
pub const MAX_PILLS: usize = 101;
const MAX_ROWS: usize = 500;

/// The S-parameter a sweep's chart shows (the first when none) and a grid's slice.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StudyView {
    pub parameter: Option<String>,
    pub slice: Slice,
}

/// A number in short: five decimals at most, trailing zeros dropped.
fn short(v: f64) -> String {
    let s = format!("{v:.5}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// The S-parameters a sweep's chart may show (none for an optimization).
pub fn parameters(study: &Study) -> Vec<String> {
    if study.kind == Kind::Sweep { study.parameters() } else { Vec::new() }
}

impl StudyView {
    /// The S-parameter shown, when `study` has it.
    pub fn shown(&self, study: &Study) -> Option<String> {
        let list = parameters(study);
        self.parameter.clone().filter(|p| list.contains(p)).or_else(|| list.first().cloned())
    }

    /// The chart `session`'s study gives for it.
    pub fn chart(&self, s: &Session) -> Result<emcraft_render::chart::Chart, String> {
        let study = s.results().study.as_ref().ok_or("no study")?;
        let units = emcraft_engine::view::variable_units(s, study);
        let parameter = self.parameter.clone().filter(|p| parameters(study).contains(p));
        emcraft_engine::view::chart_with_units(study, parameter.as_deref(), &self.slice, &units).map_err(|e| e.to_string())
    }

    /// The view with variable `k` charted along.
    pub fn along(self, k: usize) -> StudyView {
        StudyView { slice: Slice { free: k, ..self.slice }, ..self }
    }

    /// The view with variable `k` held at its `i`th value.
    pub fn held(self, k: usize, i: usize) -> StudyView {
        let mut at = self.slice.at;
        if at.len() <= k {
            at.resize(k + 1, 0);
        }
        at[k] = i;
        StudyView { slice: Slice { at, ..self.slice }, ..self }
    }
}

/// A grid sweep's held variables: each but the one charted along, with its values in short (at
/// most [`MAX_PILLS`]) and the one held.
pub fn held(study: &Study, slice: &Slice) -> Vec<(usize, String, Vec<String>, usize)> {
    study
        .variables
        .iter()
        .enumerate()
        .filter(|(k, _)| *k != slice.free)
        .filter_map(|(k, n)| {
            let axis = study.grid.get(k)?;
            Some((k, n.clone(), axis.iter().take(MAX_PILLS).map(|v| short(*v)).collect(), slice.at.get(k).copied().unwrap_or(0)))
        })
        .collect()
}

/// A study in a line, with its surrogate's account where it had one.
pub fn summary(study: &Study) -> String {
    let Some(g) = &study.surrogate else { return plain(study) };
    let extra = match study.kind {
        Kind::Sweep => {
            let predicted = study.rows.iter().filter(|r| r.is_predicted()).count();
            trf(t!(", {solved} solved, {predicted} predicted"), args!(solved = g.solves, predicted = predicted))
        }
        Kind::Optimize => match g.at_best {
            Some((m, s)) => trf(t!(" · Bayesian, surrogate {mean} ± {sigma}"), args!(mean = short(m), sigma = short(s))),
            None => tr(t!(" · Bayesian")).to_owned(),
        },
        Kind::Tolerance => String::new(),
    };
    format!("{}{extra}", plain(study))
}

/// A study in a line: what was swept, optimized or drawn, how many points, by what, how many
/// failed; a tolerance analysis' yield, an optimization's best.
fn plain(study: &Study) -> String {
    let names = study.variables.join(", ");
    let failed = study.rows.iter().filter(|r| r.error.is_some()).count();
    let failed = if failed > 0 { trf(t!(", {n} failed"), args!(n = failed)) } else { String::new() };
    let (n, solve) = (study.rows.len(), &study.solve);
    match study.kind {
        Kind::Sweep if study.is_grid() => {
            let sizes: Vec<String> = study.grid.iter().map(|v| v.len().to_string()).collect();
            trf(
                t!("Sweep of {names} on a grid: {sizes} = {n} points by {solve}{failed}"),
                args!(names = names, sizes = sizes.join(" × "), n = n, solve = solve, failed = failed),
            )
        }
        Kind::Sweep => trf(t!("Sweep of {names}: {n} values by {solve}{failed}"), args!(names = names, n = n, solve = solve, failed = failed)),
        Kind::Tolerance => {
            let stats = emcraft_engine::study::Statistics::of(study);
            let has_spec = study.tolerance.as_ref().is_some_and(|t| !t.specs.is_empty());
            let yielded = if has_spec {
                let (lo, hi) = emcraft_engine::study::wilson(stats.passed, stats.solved);
                let share = 100.0 * stats.passed as f64 / stats.solved.max(1) as f64;
                trf(
                    t!(" · yield {share} % ({low}–{high} %)"),
                    args!(share = format!("{share:.0}"), low = format!("{:.0}", 100.0 * lo), high = format!("{:.0}", 100.0 * hi)),
                )
            } else {
                String::new()
            };
            trf(
                t!("Tolerance analysis of {names}: {n} draws by {solve}{failed}{yield}"),
                args!(names = names, n = n, solve = solve, failed = failed, yield = yielded),
            )
        }
        Kind::Optimize => {
            let best = study.best.and_then(|b| study.rows.get(b)).map(|r| {
                let at: Vec<String> = study.variables.iter().zip(&r.values).map(|(n, v)| format!("{n} = {}", short(*v))).collect();
                trf(t!("best {at} (goal {goal})"), args!(at = at.join(", "), goal = r.objective.map_or_else(|| "–".into(), short)))
            });
            let state = tr(if study.converged { t!("converged") } else { t!("stopped at its budget") });
            let goal = study.goal.clone().unwrap_or_default();
            trf(
                t!("Optimization of {names}: {goal}, {n} solves, {state}; {best}"),
                args!(names = names, goal = goal, n = n, state = state, best = best.unwrap_or_default()),
            )
        }
    }
}

/// The study's points as a table: the number when optimizing, each variable (with its unit
/// where the model says it), the goal or the band's extremes of the S-parameter shown, the modes
/// where any were found, and each row's note (why it failed, best, predicted ± σ).
pub fn points(s: &Session, view: &StudyView) -> Option<Table> {
    let study = s.results().study.as_ref()?;
    let parameter = view.shown(study);
    let units = emcraft_engine::view::variable_units(s, study);
    let optimize = study.kind == Kind::Optimize;
    let modes = !study.rows.iter().all(|r| r.modes.is_empty());
    let variables = study
        .variables
        .iter()
        .zip(units.iter().chain(std::iter::repeat(&None)))
        .map(|(n, u)| u.as_ref().map_or_else(|| n.clone(), |u| format!("{n} ({u})")));
    let goal = optimize.then(|| match emcraft_engine::view::goal_unit(study) {
        Some(u) => trf(t!("goal ({unit})"), args!(unit = u)),
        None => tr(t!("goal")).to_owned(),
    });
    let extremes = parameter.as_ref().filter(|_| !optimize).map(|p| {
        let p = p.to_uppercase();
        [trf(t!("min |{p}| dB"), args!(p = p)), trf(t!("max |{p}| dB"), args!(p = p))]
    });
    let headers = optimize
        .then(|| "#".to_owned())
        .into_iter()
        .chain(variables)
        .chain(goal)
        .chain(extremes.into_iter().flatten())
        .chain(modes.then(|| trf(t!("modes ({unit})"), args!(unit = study.frequency_unit))))
        .chain(std::iter::once(String::new()))
        .collect();
    let rows = study
        .rows
        .iter()
        .enumerate()
        .take(MAX_ROWS)
        .map(|(i, r)| {
            let cell = |x: f64| if x.is_finite() { format!("{x:.3}") } else { "–".into() };
            let db = parameter.as_deref().map(|p| r.db(p).unwrap_or_default());
            let (lo, hi) = db.as_ref().map_or((f64::NAN, f64::NAN), |db| {
                (db.iter().copied().fold(f64::INFINITY, f64::min), db.iter().copied().fold(f64::NEG_INFINITY, f64::max))
            });
            let sigma = parameter.as_deref().and_then(|p| r.db_sigma(p)).map(|s| s.into_iter().fold(0.0, f64::max));
            let note = match (&r.error, study.best == Some(i), r.is_predicted(), sigma) {
                (Some(e), _, _, _) => emcraft_engine::text::clip(e, 120),
                (None, true, _, _) => tr(t!("best")).to_owned(),
                (None, false, true, Some(s)) => trf(t!("predicted ± {sigma} dB"), args!(sigma = format!("{s:.3}"))),
                (None, false, true, None) => tr(t!("predicted")).to_owned(),
                (None, false, false, _) => String::new(),
            };
            optimize
                .then(|| format!("{}", i + 1))
                .into_iter()
                .chain(r.values.iter().map(|v| short(*v)))
                .chain(optimize.then(|| r.objective.map_or_else(|| "–".into(), |o| format!("{o:.4}"))))
                .chain((!optimize && parameter.is_some()).then(|| [cell(lo), cell(hi)]).into_iter().flatten())
                .chain(modes.then(|| r.modes.iter().take(6).map(|f| format!("{f:.4}")).collect::<Vec<_>>().join(", ")))
                .chain(std::iter::once(note))
                .collect()
        })
        .collect();
    Some(Table { caption: None, headers, rows, marks: None })
}
