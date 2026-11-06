//! Crash reports, as SolidCraft makes them (`crates/engine/src/crash.rs`): nothing is sent
//! anywhere; everything stays in `<config dir>/crashes`.
//!
//! - **The journal**: every running window keeps one, locked, in `crashes/journal/`: the
//!   project's history when it began and each action asked for since ([`crate::record`]). It
//!   starts again from the project at each autosave and stops growing at
//!   [`MAX_JOURNAL_BYTES`]. A clean close deletes it.
//! - **An unclean exit** (abort, kill, a crash nothing catches): its journal stays, held by no
//!   one; the next start finds it and makes a report of it.
//! - **A panic** anywhere: the panic hook makes a report at once, with the message, where it
//!   happened and the backtrace.
//! - **A report asked for** (File ▸ Report a bug): the journal as it is.
//!
//! A report is a folder: `report.json` (what, when, how often) and `recording.json` (a
//! [`crate::record::Recording`] that plays the session back: `emcraft --replay FOLDER`). The
//! same crash again is counted, not written again; at most [`MAX_REPORTS`] reports,
//! [`MAX_BYTES`] and [`MAX_AGE_DAYS`] days are kept.

use std::fs::File;
use std::io::{BufRead, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::record::{Entry, Recording};

/// The journal's format name and version (its first line).
pub const JOURNAL_FORMAT: &str = "emcraft-journal";
/// A journal stops recording actions at this size (until the next autosave starts it again).
pub const MAX_JOURNAL_BYTES: u64 = 512 << 10;
/// Reports kept at most, their bytes at most, and their age at most.
pub const MAX_REPORTS: usize = 20;
pub const MAX_BYTES: u64 = 50 << 20;
pub const MAX_AGE_DAYS: u64 = 90;
/// Most journals looked at on start.
const MAX_JOURNALS: usize = 64;

/// The desktop app's crash folder: `crashes` in the configuration directory.
pub fn default_dir() -> Option<PathBuf> {
    crate::prefs::config_dir().map(|d| d.join("crashes"))
}

/// Seconds since 1970.
fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// What a report is of.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A panic, caught by the hook.
    Panic,
    /// The window ended without closing (found at the next start).
    UncleanExit,
    /// Asked for by the person (an unexpected result with no crash).
    Report,
}

/// A report: what happened, when and how often.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub id: String,
    pub kind: Kind,
    /// The panic's message (the first line of the reason).
    pub message: String,
    /// Where it happened (`file:line`), for a panic.
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub backtrace: Option<String>,
    /// The EMCraft that made it.
    pub emcraft: String,
    /// When it first and last happened (seconds since 1970), and how often.
    pub first: u64,
    pub last: u64,
    pub count: u32,
    /// Shown to the person since it last happened.
    #[serde(default)]
    pub seen: bool,
    /// Its folder (not written: where it was read from).
    #[serde(skip)]
    pub folder: PathBuf,
}

impl Report {
    fn new(kind: Kind, message: String, signature: &str) -> Report {
        let t = now();
        // The same crash (same kind, place and message) is one report, counted; a report asked
        // for is its own.
        let id = match kind {
            Kind::Report => format!("report-{t}-{}", std::process::id()),
            _ => format!("{:016x}", fnv(&format!("{kind:?}|{signature}"))),
        };
        Report {
            id,
            kind,
            message,
            location: None,
            backtrace: None,
            emcraft: env!("CARGO_PKG_VERSION").into(),
            first: t,
            last: t,
            count: 1,
            seen: false,
            folder: PathBuf::new(),
        }
    }

    /// The text of an issue about it, for the person to read and paste.
    pub fn issue(&self) -> String {
        let what = match self.kind {
            Kind::Panic => "a crash",
            Kind::UncleanExit => "the window ended without closing",
            Kind::Report => "an unexpected result",
        };
        let at = self.location.as_ref().map(|l| format!("at {l}\n")).unwrap_or_default();
        let trace = self
            .backtrace
            .as_ref()
            .map(|b| format!("\n<details><summary>backtrace</summary>\n\n```\n{}\n```\n</details>\n", emcraft_engine::text::clip(b, 20_000)))
            .unwrap_or_default();
        let s = format!(
            "EMCraft {}: {what} (seen {} times)\n\n```\n{}\n{at}```\n{trace}\nWhat led to it plays back with `emcraft --replay {}` (attach its recording.json).\n",
            self.emcraft,
            self.count,
            self.message,
            self.folder.display()
        );
        s
    }
}

/// FNV-1a: a hash that stays the same from build to build (a report's id).
fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

/// This run's journal, locked while the window runs.
pub struct Journal {
    file: File,
    path: PathBuf,
    bytes: u64,
}

