//! The window's actions recorded and played back: every change is an [`Action`] (a click, a
//! key, a control request), so the actions a session asked for, in order, are the session —
//! a reproduction of what a person did, to replay on another machine or keep as a test.
//!
//! A recording starts from the project as it was (its history, run again first) and lists the
//! actions asked for, each with the drafts it reads (the open dialog's values at its OK, the
//! step panel's fields at a run), since a field typed in changes no state until then. The
//! follow-ups an action makes are not listed: they follow again. A pointer resting on a row
//! (`Action::Hover`, every frame) is left out.
//!
//! Played back, one action a frame; one that waits for a job (`Action::waits_for_job`) waits
//! for the job before it, as a person would.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{Action, EmcraftApp, UiError};

/// The recording's format name and version.
pub const FORMAT: &str = "emcraft-recording";
pub const VERSION: u32 = 1;
/// Most actions a recording holds (a longer session keeps its first ones and says so).
pub const MAX_ENTRIES: usize = 100_000;
/// Largest recording file read (bytes).
pub const MAX_BYTES: u64 = 64 << 20;

/// An action asked for, with the drafts it reads.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub action: Action,
    /// The open dialog's command and values, for its OK.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dialog: Option<(String, Value)>,
    /// The step panel's fields, for what runs from them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields: Option<Value>,
}

/// A session's actions from the project it began with.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Recording {
    pub format: String,
    pub version: u32,
    /// The commands that built the project when the recording began (run again first).
    pub history: Value,
    pub entries: Vec<Entry>,
    /// Actions left out past [`MAX_ENTRIES`].
    #[serde(default)]
    pub dropped: usize,
}

impl Recording {
    /// A recording beginning with the project `history` builds.
    pub fn new(history: Value) -> Recording {
        Recording { format: FORMAT.into(), version: VERSION, history, entries: Vec::new(), dropped: 0 }
    }

    /// A recording read from a file, or from a crash report's folder (its `recording.json`).
    ///
    /// ```
    /// use emcraft_ui_egui::record::Recording;
    ///
    /// let r = Recording::new(serde_json::json!([]));
    /// let text = serde_json::to_string(&r).unwrap_or_default();
    /// assert_eq!(Recording::parse(&text), Ok(r));
    /// assert!(Recording::parse(r#"{"format": "other"}"#).is_err());
    /// ```
    pub fn read(path: &std::path::Path) -> Result<Recording, String> {
        let file = if path.is_dir() { path.join("recording.json") } else { path.to_path_buf() };
        let size = std::fs::metadata(&file).map_err(|e| format!("cannot read {}: {e}", file.display()))?.len();
        if size > MAX_BYTES {
            return Err(format!("a recording is at most {} MiB", MAX_BYTES >> 20));
        }
        let text = std::fs::read_to_string(&file).map_err(|e| format!("cannot read {}: {e}", file.display()))?;
        Recording::parse(&text)
    }

    /// A recording read from JSON, refused when it is not one this build plays.
    pub fn parse(text: &str) -> Result<Recording, String> {
        let r: Recording = serde_json::from_str(text).map_err(|e| format!("not a recording: {e}"))?;
        if r.format != FORMAT || r.version != VERSION {
            return Err(format!("not a recording this EMCraft plays: {} {}", emcraft_engine::text::clip(&r.format, 64), r.version));
        }
        if r.entries.len() > MAX_ENTRIES || !r.history.is_array() {
            return Err(format!("a recording holds at most {MAX_ENTRIES} actions after its history (a list)"));
        }
        Ok(r)
    }
}

/// The recording being made and the one being played.
#[derive(Default)]
pub struct Recorder {
    pub(crate) on: Option<Recording>,
    pub(crate) playing: Option<VecDeque<Entry>>,
    /// How deep `apply` is: only what is asked for (depth 0) is recorded, not what follows.
    pub(crate) depth: u32,
}

impl EmcraftApp {
    /// Starts recording from the project as it is.
    pub fn start_recording(&mut self) {
        let history = serde_json::to_value(self.session.history()).unwrap_or_else(|_| json!([]));
        self.recorder.on = Some(Recording::new(history));
    }

    /// Stops recording: what was recorded.
    pub fn stop_recording(&mut self) -> Option<Recording> {
        self.recorder.on.take()
    }

