//! The window's crash reports ([`emcraft_ui_core::crash`]): where they are kept, this run's
//! journal, and the File menu's reports.

use std::path::PathBuf;

use serde_json::json;

pub use emcraft_ui_core::crash::*;

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
