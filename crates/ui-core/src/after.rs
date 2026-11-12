//! What follows a command's success, whatever window shows it: the command a reply stands for,
//! the view it calls for, and whether it leaves the start page. Each is a function of the
//! command, its reply and the session; the window shows what they say.

use emcraft_engine::Session;
use emcraft_engine::workflow::{Analysis as Solver, Step};
use serde_json::Value;

use crate::replies::{ANALYSES, Replies};
use crate::view::View;

/// The command a reply stands for, with its parameters: a run again (`solve.rerun`) or the
/// confirmed settings' run (`solve.run`) is the solve it ran.
///
/// ```
/// use serde_json::json;
/// let reply = json!({"rerun": {"command": "solve.frequency", "params": {"start": 6}}});
/// assert_eq!(emcraft_ui_core::after::ran("solve.rerun", &json!({}), &reply), ("solve.frequency", &json!({"start": 6})));
/// assert_eq!(emcraft_ui_core::after::ran("mesh.generate", &json!({}), &json!({})).0, "mesh.generate");
/// ```
pub fn ran<'a>(id: &'a str, params: &'a Value, reply: &'a Value) -> (&'a str, &'a Value) {
    let rerun = match id {
        "solve.rerun" => reply.get("rerun"),
        "solve.run" => reply.get("run"),
        _ => None,
    };
    rerun.and_then(|r| Some((r.get("command")?.as_str()?, r.get("params")?))).unwrap_or((id, params))
}

/// The view of a session's results: the chosen kind of analysis' own first (a periodic cell's
/// dispersion for eigenmodes), else the first result of the model it holds; the model when
/// there is none.
pub fn results_view(s: &Session) -> View {
    let results = s.results();
    // A periodic cell's project with a pulse of its own (the pulse compressor): its pulse.
    if s.views().pulse.is_some() && results.bloch.is_some() {
        return View::Pulse;
    }
    // A system's own view holds its chain as well as its budget.
    if s.model().analysis() == Solver::System {
        return View::System;
    }
    let chosen = emcraft_engine::workflow::spec(s.model().analysis()).results.iter().copied().find(|k| results.has(*k));
    chosen.or_else(|| results.kinds().find(|k| k.of_the_model())).map_or(View::Model, View::of)
}

/// Whether the session is a board diagnosed by its PCB stages.
fn diagnosing(s: &Session) -> bool {
    s.model().pcb().is_some() && s.model().analysis() == Solver::Pcb
}

/// The view a solve calls for.
fn solved(id: &str, s: &Session, step: Option<Step>) -> Option<View> {
    match id {
        "solve.frequency" => Some(View::SParameters),
        "solve.dispersion" => Some(View::Dispersion),
        "solve.eigenmodes" => Some(View::Eigenmodes),
        "solve.radiation" => Some(View::FarField),
        "solve.time_domain" if s.results().td_radiation.is_some() => Some(View::FarField),
        "solve.time_domain" => Some(View::SParameters),
        // The Excitation step tabulates them in its panel, beside the 3D view.
        "solve.port_modes" if step == Some(Step::Excitation) => None,
        "solve.port_modes" => Some(View::PortModes),
        _ => None,
    }
}