impl Journal {
    /// A new journal in `dir/journal`, beginning from the project `history` builds.
    pub fn open(dir: &Path, history: &Value) -> Result<Journal, String> {
        let folder = dir.join("journal");
        std::fs::create_dir_all(&folder).map_err(|e| format!("cannot make {}: {e}", folder.display()))?;
        let path = folder.join(format!("run-{}-{}.jsonl", now(), std::process::id()));
        let file = File::options()
            .create(true)
            .read(true)
            .write(true)
            .truncate(true)
            .open(&path)
            .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        file.try_lock().map_err(|e| format!("cannot lock {}: {e}", path.display()))?;
        let mut j = Journal { file, path, bytes: 0 };
        j.restart(history);
        Ok(j)
    }

    /// Where it is.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Begins again from the project `history` builds (an autosave has its copy).
    pub fn restart(&mut self, history: &Value) {
        let head = json!({"format": JOURNAL_FORMAT, "version": 1, "emcraft": env!("CARGO_PKG_VERSION"), "history": history});
        let ok = self.file.set_len(0).is_ok() && self.file.rewind().is_ok();
        self.bytes = 0;
        if ok {
            self.line(&head.to_string());
        }
    }

    /// Notes an action, while there is room.
    pub fn push(&mut self, entry: &Entry) {
        if let Ok(text) = serde_json::to_string(entry)
            && self.bytes + text.len() as u64 <= MAX_JOURNAL_BYTES
        {
            self.line(&text);
        }
    }

    fn line(&mut self, text: &str) {
        if writeln!(self.file, "{text}").and_then(|()| self.file.flush()).is_ok() {
            self.bytes += text.len() as u64 + 1;
        }
    }
}

impl Drop for Journal {
    /// A clean close: no crash to find.
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// The recording a journal holds: its history and the actions after (a line that does not
/// read is left out).
pub fn read_journal(path: &Path) -> Result<Recording, String> {
    let file = File::open(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mut lines = std::io::BufReader::new(file.take(2 * MAX_JOURNAL_BYTES)).lines();
    let head: Value = lines.next().and_then(Result::ok).and_then(|l| serde_json::from_str(&l).ok()).ok_or("not a journal")?;
    if head.get("format").and_then(Value::as_str) != Some(JOURNAL_FORMAT) {
        return Err("not a journal".into());
    }
    let mut r = Recording::new(head.get("history").cloned().filter(Value::is_array).unwrap_or_else(|| json!([])));
    r.entries = lines.map_while(Result::ok).filter_map(|l| serde_json::from_str::<Entry>(&l).ok()).take(crate::record::MAX_ENTRIES).collect();
    Ok(r)
}

/// Writes `report` with `recording` into `dir`: the same one again counts once more (its first
/// recording kept). Old reports past the limits go. Its folder.
pub fn save(dir: &Path, mut report: Report, recording: &Recording) -> Result<PathBuf, String> {
    let folder = dir.join(&report.id);
    let earlier = std::fs::read_to_string(folder.join("report.json")).ok().and_then(|t| serde_json::from_str::<Report>(&t).ok());
    std::fs::create_dir_all(&folder).map_err(|e| format!("cannot make {}: {e}", folder.display()))?;
    if let Some(e) = earlier {
        report = Report { first: e.first, count: e.count.saturating_add(1), ..report };
    } else {
        let text = serde_json::to_string_pretty(recording).map_err(|e| e.to_string())?;
        std::fs::write(folder.join("recording.json"), text).map_err(|e| format!("cannot write the recording: {e}"))?;
    }
    let text = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
    std::fs::write(folder.join("report.json"), text).map_err(|e| format!("cannot write the report: {e}"))?;
    prune(dir);
    Ok(folder)
}

/// The reports in `dir`, the latest first.
pub fn list(dir: &Path) -> Vec<Report> {
    let Ok(read) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<Report> = read
        .filter_map(Result::ok)
        .take(4 * MAX_REPORTS + MAX_JOURNALS)
        .map(|e| e.path())
        .filter(|p| p.join("report.json").is_file())
        .filter_map(|p| {
            let text = std::fs::read_to_string(p.join("report.json")).ok()?;
            let r: Report = serde_json::from_str(&text).ok()?;
            Some(Report { folder: p, ..r })
        })
        .collect();
    out.sort_by(|a, b| b.last.cmp(&a.last).then_with(|| a.id.cmp(&b.id)));
    out
}

/// Marks every report in `dir` shown.
pub fn mark_seen(dir: &Path) {
    for mut r in list(dir).into_iter().filter(|r| !r.seen) {
        r.seen = true;
        if let Ok(text) = serde_json::to_string_pretty(&r) {
            let _ = std::fs::write(r.folder.join("report.json"), text);
        }
    }
}

/// Deletes the report `id` in `dir`.
pub fn delete(dir: &Path, id: &str) -> Result<(), String> {
    let r = list(dir).into_iter().find(|r| r.id == id).ok_or_else(|| format!("no report `{}`", emcraft_engine::text::clip(id, 64)))?;
    std::fs::remove_dir_all(&r.folder).map_err(|e| format!("cannot delete {}: {e}", r.folder.display()))
}

/// The bytes under `p`.
fn size(p: &Path) -> u64 {
    std::fs::read_dir(p).map_or(0, |r| r.filter_map(Result::ok).filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum())
}

/// Old reports past the limits deleted: past the age, then the oldest past the count or size.
fn prune(dir: &Path) {
    let mut reports = list(dir);
    let oldest = now().saturating_sub(MAX_AGE_DAYS * 86_400);
    let mut total: u64 = reports.iter().map(|r| size(&r.folder)).sum();
    while let Some(last) = reports.last() {
        if !(last.last < oldest || reports.len() > MAX_REPORTS || total > MAX_BYTES) {
            break;
        }
        total = total.saturating_sub(size(&last.folder));
        let _ = std::fs::remove_dir_all(&last.folder);
        reports.pop();
    }
}

/// The journals of windows that ended without closing (held by no one, not `ours`), each made
/// a report and deleted. How many.
pub fn collect_dead(dir: &Path, ours: Option<&Path>) -> usize {
    let Ok(read) = std::fs::read_dir(dir.join("journal")) else { return 0 };
    let dead: Vec<PathBuf> = read
        .filter_map(Result::ok)
        .take(MAX_JOURNALS)
        .map(|e| e.path())
        .filter(|p| Some(p.as_path()) != ours && p.extension().is_some_and(|x| x == "jsonl"))
        .filter(|p| File::open(p).is_ok_and(|f| f.try_lock().is_ok()))
        .collect();
    let mut n = 0;
    for path in dead {
        if let Ok(recording) = read_journal(&path) {
            let last = recording.entries.last().map(|e| format!("{:?}", e.action)).unwrap_or_default();
            let message = match last.as_str() {
                "" => "The window ended without closing.".to_owned(),
                l => format!("The window ended without closing, after: {}", emcraft_engine::text::clip(l, 300)),
            };
            let report = Report::new(Kind::UncleanExit, message, &last);
            if save(dir, report, &recording).is_ok() {
                n += 1;
            }
        }
        let _ = std::fs::remove_file(&path);
    }
    n
}

/// The crash folder and this run's journal, for the panic hook.
static CURRENT: Mutex<Option<(PathBuf, PathBuf)>> = Mutex::new(None);
static HOOK: OnceLock<()> = OnceLock::new();

/// Makes a report of every panic from now on into `dir`, with `journal`'s recording.
pub fn watch(dir: &Path, journal: &Path) {
    if let Ok(mut c) = CURRENT.lock() {
        *c = Some((dir.to_path_buf(), journal.to_path_buf()));
    }
    HOOK.get_or_init(|| {
        let earlier = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            report_panic(info);
            earlier(info);
        }));
    });
}

