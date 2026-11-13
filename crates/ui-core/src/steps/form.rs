//! The Setup step's form: the run's settings as typed, loaded from the confirmed ones
//! (`project.setup`) whenever those change, checked field by field ([`checked`]) into what
//! `project.setup` takes.

use serde_json::{Value, json};

use super::Solver;
use crate::lang::{args, lookup, tr, trf};

/// The form's text fields, the time domain's ports to excite (kept through it), the confirmed
/// settings it was last loaded from, the Touchstone export's path and format, each field's
/// error, and whether Confirm was pressed (an empty field's error shows only then).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fields {
    pub start: String,
    pub stop: String,
    pub points: String,
    pub order: String,
    pub count: String,
    pub frequency: String,
    pub cell: String,
    pub excite: Vec<String>,
    pub seen: Option<Value>,
    pub touchstone: String,
    pub format: String,
    pub errors: Vec<(&'static str, String)>,
    pub tried: bool,
}

/// The form's fields by name, as `project.setup` names them.
pub const NAMES: [&str; 7] = ["start", "stop", "points", "order", "count", "frequency", "cell"];

impl Fields {
    /// Field `name`'s text.
    pub fn text(&self, name: &str) -> &str {
        match name {
            "start" => &self.start,
            "stop" => &self.stop,
            "points" => &self.points,
            "order" => &self.order,
            "count" => &self.count,
            "frequency" => &self.frequency,
            "cell" => &self.cell,
            "touchstone" => &self.touchstone,
            "format" => &self.format,
            _ => "",
        }
    }

    /// The form with field `name` typed as `text` (bounded), its error cleared.
    pub fn typed(self, name: &str, text: &str) -> Fields {
        let mut f = self;
        let t = crate::dialogs::bounded(text);
        match name {
            "start" => f.start = t,
            "stop" => f.stop = t,
            "points" => f.points = t,
            "order" => f.order = t,
            "count" => f.count = t,
            "frequency" => f.frequency = t,
            "cell" => f.cell = t,
            "touchstone" => f.touchstone = t,
            "format" => f.format = t,
            _ => return f,
        }
        f.errors.retain(|(n, _)| *n != name);
        f
    }

    /// Empty fields with their defaults, as the Setup panel shows them.
    pub fn with_defaults(self) -> Fields {
        let or = |s: String, d: &str| if s.is_empty() { d.to_owned() } else { s };
        Fields { points: or(self.points, "101"), order: or(self.order, "2"), count: or(self.count, "5"), cell: or(self.cell, "1"), ..self }
    }

    /// Field `name`'s error.
    pub fn error(&self, name: &str) -> Option<&str> {
        self.errors.iter().find(|(n, _)| *n == name).map(|(_, e)| e.as_str())
    }

    /// [`Fields::error`] as the form shows it: an empty field's only once Confirm was pressed.
    pub fn shown_error(&self, name: &str) -> Option<&str> {
        let typed = NAMES.contains(&name);
        self.error(name).filter(|_| !typed || self.tried || !self.text(name).trim().is_empty())
    }