    /// Notes `action`, asked for, with the drafts it reads.
    pub(crate) fn record(&mut self, action: &Action) {
        if self.recorder.depth > 0 || matches!(action, Action::Hover(_)) || (self.recorder.on.is_none() && self.crashes.journal.is_none()) {
            return;
        }
        let dialog = matches!(action, Action::DialogOk)
            .then(|| self.ui_state.dialog().and_then(|d| Some((d.command.id.to_owned(), d.quiet_params()?))))
            .flatten();
        let fields = matches!(action, Action::ConfirmSetup | Action::StartRun | Action::ExportTouchstone).then(|| self.workflow.fields.drafts());
        let entry = Entry { action: action.clone(), dialog, fields };
        // The crash journal keeps every one (crate::crash).
        if let Some(j) = self.crashes.journal.as_mut() {
            j.push(&entry);
        }
        let Some(r) = self.recorder.on.as_mut() else { return };
        if r.entries.len() >= MAX_ENTRIES {
            r.dropped += 1;
            return;
        }
        r.entries.push(entry);
    }

    /// Plays `recording` back: its project built again, then its actions, one a frame.
    pub fn play(&mut self, recording: Recording) -> Result<Value, UiError> {
        self.idle()?;
        let n = recording.entries.len();
        crate::files::new_project(self);
        let history = recording.history.as_array().map_or(0, Vec::len);
        if history > 0 {
            self.run_script(recording.history)?;
        }
        self.recorder.playing = Some(recording.entries.into());
        Ok(json!({"actions": n, "history": history}))
    }

    /// One action of the recording played, unless it waits for the running job.
    pub(crate) fn play_next(&mut self) {
        let Some(queue) = self.recorder.playing.as_ref() else { return };
        let Some(waits) = queue.front().map(|e| e.action.waits_for_job()) else {
            self.recorder.playing = None;
            self.info_with(t!("The recording is played."), &[]);
            return;
        };
        if waits && self.busy() {
            return;
        }
        let Some(Entry { action, dialog, fields }) = self.recorder.playing.as_mut().and_then(VecDeque::pop_front) else { return };
        if let Some((id, values)) = dialog {
            self.restore_dialog(&id, values);
        }
        if let Some(f) = fields {
            self.workflow.fields.set_drafts(&f);
        }
        let report = action.reports_errors();
        if let Err(e) = self.apply(action)
            && (report || matches!(e, UiError::Busy(_)))
        {
            self.error(e);
        }
    }

    /// The dialog `id` with `values`, as they were typed at its OK.
    fn restore_dialog(&mut self, id: &str, values: Value) {
        if self.ui_state.dialog().is_some_and(|d| d.command.id == id)
            && let (Some(d), Some(o)) = (self.ui_state.dialog_mut(), values.as_object())
        {
            for (k, v) in o {
                let _ = d.set(k, v);
            }
            return;
        }
        if let Err(e) = self.open_dialog_with(id, values) {
            self.error(e);
        }
    }
}

/// A named enum of another crate (the 3D view's standard sides) by its id.
pub mod named {
    use emcraft_macros::Named;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<T: Named, S: Serializer>(v: &T, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(v.id())
    }

    pub fn deserialize<'de, T: Named, D: Deserializer<'de>>(d: D) -> Result<T, D::Error> {
        let s = String::deserialize(d)?;
        T::parse(&s).ok_or_else(|| serde::de::Error::unknown_variant(&s, T::IDS))
    }
}

