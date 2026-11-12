//! Long commands on a worker thread, so the window keeps drawing: the session is cloned into the
//! thread, the command runs there under an [`emcraft_engine::progress`] scope, and the finished
//! session comes back to replace the window's. Cancelling asks the command to stop at its next
//! step (between frequencies, between script steps) and drops the job at once: the window's
//! session never changed, and whatever the thread still computes is thrown away.
//!
//! The thread wakes the window when it ends ([`Wake`], each toolkit its own way); what the
//! window then does is decided here ([`finished`]: the steps a finished job leaves, in order) and
//! done by the window.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::Instant;

use emcraft_engine::progress::{Progress, Snapshot};
use emcraft_engine::{EngineError, Session};
use serde_json::{Value, json};

use crate::dialogs::Dialog;
use crate::lang::{args, tr, trf};
use crate::view::View;

/// What wakes the window when a job ends (none: the window looks at its job every frame).
pub type Wake = Box<dyn Fn() + Send>;

/// Whether command `id` solves (its job says where it runs).
pub fn is_solve(id: &str) -> bool {
    emcraft_engine::find_command(id).is_some_and(emcraft_engine::CommandSpec::solves)
}

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
    pub view: Option<View>,
    /// How long it was expected to take (s, `project.estimate`), when that could be said.
    pub estimate: Option<f64>,
    /// Where a solve runs; `None` for a job that is not a solve.
    device: Option<Device>,
    /// The dialog that started it: it opens again, with the error, if the job fails.
    pub dialog: Option<Dialog>,
    /// The commands to run after it, once it ends well (`run_all`).
    pub then: Vec<(String, Value)>,
    /// A project it opens lands on its step once it is in (`files::land`).
    pub land: bool,
}

/// The device a solve runs on.
struct Device {
    /// The device.
    name: emcraft_engine::Device,
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
    /// The device.
    pub device: emcraft_engine::Device,
    /// The factorization's fronts the GPU has taken so far in this job.
    pub fronts: usize,
    /// Whether the solve is the time domain's (all of it on the device) rather than a
    /// factorization's (its large fronts).
    pub stepped: bool,
}

impl Where {
    /// Where it runs, as the messages say it.
    pub fn text(&self) -> String {
        use emcraft_engine::Device;
        let device = self.device.id();
        match (self.device, self.stepped) {
            (Device::Cpu, _) => tr(t!("CPU")).to_owned(),
            // The GPU's kernels run on the CPU's SIMD there: no GPU at all.
            (Device::CubeclCpu, _) => trf(t!("CPU kernels ({device})"), args!(device = device)),
            (Device::Cuda, true) => trf(t!("GPU {device}"), args!(device = device)),
            (Device::Cuda, false) if self.fronts > 0 => trf(t!("GPU {device}: {n} fronts"), args!(device = device, n = self.fronts)),
            (Device::Cuda, false) => trf(t!("CPU (no front large enough for {device})"), args!(device = device)),
        }
    }

    /// Whether a device does (part of) the work: a GPU, or the GPU's kernels on the CPU's SIMD
    /// (`cubecl-cpu`, not a GPU).
    pub fn gpu(&self) -> bool {
        self.device != emcraft_engine::Device::Cpu
    }
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
    pub fn start(session: &Session, command: &str, params: Value, wake: Option<Wake>) -> Job {
        let (id, p) = (command.to_owned(), params.clone());
        Job::spawn(session, command, params, wake, move |s| s.execute(&id, &p))
    }

    /// Starts `work` on a copy of `session`; `command` names it.
    pub fn spawn(
        session: &Session,
        command: &str,
        params: Value,
        wake: Option<Wake>,
        work: impl FnOnce(&mut Session) -> Result<Value, EngineError> + Send + 'static,
    ) -> Job {
        let (tx, rx) = channel();
        let progress = Progress::new();
        let mut s = session.clone();
        let pr = progress.clone();
        std::thread::spawn(move || {
            let reply = emcraft_engine::progress::scope(pr, || work(&mut s));
            let _ = tx.send(Done { session: s, reply });
            if let Some(wake) = wake {
                wake();
            }
        });
        // A solve's device: the time domain names its own, else the session's.
        let device = is_solve(command).then(|| {
            let compute = session.compute();
            let named =
                params.get("device").and_then(Value::as_str).filter(|_| command == "solve.time_domain").and_then(emcraft_engine::Device::parse);
            Device { name: named.unwrap_or_else(|| compute.device()), fronts_before: compute.fronts_taken(), compute: compute.clone() }
        });
        Job {
            command: command.to_owned(),
            params,
            started: Instant::now(),
            progress,
            rx,
            waiting: Vec::new(),
            view: None,
            estimate: None,
            device,
            dialog: None,
            then: Vec::new(),
            land: false,
        }
    }

