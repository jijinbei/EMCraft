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
    let length_unit = session.display().length.symbol();
    match id {
        "file.sample" if session.results().any() || session.model().pcb().is_some() => trf(t!("Opened sample {name}."), args!(name = p.text("name"))),
        "file.sample" => trf(t!("Opened sample {name}; Run (step 6) solves it."), args!(name = p.text("name"))),
        "file.open" if !p.text("original").is_empty() => trf(t!("Recovered {name}."), args!(name = r.text("name"))),
        "file.open" => opened(p, reply),
        "file.new" => tr(t!("New project.")).to_owned(),
        "model.symmetry.auto" => symmetry_auto(r),
        "project.analysis" => {
            let kind = emcraft_engine::workflow::Analysis::parse(r.text("kind")).map_or("", crate::lang::shown);
            trf(t!("Analysis: {kind}."), args!(kind = kind))
        }
        "engine.script" => tr(t!("Script done.")).to_owned(),
        "view.driven" => trf(t!("Port {name} is the input."), args!(name = r.text("driven"))),
        "file.save" => trf(t!("Saved {n} commands to {path}."), args!(n = r.count("commands"), path = r.text("path"))),
        "solve.frequency" => {
            trf(t!("Solved {n} frequencies, {unknowns} unknowns."), args!(n = r.list("points").len(), unknowns = r.count("unknowns")))
        }
        "solve.eigenmodes" => eigenmodes(r, unit),
        "solve.dispersion" => dispersion(r),
        "solve.radiation" => radiation(reply, length_unit),
        "solve.time_domain" => match (emcraft_engine::Device::parse(r.text("device")), r.text("device")) {
            (None | Some(emcraft_engine::Device::Cpu), _) => trf(t!("Time domain: solved in {n} steps."), args!(n = r.count("steps"))),
            (Some(_), device) => trf(
                t!("Time domain: solved in {n} steps on {device} ({precision} precision)."),
                args!(n = r.count("steps"), device = device, precision = crate::lang::lookup(r.text("precision"))),
            ),
        },
        "solve.port_modes" => trf(t!("Port modes at {f} {unit}."), args!(f = r.num("frequency"), unit = unit)),
        "solve.device" => trf(
            t!("The solvers run on {device} ({precision} precision in the time domain; factorization: {factorization})."),
            args!(device = r.text("device"), precision = crate::lang::lookup(r.text("precision")), factorization = r.text("factorization")),
        ),
        "results.compare" => {
            trf(t!("Worst |S − S_ref| = {worst} ({reference})."), args!(worst = format!("{:.3e}", r.num("worst")), reference = r.text("reference")))
        }
        "edit.undo" | "edit.redo" => {
            let template =
                if id == "edit.undo" { t!("Undone ({undo} to undo, {redo} to redo).") } else { t!("Redone ({undo} to undo, {redo} to redo).") };
            trf(template, args!(undo = r.count("undo"), redo = r.count("redo")))
        }
        "project.setup" => tr(t!("Settings confirmed.")).to_owned(),
        "results.touchstone" => trf(t!("Wrote {path}."), args!(path = r.text("path"))),
        "results.import" => trf(
            t!("Read {name}: {ports} ports, {n} frequencies."),
            args!(name = r.text("name"), ports = r.count("ports"), n = r.count("frequencies")),
        ),
        "filter.synthesize" | "results.coupling" => {
            let fit = r.0.get("rms_s21").and_then(serde_json::Value::as_f64).map(|v| format!(" (RMS |ΔS21| {v:.3})")).unwrap_or_default();
            trf(t!("Coupling matrix of {n} resonators{fit}."), args!(n = r.count("order"), fit = fit))
        }
        "results.equivalent" => trf(t!("Equivalent circuit kept as {name}."), args!(name = r.text("kept"))),
        "circuit.cascade" => trf(t!("Cascade of {n} blocks kept as {name}."), args!(n = r.count("blocks"), name = r.text("name"))),
        "results.measured" => measured(r),
        "results.check" => check(r),
        "mesh.generate" => {
            let (nodes, tets) = session.model().mesh.as_ref().map_or((0, 0), |m| (m.nodes.len(), m.tets.len()));
            trf(t!("Meshed: {nodes} nodes, {tets} tetrahedra."), args!(nodes = nodes, tets = tets))
        }
        "mesh.adapt" => adapted(r, session),
        "geometry.measure" => {
            trf(t!("Distance {distance} {unit}."), args!(distance = crate::tools3d::trimmed(r.num("distance"), 4), unit = r.text("unit")))
        }
        "project.variable" => format!("{} = {}.", p.text("name"), r.num("value")),
        "project.variable.delete" => trf(t!("Deleted variable {name}."), args!(name = p.text("name"))),
        "study.sweep" | "study.optimize" => match session.results().study.as_ref() {
            Some(study) => trf(t!("{summary}."), args!(summary = crate::viewport::study_summary(study))),
            None => trf(t!("{command} done."), args!(command = label(id))),
        },
        other => trf(t!("{command} done."), args!(command = label(other))),
    }
}

