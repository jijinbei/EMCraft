//! Commands the window runs: a quick one at once, a long one (a mesh, a solve) on the worker
//! ([`emcraft_ui_core::job`]) while the window keeps drawing, polled each frame while it runs and
//! cancelled at a click. What a finished job leaves is decided by the shared rules
//! ([`job::finished`]); the window does each step in turn ([`App::follow`]).

use emcraft_ui_core::job::{self, Job, Poll, Then};
use emcraft_ui_core::lang::{args, command_label, tr, trf};
use emcraft_ui_core::view::View;
use serde_json::{Value, json};

use crate::app::App;

/// The pages the window has, in its bar's order (those with something to show are offered).
pub const PAGES: [View; 11] = [
    View::Model,
    View::PortModes,
    View::SParameters,
    View::Tdr,
    View::Pulse,
    View::Eigenmodes,
    View::FarField,
    View::Dispersion,
    View::Study,
    View::Analyses,
    View::System,
];

impl App {
    /// Runs `id` with `params`: a long command on the worker (one at a time), else at once.
    pub(crate) fn start(&mut self, id: &str, params: Value) {
        if self.job.is_some() {
            return;
        }
        if !emcraft_ui_core::commands::is_long(id) {
            return self.run_now(id, &params);
        }
        let asked = if matches!(id, "solve.rerun" | "solve.run") { json!({}) } else { json!({"command": id, "params": params}) };
        let estimate = self.session.execute("project.estimate", &asked).ok().and_then(|e| e.get("seconds").and_then(Value::as_f64));
        let mut job = Job::start(&self.session, id, params, None);
        (job.view, job.estimate) = (Some(self.page), estimate);
        self.error = None;
        self.note = None;
        self.job = Some(job);
    }

    /// Runs `id` at once on the project.
    pub(crate) fn run_now(&mut self, id: &str, params: &Value) {
        match self.session.execute(id, params) {
            Ok(reply) => {
                self.error = None;
                self.after(id, params, &reply);
            }
            Err(e) => self.error = Some(format!("{}: {e}", command_label(id))),
        }
    }

    /// What follows a command's success (`id` with `params`, its `reply`): the replies the views
    /// show kept or let go (the shared table, `Replies::after`), what is drawn from the project
    /// drawn again, the chain read again, and after a solve its results' page.
    pub(crate) fn after(&mut self, id: &str, params: &Value, reply: &Value) {
        self.replies = std::mem::take(&mut self.replies).after(id, params, reply, &self.session);
        self.generation += 1;
        self.read();
        if job::is_solve(id) {
            let results = emcraft_ui_core::after::results_view(&self.session);
            self.page = if PAGES.contains(&results) { results } else { self.page };
        }
    }

    /// A frame at `now`: the field's phase moved on while it plays, the running job looked at.
    pub(crate) fn tick(&mut self, now: std::time::Instant) {
        if let Some(last) = self.playing {
            let dt = last.map_or(0.0, |l| now.saturating_duration_since(l).as_secs_f64());
            let phase = self.view3d.field.phase + std::f64::consts::TAU * dt / crate::view3d::field::PERIOD;
            self.view3d = std::mem::take(&mut self.view3d).navigated(crate::view3d::Nav::Phase(phase));
            self.playing = Some(Some(now));
        }
        self.poll();
    }

    /// The running job looked at: its steps done once it ends.
    fn poll(&mut self) {
        let done = match self.job.as_ref().map(Job::poll) {
            None | Some(Poll::Running) => return,
            Some(Poll::Finished(done)) => Some(*done),
            Some(Poll::Lost) => None,
        };
        let Some(job) = self.job.take() else { return };
        for step in job::finished(job, done, self.page, self.dialog.is_some()) {
            self.follow(step);
        }
    }

    /// Does one step a finished job left.
    fn follow(&mut self, step: Then) {
        match step {
            Then::Session(s) => self.session = *s,
            Then::After { command, params, reply } => {
                self.note = Some(command_label(&command).to_owned());
                self.after(&command, &params, &reply);
            }
            Then::Keep(v) => self.page = v,
            Then::Note(took) => self.note = self.note.take().map(|n| n + &took),
            Then::Failed { command, error } => {
                self.note = None;
                self.error = Some(format!("{}: {error}", command_label(&command)));
            }
            Then::Answer(waiting, answer) => waiting.into_iter().for_each(|w| drop(w.send(answer.clone()))),
            Then::Run(list) => self.queue(list),
            Then::Reopen(d) => self.dialog = Some(*d),
            // The window opens no projects yet.
            Then::Land => {}
        }
    }

    /// Runs `list` in order: quick commands at once, the rest after the job before them ends.
    fn queue(&mut self, list: Vec<(String, Value)>) {
        let mut rest = list.into_iter();
        while let Some((id, params)) = rest.next() {
            self.start(&id, params);
            if let Some(job) = self.job.as_mut() {
                job.then = rest.collect();
                return;
            }
        }
    }

    /// Stops the running job: the project stays as it was before it.
    pub(crate) fn cancel(&mut self) {
        if let Some(job) = self.job.take() {
            job.cancel();
            self.note = Some(trf(t!("{command} cancelled."), args!(command = command_label(&job.command))));
        }
    }

    /// The running job as the bar says it ([`Job::text`]).
    pub fn progress(&self) -> Option<String> {
        self.job.as_ref().map(Job::text)
    }
}

/// A page's name in the bar.
pub fn page_label(v: View) -> &'static str {
    emcraft_ui_core::lang::lookup(v.label())
}

/// "Cancel", for the bar.
pub fn cancel_label() -> &'static str {
    tr(t!("Cancel"))
}
