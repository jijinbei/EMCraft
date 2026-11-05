//! `cargo xtask bench`: a fixed set of oracle recipes run by the release `emcraft-cli` (about a
//! minute in all), each
//! one's wall-clock time and the solvers' own split of it (the `seconds` they reply: assembly,
//! analysis, factorization, solves), and every other number they reply. Saved as a baseline
//! (`--save FILE`), a later run is compared with it (`--against FILE`): the times as ratios and
//! the replies **bit for bit** — a change meant to leave the results alone (a refactor, a faster
//! kernel) must reproduce them exactly. A number is written by serde_json as the shortest text
//! that reads back to the same `f64`, so equal text is equal bits.

use std::path::Path;
use std::process::Command;
use std::time::Instant;

use serde_json::{Map, Value, json};

/// The cases: one of each solver and pipeline, each a few seconds in release.
pub const CASES: &[&str] = &[
    // The driven FEM, second order, a swept band: a little of everything.
    "highpass-filter",
    // Eigenmodes of a solid's mesh.
    "loaded-sphere-cavity",
    // A board's FEM: the largest factorization of the set.
    "pcb-microstrip-fem",
    // FDTD.
    "highpass-time-domain",
    // A parameter study: the model rebuilt and solved per point.
    "study-sweep-highpass",
];
/// The solvers' split of the time, summed over a case's replies.
const PARTS: [&str; 4] = ["assembly", "analysis", "factor", "solve"];

pub fn run(root: &Path, rest: &[&str]) -> Result<(), String> {
    let (mut save, mut against, mut runs, mut only) = (None, None, 1usize, Vec::new());
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        match *a {
            "--save" => save = Some(it.next().ok_or("--save needs a file")?.to_string()),
            "--against" => against = Some(it.next().ok_or("--against needs a file")?.to_string()),
            "--runs" => runs = it.next().and_then(|n| n.parse().ok()).filter(|n| (1..=20).contains(n)).ok_or("--runs needs 1..=20")?,
            "--case" => only.push(it.next().ok_or("--case needs a name")?.to_string()),
            other => return Err(format!("bench: unknown argument `{other}`")),
        }
    }
    let baseline = match &against {
        Some(f) => {
            Some(serde_json::from_str::<Value>(&std::fs::read_to_string(f).map_err(|e| format!("{f}: {e}"))?).map_err(|e| format!("{f}: {e}"))?)
        }
        None => None,
    };
    let cli = build(root)?;
    let cases: Vec<&str> = if only.is_empty() { CASES.to_vec() } else { only.iter().map(String::as_str).collect() };
    let mut out = Map::new();
    let mut changed = Vec::new();
    println!("{:<24} {:>8} {:>9} {:>9} {:>9} {:>9}  results", "case", "wall s", "assembly", "analysis", "factor", "solve");
    for case in cases {
        let dir = root.join("tests/oracle").join(case);
        let mut best: Option<(f64, Vec<Value>)> = None;
        for _ in 0..runs {
            let (wall, replies) = once(&cli, &dir)?;
            if best.as_ref().is_none_or(|(w, _)| wall < *w) {
                best = Some((wall, replies));
            }
        }
        let Some((wall, replies)) = best else { continue };
        let mut seconds = [0.0; 4];
        for r in &replies {
            add_seconds(r, &mut seconds);
        }
        let results: Vec<Value> = replies.iter().map(without_seconds).collect();
        let old = baseline.as_ref().and_then(|b| b.get(case));
        let verdict = match old {
            None => String::new(),
            Some(o) => match first_difference(o.get("results").unwrap_or(&Value::Null), &Value::Array(results.clone()), "") {
                None => format!("same bits · {:.2}× time", wall / o["wall"].as_f64().unwrap_or(f64::NAN)),
                Some(at) => {
                    changed.push(case);
                    format!("CHANGED at {at}")
                }
            },
        };
        let split = if seconds.iter().all(|s| *s == 0.0) {
            format!("{:>39}", "(no split replied)")
        } else {
            format!("{:>9.3} {:>9.3} {:>9.3} {:>9.3}", seconds[0], seconds[1], seconds[2], seconds[3])
        };
        println!("{case:<24} {wall:>8.2} {split}  {verdict}");
        let parts: Map<String, Value> = PARTS.iter().zip(seconds).map(|(k, v)| ((*k).to_owned(), json!(v))).collect();
        out.insert(case.to_owned(), json!({"wall": wall, "seconds": parts, "results": results}));
    }
    if let Some(f) = save {
        let text = serde_json::to_string_pretty(&Value::Object(out)).map_err(|e| e.to_string())?;
        std::fs::write(&f, text).map_err(|e| format!("{f}: {e}"))?;
        println!("wrote {f}");
    }
    if changed.is_empty() { Ok(()) } else { Err(format!("bench: the results moved in {}", changed.join(", "))) }
}