/// file.open: the file opened, and the results it held that no longer fit (solved again on
/// demand).
fn opened(p: Reply<'_>, reply: &Value) -> String {
    let opened = match p.text("path") {
        "" => tr(t!("Opened the project.")).to_owned(),
        path => trf(t!("Opened {path}."), args!(path = path)),
    };
    // Results the file held that no longer fit: solved again on demand.
    let left: Vec<String> = reply["left_out"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|e| format!("{}: {}", e["part"].as_str().unwrap_or("?"), e["why"].as_str().unwrap_or("?")))
        .collect();
    match left.is_empty() {
        true => opened,
        false => format!("{opened} {}", trf(t!("Not read back (solve again): {what}"), args!(what = left.join("; ")))),
    }
}

/// model.symmetry.auto: what was set, and what was found but left out (each said once, by
/// kind).
fn symmetry_auto(r: Reply<'_>) -> String {
    let planes: Vec<&str> = r.list("planes").iter().filter_map(|q| q.get("axis").and_then(Value::as_str)).collect();
    let left: Vec<String> =
        r.list("skipped").iter().filter_map(|q| q.get("symmetry").and_then(Value::as_str)).map(crate::chart_words::translate).collect();
    let share = format!("1/{}", (1.0 / r.num("fraction")).round());
    let set = if planes.is_empty() {
        tr(t!("No symmetry set: the whole model is solved.")).to_owned()
    } else if r.flag("every_wall") {
        trf(
            t!(
                "Symmetry planes across {axes}: an eigenmode search solves each combination of their walls on {share} of the model and merges the modes."
            ),
            args!(axes = planes.join(", "), share = share),
        )
    } else {
        trf(
            t!("Symmetry planes across {axes}: {share} of the model is solved."),
            args!(axes = planes.join(", "), share = format!("1/{}", (1.0 / r.num("fraction")).round())),
        )
    };
    if left.is_empty() { set } else { format!("{set} {}", trf(t!("Found but not set: {list}."), args!(list = left.join(", ")))) }
}

/// solve.eigenmodes: the modes' frequencies.
fn eigenmodes(r: Reply<'_>, unit: &str) -> String {
    let list: Vec<String> = r.list("modes").iter().filter_map(|m| m.get("frequency")?.as_f64()).map(|x| format!("{x:.4}")).collect();
    trf(t!("Found {n} eigenmodes: {list} {unit}."), args!(n = list.len(), list = list.join(", "), unit = unit))
}

/// solve.dispersion: the phases and bands.
fn dispersion(r: Reply<'_>) -> String {
    let points = r.list("points");
    let bands = points.first().and_then(|p| p.get("frequencies")).and_then(Value::as_array).map_or(0, Vec::len);
    trf(t!("Dispersion along {axis}: {n} phases, {bands} bands each."), args!(axis = r.text("axis"), n = points.len(), bands = bands))
}

