//! The model tree as the workflow (docs/adr/0009-model-tree-window.md): the browser's sections
//! ([`crate::browser::tree`]) under the steps they belong to, in the workflow's order — 1 Geometry
//! (the bodies, then the materials), 2 Excitation, 3 Setup, 4 Region, 5 Mesh, 6 Run, 7 Results —
//! each step's node marked with its status; the sections of no step (the variables) above them,
//! and a board's and a system's sections as they are. No order is enforced: the marks follow the
//! model.

use emcraft_engine::workflow::{Analysis, Status, Step, StepState};
use emcraft_model::Model;

use crate::browser::Row;

/// What heads a part of the tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Head {
    /// A step of the workflow: a click opens its panel.
    Step(Step),
    /// A section of its own: the variables, a system's or a board's sections.
    Section(&'static str),
}

/// A part of the tree: its head and its groups of rows, each group a browser section (its
/// heading drawn when the part has more than one).
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub head: Head,
    pub groups: Vec<(&'static str, Vec<Row>)>,
}

impl Part {
    /// What folds it: its step's id, or its section's name.
    pub fn key(&self) -> &'static str {
        match self.head {
            Head::Step(s) => s.id(),
            Head::Section(name) => name,
        }
    }

    /// Its rows, group after group.
    pub fn rows(&self) -> impl Iterator<Item = &Row> {
        self.groups.iter().flat_map(|(_, rows)| rows)
    }

    /// How many rows it has.
    pub fn count(&self) -> usize {
        self.groups.iter().map(|(_, rows)| rows.len()).sum()
    }
}

/// The step a browser section belongs to; none for a section of its own.
///
/// ```
/// use emcraft_ui_core::outline::step_of_section;
/// use emcraft_engine::workflow::Step;
/// assert_eq!(step_of_section("Materials"), Some(Step::Geometry));
/// assert_eq!(step_of_section("Domain"), Some(Step::Boundaries));
/// assert_eq!(step_of_section("Variables"), None);
/// ```
pub fn step_of_section(section: &str) -> Option<Step> {
    match section {
        "Bodies" | "Materials" => Some(Step::Geometry),
        "Excitation" => Some(Step::Excitation),
        "Domain" => Some(Step::Boundaries),
        "Mesh" => Some(Step::Mesh),
        "Results" => Some(Step::Results),
        _ => None,
    }
}

