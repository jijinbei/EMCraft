//! The round trip with SolidCraft (docs/adr/0010-geometry-from-solidcraft.md): a design the
//! project imports opens in SolidCraft (the program Settings name, else `solidcraft` on the
//! PATH); the window watches its file and, saved there, reads the model again
//! (`geometry.reload`). A new design is an empty one saved where the person chooses, opened in
//! SolidCraft and imported once it is saved there with something in it.

use std::path::Path;
use std::time::{Duration, SystemTime};

use emcraft_ui_core::lang::{args, tr, trf};
use iced::Subscription;
use serde_json::json;

use crate::app::{App, Message};

/// An empty SolidCraft design: what a new one starts as (its format SolidCraft's own, read by
/// `solidcraft-exchange` at the commit EMCraft pins).
pub const EMPTY: &str =
    r#"{"format":"solidcraft/43","name":"Design","units":"mm","params":[],"features":[],"next_id":1,"next_component_color_index":0}"#;

/// The files watched: each design of the user's the project imports and the new design waiting
/// to be saved, with when each was last written (`None`: not there).
#[derive(Clone, Debug, Default)]
pub struct Watch {
    pub designs: Vec<(String, Option<SystemTime>)>,
    /// A new design, imported when it is next written.
    pub new: Option<(String, Option<SystemTime>)>,
    /// The project's generation the list was made for.
    generation: Option<u64>,
}

/// When `path` was last written.
fn written(path: &str) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

impl Watch {
    /// Whether anything is watched.
    pub fn any(&self) -> bool {
        !self.designs.is_empty() || self.new.is_some()
    }
}

impl App {
    /// The designs watched brought up to date with the project's (once a generation): a design
    /// still imported keeps when it was last read.
    pub(crate) fn watch_designs(&mut self) {
        if self.watch.generation == Some(self.generation) {
            return;
        }
        let paths: Vec<String> = emcraft_engine::designs(&self.session).into_iter().filter(|d| !d.template).map(|d| d.path).collect();
        let kept = std::mem::take(&mut self.watch.designs);
        self.watch.designs = paths
            .into_iter()
            .map(|p| match kept.iter().find(|(k, _)| *k == p) {
                Some(k) => k.clone(),
                None => {
                    let at = written(&p);
                    (p, at)
                }
            })
            .collect();
        self.watch.generation = Some(self.generation);
    }

    /// A look at the files watched: a design written since it was read is read again, a new
    /// design written is imported; nothing while a job runs (the next look sees it).
    pub(crate) fn look_at_designs(&mut self) {
        if self.job.is_some() {
            return;
        }
        if let Some((path, at)) = self.watch.new.clone() {
            let now = written(&path);
            if now.is_some() && now != at {
                self.watch.new = None;
                self.tell(&trf(t!("{file} was saved in SolidCraft: importing it."), args!(file = path)));
                self.start("geometry.import", json!({ "path": path }));
                return;
            }
        }
        let changed = self.watch.designs.iter().any(|(p, at)| written(p).is_some_and(|now| Some(now) != *at));
        if changed {
            for (p, at) in &mut self.watch.designs {
                *at = written(p);
            }
            self.tell(tr(t!("A design was saved in SolidCraft: reading the model again.")));
            self.start("geometry.reload", json!({}));
        }
    }

    /// Design `path` opened in SolidCraft, or why it could not be.
    pub(crate) fn open_design(&mut self, path: &str) {
        let program = self.prefs.solidcraft_program();
        if let Err(e) = std::process::Command::new(&program).arg(path).spawn() {
            self.fail(trf(t!("SolidCraft did not start ({program}): {error}. Set its program in Settings."), args!(program = program, error = e)));
        }
    }

    /// A new design at `path`: written empty, opened in SolidCraft, imported once saved there.
    pub(crate) fn new_design(&mut self, path: &str) {
        let path =
            if Path::new(path).extension().is_some_and(|e| e == "solidcraft" || e == "sc") { path.to_owned() } else { format!("{path}.solidcraft") };
        if let Err(e) = std::fs::write(&path, EMPTY) {
            return self.fail(format!("{path}: {e}"));
        }
        self.watch.new = Some((path.clone(), written(&path)));
        self.open_design(&path);
    }
}

/// A look a second at the files watched, while there are any.
pub fn subscription(app: &App) -> Subscription<Message> {
    if app.watch.any() { Subscription::run(ticks) } else { Subscription::none() }
}

/// A `Watch` message a second, from a thread of its own that ends when the subscription does.
fn ticks() -> impl iced::futures::Stream<Item = Message> {
    let (mut tx, rx) = iced::futures::channel::mpsc::channel(1);
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(1));
            if let Err(e) = tx.try_send(Message::Watch)
                && e.is_disconnected()
            {
                break;
            }
        }
    });
    rx
}
