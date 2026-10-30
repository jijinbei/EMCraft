//! Autosave and recovery: while the project has unsaved changes, a copy of it is written every
//! [`Autosave::every`] seconds (never while a job runs) to `<config dir>/autosave/<name>.emcraft`
//! by `file.save {"copy": true}`, which leaves the project's own file, name and unsaved state
//! alone; a note beside it (`<name>.json`) names the project file it is a copy of. A real save
//! or a clean close (the changes saved or let go) deletes it. On start, each copy newer than its
//! project (or of a project never saved) is offered on the start page: Recover opens it with
//! `file.open {"path": copy, "original": project}`, so the project keeps its own file and shows
//! unsaved; dismissing it deletes it.
//!
//! The headless harness has no folder unless one is given (`app.autosave {"dir": …}` over the
//! control channel), so tests and scenarios write where they choose.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::{Value, json};

use crate::EmcraftApp;

/// The desktop app's autosave folder: `autosave` in the configuration directory.
pub fn default_dir() -> Option<PathBuf> {
    crate::prefs::config_dir().map(|d| d.join("autosave"))
}

/// Seconds between copies while there are unsaved changes.
pub const EVERY: f64 = 60.0;
/// Most copies looked at in the folder.
const MAX_FOUND: usize = 64;
/// The key that marks a note as the autosave's.
const MARK: &str = "emcraft_autosave";
/// Most bytes of a note read.
const MAX_NOTE: u64 = 64 << 10;

/// A copy found on start, newer than its project.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    /// The copy.
    pub file: PathBuf,
    /// The project's name.
    pub name: String,
    /// The project file it is a copy of (none: a project never saved).
    pub original: Option<String>,
    /// When it was written.
    pub modified: Option<SystemTime>,
}

/// The autosave's state beside the app.
#[derive(Debug)]
pub struct Autosave {
    /// Where copies go (none: no autosave).
    pub dir: Option<PathBuf>,
    /// Seconds between copies.
    pub every: f64,
    /// The input time the changes were first seen unsaved since the last copy.
    since: Option<f64>,
    /// The copy this project writes, which a save deletes.
    pub file: Option<PathBuf>,
    /// Copies found on start, offered on the start page.
    pub found: Vec<Found>,
    /// The last failure to write, reported once.
    failed: Option<String>,
}

impl Default for Autosave {
    fn default() -> Self {
        Autosave { dir: None, every: EVERY, since: None, file: None, found: Vec::new(), failed: None }
    }
}

/// `name` as a file name: letters, digits, `-`, `_` and `.` kept, the rest `_`, at most 64
/// characters, never empty or hidden.
pub fn file_name(name: &str) -> String {
    let s: String = name.chars().take(64).map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' }).collect();
    let s = s.trim_start_matches('.').to_owned();
    if s.is_empty() { "Untitled".to_owned() } else { s }
}

/// The note beside a copy.
fn note_of(file: &Path) -> PathBuf {
    file.with_extension("json")
}

/// Removes a copy and its note (what is already gone is fine).
fn remove(file: &Path) {
    let _ = std::fs::remove_file(file);
    let _ = std::fs::remove_file(note_of(file));
}

fn modified(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// Whether `path` lies in the autosave folder `dir`.
pub fn inside(dir: Option<&Path>, path: &str) -> bool {
    dir.is_some_and(|d| Path::new(path).parent().is_some_and(|p| p == d))
}

/// The copies in `dir` newer than their projects, the latest first; those older than their
/// project (it was saved since), and notes whose copy is gone, are deleted. Only the autosave's
/// own notes (marked) are read; other files are left alone.
pub fn scan(dir: &Path) -> Vec<Found> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut found = Vec::new();
    let mut notes: Vec<PathBuf> =
        entries.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).take(MAX_FOUND * 4).collect();
    notes.sort();
    for note in notes {
        // Only the autosave's own notes: anything else in the folder is left alone.
        let read = std::fs::metadata(&note).ok().filter(|m| m.len() <= MAX_NOTE).and_then(|_| std::fs::read_to_string(&note).ok());
        let v: Value = read.and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
        if v.get(MARK) != Some(&json!(1)) {
            continue;
        }
        let file = note.with_extension(emcraft_engine::project::EXTENSION);
        if !file.is_file() {
            let _ = std::fs::remove_file(&note);
            continue;
        }
        let name = v
            .get("name")
            .and_then(Value::as_str)
            .map(|n| emcraft_engine::text::clip(n, 200))
            .unwrap_or_else(|| file.file_stem().map_or_else(|| "Untitled".to_owned(), |s| s.to_string_lossy().into_owned()));
        let original = v.get("original").and_then(Value::as_str).filter(|o| !o.trim().is_empty()).map(|o| emcraft_engine::text::clip(o, 4096));
        let when = modified(&file);
        // Saved since the copy: nothing to recover.
        let newer = match (&original, when) {
            (Some(o), Some(w)) => modified(Path::new(o)).is_none_or(|m| w > m),
            _ => true,
        };
        if !newer {
            remove(&file);
            continue;
        }
        found.push(Found { file, name, original, modified: when });
        if found.len() >= MAX_FOUND {
            break;
        }
    }
    found.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.file.cmp(&b.file)));
    found
}

