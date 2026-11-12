//! The system's file dialog for projects and parts (`rfd`), each asked on its own thread so the window
//! keeps drawing; the answer goes back through the callback the window gave. Both windows use it.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use emcraft_ui_core::files::{PART_EXTENSIONS, Picker};

/// The system's file dialog.
pub struct SystemPicker;

fn filters(d: rfd::FileDialog) -> rfd::FileDialog {
    d.add_filter("EMCraft project", &[emcraft_engine::project::EXTENSION])
}

impl Picker for SystemPicker {
    fn open(&self, done: Box<dyn FnOnce(Option<String>) + Send>) {
        std::thread::spawn(move || {
            let path = filters(rfd::FileDialog::new()).set_title("Open project").pick_file();
            done(path.map(|p| p.to_string_lossy().into_owned()));
        });
    }

    fn save(&self, suggested: &str, done: Box<dyn FnOnce(Option<String>) + Send>) {
        let name = suggested.to_owned();
        std::thread::spawn(move || {
            let path = filters(rfd::FileDialog::new()).set_title("Save project").set_file_name(name).save_file();
            done(path.map(|p| p.to_string_lossy().into_owned()));
        });
    }

    fn import(&self, done: Box<dyn FnOnce(Option<String>) + Send>) {
        std::thread::spawn(move || {
            let path = rfd::FileDialog::new()
                .set_title("Import a part")
                .add_filter("Parts (SolidCraft, 3MF, STL)", PART_EXTENSIONS)
                .add_filter("SolidCraft design", &["solidcraft", "sc"])
                .add_filter("3MF", &["3mf"])
                .add_filter("STL", &["stl"])
                .add_filter("KiCad PCB", &["kicad_pcb"])
                .pick_file();
            done(path.map(|p| p.to_string_lossy().into_owned()));
        });
    }
}
