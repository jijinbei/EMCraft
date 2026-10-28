//! The verification oracle (docs/verification.md): every case in `tests/oracle/<case>/` is our
//! own model of a problem with a known answer, run through the command engine and compared with
//! its reference values and with its frozen fixture.
//!
//! A case directory holds:
//! - `expected.json`: the case's kind (theory, benchmark, experiment), source and quantities:
//!   how each is measured, the reference value, the tolerance and the reason for it;
//! - `recipe.json`: the model as EMCraft commands (a JSON array of `{"command", "params"}`, each
//!   step optionally with an `id` that measures can read its reply by);
//! - `fixture.json`: our accepted values, written by `--bless` once the case passes;
//! - `README.md`: the problem in our own words.
//!
//! A measure reads a number from a step's reply (`{"step": id, "pointer": "/json/pointer"}`)
//! or from a command run after the recipe (`{"command", "params", "pointer"}`). With
//! `"minimum": {"x": "/frequency", "y": "/s21/mag"}` the pointer names a list (a sweep's points)
//! and the number is the x where y is least: the vertex of the parabola through y² at the least
//! sampled point and its two neighbours (a notch, where y ∝ |x − x₀| and y² is quadratic).
//! With `"crossing": {"x", "y", "level", "which": "first" | "last"}` the number is the x where y
//! first (or last) crosses `level` along the list, linear between the two points around it (a
//! filter's band edge). With `"phase_minus": "/other/pointer"` the number is the angle at the
//! pointer less the one at the other (degrees, wrapped into (−180°, 180°]): a phase difference
//! between two entries of S.
//!
//! Adapted from FieldCraft's `crates/testkit/src/oracle.rs` (MIT OR Apache-2.0), see
//! ATTRIBUTION.md.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;
use std::time::Instant;

use emcraft_engine::{EngineError, Session};
use serde_json::{Value, json};

/// Why a case could not be run or measured.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum OracleError {
    /// A file of the case could not be read, parsed or written.
    #[error("{path}: {why}")]
    File { path: String, why: String },
    /// `expected.json` does not say what the oracle needs.
    #[error("expected.json: {0}")]
    Expected(String),
    #[error("recipe.json: {0}")]
    Recipe(String),
    /// A recipe step the engine refused.
    #[error("recipe step {step} ({command}): {error}")]
    Step { step: usize, command: String, error: EngineError },
    /// A quantity could not be read.
    #[error("{0}")]
    Measure(String),
}

impl OracleError {
    fn file(path: &Path, why: impl std::fmt::Display) -> OracleError {
        OracleError::File { path: path.display().to_string(), why: why.to_string() }
    }
}

type Result<T> = std::result::Result<T, OracleError>;

/// Fixture band: a result may move by this fraction of its size (plus a thousandth of the
/// reference's absolute tolerance) before CI calls it a change. Results are deterministic; the
/// band only absorbs last-digit differences of the system maths library.
pub const FIXTURE_RELATIVE: f64 = 1e-9;
const ABSOLUTE_SHARE: f64 = 1e-3;

/// The kinds of reference.
pub const KINDS: [&str; 3] = ["theory", "benchmark", "experiment"];

#[derive(Debug, Default, Clone)]
pub struct Options {
    /// Run only these cases (all when empty).
    pub cases: Vec<String>,
    /// Write `fixture.json` for these cases when they pass (`all` for every passing case).
    pub bless: Vec<String>,
    /// Write docs/oracle.md (only after a run of every case).
    pub write: bool,
}

/// |ours − reference| ≤ relative·|reference| + absolute.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Tolerance {
    pub relative: f64,
    pub absolute: f64,
}

impl Tolerance {
    fn from_json(v: &Value) -> std::result::Result<Tolerance, String> {
        let get = |k: &str| -> std::result::Result<f64, String> {
            match v.get(k) {
                None => Ok(0.0),
                Some(x) => x.as_f64().filter(|x| x.is_finite() && *x >= 0.0).ok_or_else(|| format!("tolerance `{k}` must be a number ≥ 0")),
            }
        };
        let t = Tolerance { relative: get("relative")?, absolute: get("absolute")? };
        if t.relative == 0.0 && t.absolute == 0.0 {
            return Err("a tolerance gives `relative` or `absolute`".into());
        }
        Ok(t)
    }