/// A browser item read back: its commands and parameter the command table's own.
impl<'de> Deserialize<'de> for crate::browser::Item {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Written {
            edit: String,
            delete: String,
            key: String,
            value: Value,
        }
        let w = Written::deserialize(d)?;
        let bad = |what: &str, s: &str| serde::de::Error::custom(format!("no {what} `{}`", emcraft_engine::text::clip(s, 64)));
        let command = |s: &str| match s {
            "" => Some(""),
            s => emcraft_engine::find_command(s).map(|c| c.id),
        };
        let param = emcraft_engine::COMMANDS.iter().flat_map(|c| c.params.iter()).map(|p| p.name).find(|n| *n == w.key);
        Ok(crate::browser::Item {
            edit: command(&w.edit).ok_or_else(|| bad("command", &w.edit))?,
            delete: command(&w.delete).ok_or_else(|| bad("command", &w.delete))?,
            key: param.ok_or_else(|| bad("parameter", &w.key))?,
            value: w.value,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene3d::Pick;

    /// Every kind of action a recording holds is written and read back as it was (a named
    /// enum by its id, a browser item's command by the table's own).
    #[test]
    fn actions_are_written_and_read_back_whole() {
        use crate::files::{FileAction, Then};
        let item = crate::browser::Item { edit: "model.port.edit", delete: "model.port.delete", key: "name", value: json!("P1") };
        let actions = vec![
            Action::Click(crate::selection::Click::Double { pick: Some(Pick::Port(1)) }),
            Action::StandardView(emcraft_render::view3d::StandardView::Top),
            Action::Close(crate::ui_state::Layer::Dialog),
            Action::File(FileAction::Request(Then::Sample { name: "highpass".into(), values: json!({}), purpose: None })),
            Action::EditItem(item),
            Action::OpenStep(Some(crate::steps::Step::Setup)),
            Action::Tool(Some(crate::tools3d::Tool::Measure)),
        ];
        let mut r = Recording::new(json!([]));
        r.entries = actions.into_iter().map(|action| Entry { action, dialog: None, fields: None }).collect();
        let text = serde_json::to_string(&r).unwrap_or_default();
        assert_eq!(Recording::parse(&text), Ok(r));
    }

    /// Frames until `h` has played its recording and its jobs are done (bounded).
    fn settle(h: &mut crate::scenario::Harness) {
        let began = std::time::Instant::now();
        while (h.app.recorder.playing.is_some() || h.app.busy()) && began.elapsed().as_secs() < 120 {
            h.frames(1);
        }
    }

    /// A session recorded — a sample opened by a control request, a dialog's field typed and
    /// its OK (a job), a click in the view, another page — and played back in another window
    /// gives the same project (the history, command for command), selection and page: the
    /// dialog's value typed at OK travels with it.
    #[test]
    fn a_recorded_session_plays_back_to_the_same_window() {
        let mut h = crate::scenario::Harness::new();
        h.app.start_recording();
        h.call("engine.execute", json!({"command": "file.sample", "params": {"name": "highpass"}}));
        let _ = h.app.apply(Action::dialog("mesh.generate"));
        if let Some(d) = h.app.ui_state.dialog_mut() {
            let _ = d.set("size", &json!(5));
        }
        let _ = h.app.apply(Action::DialogOk);
        settle(&mut h);
        let _ = h.app.apply(Action::Click(crate::selection::Click::Single { pick: Some(Pick::Guide), add: false }));
        let _ = h.app.apply(Action::Show(crate::page::Tab::Start));
        let r = h.app.stop_recording().unwrap_or_else(|| Recording::new(json!([])));
        assert_eq!(r.entries.len(), 5, "{:?}", r.entries.iter().map(|e| &e.action).collect::<Vec<_>>());
        assert_eq!(r.entries[2].dialog.as_ref().map(|(id, v)| (id.as_str(), v["size"].clone())), Some(("mesh.generate", json!(5.0))));
        let text = serde_json::to_string(&r).unwrap_or_default();
        let mut again = crate::scenario::Harness::new();
        let played = Recording::parse(&text).map_err(UiError::Input).and_then(|r| again.app.play(r));
        assert!(played.is_ok(), "{played:?}");
        settle(&mut again);
        let history = |h: &crate::scenario::Harness| serde_json::to_value(h.app.session.history()).unwrap_or_default();
        assert_eq!(history(&again), history(&h));
        assert_eq!(again.app.ui_state.selection(), h.app.ui_state.selection());
        assert_eq!(again.app.ui_state.page.shown(), crate::page::Tab::Start);
    }

    /// A recording that is not one, or names a command there is none of, is refused.
    #[test]
    fn hostile_recordings_are_refused() {
        for text in [
            "",
            "[]",
            r#"{"format":"emcraft-recording","version":2,"history":[],"entries":[]}"#,
            r#"{"format":"emcraft-recording","version":1,"history":{},"entries":[]}"#,
            r#"{"format":"emcraft-recording","version":1,"history":[],"entries":[{"action":{"EditItem":{"edit":"no.such","delete":"","key":"name","value":1}}}]}"#,
        ] {
            assert!(Recording::parse(text).is_err(), "{text}");
        }
    }
}