/// The panic `info` made a report (never panicking itself).
fn report_panic(info: &std::panic::PanicHookInfo<'_>) {
    let Some((dir, journal)) = CURRENT.try_lock().ok().and_then(|c| c.clone()) else { return };
    let message = info
        .payload()
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic".into());
    let location = info.location().map(|l| format!("{}:{}", l.file(), l.line()));
    let first = message.lines().next().unwrap_or_default().to_owned();
    let mut report =
        Report::new(Kind::Panic, emcraft_engine::text::clip(&message, 2000), &format!("{}|{first}", location.clone().unwrap_or_default()));
    report.location = location;
    report.backtrace = Some(std::backtrace::Backtrace::force_capture().to_string());
    let recording = read_journal(&journal).unwrap_or_else(|_| Recording::new(json!([])));
    let _ = save(&dir, report, &recording);
}

/// A report asked for: the journal as it is. Its folder.
pub fn report_now(dir: &Path, journal: Option<&Path>, note: &str) -> Result<PathBuf, String> {
    let recording = journal.map_or_else(|| Ok(Recording::new(json!([]))), read_journal)?;
    save(dir, Report::new(Kind::Report, emcraft_engine::text::clip(note, 2000), ""), &recording)
}

/// The window's crash reports: their folder and this run's journal.
#[derive(Default)]
pub struct Crashes {
    pub dir: Option<PathBuf>,
    pub(crate) journal: Option<Journal>,
}