    pub fn allows(&self, reference: f64, ours: f64) -> bool {
        (ours - reference).abs() <= self.relative * reference.abs() + self.absolute
    }

    fn describe(&self) -> String {
        match (self.relative > 0.0, self.absolute > 0.0) {
            (true, false) => format!("{} rel.", sig(self.relative)),
            (false, true) => format!("± {}", sig(self.absolute)),
            _ => format!("{} rel. + {}", sig(self.relative), sig(self.absolute)),
        }
    }
}

fn sig(v: f64) -> String {
    if v == 0.0 {
        "0".into()
    } else if (1e-3..1e5).contains(&v.abs()) {
        let s = format!("{v:.5}");
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        format!("{v:.3e}")
    }
}

#[derive(Debug, Clone)]
pub struct Quantity {
    pub id: String,
    pub description: String,
    pub measure: Value,
    pub reference: f64,
    pub tolerance: Tolerance,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct Expected {
    pub title: String,
    pub kind: String,
    pub source: String,
    pub quantities: Vec<Quantity>,
}

fn text(v: &Value, key: &str) -> std::result::Result<String, String> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("`{key}` must be a non-empty string"))
}

impl Expected {
    pub fn parse(v: &Value) -> Result<Expected> {
        Expected::read(v).map_err(OracleError::Expected)
    }

    fn read(v: &Value) -> std::result::Result<Expected, String> {
        let kind = text(v, "kind")?;
        if !KINDS.contains(&kind.as_str()) {
            return Err(format!("`kind` must be one of {KINDS:?}"));
        }
        let list = v.get("quantities").and_then(Value::as_array).filter(|q| !q.is_empty()).ok_or("`quantities` must be a non-empty list")?;
        let mut quantities: Vec<Quantity> = Vec::with_capacity(list.len());
        for q in list {
            let id = text(q, "id")?;
            let context = |e: String| format!("quantity `{id}`: {e}");
            let reference =
                q.get("value").and_then(Value::as_f64).filter(|x| x.is_finite()).ok_or_else(|| context("`value` must be a finite number".into()))?;
            let tolerance = Tolerance::from_json(q.get("tolerance").unwrap_or(&Value::Null)).map_err(context)?;
            let measure = q.get("measure").cloned().unwrap_or(Value::Null);
            if measure.get("pointer").and_then(Value::as_str).is_none() {
                return Err(context("`measure` needs a JSON `pointer`".into()));
            }
            if quantities.iter().any(|x| x.id == id) {
                return Err(format!("quantity `{id}` is listed twice"));
            }
            quantities.push(Quantity {
                description: text(q, "quantity").map_err(context)?,
                reason: text(q, "reason").map_err(context)?,
                id,
                measure,
                reference,
                tolerance,
            });
        }
        Ok(Expected { title: text(v, "title")?, kind, source: text(v, "source")?, quantities })
    }
}

/// One quantity's outcome.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub quantity: Quantity,
    pub value: Result<f64>,
    pub pass: bool,
    /// `Some(true)` matches the fixture, `Some(false)` moved, `None` no fixture.
    pub fixture: Option<bool>,
}

fn read_json(path: &Path) -> Result<Value> {
    let text = std::fs::read_to_string(path).map_err(|e| OracleError::file(path, e))?;
    serde_json::from_str(&text).map_err(|e| OracleError::file(path, e))
}