/// How long ago `when` was, in the current language: `just now`, `5 min ago`, `3 h ago`,
/// `2 d ago`.
pub fn ago(when: Option<SystemTime>) -> String {
    use crate::lang::{args, tr, trf};
    let Some(secs) = when.and_then(|w| SystemTime::now().duration_since(w).ok()).map(|d| d.as_secs()) else { return tr("just now").to_owned() };
    match secs {
        0..60 => tr("just now").to_owned(),
        60..3600 => trf("{n} min ago", args!(n = secs / 60)),
        3600..86_400 => trf("{n} h ago", args!(n = secs / 3600)),
        _ => trf("{n} d ago", args!(n = secs / 86_400)),
    }
}

impl EmcraftApp {
    /// Writes a copy of the project when its changes have been unsaved for `every` seconds
    /// (at input time `now`, s), unless a job runs. Called once a frame.
    pub(crate) fn autosave_tick(&mut self, now: f64) {
        if self.autosave.dir.is_none() || !self.session.is_dirty() {
            self.autosave.since = None;
            return;
        }
        if self.busy() {
            return;
        }
        let since = *self.autosave.since.get_or_insert(now);
        if now - since >= self.autosave.every {
            let _ = self.autosave_now();
            self.autosave.since = Some(now);
        }
    }

    /// Writes a copy of the project now (when there is a folder): the reply of `file.save`, or
    /// why not.
    pub fn autosave_now(&mut self) -> Result<Value, String> {
        let dir = self.autosave.dir.clone().ok_or("no autosave folder")?;
        let name = self.session.name();
        let file = dir.join(format!("{}.{}", file_name(&name), emcraft_engine::project::EXTENSION));
        let written = std::fs::create_dir_all(&dir).map_err(|e| format!("cannot make {}: {e}", dir.display())).and_then(|()| {
            let path = file.display().to_string();
            self.session.execute("file.save", &json!({"path": path, "copy": true})).map_err(|e| e.to_string())
        });
        let r = written.and_then(|reply| {
            let original = self.session.path().filter(|p| !inside(Some(&dir), p));
            let note = json!({MARK: 1, "name": name, "original": original});
            std::fs::write(note_of(&file), note.to_string()).map_err(|e| format!("cannot write {}: {e}", note_of(&file).display()))?;
            Ok(reply)
        });
        match &r {
            Ok(_) => {
                // A copy of another project (one replaced without asking, by a script) stays,
                // to be recovered on the next start.
                self.autosave.file = Some(file);
                self.autosave.failed = None;
            }
            Err(e) if self.autosave.failed.as_ref() != Some(e) => {
                self.autosave.failed = Some(e.clone());
                self.error_with("Autosave failed: {error}", crate::lang::args!(error = e));
            }
            Err(_) => {}
        }
        r
    }

    /// The project's changes were saved or let go: its copy goes.
    pub(crate) fn autosave_drop(&mut self) {
        if let Some(file) = self.autosave.file.take() {
            remove(&file);
        }
        self.autosave.since = None;
    }

    /// Lets a copy found on start go: it is deleted.
    pub(crate) fn dismiss_recovery(&mut self, i: usize) {
        if i < self.autosave.found.len() {
            let f = self.autosave.found.remove(i);
            remove(&f.file);
        }
    }

    /// The autosave's state for `ui.inspect`.
    pub fn autosave_inspect(&self) -> Value {
        let a = &self.autosave;
        json!({
            "dir": a.dir.as_ref().map(|d| d.display().to_string()),
            "every": a.every,
            "file": a.file.as_ref().map(|f| f.display().to_string()),
            "found": a.found.iter().map(|f| json!({"file": f.file.display().to_string(), "name": f.name, "original": f.original})).collect::<Vec<_>>(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_become_safe_file_names() {
        assert_eq!(file_name("highpass"), "highpass");
        assert_eq!(file_name("../../etc/passwd"), "_.._etc_passwd");
        assert_eq!(file_name(""), "Untitled");
        assert_eq!(file_name("..."), "Untitled");
        assert_eq!(file_name(&"x".repeat(500)).len(), 64);
        assert_eq!(file_name("フィルタ"), "フィルタ");
    }

    /// A copy newer than its project is found; one older is deleted; a stray note goes.
    #[test]
    fn a_scan_finds_copies_newer_than_their_projects() {
        let dir = std::env::temp_dir().join(format!("emcraft-autosave-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let project = dir.join("project.emcraft");
        std::fs::write(&project, "{}").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let copy = dir.join("a.emcraft");
        std::fs::write(&copy, "{}").unwrap();
        std::fs::write(dir.join("a.json"), json!({MARK: 1, "name": "a", "original": project.display().to_string()}).to_string()).unwrap();
        std::fs::write(dir.join("b.emcraft"), "{}").unwrap();
        std::fs::write(dir.join("b.json"), json!({MARK: 1, "name": "b"}).to_string()).unwrap();
        std::fs::write(dir.join("stray.json"), json!({MARK: 1}).to_string()).unwrap();
        // Not the autosave's: never read as a copy, never deleted.
        std::fs::write(dir.join("c.emcraft"), "{}").unwrap();
        std::fs::write(dir.join("c.json"), "not json").unwrap();
        std::fs::write(dir.join("prefs.json"), "{}").unwrap();
        let found = scan(&dir);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found.iter().any(|f| f.name == "a" && f.original.is_some()));
        assert!(found.iter().any(|f| f.name == "b" && f.original.is_none()));
        assert!(!dir.join("stray.json").exists());
        assert!(dir.join("prefs.json").exists() && dir.join("c.json").exists() && dir.join("c.emcraft").exists());
        // The project saved after the copy: the copy is out of date and goes.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&project, "{}").unwrap();
        let found = scan(&dir);
        assert_eq!(found.len(), 1);
        assert!(!copy.exists());
        assert!(scan(&dir.join("missing")).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