impl crate::EmcraftApp {
    /// Keeps crash reports in `dir` from now on: the journals of windows that ended without
    /// closing reported, this window's journal begun, panics watched (`hook`: the desktop app;
    /// not a test, whose other panics are not the window's). Reports not yet seen are shown.
    pub fn keep_crash_reports(&mut self, dir: PathBuf, hook: bool) {
        self.crashes.journal = None;
        collect_dead(&dir, None);
        let history = serde_json::to_value(self.session.history()).unwrap_or_else(|_| json!([]));
        match Journal::open(&dir, &history) {
            Ok(j) => {
                if hook {
                    watch(&dir, j.path());
                }
                self.crashes.journal = Some(j);
            }
            Err(e) => self.error_with(t!("Crash reports are off: {error}"), crate::lang::args!(error = e)),
        }
        let unseen = list(&dir).iter().any(|r| !r.seen);
        self.crashes.dir = Some(dir);
        if unseen && let Err(e) = self.show_reports() {
            self.error(e);
        }
    }

    fn crash_dir(&self) -> Result<PathBuf, crate::UiError> {
        self.crashes.dir.clone().ok_or_else(|| crate::UiError::Input("no crash reports folder".into()))
    }

    /// The crash reports' card, each report then marked seen.
    pub(crate) fn show_reports(&mut self) -> Result<(), crate::UiError> {
        let dir = self.crash_dir()?;
        let reports = list(&dir);
        mark_seen(&dir);
        self.ui_state.open_reports(reports);
        Ok(())
    }

    pub(crate) fn delete_report(&self, id: &str) -> Result<(), crate::UiError> {
        delete(&self.crash_dir()?, id).map_err(crate::UiError::Input)
    }

    /// A report of the session as it is.
    pub(crate) fn report_bug(&mut self) -> Result<(), crate::UiError> {
        let dir = self.crash_dir()?;
        let journal = self.crashes.journal.as_ref().map(|j| j.path().to_path_buf());
        let folder = report_now(&dir, journal.as_deref(), "Reported from the window: describe what you expected and what happened.")
            .map_err(crate::UiError::Input)?;
        self.info_with(t!("A report of this session is in {folder}."), crate::lang::args!(folder = folder.display()));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Action;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("emcraft-crash-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    /// A journal holds its history and actions; a clean close deletes it; one left behind by a
    /// window that ended (no one holding it) becomes a report whose recording is the journal's,
    /// and the same again is counted, not written again.
    #[test]
    fn a_journal_left_behind_becomes_a_report_counted_once() {
        let dir = temp("journal");
        let entry = Entry { action: Action::Undo, dialog: None, fields: None };
        let path = {
            let mut j = Journal::open(&dir, &json!([{"command": "file.new", "params": {}}])).unwrap_or_else(|e| panic!("{e}"));
            j.push(&entry);
            let r = read_journal(j.path()).unwrap_or_else(|e| panic!("{e}"));
            assert_eq!((r.history.as_array().map(Vec::len), r.entries.len()), (Some(1), 1));
            // Held: not dead.
            assert_eq!(collect_dead(&dir, None), 0);
            j.path().to_path_buf()
        };
        assert!(!path.exists(), "a clean close deletes the journal");
        // A journal no one holds (its window gone).
        for _ in 0..2 {
            let left = dir.join("journal/run-1-1.jsonl");
            let head = json!({"format": JOURNAL_FORMAT, "version": 1, "history": []});
            let _ = std::fs::write(&left, format!("{head}\n{}\n", serde_json::to_string(&entry).unwrap_or_default()));
            assert_eq!(collect_dead(&dir, None), 1);
            assert!(!left.exists());
        }
        let reports = list(&dir);
        assert_eq!(reports.len(), 1);
        assert_eq!((reports[0].kind, reports[0].count), (Kind::UncleanExit, 2));
        let rec = std::fs::read_to_string(reports[0].folder.join("recording.json")).unwrap_or_default();
        assert_eq!(Recording::parse(&rec).map(|r| r.entries), Ok(vec![entry]));
        assert!(reports[0].issue().contains("emcraft --replay"));
        assert_eq!(delete(&dir, &reports[0].id), Ok(()));
        assert!(list(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Past the limits, the oldest reports go; a hostile report folder (not JSON) is passed over.
    #[test]
    fn reports_past_the_limits_go_and_junk_is_passed_over() {
        let dir = temp("prune");
        let rec = Recording::new(json!([]));
        for i in 0..(MAX_REPORTS + 3) {
            let mut r = Report::new(Kind::Panic, format!("boom {i}"), &format!("{i}"));
            r.last = 1_000 + i as u64 + now();
            let _ = save(&dir, r, &rec);
        }
        let _ = std::fs::create_dir_all(dir.join("junk"));
        let _ = std::fs::write(dir.join("junk/report.json"), "{not json");
        let kept = list(&dir);
        assert_eq!(kept.len(), MAX_REPORTS);
        assert!(kept.iter().all(|r| r.message != "boom 0"), "the oldest went first");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
