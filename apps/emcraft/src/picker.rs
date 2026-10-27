//! The system's file dialog for projects (`rfd`), each asked on its own thread so the window
//! keeps drawing; the answer goes back through the callback the window gave.

use emcraft_ui_egui::files::Picker;

pub struct SystemPicker;

fn filters(d: rfd::FileDialog) -> rfd::FileDialog {
    d.add_filter("EMCraft project", &[emcraft_engine::project::EXTENSION]).add_filter("Project as JSON", &["json"])
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
}
