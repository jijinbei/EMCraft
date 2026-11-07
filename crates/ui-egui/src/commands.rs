//! What the UI knows about each engine command beyond the engine's own table: its label on a
//! ribbon button (shortened where the ribbon has little room), its glyph, and whether it runs on
//! the worker thread. One row per command of `emcraft_engine::COMMANDS`, in its order (checked
//! by a test).

use crate::glyphs::Glyph;

/// The UI's facts about one command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiCommand {
    pub id: &'static str,
    /// The label on its ribbon button; the engine's label when `None`.
    pub label: Option<&'static str>,
    /// Its glyph; a plain square when `None`.
    pub glyph: Option<Glyph>,
    /// Whether it runs on the worker thread: it may take seconds to minutes.
    pub long: bool,
}

const fn ui(id: &'static str, label: Option<&'static str>, glyph: Option<Glyph>, long: bool) -> UiCommand {
    UiCommand { id, label, glyph, long }
}

/// Every engine command, in the engine's order.
pub const UI_COMMANDS: &[UiCommand] = &[
    ui("pcb.load", None, Some(Glyph::Import), false),
    ui("pcb.import", None, Some(Glyph::Import), true),
    ui("pcb.sample", None, Some(Glyph::Waveguide), false),
    ui("pcb.segment", Some(t!("Trace").en()), Some(Glyph::Lumped), false),
    ui("pcb.segment.delete", None, None, false),
    ui("pcb.via", Some(t!("Via").en()), Some(Glyph::Cylinder), false),
    ui("pcb.via.delete", None, None, false),
    ui("pcb.pour", Some(t!("Pour").en()), Some(Glyph::Box), false),
    ui("pcb.pour.delete", None, None, false),
    ui("pcb.region", Some(t!("Region").en()), Some(Glyph::Open), false),
    ui("pcb.port", Some(t!("Port").en()), Some(Glyph::Port), false),
    ui("pcb.element", Some(t!("Chip part").en()), Some(Glyph::Lumped), false),
    ui("pcb.inspect", None, Some(Glyph::Chart), false),
    ui("pcb.conditions", None, Some(Glyph::Chart), false),
    ui("view.pcb", None, Some(Glyph::Chart), false),
    ui("project.setup", Some(t!("Confirm settings").en()), Some(Glyph::Band), false),
    ui("project.analysis", None, Some(Glyph::KindSweep), false),
    ui("project.status", None, None, false),
    ui("project.estimate", None, None, false),
    ui("file.save", None, Some(Glyph::Save), false),
    ui("file.new", None, Some(Glyph::New), false),
    ui("file.open", None, Some(Glyph::OpenFile), true),
    ui("project.runs", None, None, false),
    ui("project.values", None, None, false),
    ui("edit.undo", None, None, false),
    ui("edit.redo", None, None, false),
    ui("results.touchstone", Some(t!("Touchstone").en()), Some(Glyph::Touchstone), false),
    ui("results.check", Some(t!("Checks").en()), Some(Glyph::Check), false),
    ui("units.set", Some(t!("Units").en()), Some(Glyph::Units), false),
    ui("model.material.create", Some(t!("Material").en()), Some(Glyph::Material), false),
    ui("model.material.edit", None, Some(Glyph::Material), false),
    ui("model.material.delete", None, Some(Glyph::Material), false),
    ui("geometry.waveguide", Some(t!("Waveguide").en()), Some(Glyph::Waveguide), false),
    ui("geometry.iris_filter", Some(t!("Iris filter").en()), Some(Glyph::Waveguide), false),
    ui("model.region", None, Some(Glyph::Box), true),
    ui("model.background", None, Some(Glyph::Box), true),
    ui("geometry.solid", Some(t!("Region").en()), Some(Glyph::Box), false),
    ui("geometry.import", Some(t!("Import").en()), Some(Glyph::Import), true),
    ui("geometry.import.inspect", None, Some(Glyph::Import), false),
    ui("geometry.body", Some(t!("Body").en()), Some(Glyph::Feature), false),
    ui("geometry.body.material", Some(t!("Body material").en()), Some(Glyph::Feature), false),
    ui("geometry.body.edit", None, Some(Glyph::Feature), false),
    ui("geometry.body.delete", None, Some(Glyph::Feature), false),
    ui("geometry.inclusion", Some(t!("Inclusion").en()), Some(Glyph::Inclusion), false),
    ui("geometry.feature", Some(t!("Boolean").en()), Some(Glyph::Feature), false),
    ui("geometry.measure", None, Some(Glyph::Ruler), false),
    ui("mesh.generate", Some(t!("Mesh").en()), Some(Glyph::Mesh), true),
    ui("mesh.adapt", None, Some(Glyph::Mesh), true),
    ui("model.walls", Some(t!("Walls").en()), Some(Glyph::Walls), false),
    ui("model.symmetry", Some(t!("Symmetry").en()), Some(Glyph::Symmetry), false),
    ui("solve.revolution", None, Some(Glyph::Cylinder), true),
    ui("solve.revolution.frequency", None, Some(Glyph::Cylinder), true),
    ui("solve.revolution.radiation", None, Some(Glyph::Cylinder), true),
    ui("solve.revolution.time_domain", None, Some(Glyph::Cylinder), true),
    ui("solve.cyclic", Some(t!("Cyclic modes").en()), Some(Glyph::Eigen), true),
    ui("solve.harmonics", Some(t!("Harmonics sweep").en()), Some(Glyph::Sweep), true),
    ui("model.symmetry.auto", Some(t!("Find symmetries").en()), Some(Glyph::Symmetry), false),
    ui("model.symmetry.find", Some(t!("Find symmetry").en()), Some(Glyph::Symmetry), false),
    ui("model.rotation", Some(t!("Rotation").en()), Some(Glyph::Symmetry), false),
    ui("model.rotation.find", Some(t!("Find rotation").en()), Some(Glyph::Symmetry), false),
    ui("model.periodic", Some(t!("Periodic boundaries").en()), Some(Glyph::Periodic), false),
    ui("model.port.create", Some(t!("Port").en()), Some(Glyph::Port), false),
    ui("model.port.edit", None, Some(Glyph::Port), false),
    ui("model.port.deembed", None, Some(Glyph::Port), false),
    ui("model.port.delete", None, Some(Glyph::Port), false),
    ui("solve.frequency", Some(t!("Frequency sweep").en()), Some(Glyph::Sweep), true),
    ui("solve.refine", Some(t!("Refine sweep").en()), Some(Glyph::Sweep), true),
    ui("solve.time_domain", Some(t!("Time domain").en()), Some(Glyph::Time), true),
    ui("solve.run", None, None, true),
    ui("solve.rerun", None, None, true),
    ui("solve.device", Some(t!("Device").en()), Some(Glyph::Settings), true),
    ui("solve.devices", None, None, true),
    ui("solve.eigenmodes", Some(t!("Eigenmodes").en()), Some(Glyph::Eigen), true),
    ui("solve.dispersion", Some(t!("Dispersion").en()), Some(Glyph::Dispersion), true),
    ui("results.dispersion", None, Some(Glyph::Dispersion), false),
    ui("view.dispersion", Some(t!("Dispersion PNG").en()), Some(Glyph::Dispersion), false),
    ui("results.eigenmodes", None, Some(Glyph::Eigen), false),
    ui("results.cavity", None, Some(Glyph::Eigen), false),
    ui("solve.port_modes", Some(t!("Port modes").en()), Some(Glyph::PortModes), true),
    ui("results.sparameters", None, Some(Glyph::Chart), false),
    ui("results.power", None, Some(Glyph::Chart), false),
    ui("results.compare", Some(t!("Compare").en()), Some(Glyph::Compare), true),
    ui("results.field_error", None, Some(Glyph::Compare), true),
    ui("results.history", None, None, false),
    ui("results.history.restore", None, None, false),
    ui("results.history.pin", None, None, false),
    ui("results.history.label", None, None, false),
    ui("results.history.delete", None, None, false),
    ui("results.probes", None, Some(Glyph::Probe), false),
    ui("view.probe.add", None, Some(Glyph::Probe), false),
    ui("view.probe.move", None, Some(Glyph::Probe), false),
    ui("view.probe.delete", None, Some(Glyph::Probe), false),
    ui("view.visibility", None, Some(Glyph::Material), false),
    ui("view.driven", None, Some(Glyph::Port), false),
    ui("view.chart.style", None, Some(Glyph::Chart), false),
    ui("results.probe", None, Some(Glyph::Probe), false),
    ui("view.revolution_field", None, None, false),
    ui("view.port_mode", None, None, false),
    ui("results.tdr", Some(t!("TDR").en()), Some(Glyph::Chart), false),
    ui("view.tdr", None, Some(Glyph::Chart), false),
    ui("view.field", Some(t!("Field PNG").en()), Some(Glyph::Field), false),
    ui("view.animation", Some(t!("Field animation").en()), Some(Glyph::Play), false),
    ui("view.sparameters", Some(t!("S-param PNG").en()), Some(Glyph::Chart), false),
    ui("model.open", Some(t!("Open boundary").en()), Some(Glyph::Open), false),
    ui("source.dipole", Some(t!("Dipole").en()), Some(Glyph::Dipole), false),
    ui("source.plane_wave", Some(t!("Plane wave").en()), Some(Glyph::PlaneWave), false),
    ui("model.port.lumped", Some(t!("Lumped port").en()), Some(Glyph::Lumped), false),
    ui("model.element.create", Some(t!("Element").en()), Some(Glyph::Lumped), false),
    ui("model.element.remove", None, Some(Glyph::Clear), false),
    ui("model.port.lumped.edit", None, Some(Glyph::Lumped), false),
    ui("model.port.lumped.delete", None, Some(Glyph::Lumped), false),
    ui("source.clear", Some(t!("Clear sources").en()), Some(Glyph::Clear), false),
    ui("source.edit", None, Some(Glyph::Dipole), false),
    ui("source.delete", None, Some(Glyph::Clear), false),
    ui("solve.radiation", Some(t!("Radiation").en()), Some(Glyph::Radiation), true),
    ui("results.far_field", None, Some(Glyph::Pattern), true),
    ui("view.pattern", Some(t!("Pattern PNG").en()), Some(Glyph::Pattern), false),
    ui("file.sample", Some(t!("Sample").en()), Some(Glyph::Sample), true),
    ui("file.templates", None, Some(Glyph::Sample), false),
    ui("project.variable", Some(t!("Variables").en()), Some(Glyph::Variable), true),
    ui("project.variable.delete", None, Some(Glyph::Variable), false),
    ui("project.variables", None, Some(Glyph::Variable), false),
    ui("study.sweep", Some(t!("Sweep").en()), Some(Glyph::ParamSweep), true),
    ui("study.optimize", Some(t!("Optimize").en()), Some(Glyph::Optimize), true),
    ui("view.study", None, Some(Glyph::Chart), false),
    ui("theme.check", None, None, false),
    ui("model.summary", None, Some(Glyph::Summary), false),
];

