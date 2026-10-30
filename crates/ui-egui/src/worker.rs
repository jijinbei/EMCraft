//! Long commands on a worker thread, so the window keeps drawing: the session is cloned into the
//! thread, the command runs there under an [`emcraft_engine::progress`] scope, and the finished
//! session comes back to replace the window's. Cancelling asks the command to stop at its next
//! step (between frequencies, between script steps) and drops the job at once: the window's
//! session never changed, and whatever the thread still computes is thrown away.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::Instant;

use emcraft_engine::progress::{Progress, Snapshot};
use emcraft_engine::{EngineError, Session};
use serde_json::Value;

/// What a finished job hands back: the session after the command, and its reply.
pub struct Done {
    pub session: Session,
    pub reply: Result<Value, EngineError>,
}

/// A running job.
pub struct Job {
    pub command: String,
    pub params: Value,
    pub started: Instant,
    pub progress: Arc<Progress>,
    rx: Receiver<Done>,
    /// Control-channel requests waiting for this job's reply.
    pub waiting: Vec<Sender<Value>>,
    /// The view the window showed when the job started: one chosen since is kept when it ends.
    pub view: Option<crate::View>,
    /// How long it was expected to take (s, `project.estimate`), when that could be said.
    pub estimate: Option<f64>,
}

/// What [`Job::poll`] found.
pub enum Poll {
    Running,
    Finished(Box<Done>),
    /// The thread ended without a reply (it cannot panic past `Session::execute`'s guard, but a
    /// stack overflow or an abort elsewhere could end it).
    Lost,
}

impl Job {
    /// Starts `command` on a copy of `session`.
    pub fn start(session: &Session, command: &str, params: Value, ctx: Option<egui::Context>) -> Job {
        let (id, p) = (command.to_owned(), params.clone());
        Job::spawn(session, command, params, ctx, move |s| s.execute(&id, &p))
    }

    /// Starts `work` on a copy of `session`; `command` names it.
    pub fn spawn(
        session: &Session,
        command: &str,
        params: Value,
        ctx: Option<egui::Context>,
        work: impl FnOnce(&mut Session) -> Result<Value, EngineError> + Send + 'static,
    ) -> Job {
        let (tx, rx) = channel();
        let progress = Progress::new();
        let mut s = session.clone();
        let pr = progress.clone();
        std::thread::spawn(move || {
            let reply = emcraft_engine::progress::scope(pr, || work(&mut s));
            let _ = tx.send(Done { session: s, reply });
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        });
        Job { command: command.to_owned(), params, started: Instant::now(), progress, rx, waiting: Vec::new(), view: None, estimate: None }
    }

    pub fn poll(&self) -> Poll {
        match self.rx.try_recv() {
            Ok(done) => Poll::Finished(Box::new(done)),
            Err(TryRecvError::Empty) => Poll::Running,
            Err(TryRecvError::Disconnected) => Poll::Lost,
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        self.progress.snapshot()
    }

    /// Asks the command to stop; the caller drops the job.
    pub fn cancel(&self) {
        self.progress.cancel();
    }

    /// Seconds since it started.
    pub fn elapsed(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    /// Seconds left: from how far it is once that says enough (a tenth done), else from the
    /// estimate; `None` when neither says.
    pub fn remaining(&self) -> Option<f64> {
        remaining(self.snapshot().fraction(), self.elapsed(), self.estimate)
    }
}

/// Seconds left of a job `fraction` done after `elapsed` seconds that was expected to take
/// `estimate`: the rate so far once a tenth is done, else what is left of the estimate.
pub fn remaining(fraction: Option<f64>, elapsed: f64, estimate: Option<f64>) -> Option<f64> {
    match fraction {
        Some(f) if (0.1..1.0).contains(&f) => Some(elapsed * (1.0 - f) / f),
        Some(f) if f >= 1.0 => Some(0.0),
        _ => estimate.filter(|e| e.is_finite()).map(|e| (e - elapsed).max(0.0)),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn time_left_follows_the_progress_then_the_estimate() {
        assert_eq!(super::remaining(Some(0.5), 10.0, Some(100.0)), Some(10.0));
        assert_eq!(super::remaining(Some(0.05), 10.0, Some(100.0)), Some(90.0));
        assert_eq!(super::remaining(None, 120.0, Some(100.0)), Some(0.0));
        assert_eq!(super::remaining(None, 1.0, None), None);
        assert_eq!(super::remaining(Some(1.0), 5.0, None), Some(0.0));
    }
}
