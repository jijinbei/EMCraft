//! A session recorded and played back in the iced window, and the crash reports it keeps
//! ([`emcraft_ui_core::record`], [`emcraft_ui_core::crash`]): what the window records is what it
//! was asked to run — each command (or list of commands) asked for, with the parameters it ran
//! with, and the pages looked at — from the project as it was. That is the project again,
//! command for command; the follow-ups a command makes (the rest of a list after a job) are not
//! listed, they follow again.
//!
//! Played back (a recording, or a crash report's folder: `emcraft --replay FOLDER`), the
//! project is built again from its history, then the entries one a frame, each after the job
//! before it. A recording from the former egui window plays too, as far as its actions run commands
//! (its dialogs' OK with their values, its runs, Undo and Redo, its pages); the rest are counted
//! and said.
//!
//! Every window that keeps crash reports writes the same entries to its journal, begun again from
//! the project at each autosave; a panic or an unclean exit leaves a report that plays it back.

use std::collections::VecDeque;
use std::path::PathBuf;

use emcraft_ui_core::action::Action;
use emcraft_ui_core::crash::{self, Journal, Report};
use emcraft_ui_core::lang::{args, tr, trf};
use emcraft_ui_core::record::{Entry, MAX_ENTRIES, Recording};
use iced::Task;
use serde_json::{Value, json};

use crate::app::{App, Message};

/// The recording being made, the one being played (and the entries this window does not play),
/// and whether what runs now follows from an entry already noted.
#[derive(Default)]
pub struct Recorder {
    pub on: Option<Recording>,
    pub playing: Option<VecDeque<Entry>>,
    pub skipped: usize,
    pub(crate) quiet: bool,
}

/// The crash reports' folder, this run's journal, and the reports shown on the card.
#[derive(Default)]
pub struct Crashes {
    pub dir: Option<PathBuf>,
    pub(crate) journal: Option<Journal>,
    pub shown: Option<Vec<Report>>,
}

/// What the crash reports' card asks for.
#[derive(Clone, Debug)]
pub enum ReportAct {
    /// The card opened (the reports then marked seen), or closed.
    Show(bool),
    /// The text of an issue about report `i`, put on the clipboard.
    Copy(usize),
    Delete(String),
    /// A report of the session as it is.
    Bug,
}

/// The project's history, as a recording begins from it.
fn history(app: &App) -> Value {
    serde_json::to_value(app.session.history()).unwrap_or_else(|_| json!([]))
}

impl App {
    /// `action`, asked for, noted in the journal and the recording (unless it follows from one
    /// noted already).
    pub(crate) fn note(&mut self, action: Action) {
        if self.recorder.quiet || (self.recorder.on.is_none() && self.crashes.journal.is_none()) {
            return;
        }
        let entry = Entry { action, dialog: None, fields: None, busy: self.job.is_some() };
        if let Some(j) = self.crashes.journal.as_mut() {
            j.push(&entry);
        }
        if let Some(r) = self.recorder.on.as_mut() {
            if r.entries.len() < MAX_ENTRIES {
                r.entries.push(entry);
            } else {
                r.dropped += 1;
            }
        }
    }

    /// Command `id` with `params`, asked for, noted.
    pub(crate) fn note_run(&mut self, id: &str, params: &Value) {
        self.note(Action::Run { id: id.to_owned(), params: params.clone() });
    }

    /// `run` done without noting what it runs (it follows from what was noted).
    pub(crate) fn quietly<T>(&mut self, run: impl FnOnce(&mut App) -> T) -> T {
        let was = std::mem::replace(&mut self.recorder.quiet, true);
        let out = run(self);
        self.recorder.quiet = was;
        out
    }

    /// Starts recording from the project as it is.
    pub fn start_recording(&mut self) {
        self.recorder.on = Some(Recording::new(history(self)));
    }

    /// Stops recording: what was recorded.
    pub fn stop_recording(&mut self) -> Option<Recording> {
        self.recorder.on.take()
    }

    /// Plays `recording` back: a new project built from its history (a job), then its entries.
    pub fn play(&mut self, recording: Recording) -> Result<Value, String> {
        if self.job.is_some() {
            return Err("a job is running: one at a time".into());
        }
        let (n, script) = (recording.entries.len(), recording.history);
        let steps = script.as_array().map_or(0, Vec::len);
        self.run_now("file.new", &json!({}));
        if steps > 0 {
            self.job = Some(emcraft_ui_core::job::Job::spawn(&self.session, "engine.script", json!({}), None, move |s| {
                s.run_script(&script).map(Value::Array)
            }));
        }
        self.recorder.playing = Some(recording.entries.into());
        self.recorder.skipped = 0;
        Ok(json!({"actions": n, "history": steps}))
    }