    /// The form with field `name`'s error set (or cleared).
    pub fn with_error(self, name: &'static str, e: Option<String>) -> Fields {
        let mut errors: Vec<(&'static str, String)> = self.errors.into_iter().filter(|(n, _)| *n != name).collect();
        errors.extend(e.map(|e| (name, e)));
        Fields { errors, ..self }
    }

    /// The form filled from settings as `project.setup` takes them (a confirmed run's, or a
    /// proposal's), its errors cleared.
    pub fn load(self, v: &Value) -> Fields {
        let text = |k: &str, old: String| v.get(k).map_or(old, crate::dialogs::value_text);
        let excite = v
            .get("excite")
            .and_then(Value::as_array)
            .map_or_else(|| self.excite.clone(), |l| l.iter().filter_map(Value::as_str).map(str::to_owned).collect());
        Fields {
            start: text("start", self.start),
            stop: text("stop", self.stop),
            points: text("points", self.points),
            order: text("order", self.order),
            count: text("count", self.count),
            frequency: text("frequency", self.frequency),
            cell: text("cell", self.cell),
            excite,
            errors: Vec::new(),
            ..self
        }
        .with_defaults()
    }

    /// The form following the confirmed settings `confirmed` (`project.values` of
    /// `project.setup`) when they changed since it was last loaded; settings taken back leave it
    /// as typed.
    pub fn synced(self, confirmed: Value) -> Fields {
        if self.seen.as_ref() == Some(&confirmed) {
            return self;
        }
        let f = if confirmed.as_object().is_some_and(|o| !o.is_empty()) { self.load(&confirmed) } else { self.with_defaults() };
        Fields { seen: Some(confirmed), ..f }
    }

    /// The fields as typed, for a recording.
    pub fn drafts(&self) -> Value {
        json!({
            "start": self.start, "stop": self.stop, "points": self.points, "order": self.order, "count": self.count,
            "frequency": self.frequency, "cell": self.cell, "touchstone": self.touchstone, "format": self.format, "excite": self.excite,
        })
    }

    /// The fields typed as [`Fields::drafts`] wrote them (a text missing or not one left as it is).
    pub fn with_drafts(self, v: &Value) -> Fields {
        let f = ["start", "stop", "points", "order", "count", "frequency", "cell", "touchstone", "format"].into_iter().fold(self, |f, k| {
            match v.get(k).and_then(Value::as_str) {
                Some(t) => {
                    let errors = f.errors.clone();
                    Fields { errors, ..f.typed(k, t) }
                }
                None => f,
            }
        });
        match v.get("excite").and_then(Value::as_array) {
            Some(list) => Fields { excite: list.iter().filter_map(Value::as_str).take(1024).map(crate::dialogs::bounded).collect(), ..f },
            None => f,
        }
    }

    /// The band (display units) start and stop give, when they do.
    pub fn band(&self) -> Option<(f64, f64)> {
        let (a, z) = (positive(&self.start).ok()?, positive(&self.stop).ok()?);
        (a <= z).then_some((a, z))
    }
}

/// A positive finite number from a field, or why not.
pub fn positive(text: &str) -> Result<f64, String> {
    let v = crate::dialogs::parse_number(text)?;
    if !(v > 0.0) {
        return Err(tr(t!("must be greater than 0")).into());
    }
    Ok(v)
}

/// An integer within `lo..=hi`, or why not.
///
/// ```
/// use emcraft_ui_core::steps::form::integer;
/// assert_eq!(integer(" 7 ", 1, 10), Ok(7));
/// assert!(integer("0", 1, 10).is_err() && integer("x", 1, 10).is_err());
/// ```
pub fn integer(text: &str, lo: u64, hi: u64) -> Result<u64, String> {
    let range = args!(lo = lo, hi = hi);
    let v: u64 = text.trim().parse().map_err(|_| trf(t!("must be a whole number from {lo} to {hi}"), range))?;
    if !(lo..=hi).contains(&v) {
        return Err(trf(t!("must be from {lo} to {hi}"), range));
    }
    Ok(v)
}

/// The form `f` checked for `solver` (`limit` the most frequencies a sweep takes): the settings
/// as `project.setup` takes them, or every bad field in a line; and the form with each field's
/// error marked. The engine checks the rest when they are confirmed.
///
/// ```
/// use emcraft_ui_core::steps::{Solver, form::{Fields, checked}};
/// let f = Fields { start: "6".into(), stop: "9".into(), ..Fields::default() };
/// let (params, f) = checked(Solver::Frequency, 1000, f);
/// assert_eq!(params.map(|p| p["points"].clone()), Ok(serde_json::json!(101)));
/// let (params, f) = checked(Solver::Frequency, 1000, Fields { stop: "5".into(), ..f });
/// assert!(params.is_err() && f.error("stop").is_some());
/// ```
pub fn checked(solver: Solver, limit: u64, f: Fields) -> (Result<Value, String>, Fields) {
    let f = f.with_defaults();
    let number = |text: &str| positive(text).map(|v| json!(v));
    let whole = |text: &str, hi: u64| integer(text, 1, hi).map(|v| json!(v));
    let modes = emcraft_model::MAX_MODES as u64;
    // Each field the kind reads, with what it reads as.
    let wanted: Vec<(&'static str, Result<Value, String>)> = match solver {
        Solver::Frequency => {
            vec![("start", number(&f.start)), ("stop", number(&f.stop)), ("points", whole(&f.points, limit)), ("order", whole(&f.order, 2))]
        }
        Solver::TimeDomain => {
            vec![("start", number(&f.start)), ("stop", number(&f.stop)), ("points", whole(&f.points, limit)), ("cell", number(&f.cell))]
        }
        Solver::Eigenmodes => vec![("frequency", number(&f.frequency)), ("count", whole(&f.count, modes)), ("order", whole(&f.order, 2))],
        Solver::Radiation => vec![("frequency", number(&f.frequency)), ("order", whole(&f.order, 2))],
        Solver::Pcb => return (Err(tr(t!("the PCB diagnosis has no run settings")).into()), f),
        Solver::System => {
            let budget = (!f.frequency.trim().is_empty()).then(|| ("frequency", number(&f.frequency)));
            vec![("start", number(&f.start)), ("stop", number(&f.stop)), ("points", whole(&f.points, limit))].into_iter().chain(budget).collect()
        }
    };
    let below = matches!(solver, Solver::Frequency | Solver::TimeDomain | Solver::System)
        && matches!((positive(&f.start), positive(&f.stop)), (Ok(a), Ok(z)) if z < a);
    let stop_below = below.then(|| ("stop", Err::<Value, String>(tr(t!("must not be below the start")).to_owned())));
    let all: Vec<(&'static str, Result<Value, String>)> = wanted.into_iter().chain(stop_below).collect();
    let f = all.iter().fold(f, |f, (name, r)| f.with_error(name, r.as_ref().err().cloned()));
    // A stop below the start keeps its error even after its number read well.
    let f = if below { f.with_error("stop", Some(tr(t!("must not be below the start")).to_owned())) } else { f };
    let bad: Vec<String> = all.iter().filter_map(|(name, r)| r.as_ref().err().map(|e| format!("{}: {e}", lookup(name)))).collect();
    if !bad.is_empty() {
        return (Err(bad.join("; ")), f);
    }
    let mut p = json!({"kind": solver.id()});
    for (name, r) in all {
        if let Ok(v) = r {
            p[name] = v;
        }
    }
    if solver == Solver::TimeDomain && !f.excite.is_empty() {
        p["excite"] = json!(f.excite);
    }
    // The lowest modes, none left out: the frequency is the one the mesh is for, not a target
    // whose nearest modes could leave a lower one out (a cavity's dimensions changed moved its
    // modes past a fixed target).
    if solver == Solver::Eigenmodes {
        p["lowest"] = json!(true);
    }
    (Ok(p), f)
}