    /// Where the job's solve runs, and how much of it the GPU has taken so far.
    pub fn place(&self) -> Option<Where> {
        let d = self.device.as_ref()?;
        let stepped = self.command == "solve.time_domain";
        let fronts = d.compute.fronts_taken().saturating_sub(d.fronts_before);
        Some(Where { device: d.name, fronts, stepped })
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

    /// A running job in brief: what runs, how far it is (its count, from the job's progress) and
    /// for how long — `Frequency sweep · 3/11 · 4 s`.
    pub fn text(&self) -> String {
        let snap = self.snapshot();
        let command = if self.command == "engine.script" { tr(t!("Script")) } else { crate::lang::command_label(&self.command) };
        let secs = format!("{:.0}", self.elapsed());
        let (done, total) = (snap.done.min(snap.total) as usize, snap.total as usize);
        let what = match snap.stage.as_str() {
            _ if total == 0 => tr(t!("starting")).to_owned(),
            // Its full solves are added until the residuals are small: how many so far, and the
            // worst residual of the last pass in the basis (falling to the tolerance, 1e-4).
            "fast sweep" => match snap.residual {
                Some(r) => trf(t!("fast: {n} in full, residual {r}"), args!(n = done, r = format!("{r:.0e}"))),
                None => trf(t!("fast: {n} in full"), args!(n = done)),
            },
            "frequency" | "phase" | "adaptive pass" => format!("{done}/{total}"),
            // A study's points (each a thousand parts) and a script's steps: the engine's stage.
            stage if total >= 1000 => format!("{stage}, {} %", (100 * done) / total),
            stage => format!("{stage} {done} / {total}"),
        };
        match self.remaining() {
            Some(left) => {
                trf(t!("{command} · {what} · {secs} s · {left} left"), args!(command = command, what = what, secs = secs, left = duration(left)))
            }
            None => trf(t!("{command} · {what} · {secs} s"), args!(command = command, what = what, secs = secs)),
        }
    }

    /// Seconds left ([`remaining`]); `None` when nothing can honestly say (the job row then shows
    /// only the time it has run).
    pub fn remaining(&self) -> Option<f64> {
        remaining(self.snapshot().fraction(), self.elapsed(), self.estimate)
    }
}

/// What a finished job leaves the window to do, in order ([`finished`]).
pub enum Then {
    /// The session the command left (it ended well).
    Session(Box<Session>),
    /// What follows the command's success: the view it calls for, its message.
    After { command: String, params: Value, reply: Value },
    /// The view chosen while it ran, kept.
    Keep(View),
    /// A project it opened lands on its step.
    Land,
    /// How long it took and where it ran, after its message.
    Note(String),
    /// Its failure.
    Failed { command: String, error: String },
    /// The dialog that started it, open again with the error under its field.
    Reopen(Box<Dialog>),
    /// The control requests waiting on it, answered.
    Answer(Vec<Sender<Value>>, Value),
    /// The commands queued after it, now it ended well.
    Run(Vec<(String, Value)>),
}

/// The steps `job`, finished with `done` (`None`: its thread ended without a reply), leaves the
/// window showing `now` (`dialog_open`: whether a dialog is open there) to do, in order: its
/// session taken and what follows its success — a view chosen while it ran kept, a project it
/// opened landed, its time noted — or its failure and its dialog again; the control requests
/// waiting on it answered; the commands queued after it run.
pub fn finished(job: Job, done: Option<Done>, now: View, dialog_open: bool) -> Vec<Then> {
    let secs = job.elapsed();
    let place = job.place();
    let Job { command, params, waiting, view, dialog, then, land, .. } = job;
    let reply = done.map_or_else(|| Err(EngineError::failed("the worker stopped without an answer")), |d| d.reply.map(|r| (r, d.session)));
    let answer = match &reply {
        Ok((v, _)) => json!({"ok": true, "result": v}),
        Err(e) => json!({"ok": false, "error": e.to_string()}),
    };
    // A view chosen while the job ran stays: the job's end does not take it away.
    let chosen = view.filter(|v| *v != now).map(|_| now);
    let steps: Vec<Then> = match reply {
        Ok((reply, session)) => {
            let took = place.map_or_else(|| format!(" ({secs:.1} s)"), |w| format!(" ({secs:.1} s · {})", w.text()));
            [
                Some(Then::Session(Box::new(session))),
                Some(Then::After { command, params, reply }),
                chosen.map(Then::Keep),
                (land && chosen.is_none()).then_some(Then::Land),
                Some(Then::Note(took)),
            ]
            .into_iter()
            .flatten()
            .chain((!then.is_empty()).then_some(Then::Run(then)))
            .collect()
        }
        Err(e) => {
            let reopen = dialog.filter(|d| d.command.id == command && !dialog_open).map(|mut d| {
                d.refused(&e.to_string());
                Then::Reopen(Box::new(d))
            });
            std::iter::once(Then::Failed { command, error: e.to_string() }).chain(reopen).collect()
        }
    };
    // The waiting requests are answered before what was queued runs.
    let (before, after): (Vec<Then>, Vec<Then>) = steps.into_iter().partition(|t| !matches!(t, Then::Run(_)));
    before.into_iter().chain(std::iter::once(Then::Answer(waiting, answer))).chain(after).collect()
}

/// A duration in brief: `0.4 s`, `12 s`, `3 min`, `1.5 h`.
///
/// ```
/// use emcraft_ui_core::job::duration;
/// assert_eq!((duration(0.42), duration(12.3), duration(180.0)), ("0.4 s".into(), "12 s".into(), "3 min".into()));
/// assert_eq!(duration(f64::NAN), "–");
/// ```
pub fn duration(seconds: f64) -> String {
    match seconds {
        s if !s.is_finite() || s < 0.0 => "–".to_owned(),
        s if s < 10.0 => format!("{s:.1} s"),
        s if s < 90.0 => format!("{s:.0} s"),
        s if s < 5400.0 => format!("{:.0} min", s / 60.0),
        s => format!("{:.1} h", s / 3600.0),
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
/// use emcraft_ui_core::job::remaining;
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

    /// The job's reply, once its thread sends it (a few seconds at most).
    fn done(job: &Job) -> Option<Done> {
        (0..500).find_map(|_| match job.poll() {
            Poll::Finished(d) => Some(*d),
            Poll::Lost => None,
            Poll::Running => {
                std::thread::sleep(std::time::Duration::from_millis(10));
                None
            }
        })
    }

    fn kind(t: &Then) -> &'static str {
        match t {
            Then::Session(_) => "session",
            Then::After { .. } => "after",
            Then::Keep(_) => "keep",
            Then::Land => "land",
            Then::Note(_) => "note",
            Then::Failed { .. } => "failed",
            Then::Reopen(_) => "reopen",
            Then::Answer(..) => "answer",
            Then::Run(_) => "run",
        }
    }

    /// A job that ends well: its session and what follows, the view chosen while it ran kept,
    /// its time noted, the waiting requests answered, then what was queued after it. One that
    /// fails: its failure, its dialog again with the error, the requests answered; nothing
    /// queued runs.
    #[test]
    fn a_finished_job_leaves_its_steps_in_order() {
        let s = Session::new();
        let mut job = Job::start(&s, "units.set", serde_json::json!({"length": "m"}), None);
        let (tx, rx) = channel();
        (job.view, job.then, job.waiting) = (Some(View::Model), vec![("mesh.generate".into(), serde_json::json!({}))], vec![tx]);
        let d = done(&job);
        let steps = finished(job, d, View::Field, false);
        assert_eq!(steps.iter().map(kind).collect::<Vec<_>>(), ["session", "after", "keep", "note", "answer", "run"]);
        assert!(matches!(steps[2], Then::Keep(View::Field)));
        assert_eq!(rx.try_recv().ok(), None, "answered when the window does the step");
        let mut job = Job::start(&s, "units.set", serde_json::json!({"length": "furlong"}), None);
        let spec = emcraft_engine::find_command("units.set").unwrap_or_else(|| panic!("units.set"));
        job.dialog = Some(Dialog::new(spec, &s, &serde_json::json!({}), false));
        job.then = vec![("mesh.generate".into(), serde_json::json!({}))];
        let d = done(&job);
        let steps = finished(job, d, View::Model, false);
        assert_eq!(steps.iter().map(kind).collect::<Vec<_>>(), ["failed", "reopen", "answer"]);
        // A thread that ended without a reply is a failure too.
        let job = Job::start(&s, "units.set", serde_json::json!({}), None);
        assert_eq!(finished(job, None, View::Model, true).iter().map(kind).collect::<Vec<_>>(), ["failed", "answer"]);
    }

    use super::*;

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