/// Runs one case: its recipe, then every quantity's measure.
pub fn run_case(dir: &Path) -> Result<(Expected, Vec<Outcome>)> {
    let expected = Expected::parse(&read_json(&dir.join("expected.json"))?)?;
    let recipe = read_json(&dir.join("recipe.json"))?;
    let steps = recipe.as_array().ok_or_else(|| OracleError::Recipe("must be a JSON array".into()))?;
    let mut session = Session::new();
    let mut replies: BTreeMap<String, Value> = BTreeMap::new();
    for (i, step) in steps.iter().enumerate() {
        let id = step.get("command").and_then(Value::as_str).ok_or_else(|| OracleError::Recipe(format!("step {}: no `command`", i + 1)))?;
        let mut params = step.get("params").cloned().unwrap_or_else(|| json!({}));
        // Files a case brings (an imported surface) are named relative to its directory.
        if let Some(path) = params.get("path").and_then(Value::as_str).filter(|p| id == "geometry.import" && Path::new(p).is_relative()) {
            params["path"] = json!(dir.join(path).to_string_lossy());
        }
        let reply = session.execute(id, &params).map_err(|error| OracleError::Step { step: i + 1, command: id.to_owned(), error })?;
        if let Some(name) = step.get("id").and_then(Value::as_str) {
            replies.insert(name.to_owned(), reply);
        }
    }
    let fixture = read_json(&dir.join("fixture.json")).ok();
    let mut out = Vec::new();
    for q in &expected.quantities {
        let m = &q.measure;
        let pointer = m.get("pointer").and_then(Value::as_str).unwrap_or_default();
        let reply = if let Some(step) = m.get("step").and_then(Value::as_str) {
            replies.get(step).cloned().ok_or_else(|| OracleError::Measure(format!("no recipe step with id `{step}`")))
        } else if let Some(cmd) = m.get("command").and_then(Value::as_str) {
            session.execute(cmd, m.get("params").unwrap_or(&json!({}))).map_err(|e| OracleError::Measure(e.to_string()))
        } else {
            Err(OracleError::Measure("`measure` gives a `step` or a `command`".into()))
        };
        let value = reply.and_then(|r| match (m.get("minimum"), m.get("crossing"), m.get("phase_minus")) {
            (Some(spec), _, _) => minimum(&r, pointer, spec),
            (None, Some(spec), _) => crossing(&r, pointer, spec),
            (None, None, Some(other)) => phase_minus(&r, pointer, other),
            (None, None, None) => r.pointer(pointer).and_then(Value::as_f64).ok_or_else(|| OracleError::Measure(format!("no number at `{pointer}`"))),
        });
        let pass = value.as_ref().is_ok_and(|v| q.tolerance.allows(q.reference, *v));
        let fixed = fixture.as_ref().and_then(|f| f.pointer(&format!("/values/{}", q.id)));
        let fixture = match (fixed, &value) {
            (Some(f), Ok(v)) => {
                let (fv, band) = (f["value"].as_f64().unwrap_or(f64::NAN), f["band"].as_f64().unwrap_or(0.0));
                Some((v - fv).abs() <= band)
            }
            (Some(_), Err(_)) => Some(false),
            (None, _) => None,
        };
        out.push(Outcome { quantity: q.clone(), value, pass, fixture });
    }
    Ok((expected, out))
}

/// The angle (degrees) at `pointer` less the one at `other`, wrapped into (−180°, 180°].
fn phase_minus(reply: &Value, pointer: &str, other: &Value) -> Result<f64> {
    let other = other.as_str().ok_or_else(|| OracleError::Measure("`phase_minus` must be a JSON pointer".into()))?;
    let at = |p: &str| {
        reply.pointer(p).and_then(Value::as_f64).filter(|v| v.is_finite()).ok_or_else(|| OracleError::Measure(format!("no number at `{p}`")))
    };
    let d = (at(pointer)? - at(other)?).rem_euclid(360.0);
    Ok(if d > 180.0 { d - 360.0 } else { d })
}

/// The x where y is least over the list at `pointer` (see the module docs): the parabola's vertex
/// through y² at the least point and its neighbours, which must be evenly spaced in x.
fn minimum(reply: &Value, pointer: &str, spec: &Value) -> Result<f64> {
    let fail = |why: String| OracleError::Measure(format!("minimum over `{pointer}`: {why}"));
    let list = reply.pointer(pointer).and_then(Value::as_array).ok_or_else(|| fail("not a list".into()))?;
    let key = |k: &str| spec.get(k).and_then(Value::as_str).ok_or_else(|| fail(format!("`minimum` needs a JSON pointer `{k}`")));
    let (kx, ky) = (key("x")?, key("y")?);
    let points = list
        .iter()
        .map(|p| match (p.pointer(kx).and_then(Value::as_f64), p.pointer(ky).and_then(Value::as_f64)) {
            (Some(x), Some(y)) if x.is_finite() && y.is_finite() => Ok((x, y * y)),
            _ => Err(fail(format!("a point without numbers at `{kx}` and `{ky}`"))),
        })
        .collect::<Result<Vec<_>>>()?;
    let k = (0..points.len()).min_by(|a, b| points[*a].1.total_cmp(&points[*b].1)).ok_or_else(|| fail("no points".into()))?;
    if k == 0 || k + 1 == points.len() {
        return Err(fail(format!("the least value is at the list's end (x = {}): widen the sweep", points[k].0)));
    }
    let [(x0, y0), (x1, y1), (x2, y2)] = [points[k - 1], points[k], points[k + 1]];
    let h = x1 - x0;
    if !(h > 0.0) || ((x2 - x1) - h).abs() > 1e-9 * h.abs().max(x1.abs()) {
        return Err(fail("the points must be evenly spaced, ascending".into()));
    }
    let curvature = y0 - 2.0 * y1 + y2;
    if !(curvature > 0.0) {
        return Err(fail("no minimum between the points".into()));
    }
    Ok(x1 + 0.5 * h * (y0 - y2) / curvature)
}

