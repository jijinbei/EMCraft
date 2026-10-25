//! Progress and cancellation of long commands.
//!
//! A caller that runs a command on a worker thread (the desktop app) installs a [`Progress`] for
//! that thread with [`scope`]; long loops in the commands call [`report`] between their steps
//! (between frequencies of a sweep, between the steps of a script). `report` records how far the
//! work is and returns an error once the caller asked to cancel, so the command stops at the next
//! step and [`crate::Session::execute`] restores the session as for any failed command.
//!
//! Without a scope (scripts, the command line, tests) `report` does nothing and never fails.
//! Nothing here changes a result: the hook only reads a flag between steps.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::{EngineError, Result};

/// The error message of a cancelled command.
pub const CANCELLED: &str = "cancelled";

/// How far a command is, shared between the thread that runs it and the one that watches.
#[derive(Debug, Default)]
pub struct Progress {
    cancel: AtomicBool,
    done: AtomicU64,
    total: AtomicU64,
    stage: Mutex<String>,
}

/// What [`Progress::snapshot`] reads.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub done: u64,
    /// 0 while the work has not said how much there is.
    pub total: u64,
    pub stage: String,
    pub cancelled: bool,
}

impl Snapshot {
    /// The share done, when the total is known.
    pub fn fraction(&self) -> Option<f64> {
        (self.total > 0).then(|| (self.done as f64 / self.total as f64).clamp(0.0, 1.0))
    }
}

impl Progress {
    pub fn new() -> Arc<Progress> {
        Arc::new(Progress::default())
    }

    /// Asks the command to stop at its next step.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            done: self.done.load(Ordering::Relaxed),
            total: self.total.load(Ordering::Relaxed),
            stage: self.stage.lock().map(|s| s.clone()).unwrap_or_default(),
            cancelled: self.cancelled(),
        }
    }

    fn set(&self, done: usize, total: usize, stage: &str) {
        self.done.store(done as u64, Ordering::Relaxed);
        self.total.store(total as u64, Ordering::Relaxed);
        if let Ok(mut s) = self.stage.lock() {
            s.clear();
            s.extend(stage.chars().take(200));
        }
    }
}

thread_local! {
    static CURRENT: RefCell<Option<Arc<Progress>>> = const { RefCell::new(None) };
}

/// Restores the previous progress when a scope ends, even by unwinding.
struct Restore(Option<Arc<Progress>>);

impl Drop for Restore {
    fn drop(&mut self) {
        let previous = self.0.take();
        CURRENT.with(|c| *c.borrow_mut() = previous);
    }
}

/// Runs `f` with `progress` receiving this thread's reports.
pub fn scope<R>(progress: Arc<Progress>, f: impl FnOnce() -> R) -> R {
    let previous = CURRENT.with(|c| c.borrow_mut().replace(progress));
    let _restore = Restore(previous);
    f()
}

/// Records that `done` of `total` steps of `stage` are finished; an error once the work was
/// cancelled.
pub fn report(done: usize, total: usize, stage: &str) -> Result<()> {
    CURRENT.with(|c| match c.borrow().as_ref() {
        Some(p) if p.cancelled() => Err(EngineError::Failed(CANCELLED.into())),
        Some(p) => {
            p.set(done, total, stage);
            Ok(())
        }
        None => Ok(()),
    })
}

/// An error once the work was cancelled.
pub fn check() -> Result<()> {
    CURRENT.with(|c| match c.borrow().as_ref() {
        Some(p) if p.cancelled() => Err(EngineError::Failed(CANCELLED.into())),
        _ => Ok(()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_reach_the_scope_and_cancelling_stops_the_next_step() {
        assert!(report(1, 2, "outside").is_ok());
        let p = Progress::new();
        scope(p.clone(), || {
            report(3, 17, "frequency").unwrap();
            let s = p.snapshot();
            assert_eq!((s.done, s.total, s.stage.as_str()), (3, 17, "frequency"));
            assert!((s.fraction().unwrap() - 3.0 / 17.0).abs() < 1e-12);
            p.cancel();
            assert_eq!(report(4, 17, "frequency"), Err(EngineError::Failed(CANCELLED.into())));
            assert!(check().is_err());
        });
        // The scope is gone with its end.
        assert!(check().is_ok());
    }

    /// A cancelled sweep stops between frequencies and leaves the session as it was.
    #[test]
    fn a_cancelled_command_restores_the_session() {
        let mut s = crate::Session::new();
        s.run_script(&serde_json::json!([
            {"command": "geometry.waveguide", "params": {"width": 22.86, "height": 10.16, "sections": [{"length": 10, "material": "vacuum"}]}},
            {"command": "mesh.generate", "params": {"size": 6}},
            {"command": "model.port.create", "params": {"name": "1", "end": "start", "modes": 1}},
            {"command": "model.port.create", "params": {"name": "2", "end": "end", "modes": 1}}
        ]))
        .unwrap();
        let p = Progress::new();
        p.cancel();
        let r = scope(p, || s.execute("solve.frequency", &serde_json::json!({"start": 8, "stop": 9, "points": 3, "order": 1})));
        assert_eq!(r, Err(EngineError::Failed(CANCELLED.into())));
        assert!(s.sweep.is_none());
    }
}