/// The release `emcraft-cli`, built.
fn build(root: &Path) -> Result<std::path::PathBuf, String> {
    let mut c = crate::cargo();
    c.args(["build", "--release", "-p", "emcraft-cli", "--message-format=json-render-diagnostics"]);
    let out = c.output().map_err(|e| format!("cargo build: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo build failed:\n{}", String::from_utf8_lossy(&out.stderr)));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|m| m["reason"] == "compiler-artifact" && m["target"]["name"] == "emcraft-cli")
        .find_map(|m| m["executable"].as_str().map(std::path::PathBuf::from))
        .ok_or_else(|| format!("no emcraft-cli binary under {}", root.display()))
}

/// One run of a case's recipe (from its folder, where its files are): the wall-clock seconds
/// and the replies, the model's summary last.
fn once(cli: &Path, dir: &Path) -> Result<(f64, Vec<Value>), String> {
    let start = Instant::now();
    let out = Command::new(cli).arg("run").arg("recipe.json").current_dir(dir).output().map_err(|e| format!("{}: {e}", dir.display()))?;
    let wall = start.elapsed().as_secs_f64();
    if !out.status.success() {
        return Err(format!("{}: {}", dir.display(), String::from_utf8_lossy(&out.stderr)));
    }
    let replies = serde_json::Deserializer::from_slice(&out.stdout)
        .into_iter::<Value>()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok((wall, replies))
}

/// Adds every `seconds` split under `v` to `sum`.
fn add_seconds(v: &Value, sum: &mut [f64; 4]) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                if k == "seconds" {
                    for (s, p) in sum.iter_mut().zip(PARTS) {
                        *s += x.get(p).and_then(Value::as_f64).unwrap_or(0.0);
                    }
                } else {
                    add_seconds(x, sum);
                }
            }
        }
        Value::Array(a) => a.iter().for_each(|x| add_seconds(x, sum)),
        _ => {}
    }
}

/// `v` without its timings, the one part of a reply that changes from run to run.
fn without_seconds(v: &Value) -> Value {
    match v {
        Value::Object(m) => Value::Object(m.iter().filter(|(k, _)| *k != "seconds").map(|(k, x)| (k.clone(), without_seconds(x))).collect()),
        Value::Array(a) => Value::Array(a.iter().map(without_seconds).collect()),
        other => other.clone(),
    }
}

/// Where `a` and `b` first differ (a JSON pointer), comparing numbers by their text.
fn first_difference(a: &Value, b: &Value, at: &str) -> Option<String> {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            if x.len() != y.len() {
                return Some(format!("{at} (keys)"));
            }
            x.iter().find_map(|(k, v)| y.get(k).map_or_else(|| Some(format!("{at}/{k}")), |w| first_difference(v, w, &format!("{at}/{k}"))))
        }
        (Value::Array(x), Value::Array(y)) => {
            if x.len() != y.len() {
                return Some(format!("{at} (length)"));
            }
            x.iter().zip(y).enumerate().find_map(|(i, (v, w))| first_difference(v, w, &format!("{at}/{i}")))
        }
        (Value::Number(x), Value::Number(y)) => (x.to_string() != y.to_string()).then(|| at.to_owned()),
        _ => (a != b).then(|| at.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timings_are_summed_and_left_out_of_the_comparison() {
        let r = json!({"results": [{"seconds": {"assembly": 1.0, "factor": 2.0}, "s": 0.5}, {"seconds": {"assembly": 0.5}}]});
        let mut sum = [0.0; 4];
        add_seconds(&r, &mut sum);
        assert_eq!(sum, [1.5, 0.0, 2.0, 0.0]);
        assert_eq!(without_seconds(&r), json!({"results": [{"s": 0.5}, {}]}));
    }

    #[test]
    fn the_first_difference_is_found_to_the_last_bit() {
        let a = json!({"s": [1.0, 0.1 + 0.2]});
        assert_eq!(first_difference(&a, &a.clone(), ""), None);
        assert_eq!(first_difference(&a, &json!({"s": [1.0, 0.3]}), ""), Some("/s/1".into()));
        assert_eq!(first_difference(&a, &json!({"s": [1.0]}), ""), Some("/s (length)".into()));
    }
}