/// The x where y first (or last) crosses the level over the list at `pointer` (see the module
/// docs), linear between the two points around the crossing.
fn crossing(reply: &Value, pointer: &str, spec: &Value) -> Result<f64> {
    let fail = |why: String| OracleError::Measure(format!("crossing over `{pointer}`: {why}"));
    let list = reply.pointer(pointer).and_then(Value::as_array).ok_or_else(|| fail("not a list".into()))?;
    let key = |k: &str| spec.get(k).and_then(Value::as_str).ok_or_else(|| fail(format!("`crossing` needs a JSON pointer `{k}`")));
    let (kx, ky) = (key("x")?, key("y")?);
    let level =
        spec.get("level").and_then(Value::as_f64).filter(|v| v.is_finite()).ok_or_else(|| fail("`crossing` needs a finite `level`".into()))?;
    let last = match spec.get("which").and_then(Value::as_str) {
        Some("first") | None => false,
        Some("last") => true,
        Some(other) => return Err(fail(format!("`which` is `first` or `last`, not `{other}`"))),
    };
    let points = list
        .iter()
        .map(|p| match (p.pointer(kx).and_then(Value::as_f64), p.pointer(ky).and_then(Value::as_f64)) {
            (Some(x), Some(y)) if x.is_finite() && y.is_finite() => Ok((x, y)),
            _ => Err(fail(format!("a point without numbers at `{kx}` and `{ky}`"))),
        })
        .collect::<Result<Vec<_>>>()?;
    let mut found = points.windows(2).filter(|w| (w[0].1 - level) * (w[1].1 - level) <= 0.0 && w[0].1 != w[1].1);
    let w = if last { found.next_back() } else { found.next() }.ok_or_else(|| fail(format!("y never crosses {level}: widen the sweep")))?;
    let [(x0, y0), (x1, y1)] = [w[0], w[1]];
    Ok(x0 + (level - y0) * (x1 - x0) / (y1 - y0))
}

fn band(q: &Quantity, v: f64) -> f64 {
    FIXTURE_RELATIVE * v.abs().max(q.reference.abs()) + ABSOLUTE_SHARE * q.tolerance.absolute + 1e-15
}

/// Writes a case's fixture from passing outcomes.
fn bless(dir: &Path, outcomes: &[Outcome]) -> Result<()> {
    let mut values = serde_json::Map::new();
    for o in outcomes {
        let v = o.value.as_ref().map_err(|e| OracleError::Measure(format!("{}: {e}", o.quantity.id)))?;
        values.insert(o.quantity.id.clone(), json!({"value": v, "band": band(&o.quantity, *v)}));
    }
    let doc = json!({"note": "EMCraft's accepted results for this case, written by `cargo xtask oracle --bless`. A result outside its band fails CI.", "values": values});
    let path = dir.join("fixture.json");
    let text = serde_json::to_string_pretty(&doc).map_err(|e| OracleError::file(&path, e))?;
    std::fs::write(&path, text + "\n").map_err(|e| OracleError::file(&path, e))
}

/// The case directories, sorted.
pub fn cases(root: &Path) -> Result<Vec<String>> {
    let dir = root.join("tests/oracle");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .map_err(|e| OracleError::file(&dir, e))?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().join("expected.json").exists())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    names.sort();
    Ok(names)
}

