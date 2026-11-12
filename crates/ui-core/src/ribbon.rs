//! The ribbon's tabs and their panels of commands (as data: each window lays them out its own
//! way), and which commands a kind of analysis shows.

use crate::tools::Tool;
use emcraft_engine::workflow::Analysis;

emcraft_macros::named_enum! {
    /// A ribbon tab: its id is its title (the control channel's `tab:<id>` handles, `ui.set
    /// {"tab": id}`).
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum RibbonTab {
        #[default]
        Model = "MODEL",
        Materials = "MATERIALS",
        PortsSources = "PORTS & SOURCES",
        Boundaries = "BOUNDARIES",
        Mesh = "MESH",
        Simulate = "SIMULATE",
        Results = "RESULTS",
        /// A system's chain: its stages, source and receiver.
        System = "SYSTEM",
    }
}
emcraft_macros::named_serde!(RibbonTab);

/// The ribbon: tabs of panels of commands, each command in one place (views are switched
/// from the view tabs over the viewport, files from the File menu and the start page).
pub const TABS: &[(RibbonTab, &[(&str, &[&str])])] = &[
    (
        RibbonTab::Model,
        &[
            (t!("GEOMETRY").en(), &["geometry.import", "geometry.waveguide", "geometry.body", "model.region", "model.background"]),
            (t!("SETTINGS").en(), &["units.set"]),
            (t!("PCB").en(), &["pcb.import", "pcb.sample", "pcb.segment", "pcb.via", "pcb.pour", "pcb.element"]),
        ],
    ),
    (RibbonTab::Materials, &[(t!("MATERIALS").en(), &["model.material.create"])]),
    (
        RibbonTab::PortsSources,
        &[
            (t!("PORTS").en(), &["model.port.create", "model.port.lumped"]),
            (t!("ELEMENTS").en(), &["model.element.create"]),
            (t!("SOURCES").en(), &["source.dipole", "source.plane_wave", "source.clear"]),
        ],
    ),
    // The tabs follow the step bar: the boundaries are step 4, after the ports (step 3).
    (
        RibbonTab::Boundaries,
        &[(
            "BOUNDARIES",
            &["model.region", "model.walls", "model.open", "model.symmetry.auto", "model.symmetry", "model.rotation", "model.periodic"],
        )],
    ),
    (RibbonTab::Mesh, &[(t!("MESH").en(), &["mesh.generate", "mesh.adapt"])]),
    (
        RibbonTab::Simulate,
        &[
            // The solve itself is Run's (step 6), its device Setup's (step 3).
            (t!("PERIODIC").en(), &["solve.dispersion"]),
            (t!("STUDY").en(), &["project.variable", "study.sweep", "study.optimize", "study.tolerance"]),
        ],
    ),
    (
        RibbonTab::Results,
        &[
            // Comparison, checks, TDR and Touchstone are the result views' and Results' (step 8).
            (t!("EXPORT").en(), &["view.sparameters", "view.field", "view.animation", "view.pattern", "view.dispersion"]),
            (
                t!("NETWORK").en(),
                &["results.import", "results.measured", "results.network", "results.mixed_mode", "results.equivalent", "circuit.cascade"],
            ),
            (t!("FILTER").en(), &["filter.synthesize", "results.coupling"]),
            // The analyses read from a sweep are the Analyses view's buttons.
        ],
    ),
    // A system's own: its stages (each kind a button, [`STAGES`]), its source and receiver,
    // the Touchstone files its S-parameter stages read, its variables.
    (
        RibbonTab::System,
        &[(STAGES, &[]), (t!("CHAIN").en(), &["system.source", "system.receiver", "results.import"]), (t!("STUDY").en(), &["project.variable"])],
    ),
];

/// The system tab's panel whose buttons add each kind of stage.
pub const STAGES: &str = t!("STAGES").en();

/// The 3D view's tools on the ribbon, after a panel's commands: the ports made by pointing at
/// the model, beside the port commands.
pub const TOOLS: &[(RibbonTab, &str, &[Tool])] = &[(RibbonTab::PortsSources, "PORTS", &[Tool::Port, Tool::Lumped])];

/// The tabs a job of this kind of analysis shows: a system's own alone, else every other (each
/// with a command that [`fits`]).
///
/// ```
/// use emcraft_engine::workflow::Analysis;
/// use emcraft_ui_core::ribbon::{RibbonTab, tabs};
/// assert_eq!(tabs(Analysis::System), [RibbonTab::System]);
/// assert!(!tabs(Analysis::Frequency).contains(&RibbonTab::System));
/// ```
pub fn tabs(kind: Analysis) -> Vec<RibbonTab> {
    TABS.iter()
        .map(|(tab, _)| *tab)
        .filter(|tab| (*tab == RibbonTab::System) == (kind == Analysis::System))
        .filter(|tab| TABS.iter().any(|(t, panels)| t == tab && panels.iter().any(|(_, ids)| ids.iter().any(|id| fits(kind, id)))))
        .collect()
}

/// Whether a ribbon command belongs to a job of this kind of analysis: those of the other kinds
/// are left out of the ribbon once a kind is chosen (the search still finds them).
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
