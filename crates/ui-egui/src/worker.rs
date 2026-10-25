//! Long commands on a worker thread, so the window keeps drawing: the session is cloned into the
//! thread, the command runs there under an [`emcraft_engine::progress`] scope, and the finished
//! session comes back to replace the window's. Cancelling asks the command to stop at its next
//! step (between frequencies, between script steps) and drops the job at once: the window's
//! session never changed, and whatever the thread still computes is thrown away.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::Instant;

use emcraft_engine::Session;
use emcraft_engine::progress::{Progress, Snapshot};
use serde_json::Value;

/// What a finished job hands back: the session after the command, and its reply.
pub struct Done {
    pub session: Session,
    pub reply: Result<Value, String>,
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
        Job::spawn(session, command, params, ctx, move |s| s.execute(&id, &p).map_err(|e| e.to_string()))
    }

    /// Starts `work` on a copy of `session`; `command` names it.
    pub fn spawn(
        session: &Session,
        command: &str,
        params: Value,
        ctx: Option<egui::Context>,
        work: impl FnOnce(&mut Session) -> Result<Value, String> + Send + 'static,
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
        Job { command: command.to_owned(), params, started: Instant::now(), progress, rx, waiting: Vec::new() }
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
}

/// Commands that run on the worker thread: they may take seconds to minutes.
pub fn is_long(id: &str) -> bool {
    id.starts_with("solve.")
        || matches!(id, "mesh.generate" | "file.sample" | "geometry.import" | "results.compare" | "results.field_error" | "results.far_field")
}
