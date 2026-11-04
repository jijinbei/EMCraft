//! What follows a command's success: the cached pictures let go, the view the command calls
//! for, the view state it resets, its message, and the workflow's next step.

use emcraft_engine::view::{Component, Plane};
use serde_json::Value;

use crate::{EmcraftApp, View, files, status, steps};

impl EmcraftApp {
    /// What follows a command's success: the view it calls for, its message.
    pub(crate) fn after(&mut self, id: &str, params: &Value, reply: &Value) {
        // A run again, or the confirmed settings' run, is the solve it runs.
        let rerun = match id {
            "solve.rerun" => reply.get("rerun"),
            "solve.run" => reply.get("run"),
            _ => None,
        };
        let (id, params) = match rerun.and_then(|r| Some((r.get("command")?.as_str()?, r.get("params")?))) {
            Some((command, p)) => (command, p),
            None => (id, params),
        };
        // Commands that transition machine/UI state without changing the model.
        match id {
            "geometry.measure" => {
                self.info(status::text(id, params, reply, &self.session));
                return;
            }
            "solve.device" => self.workflow.device_selection = steps::DeviceSelection::Explicit,
            _ => {}
        }
        self.revision = self.revision.wrapping_add(1);
        self.failure = None;
        // How results are looked at changes no result: the field's samples stay.
        if !id.starts_with("view.probe.") && id != "view.chart.style" {
            self.field.forget_samples();
        }
        self.views.chart.clear();
        // The report reads in the old units: made again in the new ones.
        if id == "units.set" {
            self.replies.pcb = None;
        }
        match id {
            "view.pcb" => {
                if params.get("net").is_some() || params.get("victim").is_some() {
                    self.replies.pcb = None;
                }
                self.view = View::Pcb;
                self.workflow.step = None;
                self.page = self.page.leave_home();
            }
            "pcb.inspect" => {
                let report: Option<emcraft_engine::pcb::Inspection> = serde_json::from_value(reply.clone()).ok();
                if let Some(r) = &report {
                    self.views.pcb.ground = r.ground.clone();
                    self.views.pcb.reference = r.reference.to_string();
                    self.views.pcb.stop = r.stop.to_string();
                }
                self.replies.pcb = report;
                self.view = View::Pcb;
                self.workflow.step = None;
            }
            "project.variable" if self.session.model().pcb.is_some() && self.session.model().analysis() == crate::steps::Solver::Pcb => {
                self.replies.pcb = None;
                self.view = View::Pcb;
                self.workflow.step = None;
            }
            "pcb.load" | "pcb.import" | "pcb.sample" => {
                self.replies.pcb = None;
                self.view = View::Pcb;
                self.workflow.step = None;
                self.page = self.page.leave_home();
            }
            "file.sample" | "engine.script" | "file.open" | "file.new" | "file.save" => self.after_file(id, reply),
            _ if id.starts_with("solve.") => self.after_solve(id, reply),
            "results.tdr" => {
                self.views.tdr.params = crate::state::TdrParams::of(params);
                self.view = View::Tdr;
            }
            "results.compare" => {
                self.replies.compare = serde_json::from_value(reply.clone()).ok();
                self.view = View::Compare;
            }
            "study.sweep" | "study.optimize" => {
                self.views.study.chart.clear();
                // A new study: its grid charted along its first variable, the others at their first values.
                self.views.study.slice = emcraft_engine::study::Slice::default();
                self.view = View::Study;
            }
            "edit.undo" | "edit.redo" => {
                self.replies.port_modes = None;
                self.replies.pcb = None;
                if self.session.model().pcb.is_some() {
                    self.view = View::Pcb;
                    self.workflow.step = None;
                } else if self.view == View::Pcb {
                    self.view = View::Model;
                }
            }
            "project.analysis" => {
                // Away from the PCB diagnosis its stages go; the board is the model's.
                if self.view == View::Pcb && !View::Pcb.available(self) {
                    self.view = View::Model;
                }
            }
            _ if id.starts_with("geometry.") => {
                self.view = View::Model;
                self.replies.port_modes = None;
                self.replies.compare = None;
            }
            _ => {}
        }
        self.info(status::text(id, params, reply, &self.session));
        // A run started from the Run step goes on to Results.
        if steps::is_solve(id) && self.workflow.step == Some(steps::Step::Run) {
            steps::open_step(self, Some(steps::Step::Results));
        }
    }