/// The tree of a job of kind `kind` from the browser's `sections`: the sections of their own,
/// then a node a step from the geometry on, each holding its sections; a board's and a system's
/// sections each a part of its own (their work is not stepped).
pub fn outline(sections: Vec<(&'static str, Vec<Row>)>, kind: Analysis) -> Vec<Part> {
    let own = |(name, rows): (&'static str, Vec<Row>)| Part { head: Head::Section(name), groups: vec![(name, rows)] };
    if matches!(kind, Analysis::Pcb | Analysis::System) {
        return sections.into_iter().map(own).collect();
    }
    let (stepped, apart): (Vec<_>, Vec<_>) = sections.into_iter().partition(|(name, _)| step_of_section(name).is_some());
    let steps = Step::ALL.into_iter().filter(|s| *s != Step::Intent).map(|step| {
        let groups = stepped.iter().filter(|(name, _)| step_of_section(name) == Some(step)).cloned().collect();
        Part { head: Head::Step(step), groups }
    });
    apart.into_iter().map(own).chain(steps).collect()
}

/// How a step's node looks: the run under way, a step the kind of analysis does without (done:
/// skipped; else with its status), or its status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    Running,
    Skipped,
    Unneeded(Status),
    Plain(Status),
}

impl Mark {
    /// Whether its name is drawn dimmer: a step not needed, or waiting on an earlier one.
    pub fn dim(self) -> bool {
        matches!(self, Mark::Skipped | Mark::Unneeded(_) | Mark::Plain(Status::Waiting))
    }
}

/// The mark of step `state`, a run going on or not, in model `m`.
///
/// ```
/// use emcraft_engine::workflow::{Status, Step, StepState};
/// use emcraft_ui_core::outline::{Mark, mark};
/// let m = emcraft_model::Model::default();
/// let run = StepState { step: Step::Run, status: Status::Todo, messages: Vec::new() };
/// assert_eq!(mark(&run, true, &m), Mark::Running);
/// assert_eq!(mark(&run, false, &m), Mark::Plain(Status::Todo));
/// ```
pub fn mark(state: &StepState, running: bool, m: &Model) -> Mark {
    let unneeded = crate::steps::not_needed(m, state.step).is_some();
    match (running && state.step == Step::Run, unneeded, state.status) {
        (true, _, _) => Mark::Running,
        (false, true, Status::Done) => Mark::Skipped,
        (false, true, status) => Mark::Unneeded(status),
        (false, false, status) => Mark::Plain(status),
    }
}

/// The kind of analysis the tree's head shows: the chosen one, else the one the model infers
/// (`true`: marked automatic), none for an empty model with nothing chosen (it asks for one).
///
/// ```
/// use emcraft_ui_core::outline::shown_kind;
/// assert_eq!(shown_kind(&emcraft_model::Model::default()), None);
/// ```
pub fn shown_kind(m: &Model) -> Option<(Analysis, bool)> {
    let empty = m.bodies().is_empty() && m.ports.is_empty() && m.pcb().is_none() && m.system.stages.is_empty() && m.sources.is_empty();
    match m.job.chosen() {
        Some(k) => Some((k, false)),
        None if empty => None,
        None => Some((m.analysis(), true)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replies::Replies;
    use serde_json::json;

    fn heads(parts: &[Part]) -> Vec<Head> {
        parts.iter().map(|p| p.head).collect()
    }

    /// A filter's tree: a node a step from the geometry on, in order, the bodies and materials
    /// under the geometry, the region's walls under Region, Setup and Run with no rows of their
    /// own; every row of the browser in it once.
    #[test]
    fn a_filters_tree_is_its_workflow() {
        let mut s = emcraft_engine::Session::new();
        s.execute("file.sample", &json!({"name": "highpass"})).unwrap();
        let sections = crate::browser::tree(&s, &Replies::default());
        let all = sections.iter().map(|(_, r)| r.len()).sum::<usize>();
        let parts = outline(sections, s.model().analysis());
        let steps = [Step::Geometry, Step::Excitation, Step::Setup, Step::Boundaries, Step::Mesh, Step::Run, Step::Results];
        assert_eq!(heads(&parts), steps.map(Head::Step));
        let groups: Vec<&str> = parts[0].groups.iter().map(|(n, _)| *n).collect();
        assert_eq!(groups, ["Bodies", "Materials"]);
        assert!(parts[0].count() > 0 && parts[1].count() > 0);
        assert_eq!((parts[2].count(), parts[5].count()), (0, 0));
        assert_eq!(parts.iter().map(Part::count).sum::<usize>(), all);
        assert_eq!(parts[3].key(), "boundaries");
    }

    /// A system's tree is its own sections, none stepped.
    #[test]
    fn a_systems_tree_is_its_sections() {
        let sections = vec![("Source", Vec::new()), ("Stages", Vec::new())];
        let parts = outline(sections, Analysis::System);
        assert_eq!(heads(&parts), [Head::Section("Source"), Head::Section("Stages")]);
    }

    /// A blank project asks for its kind; one chosen shows it; a model that says it shows it as
    /// automatic.
    #[test]
    fn the_kind_shown_is_asked_for_until_there_is_one() {
        let mut s = emcraft_engine::Session::new();
        assert_eq!(shown_kind(s.model()), None);
        s.execute("project.analysis", &json!({"kind": "eigenmodes"})).unwrap();
        assert_eq!(shown_kind(s.model()), Some((Analysis::Eigenmodes, false)));
        s.execute("file.sample", &json!({"name": "highpass"})).unwrap();
        assert!(shown_kind(s.model()).is_some());
    }
}