    /// The next entry of the recording played, once the job before it has ended.
    pub(crate) fn play_next(&mut self) {
        if self.job.is_some() {
            return;
        }
        let Some(queue) = self.recorder.playing.as_mut() else { return };
        let Some(entry) = queue.pop_front() else {
            self.recorder.playing = None;
            let skipped = std::mem::take(&mut self.recorder.skipped);
            let said = match skipped {
                0 => tr(t!("The recording is played.")).to_owned(),
                n => trf(t!("The recording is played ({n} actions this window does not play left out)."), args!(n = n)),
            };
            return self.tell(&said);
        };
        match entry.action {
            Action::Run { id, params } => self.start(&id, params),
            Action::RunAll(list) => self.queue(list),
            // A dialog of the former egui window, OK'd with the values it had.
            Action::DialogOk => match entry.dialog {
                Some((id, values)) => self.start(&id, values),
                None => self.recorder.skipped += 1,
            },
            Action::Undo => self.start("edit.undo", json!({})),
            Action::Redo => self.start("edit.redo", json!({})),
            Action::SetView(v) | Action::ShowView(v) => self.update(Message::Page(v)),
            _ => self.recorder.skipped += 1,
        }
    }

    /// Keeps crash reports in `dir` from now on: the journals of windows that ended without
    /// closing reported, this window's journal begun, panics watched (`hook`: the desktop app,
    /// not a test). Reports not yet seen are shown.
    pub fn with_crash_reports(mut self, dir: Option<PathBuf>, hook: bool) -> App {
        if let Some(dir) = dir {
            self.keep_crash_reports(dir, hook);
        }
        self
    }

    /// [`App::with_crash_reports`] in `dir`.
    pub(crate) fn keep_crash_reports(&mut self, dir: PathBuf, hook: bool) {
        self.crashes.journal = None;
        crash::collect_dead(&dir, None);
        match Journal::open(&dir, &history(self)) {
            Ok(j) => {
                if hook {
                    crash::watch(&dir, j.path());
                }
                self.crashes.journal = Some(j);
            }
            Err(e) => self.fail(trf(t!("Crash reports are off: {error}"), args!(error = e))),
        }
        let unseen = crash::list(&dir).iter().any(|r| !r.seen);
        self.crashes.dir = Some(dir);
        if unseen {
            let _ = self.report_act(ReportAct::Show(true));
        }
    }

    /// The journal begun again from the project as it is (an autosave holds it now).
    pub(crate) fn restart_journal(&mut self) {
        let h = history(self);
        if let Some(j) = self.crashes.journal.as_mut() {
            j.restart(&h);
        }
    }

    /// What the crash reports' card asks for.
    pub(crate) fn report_act(&mut self, a: ReportAct) -> Task<Message> {
        let Some(dir) = self.crashes.dir.clone() else {
            self.fail(trf(t!("Crash reports are off: {error}"), args!(error = "no folder")));
            return Task::none();
        };
        match a {
            ReportAct::Show(true) => {
                self.crashes.shown = Some(crash::list(&dir));
                crash::mark_seen(&dir);
            }
            ReportAct::Show(false) => self.crashes.shown = None,
            ReportAct::Copy(i) => {
                let issue = self.crashes.shown.as_ref().and_then(|r| r.get(i)).map(Report::issue);
                if let Some(text) = issue {
                    self.tell(tr(t!("Copied: paste it into an issue.")));
                    return iced::clipboard::write(text).discard();
                }
            }
            ReportAct::Delete(id) => match crash::delete(&dir, &id) {
                Ok(()) => self.crashes.shown = self.crashes.shown.take().map(|_| crash::list(&dir)),
                Err(e) => self.fail(e),
            },
            ReportAct::Bug => {
                let journal = self.crashes.journal.as_ref().map(|j| j.path().to_path_buf());
                match crash::report_now(&dir, journal.as_deref(), "Reported from the window: describe what you expected and what happened.") {
                    Ok(folder) => self.tell(&trf(t!("A report of this session is in {folder}."), args!(folder = folder.display()))),
                    Err(e) => self.fail(e),
                }
            }
        }
        Task::none()
    }
}
