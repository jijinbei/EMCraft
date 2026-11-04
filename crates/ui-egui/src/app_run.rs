//! The window running commands: one at once (a long one on the worker), scripts, the job's polling
//! and cancelling, quitting.

use super::*;

impl EmcraftApp {
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    /// Refused while a job runs: one at a time.
    pub(crate) fn idle(&self) -> Result<(), UiError> {
        match &self.job {
            Some(job) => Err(UiError::Busy(label(&job.command).to_owned())),
            None => Ok(()),
        }
    }

    /// Runs a command as a person or an agent asks for it: a quick one at once, a long one on
    /// the worker thread (the reply is then `{"started": …}` and the result comes with the job).
    /// Refused while a job runs.
    pub fn run(&mut self, id: &str, params: Value) -> Result<Value, UiError> {
        self.idle()?;
        find_command(id)?;
        if commands::is_long(id) {
            // How long it should take, from the model (cheap), for the time left while it runs.
            let asked = if matches!(id, "solve.rerun" | "solve.run") { json!({}) } else { json!({"command": id, "params": params}) };
            let estimate = self.session.execute("project.estimate", &asked).ok().and_then(|e| e.get("seconds").and_then(Value::as_f64));
            let mut job = worker::Job::start(&self.session, id, params, self.ctx.clone());
            job.view = Some(self.view);
            job.estimate = estimate;
            self.job = Some(job);
            self.info(format!("{}…", label(id)));
            return Ok(json!({"started": id}));
        }
        let r = self.session.execute(id, &params).map_err(UiError::from);
        match &r {
            Ok(reply) => self.after(id, &params, reply),
            Err(e) => self.failed(id, e),
        }
        r
    }

    /// A command's failure: kept for the panel that asked for it, and in the messages.
    pub(crate) fn failed(&mut self, id: &str, e: &impl std::fmt::Display) {
        self.failure = Some(Failure { command: id.to_owned(), error: e.to_string() });
        self.error(format!("{}: {e}", label(id)));
    }

    /// Runs a script (a JSON array of `{command, params}`) on the worker thread, as one job.
    pub fn run_script(&mut self, script: Value) -> Result<Value, UiError> {
        self.idle()?;
        let n = script.as_array().map(Vec::len).ok_or_else(|| UiError::Input("a script is a JSON array of {command, params}".into()))?;
        self.job =
            Some(worker::Job::spawn(&self.session, "engine.script", json!({}), self.ctx.clone(), move |s| s.run_script(&script).map(Value::Array)));
        self.info_with("Running a script of {n} commands…", args!(n = n));
        Ok(json!({"started": "engine.script"}))
    }

    /// The sweep's frequencies (Hz).
    pub fn frequencies(&self) -> Vec<f64> {
        self.session.results().sweep.as_ref().map(|s| s.results.iter().map(|r| r.frequency).collect()).unwrap_or_default()
    }

    /// Takes a finished job's session and reply; answers the control requests waiting on it.
    /// Runs `list` in order: quick commands at once, the rest after the job before them ends
    /// well; the first failure drops what is left.
    pub fn run_all(&mut self, list: Vec<(String, Value)>) -> Result<Value, UiError> {
        let mut rest = list.into_iter();
        let mut last = json!({});
        while let Some((id, params)) = rest.next() {
            last = self.run(&id, params)?;
            if let Some(job) = self.job.as_mut() {
                job.then = rest.collect();
                break;
            }
        }
        Ok(last)
    }

    pub fn poll_job(&mut self) {
        let Some(job) = &self.job else { return };
        let done = match job.poll() {
            worker::Poll::Running => return,
            worker::Poll::Finished(d) => Some(d),
            worker::Poll::Lost => None,
        };
        let Some(job) = self.job.take() else { return };
        let secs = job.elapsed();
        let reply = match done {
            Some(d) => {
                if d.reply.is_ok() {
                    self.session = d.session;
                }
                d.reply
            }
            None => Err(EngineError::failed("the worker stopped without an answer")),
        };
        // A view chosen while the job ran stays: the job's end does not take it away.
        let chosen = job.view.filter(|v| *v != self.view).map(|_| self.view);
        match &reply {
            Ok(v) => {
                self.after(&job.command, &job.params, v);
                if let Some(v) = chosen {
                    self.view = v;
                }
                if job.land && chosen.is_none() {
                    files::land(self, true);
                }
                if let Some(m) = self.messages.last_mut() {
                    match job.place() {
                        Some(w) => m.text.push_str(&format!(" ({secs:.1} s · {})", messages::place_text(&w))),
                        None => m.text.push_str(&format!(" ({secs:.1} s)")),
                    }
                }
            }
            Err(e) => {
                self.failed(&job.command, e);
                // The dialog that started it opens again, the error under its field.
                if let Some(mut d) = job.dialog.filter(|d| d.command.id == job.command && self.dialog.is_none()) {
                    d.refused(&e.to_string());
                    self.dialog = Some(d);
                }
            }
        }
        let answer = match reply {
            Ok(v) => json!({"ok": true, "result": v}),
            Err(e) => json!({"ok": false, "error": e.to_string()}),
        };
        let ok = answer["ok"] == json!(true);
        for w in job.waiting {
            let _ = w.send(answer.clone());
        }
        // What was queued behind it, once it ended well.
        if ok && !job.then.is_empty() {
            let _ = self.run_all(job.then);
        }
    }

    /// The window closes now: the changes were saved or let go (their autosaved copy goes),
    /// the running job stops.
    pub fn quit_now(&mut self) {
        self.cancel_job();
        self.autosave_drop();
        self.quit_requested = true;
    }

    /// The window closes without asking (`app.quit {"force": true}`): the running job stops;
    /// unsaved changes are kept in an autosaved copy, recovered on the next start.
    pub fn quit_forced(&mut self) {
        self.cancel_job();
        if self.session.is_dirty() {
            let _ = self.autosave_now();
        }
        self.quit_requested = true;
    }

    /// Stops the running job: the session stays as it was before it.
    pub fn cancel_job(&mut self) -> bool {
        let Some(job) = self.job.take() else { return false };
        job.cancel();
        self.info_with("{command} cancelled.", args!(command = label(&job.command)));

        for w in job.waiting {
            let _ = w.send(json!({"ok": false, "error": emcraft_engine::progress::CANCELLED}));
        }
        true
    }
}