    /// A project or sample opened, a script run, a new or saved project.
    fn after_file(&mut self, id: &str, reply: &Value) {
        if matches!(id, "file.open" | "file.save")
            && let Some(path) = reply.get("path").and_then(Value::as_str)
        {
            files::opened(self, path);
        }
        if id == "file.save" {
            // A real save (not an autosaved copy): the copy goes.
            if reply.get("copy").and_then(Value::as_bool) != Some(true) {
                self.autosave_drop();
            }
            return;
        }
        if matches!(id, "file.new" | "file.sample") {
            self.page = crate::home::Page::Workspace;
        }
        // The Setup step's form: the opened project's confirmed settings, else empty.
        if matches!(id, "file.open" | "file.sample" | "file.new") {
            self.workflow.fields = steps::Fields::default();
            steps::sync_setup(self);
        }
        // An open problem's fields read best as |E| (a source's own component may vanish on
        // the planes through it); a guide's as Ey, its TE10 field.
        self.field.view.component = if self.session.model().open.is_some() { Component::E } else { Component::Ey };
        self.replies.port_modes = None;
        self.replies.compare = None;
        // The chosen kind's own result first, else the first the session holds.
        let results = self.session.results();
        let chosen = emcraft_engine::workflow::spec(self.session.model().analysis()).results.iter().copied().find(|k| results.has(*k));
        self.view = chosen.or_else(|| results.kinds().find(|k| k.of_the_model())).map_or(View::Model, View::of);
        self.replies.pcb = None;
        if self.session.model().pcb.is_some() && self.session.model().analysis() == crate::steps::Solver::Pcb {
            self.view = View::Pcb;
            self.workflow.step = None;
        }
        self.pick_frequency();
        self.view3d.fit_next = true;
        self.workflow.check = None;
        self.workflow.refusal = None;
    }

    /// A solve: the view of its results, the field view on what it solved.
    fn after_solve(&mut self, id: &str, reply: &Value) {
        let results = self.session.results();
        match id {
            "solve.frequency" => {
                self.view = View::SParameters;
                self.pick_frequency();
                // A board's route: the field on the plane halfway up the substrate (the model's
                // y is the board's height), where the line's E stands between trace and
                // reference.
                if let Some(board) = self.session.model().pcb.as_ref()
                    && let Some(r) = board.region
                {
                    let thickness = board.heights().into_iter().fold(0.0, f64::max);
                    self.field.view.plane = Plane::H;
                    self.field.view.component = Component::Ey;
                    self.field.at = Some(self.session.display().length.from_si(r.below + 0.5 * thickness));
                }
            }
            "solve.dispersion" => self.view = View::Dispersion,
            "solve.eigenmodes" => {
                self.view = View::Eigenmodes;
                self.field.view.mode = Some(0);
            }
            "solve.radiation" => {
                self.view = View::FarField;
                self.field.view.mode = None;
                if self.field.view.component == Component::Ey {
                    self.field.view.component = Component::E;
                }
                if let Some(r) = &results.radiation {
                    self.field.view.frequency = r.result.frequency;
                }
            }
            "solve.time_domain" => {
                self.view = match &results.td_radiation {
                    Some(_) => View::FarField,
                    None => View::SParameters,
                };
            }
            "solve.port_modes" => {
                self.replies.port_modes = Some(reply.clone());
                // The Excitation step tabulates them in its panel, beside the 3D view.
                if self.workflow.step != Some(steps::Step::Excitation) {
                    self.view = View::PortModes;
                }
            }
            _ => {}
        }
    }

    /// Puts the field view on a solved frequency.
    pub(crate) fn pick_frequency(&mut self) {
        let freqs = self.frequencies();
        if let Some(f) = crate::nearest(&freqs, self.field.view.frequency) {
            self.field.view.frequency = f;
        }
        self.field.view.mode = match (&self.session.results().sweep, &self.session.results().eigen) {
            (None, Some(_)) => Some(0),
            _ => None,
        };
        self.field.at = None;
    }
}
