//! The status line's text for a command that succeeded, read from its reply.

use emcraft_engine::Session;
use serde_json::Value;

use crate::label;

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
    let unit = session.display.frequency.symbol();
    match id {
        "file.sample" if session.results.any() => format!("Opened sample {}.", p.text("name")),
        "file.sample" => format!("Opened sample {}; Run (step 7) solves it.", p.text("name")),
        "file.open" => match p.text("path") {
            "" => "Opened the project.".to_owned(),
            path => format!("Opened {path}."),
        },
        "file.new" => "New project.".to_owned(),
        "engine.script" => "Script done.".to_owned(),
        "file.save" => format!("Saved {} commands to {}.", r.count("commands"), r.text("path")),
        "solve.frequency" => format!("Solved {} frequencies, {} unknowns.", r.list("points").len(), r.count("unknowns")),
        "solve.eigenmodes" => {
            let list: Vec<String> = r.list("modes").iter().filter_map(|m| m.get("frequency")?.as_f64()).map(|x| format!("{x:.4}")).collect();
            format!("Found {} eigenmodes: {} {unit}.", list.len(), list.join(", "))
        }
        "solve.radiation" => {
            let power = reply.get("power").or(reply.get("radiated_power")).and_then(Value::as_f64).unwrap_or(f64::NAN);
            format!("Solved the open problem: radiated power {power:.4e} W.")
        }
        "solve.time_domain" => match r.text("device") {
            "" | "cpu" => format!("Time-domain solve done in {} steps.", r.count("steps")),
            device => format!("Time-domain solve done in {} steps on {device} ({} precision).", r.count("steps"), r.text("precision")),
        },
        "solve.port_modes" => format!("Port modes at {} {unit}.", r.num("frequency")),
        "solve.device" => format!(
            "The solvers run on {} ({} precision in the time domain; factorization: {}).",
            r.text("device"),
            r.text("precision"),
            r.text("factorization")
        ),
        "results.compare" => format!("Worst |S − S_ref| = {:.3e} ({}).", r.num("worst"), r.text("reference")),
        "edit.undo" | "edit.redo" => {
            format!("{} ({} to undo, {} to redo).", if id == "edit.undo" { "Undone" } else { "Redone" }, r.count("undo"), r.count("redo"))
        }
        "project.frequency" => {
            let unit = match r.text("unit") {
                "" => unit,
                u => u,
            };
            format!("Band {} to {} {unit}.", r.num("start"), r.num("stop"))
        }
        "results.touchstone" => format!("Wrote {}.", r.text("path")),
        "results.check" => format!(
            "Largest singular value of S {:.6} ({}), reciprocity error {:.3e}.",
            r.num("largest_singular_value"),
            if r.flag("passive") { "passive" } else { "not passive" },
            r.num("reciprocity_error")
        ),
        "mesh.generate" => {
            let (nodes, tets) = session.model.mesh.as_ref().map_or((0, 0), |m| (m.nodes.len(), m.tets.len()));
            format!("Meshed: {nodes} nodes, {tets} tetrahedra.")
        }
        "mesh.adapt" => {
            let passes = r.list("passes");
            let last = passes.last().and_then(|p| p.get("delta_s")?.as_f64()).map_or("—".to_owned(), |d| format!("{d:.2e}"));
            let tets = session.model.mesh.as_ref().map_or(0, |m| m.tets.len());
            let converged = if r.flag("converged") { "converged" } else { "not converged" };
            format!("Adaptive mesh: {} passes, {tets} tetrahedra, last ΔS {last}, {converged}.", passes.len())
        }
        other => format!("{} done.", label(other)),
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
        assert_eq!(text("project.frequency", &none, &json!({"start": 5.0, "stop": 9.0}), &s), "Band 5 to 9 GHz.");
        assert_eq!(text("file.sample", &json!({"name": "highpass"}), &none, &s), "Opened sample highpass; Run (step 7) solves it.");
    }
}
