//! The workflow's steps as the window shows them.

/// A port mode picked in the Excitation panel: the Port modes view marks its row and scrolls
/// to it once.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ModeFocus {
    pub port: String,
    pub mode: usize,
    pub scroll: bool,
}

/// Two settings alike, numbers to a part in 10⁹ (a unit's round trip).
///
/// ```
/// use serde_json::json;
/// assert!(emcraft_ui_core::steps::same(&json!({"stop": 1.0}), &json!({"stop": 1.0000000001})));
/// assert!(!emcraft_ui_core::steps::same(&json!({"stop": 1.0}), &json!({"stop": 1.1})));
/// ```
pub fn same(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    use serde_json::Value;
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w))),
        (Value::Number(x), Value::Number(y)) => match (x.as_f64(), y.as_f64()) {
            (Some(x), Some(y)) => (x - y).abs() <= 1e-9 * x.abs().max(y.abs()),
            _ => x == y,
        },
        _ => a == b,
    }
}

pub mod band;
pub mod devices;
pub mod form;
pub mod kinds;
pub mod run;

pub use emcraft_engine::workflow::{Analysis as Solver, Check, Status, Step, StepState};

use crate::scene3d::Overlay;

/// A step's check as the window shows it: its template translated, its values filled in (the
/// steps it names by their shown names); the English text when it has no template.
pub fn check_text(c: &Check) -> String {
    if c.template.is_empty() {
        return crate::lang::lookup(&c.text).to_owned();
    }
    let steps = |ids: &str| {
        ids.split(", ").map(|id| Step::ALL.iter().find(|s| s.id() == id).map_or(id, |s| crate::lang::shown(*s))).collect::<Vec<_>>().join("、")
    };
    let values: Vec<String> = c.args.iter().map(|(k, v)| if k == "steps" { steps(v) } else { v.clone() }).collect();
    let args: Vec<(&str, &dyn std::fmt::Display)> = c.args.iter().zip(&values).map(|((k, _), v)| (k.as_str(), v as &dyn std::fmt::Display)).collect();
    crate::lang::fill(crate::lang::lookup(&c.template), &args)
}

/// What the 3D view puts forward in a step.
///
/// ```
/// use emcraft_ui_core::steps::{Step, overlay};
/// use emcraft_ui_core::scene3d::Overlay;
/// assert_eq!((overlay(Step::Mesh), overlay(Step::Excitation), overlay(Step::Run)), (Overlay::Mesh, Overlay::Ports, Overlay::Plain));
/// ```
pub fn overlay(step: Step) -> Overlay {
    match step {
        // The bodies drawn in their materials' colours, the step listing the materials.
        Step::Geometry => Overlay::Materials,
        Step::Excitation => Overlay::Ports,
        Step::Boundaries => Overlay::Boundaries,
        Step::Mesh => Overlay::Mesh,
        _ => Overlay::Plain,
    }
}

/// Whether command `id` is a run (a solve or a study, as step 6 starts one): what a failure
/// marks on Run and what Run shows running.
pub fn is_run(id: &str) -> bool {
    crate::job::is_solve(id) || id.starts_with("study.")
}

/// The commands whose dialogs show how long their run will take (`project.estimate`).
pub const ESTIMATED: [&str; 9] = [
    "solve.frequency",
    "solve.refine",
    "solve.eigenmodes",
    "solve.radiation",
    "solve.dispersion",
    "solve.time_domain",
    "mesh.adapt",
    "study.sweep",
    "study.optimize",
];

/// Why step `step` does nothing for the job's kind of analysis, when it does nothing for it.
pub fn not_needed(m: &emcraft_model::Model, step: Step) -> Option<String> {
    let chosen = m.job.chosen()?;
    emcraft_engine::workflow::not_needed(Some(chosen))
        .contains(&step)
        .then(|| crate::lang::trf(t!("Not needed for {kind}"), crate::lang::args!(kind = crate::lang::tr(kinds::card(chosen).name))))
}

/// A check's Fix button: to its step for a solve, to the analysis for `project.analysis`, else
/// the command it fixes with; none when it only leads back to `here` (its solve, Setup's own
/// confirmation).
pub fn fix_label(command: &str, here: Step) -> Option<String> {
    use crate::lang::{args, shown, tr, trf};
    let step = Step::of_command(command);
    if (crate::job::is_solve(command) || command == "project.setup") && step == Some(here) {
        return None;
    }
    Some(match step {
        Some(step) if crate::job::is_solve(command) => trf(t!("Go to {step}"), args!(step = shown(step))),
        Some(Step::Intent) if command == "project.analysis" => tr(t!("Choose the analysis")).to_owned(),
        _ => trf(t!("Fix: {command}"), args!(command = crate::commands::button_label(command))),
    })
}

/// What a check's Fix does: the step that owns its command, and that command's dialog unless it
/// is the analysis, the setup's confirmation or a run.
pub fn fix(command: &str) -> (Option<Step>, bool) {
    (Step::of_command(command), !matches!(command, "project.setup" | "project.analysis") && !is_run(command))
}
