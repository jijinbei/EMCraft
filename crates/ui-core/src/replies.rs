//! The replies of the commands whose results the views show as they came — a board's
//! inspection, the port modes, the comparison with theory, a pulse, a coupling matrix, an
//! analysis read from the sweep, what an import's file holds — and which command keeps or lets
//! go of each, as one table ([`Replies::after`]).

use emcraft_engine::Session;
use emcraft_engine::workflow::Analysis as Solver;
use serde_json::Value;

/// The analyses read from a sweep that the Analyses view shows, in its buttons' order.
pub const ANALYSES: [&str; 5] = ["results.material", "results.noise", "results.active", "results.breakdown", "results.thermal"];

/// An analysis' reply as the Analyses view keeps it.
#[derive(Clone, Debug, PartialEq)]
pub struct Analysis {
    pub command: String,
    pub reply: Value,
    pub run: u64,
}

/// `geometry.import.inspect`'s reply for the file (and STL unit) `key` names, or why not.
#[derive(Clone, Debug, PartialEq)]
pub struct ImportProbe {
    pub key: String,
    pub reply: Result<emcraft_engine::ImportInspection, String>,
}

/// The replies the views show.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Replies {
    pub pcb: Option<emcraft_engine::pcb::Inspection>,
    pub port_modes: Option<emcraft_engine::PortModes>,
    pub compare: Option<emcraft_engine::Comparison>,
    /// What the import dialog's file holds (`geometry.import.inspect`; the dialog keeps it).
    pub import: Option<ImportProbe>,
    /// The last `results.pulse`'s reply.
    pub pulse: Option<Value>,
    /// The last coupling matrix (`filter.synthesize`, `results.coupling`).
    pub coupling: Option<Value>,
    /// The last analysis read from the sweep ([`ANALYSES`]): its command, reply and run count.
    pub analysis: Option<Analysis>,
}

/// Whether the session is a board diagnosed by its PCB stages.
fn diagnosing(s: &Session) -> bool {
    s.model().pcb().is_some() && s.model().analysis() == Solver::Pcb
}

/// A reply read as what it holds (`None`: not that).
fn parsed<T: serde::de::DeserializeOwned>(reply: &Value) -> Option<T> {
    serde_json::from_value(reply.clone()).ok()
}

impl Replies {
    /// The replies once command `id` (with `params`) answered `reply` on `s`: the one it keeps,
    /// those it makes stale let go, the rest as they were.
    ///
    /// ```
    /// use emcraft_ui_core::replies::Replies;
    /// use serde_json::json;
    /// let s = emcraft_engine::Session::new();
    /// let kept = Replies::default().after("results.pulse", &json!({}), &json!({"peak": 1}), &s);
    /// assert_eq!(kept.pulse, Some(json!({"peak": 1})));
    /// // Another project lets it go; a save keeps it.
    /// assert_eq!(kept.clone().after("file.save", &json!({}), &json!({}), &s).pulse, kept.pulse);
    /// assert_eq!(kept.after("file.new", &json!({}), &json!({}), &s).pulse, None);
    /// ```
    pub fn after(self, id: &str, params: &Value, reply: &Value, s: &Session) -> Replies {
        match id {
            "pcb.inspect" => Replies { pcb: parsed(reply), ..self },
            "units.set" | "pcb.load" | "pcb.import" | "pcb.sample" => Replies { pcb: None, ..self },
            "view.pcb" if params.get("net").is_some() || params.get("victim").is_some() => Replies { pcb: None, ..self },
            "project.variable" if diagnosing(s) => Replies { pcb: None, ..self },
            "file.sample" | "engine.script" | "file.open" | "file.new" => {
                Replies { pcb: None, port_modes: None, compare: None, pulse: None, coupling: None, ..self }
            }
            "solve.port_modes" => Replies { port_modes: parsed(reply), ..self },
            "results.pulse" => Replies { pulse: Some(reply.clone()), ..self },
            "filter.synthesize" | "results.coupling" => Replies { coupling: Some(reply.clone()), ..self },
            "results.compare" => Replies { compare: parsed(reply), ..self },
            id if ANALYSES.contains(&id) => {
                let run = self.analysis.as_ref().map_or(0, |a| a.run + 1);
                Replies { analysis: Some(Analysis { command: id.to_owned(), reply: reply.clone(), run }), ..self }
            }
            "edit.undo" | "edit.redo" => Replies { port_modes: None, pcb: None, ..self },
            id if id.starts_with("geometry.") && id != "geometry.measure" => Replies { port_modes: None, compare: None, ..self },
            _ => self,
        }
    }
}