/// The view to show after command `id` (as [`ran`] names it) on `s`, the
/// window showing `now` with step `step` open and keeping `replies`: `None` keeps it.
///
/// ```
/// use emcraft_ui_core::{after::next_view, replies::Replies, view::View};
/// use serde_json::json;
/// let s = emcraft_engine::Session::new();
/// let next = |id: &str, now: View| next_view(id, &s, &Replies::default(), now, None);
/// assert_eq!(next("results.tdr", View::Model), Some(View::Tdr));
/// assert_eq!(next("geometry.box", View::Field), Some(View::Model));
/// assert_eq!(next("view.chart.style", View::Field), None);
/// ```
pub fn next_view(id: &str, s: &Session, replies: &Replies, now: View, step: Option<Step>) -> Option<View> {
    let system = s.model().analysis() == Solver::System;
    match id {
        "geometry.measure" => None,
        "view.pcb" | "pcb.inspect" | "pcb.load" | "pcb.import" | "pcb.sample" => Some(View::Pcb),
        "project.variable" if diagnosing(s) => Some(View::Pcb),
        "file.sample" | "engine.script" | "file.open" | "file.new" => Some(if diagnosing(s) { View::Pcb } else { results_view(s) }),
        _ if id.starts_with("solve.") => solved(id, s, step),
        "results.pulse" => Some(View::Pulse),
        id if ANALYSES.contains(&id) => Some(View::Analyses),
        "results.tdr" => Some(View::Tdr),
        // A measurement drawn over the S-parameter chart (a system's: its S-parameter stages).
        "results.import" | "results.import.delete" if system => Some(View::System),
        "results.import" | "results.import.delete" | "results.equivalent" | "circuit.cascade" => Some(View::SParameters),
        id if id.starts_with("system.") => Some(View::System),
        "filter.synthesize" | "results.coupling" => Some(View::SParameters),
        "results.compare" => Some(View::Compare),
        "study.sweep" | "study.optimize" => Some(View::Study),
        "edit.undo" | "edit.redo" if s.model().pcb().is_some() => Some(View::Pcb),
        "edit.undo" | "edit.redo" => (now == View::Pcb).then_some(View::Model),
        // A system's chain is its own view's; away from the PCB diagnosis or a system, the model's.
        "project.analysis" if system => Some(View::System),
        "project.analysis" => (matches!(now, View::Pcb | View::System) && !now.available(s, replies)).then_some(View::Model),
        _ if id.starts_with("geometry.") => Some(View::Model),
        _ => None,
    }
}

/// Whether command `id` leaves the start page for the project.
pub fn leaves_start(id: &str) -> bool {
    matches!(id, "view.pcb" | "pcb.load" | "pcb.import" | "pcb.sample" | "file.new" | "file.sample")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The view each kind of command calls for: a solve its
    /// results' (port modes left to the Excitation step's table there), an import over the
    /// S-parameters (a system's on its own view), a PCB's command its window, undo away from a
    /// window the project no longer has.
    #[rstest::rstest]
    #[case("solve.frequency", View::Model, None, Some(View::SParameters))]
    #[case("solve.eigenmodes", View::Model, None, Some(View::Eigenmodes))]
    #[case("solve.port_modes", View::Model, None, Some(View::PortModes))]
    #[case("solve.port_modes", View::Model, Some(Step::Excitation), None)]
    #[case("solve.device", View::Field, None, None)]
    #[case("results.import", View::Model, None, Some(View::SParameters))]
    #[case("results.material", View::Model, None, Some(View::Analyses))]
    #[case("system.stage.add", View::Model, None, Some(View::System))]
    #[case("pcb.load", View::Model, None, Some(View::Pcb))]
    #[case("edit.undo", View::Pcb, None, Some(View::Model))]
    #[case("edit.undo", View::Field, None, None)]
    #[case("geometry.measure", View::Field, None, None)]
    #[case("file.save", View::Field, None, None)]
    fn each_command_calls_for_its_view(#[case] id: &str, #[case] now: View, #[case] step: Option<Step>, #[case] want: Option<View>) {
        assert_eq!(next_view(id, &Session::new(), &Replies::default(), now, step), want);
    }

    /// A project opened shows its results' view: a sample solved on open its sweep's, a system
    /// its own.
    #[test]
    fn a_project_opened_shows_its_results() {
        let mut s = Session::new();
        let open = |s: &mut Session, name: &str| {
            s.execute("file.sample", &json!({"name": name})).map(|_| next_view("file.sample", s, &Replies::default(), View::Model, None))
        };
        assert_eq!(open(&mut s, "adsb-receiver").ok().flatten(), Some(View::System));
        assert!(leaves_start("file.sample") && !leaves_start("file.save"));
    }
}