/// Runs the cases; prints a line per quantity; returns whether everything passed and matched
/// its fixture.
pub fn run(root: &Path, opts: &Options) -> Result<bool> {
    let all = cases(root)?;
    let selected: Vec<String> = if opts.cases.is_empty() { all.clone() } else { opts.cases.clone() };
    let mut ok = true;
    let mut page_rows: Vec<String> = Vec::new();
    let (mut passed, mut total) = (0, 0);
    for name in &selected {
        let dir = root.join("tests/oracle").join(name);
        let t0 = Instant::now();
        let (expected, outcomes) = match run_case(&dir) {
            Ok(r) => r,
            Err(e) => {
                println!("FAIL  {name}: {e}");
                ok = false;
                continue;
            }
        };
        println!("{name} ({}, {:.1} s): {}", expected.kind, t0.elapsed().as_secs_f64(), expected.title);
        let case_pass = outcomes.iter().all(|o| o.pass);
        for o in &outcomes {
            total += 1;
            passed += usize::from(o.pass);
            let v = o.value.as_ref().map_or_else(ToString::to_string, |v| sig(*v));
            let fx = match o.fixture {
                Some(true) => "matches",
                Some(false) => "MOVED",
                None => "none",
            };
            println!(
                "  {:<5} {:<14} ours {:<14} reference {:<14} tol {:<14} fixture {}",
                if o.pass { "pass" } else { "FAIL" },
                o.quantity.id,
                v,
                sig(o.quantity.reference),
                o.quantity.tolerance.describe(),
                fx
            );
            if !o.pass || o.fixture == Some(false) {
                ok = false;
            }
            let err = o.value.as_ref().map_or("—".into(), |v| {
                if o.quantity.reference != 0.0 {
                    let pct = 100.0 * (v - o.quantity.reference) / o.quantity.reference.abs();
                    if pct.abs() < 0.01 { format!("{pct:.1e}%") } else { format!("{pct:.3}%") }
                } else {
                    sig(v - o.quantity.reference)
                }
            });
            page_rows.push(format!(
                "| [{name}](../tests/oracle/{name}/) | {} | {} | {} | {err} | {} | {} | {fx} |",
                o.quantity.description.replace('|', "\\|"),
                sig(o.quantity.reference),
                v,
                o.quantity.tolerance.describe(),
                if o.pass { "pass" } else { "**fail**" }
            ));
        }
        let wants_bless = opts.bless.iter().any(|b| b == name || b == "all");
        if wants_bless {
            if case_pass {
                bless(&dir, &outcomes)?;
                println!("  blessed {name}/fixture.json");
            } else {
                println!("  not blessed: the case fails its references");
            }
        }
    }
    println!("oracle: {passed} of {total} quantities pass");
    if opts.write && opts.cases.is_empty() {
        let mut page = String::new();
        let _ = writeln!(
            page,
            "# Verification oracle\n\nGenerated by `cargo xtask oracle` from [`tests/oracle/`](../tests/oracle/); the policy is [verification.md](verification.md). Every case is our own model, built with EMCraft commands, compared with the closed-form reference its `expected.json` cites, within the stated tolerance and for the stated reason. Accepted results are frozen in each case's `fixture.json`.\n\n**{passed} of {total} quantities pass.**\n\n| Case | Quantity | Reference | EMCraft | Error | Tolerance | Result | Fixture |\n|---|---|---|---|---|---|---|---|"
        );
        for r in &page_rows {
            let _ = writeln!(page, "{r}");
        }
        let path = root.join("docs/oracle.md");
        std::fs::write(&path, page).map_err(|e| OracleError::file(&path, e))?;
        println!("wrote docs/oracle.md");
    }
    Ok(ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tolerances_and_parsing() {
        let t = Tolerance { relative: 0.01, absolute: 0.0 };
        assert!(t.allows(100.0, 100.9) && !t.allows(100.0, 101.1));
        assert!(Tolerance::from_json(&json!({})).is_err());
        assert!(Tolerance::from_json(&json!({"relative": -1})).is_err());
        let e = json!({"title": "t", "kind": "theory", "source": "s", "quantities": [
            {"id": "a", "quantity": "q", "measure": {"step": "x", "pointer": "/v"}, "value": 1.0, "tolerance": {"absolute": 0.1}, "reason": "r"}]});
        assert!(Expected::parse(&e).is_ok());
        let mut bad = e.clone();
        bad["kind"] = json!("guess");
        assert!(Expected::parse(&bad).is_err());
        let mut twice = e.clone();
        let q = twice["quantities"][0].clone();
        twice["quantities"].as_array_mut().unwrap().push(q);
        assert!(Expected::parse(&twice).is_err());
    }

    #[test]
    fn a_minimum_is_the_parabolas_vertex() {
        // y = |x − 2.3| sampled at 0.5 steps: y² is a parabola, its vertex exact.
        let points: Vec<Value> = (0..9).map(|i| f64::from(i) * 0.5).map(|x| json!({"f": x, "s": {"mag": (x - 2.3f64).abs()}})).collect();
        let reply = json!({"points": points});
        let spec = json!({"x": "/f", "y": "/s/mag"});
        assert!((minimum(&reply, "/points", &spec).unwrap() - 2.3).abs() < 1e-12);
        // At the list's end, uneven, or without the keys: refused.
        let edge: Vec<Value> = (0..4).map(|i| json!({"f": f64::from(i), "s": {"mag": f64::from(i)}})).collect();
        assert!(minimum(&json!({"points": edge}), "/points", &spec).is_err());
        let uneven = json!({"points": [{"f": 0.0, "s": {"mag": 1.0}}, {"f": 1.0, "s": {"mag": 0.1}}, {"f": 3.0, "s": {"mag": 1.0}}]});
        assert!(minimum(&uneven, "/points", &spec).is_err());
        assert!(minimum(&reply, "/points", &json!({"x": "/f"})).is_err());
        assert!(minimum(&reply, "/nothing", &spec).is_err());
    }

    #[test]
    fn a_phase_difference_is_wrapped() {
        let reply = json!({"a": 170.0, "b": -100.0, "c": "x"});
        assert!((phase_minus(&reply, "/a", &json!("/b")).unwrap() - -90.0).abs() < 1e-12);
        assert!((phase_minus(&reply, "/b", &json!("/a")).unwrap() - 90.0).abs() < 1e-12);
        assert!(phase_minus(&reply, "/a", &json!("/c")).is_err() && phase_minus(&reply, "/a", &json!(3)).is_err());
    }

    #[test]
    fn a_crossing_is_interpolated_between_its_points() {
        // A band: y rises through 0.5 at x = 1.25 and falls through it at x = 3.75.
        let points: Vec<Value> = (0..11).map(|i| f64::from(i) * 0.5).map(|x| json!({"f": x, "s": {"mag": 1.0 - (x - 2.5f64).abs() * 0.4}})).collect();
        let reply = json!({"points": points});
        let first = json!({"x": "/f", "y": "/s/mag", "level": 0.5});
        let last = json!({"x": "/f", "y": "/s/mag", "level": 0.5, "which": "last"});
        assert!((crossing(&reply, "/points", &first).unwrap() - 1.25).abs() < 1e-12);
        assert!((crossing(&reply, "/points", &last).unwrap() - 3.75).abs() < 1e-12);
        assert!(crossing(&reply, "/points", &json!({"x": "/f", "y": "/s/mag", "level": 2.0})).is_err());
        assert!(crossing(&reply, "/points", &json!({"x": "/f", "y": "/s/mag", "level": 0.5, "which": "middle"})).is_err());
        assert!(crossing(&reply, "/points", &json!({"x": "/f", "y": "/s/mag"})).is_err());
    }

    /// Every committed case parses and has a recipe and a README (running them is
    /// `cargo xtask oracle`).
    #[test]
    fn committed_cases_are_well_formed() {
        let root = crate::root();
        let names = cases(&root).unwrap();
        assert!(names.len() >= 3, "{names:?}");
        for n in names {
            let dir = root.join("tests/oracle").join(&n);
            Expected::parse(&read_json(&dir.join("expected.json")).unwrap()).unwrap_or_else(|e| panic!("{n}: {e}"));
            assert!(read_json(&dir.join("recipe.json")).unwrap().is_array(), "{n}");
            assert!(dir.join("README.md").exists(), "{n}");
            assert!(dir.join("fixture.json").exists(), "{n}: bless it once it passes");
        }
    }
}