/// solve.radiation: a plane wave's problem is scattering (its cross-section), a source's
/// radiation (its power).
fn radiation(reply: &Value, length_unit: &str) -> String {
    // A plane wave's problem is scattering: its cross-section, not a radiated power.
    let sigma = reply.get("scattering_cross_section").and_then(Value::as_f64);
    let power = reply.get("power").or_else(|| reply.get("radiated_power")).and_then(Value::as_f64);
    match (sigma, power) {
        (Some(sigma), _) => trf(
            t!("Solved the scattering problem: scattering cross-section {sigma} {unit}²."),
            args!(sigma = format!("{sigma:.4e}"), unit = length_unit),
        ),
        (None, Some(power)) => trf(t!("Solved the open problem: radiated power {power} W."), args!(power = format!("{power:.4e}"))),
        (None, None) => tr(t!("Solved the open problem.")).to_owned(),
    }
}

/// results.measured: the worst difference from the measurement and its entry.
fn measured(r: Reply<'_>) -> String {
    let worst =
        r.0.pointer("/compared/worst/max_difference").and_then(serde_json::Value::as_f64).map_or_else(|| "—".to_owned(), |v| format!("{v:.3e}"));
    let entry = r.0.pointer("/compared/worst/entry").and_then(serde_json::Value::as_str).unwrap_or("—").to_owned();
    trf(t!("Worst |S − S_measured| = {worst} ({entry})."), args!(worst = worst, entry = entry))
}

/// results.check: passivity and reciprocity.
fn check(r: Reply<'_>) -> String {
    let passive = tr(if r.flag("passive") { t!("passive") } else { t!("not passive") });
    let (largest, error) = (format!("{:.6}", r.num("largest_singular_value")), format!("{:.3e}", r.num("reciprocity_error")));
    trf(
        t!("Largest singular value of S {largest} ({passive}), reciprocity error {error}."),
        args!(largest = largest, passive = passive, error = error),
    )
}

/// mesh.adapt: the passes, the mesh and the last change. An open problem driven by sources
/// alone converges on its far field.
fn adapted(r: Reply<'_>, session: &Session) -> String {
    let passes = r.list("passes");
    // An open problem driven by sources alone converges on its far field.
    let far = r.text("quantity") == "far_field";
    let key = if far { "delta_far_field" } else { "delta_s" };
    let last = passes.last().and_then(|p| p.get(key)?.as_f64()).map_or_else(|| "—".to_owned(), |d| format!("{d:.2e}"));
    let tets = session.model().mesh.as_ref().map_or(0, |m| m.tets.len());
    let converged = tr(if r.flag("converged") { t!("converged") } else { t!("not converged") });
    let what = tr(if far { t!("far-field change") } else { t!("ΔS") });
    trf(
        t!("Adaptive mesh: {n} passes, {tets} tetrahedra, last {what} {last}, {converged}."),
        args!(n = passes.len(), tets = tets, what = what, last = last, converged = converged),
    )
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
        assert_eq!(text("project.setup", &none, &json!({"start": 5.0, "stop": 9.0}), &s), "Settings confirmed.");
        assert_eq!(text("file.sample", &json!({"name": "highpass"}), &none, &s), "Opened sample highpass; Run (step 6) solves it.");
        // An open problem's: the power radiated, a plane wave's scattering cross-section (it
        // radiates nothing of its own: no NaN), or neither.
        assert_eq!(text("solve.radiation", &none, &json!({"radiated_power": 0.00245}), &s), "Solved the open problem: radiated power 2.4500e-3 W.");
        assert_eq!(
            text("solve.radiation", &none, &json!({"scattering_cross_section": 12.5}), &s),
            "Solved the scattering problem: scattering cross-section 1.2500e1 mm²."
        );
        assert_eq!(text("solve.radiation", &none, &none, &s), "Solved the open problem.");
    }
}
