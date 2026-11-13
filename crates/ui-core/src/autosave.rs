//! Autosave and recovery: while the project has unsaved changes, a copy of it is written every
//! [`Autosave::every`] seconds (never while a job runs) to `<config dir>/autosave/<name>.emcraft`
//! by `file.save {"copy": true, "fields": false}` (no field vectors: quick), which leaves the project's own file, name and unsaved state
//! alone; a note beside it (`<name>.json`) names the project file it is a copy of. A real save
//! or a clean close (the changes saved or let go) deletes it. On start, each copy newer than its
//! project (or of a project never saved) is offered on the start page: Recover opens it with
//! `file.open {"path": copy, "original": project}`, so the project keeps its own file and shows
//! unsaved; dismissing it deletes it.
//!
//! The headless harness has no folder unless one is given (`app.autosave {"dir": …}` over the
//! control channel), so tests and scenarios write where they choose. Each window keeps an
//! [`Autosave`] and asks it each frame whether a copy is due ([`Autosave::due`]).

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::{Value, json};

use emcraft_engine::Session;

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
    /// The time (s) the changes were first seen unsaved since the last copy.
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
        let name = v.get("name").and_then(Value::as_str).map_or_else(
            || file.file_stem().map_or_else(|| "Untitled".to_owned(), |s| s.to_string_lossy().into_owned()),
            |n| emcraft_engine::text::clip(n, 200),
        );
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
    let Some(secs) = when.and_then(|w| SystemTime::now().duration_since(w).ok()).map(|d| d.as_secs()) else { return tr(t!("just now")).to_owned() };
    match secs {
        0..60 => tr(t!("just now")).to_owned(),
        60..3600 => trf(t!("{n} min ago"), args!(n = secs / 60)),
        3600..86_400 => trf(t!("{n} h ago"), args!(n = secs / 3600)),
        _ => trf(t!("{n} d ago"), args!(n = secs / 86_400)),
    }
}

impl Autosave {
    /// Copies into `dir` (none: no copies), with those found there newer than their projects
    /// offered for recovery ([`scan`]).
    pub fn at(dir: Option<PathBuf>) -> Autosave {
        let found = dir.as_deref().map_or_default(scan);
        Autosave { dir, found, ..Autosave::default() }
    }

    /// Whether a copy is due at time `now` (s): the changes `dirty` for `every` seconds, no job
    /// running (`busy`). The clock starts when they are first seen unsaved.
    pub fn due(&mut self, now: f64, dirty: bool, busy: bool) -> bool {
        if self.dir.is_none() || !dirty {
            self.since = None;
            return false;
        }
        if busy {
            return false;
        }
        let since = *self.since.get_or_insert(now);
        let due = now - since >= self.every;
        if due {
            self.since = Some(now);
        }
        due
    }

    /// Writes a copy of `session`'s project now (when there is a folder): the reply of
    /// `file.save`, or why not; and whether that failure is new (to report once).
    pub fn write(&mut self, session: &mut Session) -> (Result<Value, String>, bool) {
        let Some(dir) = self.dir.clone() else { return (Err("no autosave folder".into()), false) };
        let name = session.name();
        let file = dir.join(format!("{}.{}", file_name(&name), emcraft_engine::project::EXTENSION));
        let written = std::fs::create_dir_all(&dir).map_err(|e| format!("cannot make {}: {e}", dir.display())).and_then(|()| {
            let path = file.display().to_string();
            session.execute("file.save", &json!({"path": path, "copy": true, "fields": false})).map_err(|e| e.to_string())
        });
        let r = written.and_then(|reply| {
            let original = session.path().filter(|p| !inside(Some(&dir), p));
            let note = json!({MARK: 1, "name": name, "original": original});
            std::fs::write(note_of(&file), note.to_string()).map_err(|e| format!("cannot write {}: {e}", note_of(&file).display()))?;
            Ok(reply)
        });
        let new_failure = match &r {
            Ok(_) => {
                // A copy of another project (one replaced without asking, by a script) stays,
                // to be recovered on the next start.
                self.file = Some(file);
                self.failed = None;
                false
            }
            Err(e) if self.failed.as_ref() != Some(e) => {
                self.failed = Some(e.clone());
                true
            }
            Err(_) => false,
        };
        (r, new_failure)
    }

    /// The project's changes were saved or let go: its copy goes.
    pub fn drop_copy(&mut self) {
        if let Some(file) = self.file.take() {
            remove(&file);
        }
        self.since = None;
    }

    /// Lets copy `i` found on start go: it is deleted.
    pub fn dismiss(&mut self, i: usize) {
        if i < self.found.len() {
            let f = self.found.remove(i);
            remove(&f.file);
        }
    }

    /// Its state for `ui.inspect`.
    pub fn inspect(&self) -> Value {
        json!({
            "dir": self.dir.as_ref().map(|d| d.display().to_string()),
            "every": self.every,
            "file": self.file.as_ref().map(|f| f.display().to_string()),
            "found": self.found.iter().map(|f| json!({"file": f.file.display().to_string(), "name": f.name, "original": f.original})).collect::<Vec<_>>(),
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
