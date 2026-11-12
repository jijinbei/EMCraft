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

/// What each step's ＋ adds (docs/adr/0009-model-tree-window.md: every command the ribbon had,
/// each in one place), in the workflow's order.
const STEP_ADDS: &[(Step, &[&str])] = &[
    (Step::Geometry, &["geometry.import", "geometry.waveguide", "geometry.body", "model.background", "model.material.create"]),
    (Step::Excitation, &["model.port.create", "model.port.lumped", "model.element.create", "source.dipole", "source.plane_wave", "source.clear"]),
    (Step::Setup, &["units.set"]),
    (Step::Boundaries, &["model.region", "model.walls", "model.open", "model.symmetry.auto", "model.symmetry", "model.rotation", "model.periodic"]),
    (Step::Mesh, &["mesh.adapt"]),
    (Step::Run, &["solve.dispersion", "project.variable", "study.sweep", "study.optimize", "study.tolerance"]),
    (
        Step::Results,
        &[
            "view.sparameters",
            "view.field",
            "view.animation",
            "view.pattern",
            "view.dispersion",
            "results.import",
            "results.measured",
            "results.network",
            "results.mixed_mode",
            "results.equivalent",
            "circuit.cascade",
            "filter.synthesize",
            "results.coupling",
        ],
    ),
];

/// What the ＋ of a section of its own adds: the variables', a board's and a system chain's.
const SECTION_ADDS: &[(&str, &[&str])] = &[
    ("Variables", &["project.variable"]),
    ("PCB", &["pcb.import", "pcb.sample", "pcb.segment", "pcb.via", "pcb.pour", "pcb.element", "units.set"]),
    ("Source", &["system.source", "units.set"]),
    ("Receiver", &["system.receiver"]),
    ("Measurements", &["results.import"]),
];

/// What a part's ＋ offers: its commands that belong to a job of kind `kind`, then the 3D view's
/// tools that add to it (the ports made by pointing at a face or a gap).
///
/// ```
/// use emcraft_engine::workflow::{Analysis, Step};
/// use emcraft_ui_core::outline::{Head, adds};
/// use emcraft_ui_core::tools::Tool;
/// let (commands, tools) = adds(Head::Step(Step::Excitation), Analysis::Frequency);
/// assert!(commands.contains(&"model.port.create") && !commands.contains(&"source.dipole"));
/// assert_eq!(tools, [Tool::Port, Tool::Lumped]);
/// // Nothing drives a resonator.
/// assert!(adds(Head::Step(Step::Excitation), Analysis::Eigenmodes).1.is_empty());
/// ```
pub fn adds(head: Head, kind: Analysis) -> (Vec<&'static str>, Vec<crate::tools::Tool>) {
    let listed: &[&str] = match head {
        Head::Step(step) => STEP_ADDS.iter().find(|(s, _)| *s == step).map_or(&[], |(_, ids)| ids),
        Head::Section(name) => SECTION_ADDS.iter().find(|(n, _)| *n == name).map_or(&[], |(_, ids)| ids),
    };
    let commands = listed.iter().copied().filter(|id| fits(kind, id)).collect();
    let tools = match head {
        Head::Step(Step::Excitation) if fits(kind, "model.port.create") => vec![crate::tools::Tool::Port, crate::tools::Tool::Lumped],
        _ => Vec::new(),
    };
    (commands, tools)
}

/// The command a step's ▶ runs: the mesh made, the confirmed run.
///
/// ```
/// use emcraft_engine::workflow::Step;
/// assert_eq!(emcraft_ui_core::outline::play(Step::Mesh), Some("mesh.generate"));
/// assert_eq!(emcraft_ui_core::outline::play(Step::Geometry), None);
/// ```
pub fn play(step: Step) -> Option<&'static str> {
    match step {
        Step::Mesh => Some("mesh.generate"),
        Step::Run => Some("solve.run"),
        _ => None,
    }
}

/// Whether a command belongs to a job of this kind of analysis: those of the other kinds are left
/// out of the ＋ lists once a kind is chosen (the search still finds them).
pub fn fits(kind: Analysis, id: &str) -> bool {
    use Analysis::{Eigenmodes, Frequency, Pcb, Radiation, System, TimeDomain};
    if kind == Pcb {
        return id.starts_with("pcb.") || id.starts_with("file.") || matches!(id, "project.variable" | "project.analysis" | "units.set");
    }
    if kind == System {
        return id.starts_with("system.")
            || id.starts_with("file.")
            || matches!(id, "solve.system" | "results.import" | "project.variable" | "project.analysis" | "units.set");
    }
    if id.starts_with("pcb.") || id.starts_with("system.") || id == "solve.system" {
        return false;
    }
    match id {
        "solve.frequency" => kind == Frequency,
        "solve.eigenmodes" | "solve.dispersion" | "view.dispersion" => kind == Eigenmodes,
        "solve.radiation" => kind == Radiation,
        "solve.time_domain" => kind == TimeDomain,
        // Open problems: the layer, sources and the far field.
        "model.open" | "view.pattern" | "source.dipole" | "source.plane_wave" | "source.clear" => matches!(kind, Radiation | TimeDomain),
        // S-parameters between ports.
        "results.touchstone" | "view.sparameters" | "results.tdr" | "results.compare" | "results.check" => matches!(kind, Frequency | TimeDomain),
        // Nothing drives a resonator.
        "model.port.create" | "model.port.lumped" | "solve.port_modes" | "model.element.create" => kind != Eigenmodes,
        _ => true,
    }
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

    /// Every command a ＋ offers is a command with a glyph and a label, each in one place; every
    /// kind of analysis has something to add to its geometry or its board or chain.
    #[test]
    fn every_command_added_is_one_with_a_glyph_in_one_place() {
        let all: Vec<&str> =
            STEP_ADDS.iter().flat_map(|(_, ids)| ids.iter()).chain(SECTION_ADDS.iter().flat_map(|(_, ids)| ids.iter())).copied().collect();
        for id in &all {
            let c = crate::commands::find(id).unwrap_or_else(|| panic!("{id}: no command"));
            assert!(c.glyph.is_some() && !crate::commands::button_label(id).is_empty(), "{id}");
        }
        let stepped: Vec<&str> = STEP_ADDS.iter().flat_map(|(_, ids)| ids.iter()).copied().collect();
        let once = stepped.iter().all(|id| stepped.iter().filter(|x| *x == id).count() == 1);
        assert!(once, "a command on two steps' ＋");
        for kind in Analysis::ALL {
            let heads = [Head::Step(Step::Geometry), Head::Section("PCB"), Head::Section("Source")];
            assert!(heads.iter().any(|h| !adds(*h, kind).0.is_empty()), "{kind:?}");
        }
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
