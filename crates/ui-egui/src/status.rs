//! The status line's text for a command that succeeded, read from its reply.

use emcraft_engine::Session;
use serde_json::Value;

use crate::label;
use crate::lang::{args, tr, trf};

/// A command's JSON reply, read leniently: a missing or mistyped field reads as a placeholder
/// (NaN, 0, empty), so the text never fails.
#[derive(Clone, Copy)]
struct Reply<'a>(&'a Value);

impl<'a> Reply<'a> {
    fn num(self, k: &str) -> f64 {
        self.0.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN)
    }

    fn count(self, k: &str) -> u64 {
        self.0.get(k).and_then(Value::as_u64).unwrap_or(0)
    }

    fn text(self, k: &str) -> &'a str {
        self.0.get(k).and_then(Value::as_str).unwrap_or("")
    }

    fn list(self, k: &str) -> &'a [Value] {
        self.0.get(k).and_then(Value::as_array).map_or(&[], Vec::as_slice)
    }

    fn flag(self, k: &str) -> bool {
        self.0.get(k).and_then(Value::as_bool) == Some(true)
    }
}

/// The message for command `id` run with `params`, which replied `reply`, the session as it
/// left it.
pub fn text(id: &str, params: &Value, reply: &Value, session: &Session) -> String {
    let (p, r) = (Reply(params), Reply(reply));
    let unit = session.display().frequency.symbol();
    match id {
        "file.sample" if session.results().any() => trf("Opened sample {name}.", args!(name = p.text("name"))),
        "file.sample" => trf("Opened sample {name}; Run (step 7) solves it.", args!(name = p.text("name"))),
        "file.open" if !p.text("original").is_empty() => trf("Recovered {name}.", args!(name = r.text("name"))),
        "file.open" => match p.text("path") {
            "" => tr("Opened the project.").to_owned(),
            path => trf("Opened {path}.", args!(path = path)),
        },
        "file.new" => tr("New project.").to_owned(),
        "project.analysis" => {
            let kind = emcraft_engine::workflow::Analysis::parse(r.text("kind")).map_or("", crate::lang::shown);
            trf("Analysis: {kind}.", args!(kind = kind))
        }
        "engine.script" => tr("Script done.").to_owned(),
        "file.save" => trf("Saved {n} commands to {path}.", args!(n = r.count("commands"), path = r.text("path"))),
        "solve.frequency" => trf("Solved {n} frequencies, {unknowns} unknowns.", args!(n = r.list("points").len(), unknowns = r.count("unknowns"))),
        "solve.eigenmodes" => {
            let list: Vec<String> = r.list("modes").iter().filter_map(|m| m.get("frequency")?.as_f64()).map(|x| format!("{x:.4}")).collect();
            trf("Found {n} eigenmodes: {list} {unit}.", args!(n = list.len(), list = list.join(", "), unit = unit))
        }
        "solve.dispersion" => {
            let points = r.list("points");
            let bands = points.first().and_then(|p| p.get("frequencies")).and_then(Value::as_array).map_or(0, Vec::len);
            trf("Dispersion along {axis}: {n} phases, {bands} bands each.", args!(axis = r.text("axis"), n = points.len(), bands = bands))
        }
        "solve.radiation" => {
            let power = reply.get("power").or(reply.get("radiated_power")).and_then(Value::as_f64).unwrap_or(f64::NAN);
            trf("Solved the open problem: radiated power {power} W.", args!(power = format!("{power:.4e}")))
        }
        "solve.time_domain" => match r.text("device") {
            "" | "cpu" => trf("Time domain: solved in {n} steps.", args!(n = r.count("steps"))),
            device => trf(
                "Time domain: solved in {n} steps on {device} ({precision} precision).",
                args!(n = r.count("steps"), device = device, precision = tr(r.text("precision"))),
            ),
        },
        "solve.port_modes" => trf("Port modes at {f} {unit}.", args!(f = r.num("frequency"), unit = unit)),
        "solve.device" => trf(
            "The solvers run on {device} ({precision} precision in the time domain; factorization: {factorization}).",
            args!(device = r.text("device"), precision = tr(r.text("precision")), factorization = r.text("factorization")),
        ),
        "results.compare" => {
            trf("Worst |S − S_ref| = {worst} ({reference}).", args!(worst = format!("{:.3e}", r.num("worst")), reference = r.text("reference")))
        }
        "edit.undo" | "edit.redo" => {
            let template = if id == "edit.undo" { "Undone ({undo} to undo, {redo} to redo)." } else { "Redone ({undo} to undo, {redo} to redo)." };
            trf(template, args!(undo = r.count("undo"), redo = r.count("redo")))
        }
        "project.frequency" => {
            let unit = match r.text("unit") {
                "" => unit,
                u => u,
            };
            trf("Band {start} to {stop} {unit}.", args!(start = r.num("start"), stop = r.num("stop"), unit = unit))
        }
        "results.touchstone" => trf("Wrote {path}.", args!(path = r.text("path"))),
        "results.check" => {
            let passive = tr(if r.flag("passive") { "passive" } else { "not passive" });
            let (largest, error) = (format!("{:.6}", r.num("largest_singular_value")), format!("{:.3e}", r.num("reciprocity_error")));
            trf(
                "Largest singular value of S {largest} ({passive}), reciprocity error {error}.",
                args!(largest = largest, passive = passive, error = error),
            )
        }
        "mesh.generate" => {
            let (nodes, tets) = session.model().mesh.as_ref().map_or((0, 0), |m| (m.nodes.len(), m.tets.len()));
            trf("Meshed: {nodes} nodes, {tets} tetrahedra.", args!(nodes = nodes, tets = tets))
        }
        "mesh.adapt" => {
            let passes = r.list("passes");
            // An open problem driven by sources alone converges on its far field.
            let far = r.text("quantity") == "far_field";
            let key = if far { "delta_far_field" } else { "delta_s" };
            let last = passes.last().and_then(|p| p.get(key)?.as_f64()).map_or("—".to_owned(), |d| format!("{d:.2e}"));
            let tets = session.model().mesh.as_ref().map_or(0, |m| m.tets.len());
            let converged = tr(if r.flag("converged") { "converged" } else { "not converged" });
            let what = tr(if far { "far-field change" } else { "ΔS" });
            trf(
                "Adaptive mesh: {n} passes, {tets} tetrahedra, last {what} {last}, {converged}.",
                args!(n = passes.len(), tets = tets, what = what, last = last, converged = converged),
            )
        }
        "geometry.measure" => {
            trf("Distance {distance} {unit}.", args!(distance = crate::tools3d::trimmed(r.num("distance"), 4), unit = r.text("unit")))
        }
        "project.variable" => format!("{} = {}.", p.text("name"), r.num("value")),
        "project.variable.delete" => trf("Deleted variable {name}.", args!(name = p.text("name"))),
        "study.sweep" | "study.optimize" => match session.results().study.as_ref() {
            Some(study) => trf("{summary}.", args!(summary = crate::viewport::study_summary(study))),
            None => trf("{command} done.", args!(command = label(id))),
        },
        other => trf("{command} done.", args!(command = label(other))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_reply_with_missing_fields_still_reads() {
        let s = Session::new();
        let none = json!({});
        assert_eq!(text("results.touchstone", &none, &none, &s), "Wrote .");
        assert_eq!(text("file.open", &none, &none, &s), "Opened the project.");
        assert_eq!(text("mesh.adapt", &none, &none, &s), "Adaptive mesh: 0 passes, 0 tetrahedra, last ΔS —, not converged.");
        assert_eq!(
            text("mesh.adapt", &none, &json!({"passes": [{"delta_s": 0.5}, {"delta_s": 0.01}], "converged": true}), &s),
            "Adaptive mesh: 2 passes, 0 tetrahedra, last ΔS 1.00e-2, converged."
        );
        assert_eq!(
            text("mesh.adapt", &none, &json!({"quantity": "far_field", "passes": [{"delta_far_field": 0.004}]}), &s),
            "Adaptive mesh: 1 passes, 0 tetrahedra, last far-field change 4.00e-3, not converged."
        );
        assert_eq!(text("project.frequency", &none, &json!({"start": 5.0, "stop": 9.0}), &s), "Band 5 to 9 GHz.");
        assert_eq!(text("file.sample", &json!({"name": "highpass"}), &none, &s), "Opened sample highpass; Run (step 7) solves it.");
    }
}
