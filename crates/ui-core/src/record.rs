//! A recording of a window's actions: the project it began with and each action asked for since,
//! with the drafts it reads (the open dialog's values at its OK, the step panel's fields at a
//! run). A window records and plays them back ([`crate::action::Action`]); a crash report keeps
//! one ([`crate::crash`]).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::action::Action;

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
    /// A job was running when it was asked for (played back, one asked for with none waits
    /// for the job before it, as it did).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub busy: bool,
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
    /// use emcraft_ui_core::record::Recording;
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