/// The row of command `id`.
pub fn find(id: &str) -> Option<&'static UiCommand> {
    UI_COMMANDS.iter().find(|c| c.id == id)
}

/// Whether command `id` runs on the worker thread.
pub fn is_long(id: &str) -> bool {
    find(id).is_some_and(|c| c.long)
}

/// A ribbon button's label in the current language: the command's own, shortened where the
/// ribbon has little room.
pub fn button_label(id: &str) -> &str {
    find(id).and_then(|c| c.label).map_or_else(|| crate::label(id), crate::lang::lookup)
}

/// The glyph of command `id`.
pub fn glyph(id: &str) -> Glyph {
    find(id).and_then(|c| c.glyph).unwrap_or(Glyph::Blank)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_engine_command_has_one_row_in_its_order() {
        let engine: Vec<&str> = emcraft_engine::COMMANDS.iter().map(|c| c.id).collect();
        let ours: Vec<&str> = UI_COMMANDS.iter().map(|c| c.id).collect();
        assert_eq!(ours, engine);
    }

    #[test]
    fn every_ribbon_command_has_a_glyph_and_a_label() {
        for (tab, panels) in crate::ribbon::TABS {
            for (_, items) in *panels {
                for id in *items {
                    let c = find(id).unwrap_or_else(|| panic!("{} › {id}: no row", tab.id()));
                    assert!(c.glyph.is_some(), "{id}: no glyph");
                    assert!(!button_label(id).is_empty(), "{id}");
                }
            }
        }
        assert!(is_long("solve.frequency") && is_long("mesh.generate") && !is_long("units.set") && !is_long("no.such"));
        assert_eq!((button_label("geometry.solid"), button_label("results.probe")), ("Region", "Probe fields"));
        assert_eq!((glyph("mesh.adapt"), glyph("model.summary"), glyph("project.status")), (Glyph::Mesh, Glyph::Summary, Glyph::Blank));
    }
}
