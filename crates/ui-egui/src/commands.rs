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
    ui("project.frequency", Some("Band"), Some(Glyph::Band), false),
    ui("project.status", None, None, false),
    ui("project.estimate", None, None, false),
    ui("file.save", None, Some(Glyph::Save), false),
    ui("file.new", None, Some(Glyph::New), false),
    ui("file.open", None, Some(Glyph::OpenFile), true),
    ui("edit.undo", None, None, false),
    ui("edit.redo", None, None, false),
    ui("results.touchstone", Some("Touchstone"), Some(Glyph::Touchstone), false),
    ui("results.check", Some("Checks"), Some(Glyph::Check), false),
    ui("units.set", Some("Units"), Some(Glyph::Units), false),
    ui("model.material.create", Some("Material"), Some(Glyph::Material), false),
    ui("geometry.waveguide", Some("Waveguide"), Some(Glyph::Waveguide), false),
    ui("geometry.solid", Some("Region"), Some(Glyph::Box), false),
    ui("geometry.import", Some("Import"), Some(Glyph::Import), true),
    ui("geometry.body", Some("Body"), Some(Glyph::Feature), false),
    ui("geometry.body.material", Some("Body material"), Some(Glyph::Feature), false),
    ui("geometry.inclusion", Some("Inclusion"), Some(Glyph::Inclusion), false),
    ui("geometry.feature", Some("Boolean"), Some(Glyph::Feature), false),
    ui("mesh.generate", Some("Mesh"), Some(Glyph::Mesh), true),
    ui("mesh.adapt", None, Some(Glyph::Mesh), true),
    ui("model.walls", Some("Walls"), Some(Glyph::Walls), false),
    ui("model.symmetry", Some("Symmetry"), Some(Glyph::Symmetry), false),
    ui("model.periodic", Some("Periodic"), Some(Glyph::Periodic), false),
    ui("model.port.create", Some("Port"), Some(Glyph::Port), false),
    ui("solve.frequency", Some("Frequency sweep"), Some(Glyph::Sweep), true),
    ui("solve.time_domain", Some("Time domain"), Some(Glyph::Time), true),
    ui("solve.device", Some("Device"), Some(Glyph::Settings), true),
    ui("solve.devices", None, None, true),
    ui("solve.eigenmodes", Some("Eigenmodes"), Some(Glyph::Eigen), true),
    ui("solve.dispersion", Some("Dispersion"), Some(Glyph::Dispersion), true),
    ui("results.dispersion", None, Some(Glyph::Dispersion), false),
    ui("view.dispersion", Some("Dispersion PNG"), Some(Glyph::Dispersion), false),
    ui("results.eigenmodes", None, Some(Glyph::Eigen), false),
    ui("solve.port_modes", Some("Port modes"), Some(Glyph::PortModes), true),
    ui("results.sparameters", None, Some(Glyph::Chart), false),
    ui("results.compare", Some("Compare"), Some(Glyph::Compare), true),
    ui("results.field_error", None, Some(Glyph::Compare), true),
    ui("results.probe", None, Some(Glyph::Probe), false),
    ui("view.port_mode", None, None, false),
    ui("results.tdr", Some("TDR"), Some(Glyph::Chart), false),
    ui("view.tdr", None, Some(Glyph::Chart), false),
    ui("view.field", Some("Field PNG"), Some(Glyph::Field), false),
    ui("view.animation", Some("Field video"), Some(Glyph::Play), false),
    ui("view.sparameters", Some("S-param PNG"), Some(Glyph::Chart), false),
    ui("model.open", Some("Open boundary"), Some(Glyph::Open), false),
    ui("source.dipole", Some("Dipole"), Some(Glyph::Dipole), false),
    ui("source.plane_wave", Some("Plane wave"), Some(Glyph::PlaneWave), false),
    ui("model.port.lumped", Some("Lumped port"), Some(Glyph::Lumped), false),
    ui("source.clear", Some("Clear sources"), Some(Glyph::Clear), false),
    ui("solve.radiation", Some("Radiation"), Some(Glyph::Radiation), true),
    ui("results.far_field", None, Some(Glyph::Pattern), true),
    ui("view.pattern", Some("Pattern PNG"), Some(Glyph::Pattern), false),
    ui("file.sample", Some("Sample"), Some(Glyph::Sample), true),
    ui("project.variable", Some("Variables"), Some(Glyph::Variable), true),
    ui("project.variable.delete", None, Some(Glyph::Variable), false),
    ui("project.variables", None, Some(Glyph::Variable), false),
    ui("study.sweep", Some("Sweep"), Some(Glyph::ParamSweep), true),
    ui("study.optimize", Some("Optimize"), Some(Glyph::Optimize), true),
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
    find(id).and_then(|c| c.label).map_or_else(|| crate::label(id), crate::lang::tr)
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
