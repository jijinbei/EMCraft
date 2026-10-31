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
    /// Where a solve runs; `None` for a job that is not a solve.
    device: Option<Device>,
}

/// The device a solve runs on.
struct Device {
    /// Its name (`cpu`, `cuda`, …).
    name: String,
    /// The fronts the device had taken when the job started (the factorization's share on a
    /// GPU is the difference).
    fronts_before: usize,
    /// The session's device as the job started (its count of fronts is shared with the
    /// worker's copy).
    compute: emcraft_engine::Compute,
}

/// Where a solve runs, as the job row and its message say it.
#[derive(Clone, Debug, PartialEq)]
pub struct Where {
    /// The device's name (`cpu`, `cuda`, …).
    pub device: String,
    /// Whether a device does (part of) the work: a GPU, or the GPU's kernels on the CPU's SIMD
    /// (`cubecl-cpu`, not a GPU).
    pub gpu: bool,
    /// The factorization's fronts the GPU has taken so far in this job.
    pub fronts: usize,
    /// Whether the solve is the time domain's (all of it on the device) rather than a
    /// factorization's (its large fronts).
    pub stepped: bool,
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
        // A solve's device: the time domain names its own, else the session's.
        let device = crate::steps::is_solve(command).then(|| {
            let compute = session.compute();
            let named = params.get("device").and_then(Value::as_str).filter(|_| command == "solve.time_domain");
            Device { name: named.unwrap_or(compute.device()).to_owned(), fronts_before: compute.fronts_taken(), compute: compute.clone() }
        });
        Job { command: command.to_owned(), params, started: Instant::now(), progress, rx, waiting: Vec::new(), view: None, estimate: None, device }
    }

    /// Where the job's solve runs, and how much of it the GPU has taken so far.
    pub fn place(&self) -> Option<Where> {
        let d = self.device.as_ref()?;
        let stepped = self.command == "solve.time_domain";
        let fronts = d.compute.fronts_taken().saturating_sub(d.fronts_before);
        Some(Where { device: d.name.clone(), gpu: d.name != "cpu", fronts, stepped })
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

    /// Seconds left ([`remaining`]); `None` when nothing can honestly say (the job row then shows
    /// only the time it has run).
    pub fn remaining(&self) -> Option<f64> {
        remaining(self.snapshot().fraction(), self.elapsed(), self.estimate)
    }
}

/// The share done at which the rate so far has replaced the estimate entirely.
const TRUST_RATE: f64 = 0.25;
/// Below this many seconds left, the time left is not said (a count of whole steps cannot say
/// the last step's end that finely, and `0.0 s` while work remains would be false).
const LEAST_LEFT: f64 = 1.0;

/// Seconds left of a job `fraction` done (`None`: how much work there is is not known, as a fast
/// sweep's full solves) after `elapsed` seconds, expected to take `estimate`:
/// - from the run's own rate, `elapsed (1 − f)/f`, blended with what is left of the estimate
///   while little is done (the rate alone from a quarter on, or once the estimate is passed);
/// - without a share done, what is left of the estimate;
/// - `None` once nothing can say (the estimate passed with no share, everything counted done
///   while the job still runs) or under a second: never a countdown at 0 while work remains.
///
/// ```
/// use emcraft_ui_egui::worker::remaining;
/// // Half done in 10 s: 10 s more.
/// assert_eq!(remaining(Some(0.5), 10.0, Some(100.0)), Some(10.0));
/// // Past its estimate with no count to go by: only the elapsed time is said.
/// assert_eq!(remaining(None, 658.0, Some(300.0)), None);
/// ```
pub fn remaining(fraction: Option<f64>, elapsed: f64, estimate: Option<f64>) -> Option<f64> {
    if !(elapsed.is_finite() && elapsed >= 0.0) {
        return None;
    }
    let by_estimate = estimate.filter(|e| e.is_finite()).map(|e| e - elapsed).filter(|left| *left > 0.0);
    let left = match fraction.filter(|f| f.is_finite()) {
        Some(f) if f >= 1.0 => None,
        Some(f) if f > 0.0 => {
            let by_rate = elapsed * (1.0 - f) / f;
            match by_estimate {
                Some(e) => {
                    let w = (f / TRUST_RATE).min(1.0);
                    Some(w * by_rate + (1.0 - w) * e)
                }
                None => Some(by_rate),
            }
        }
        _ => by_estimate,
    };
    left.filter(|l| l.is_finite() && *l >= LEAST_LEFT)
}

#[cfg(test)]
mod tests {
    use super::remaining;

    #[test]
    fn time_left_follows_the_progress_then_the_estimate() {
        // A quarter done or more: the rate alone.
        assert_eq!(remaining(Some(0.5), 10.0, Some(100.0)), Some(10.0));
        assert_eq!(remaining(Some(0.25), 10.0, None), Some(30.0));
        // Early: mostly the estimate (a twentieth done, 10 s in: 190 s by the rate, 90 s by the
        // estimate).
        let early = remaining(Some(0.05), 10.0, Some(100.0)).unwrap();
        assert!((early - (0.2 * 190.0 + 0.8 * 90.0)).abs() < 1e-9, "{early}");
        // No share: the estimate, while it lasts.
        assert_eq!(remaining(None, 40.0, Some(100.0)), Some(60.0));
        assert_eq!(remaining(None, 1.0, None), None);
    }

    #[test]
    fn time_left_is_never_zero_while_work_remains() {
        // Past the estimate with no share: nothing to count down (elapsed only).
        assert_eq!(remaining(None, 120.0, Some(100.0)), None);
        assert_eq!(remaining(None, 99.5, Some(100.0)), None);
        // Past the estimate with a share: the rate says it.
        assert_eq!(remaining(Some(0.1), 200.0, Some(100.0)), Some(1800.0));
        // Everything counted while the job still runs: not 0.
        assert_eq!(remaining(Some(1.0), 5.0, Some(4.0)), None);
        // Nothing done yet past the estimate: unknown.
        assert_eq!(remaining(Some(0.0), 500.0, Some(100.0)), None);
        // Hostile numbers say nothing.
        assert_eq!(remaining(Some(f64::NAN), f64::INFINITY, Some(f64::NAN)), None);
        // Whatever the inputs, a time left said is at least a second.
        for f in [None, Some(0.0), Some(0.01), Some(0.3), Some(0.99), Some(1.0)] {
            for t in [0.0, 0.5, 10.0, 99.0, 101.0, 1e4] {
                for e in [None, Some(0.0), Some(1.0), Some(100.0)] {
                    assert!(remaining(f, t, e).is_none_or(|l| l >= 1.0), "{f:?} {t} {e:?}");
                }
            }
        }
    }

    #[test]
    fn time_left_falls_as_a_steady_run_goes_on() {
        // A run of 100 equal steps, a second each. Estimated right (or high), the time left falls
        // step by step; estimated at half, it climbs to the truth as the rate takes over (by a
        // quarter done) and falls from there, never 0 before the end.
        for estimate in [100.0, 150.0, 50.0] {
            let mut last = f64::INFINITY;
            for k in 1..99 {
                let t = k as f64;
                let left = remaining(Some(k as f64 / 100.0), t, Some(estimate)).unwrap_or(0.0);
                assert!(left >= 1.0, "{estimate} s, step {k}: {left}");
                if estimate >= 100.0 || k > 25 {
                    assert!(left <= last + 1e-9, "{estimate} s, step {k}: {left} after {last}");
                }
                if k >= 25 {
                    assert!((left - (100.0 - t)).abs() < 1e-9, "{estimate} s, step {k}: {left}");
                }
                last = left;
            }
        }
    }
}
