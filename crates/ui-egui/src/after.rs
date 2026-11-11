//! What follows a command's success: the cached pictures let go, the view the command calls
//! for, the view state it resets, its message, and the workflow's next step.

use crate::view::ViewExt;
use emcraft_engine::view::{Component, Plane};
use serde_json::Value;

use crate::{EmcraftApp, View, files, status, steps};

/// The view of a session's results: the chosen kind of analysis' own first (a periodic cell's
/// dispersion for eigenmodes), else the first result of the model it holds; the model when
/// there is none.
pub(crate) fn results_view(session: &emcraft_engine::Session) -> View {
    let results = session.results();
    // A periodic cell's project with a pulse of its own (the pulse compressor): its pulse.
    if session.views().pulse.is_some() && results.bloch.is_some() {
        return View::Pulse;
    }
    // A system's own view holds its chain as well as its budget.
    if session.model().analysis() == emcraft_model::Analysis::System {
        return View::System;
    }
    let chosen = emcraft_engine::workflow::spec(session.model().analysis()).results.iter().copied().find(|k| results.has(*k));
    chosen.or_else(|| results.kinds().find(|k| k.of_the_model())).map_or(View::Model, View::of)
}

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
                self.place = self.place.show(View::Pcb);
                self.ui_state.page.leave_start();
            }
            "pcb.inspect" => {
                let report: Option<emcraft_engine::pcb::Inspection> = serde_json::from_value(reply.clone()).ok();
                if let Some(r) = &report {
                    self.views.pcb.ground = r.ground.clone();
                    self.views.pcb.reference = r.reference.to_string();
                    self.views.pcb.stop = r.stop.to_string();
                }
                self.replies.pcb = report;
                self.place = self.place.show(View::Pcb);
            }
            "project.variable" if self.session.model().pcb().is_some() && self.session.model().analysis() == crate::steps::Solver::Pcb => {
                self.replies.pcb = None;
                self.place = self.place.show(View::Pcb);
            }
            "pcb.load" | "pcb.import" | "pcb.sample" => {
                self.replies.pcb = None;
                self.place = self.place.show(View::Pcb);
                self.ui_state.page.leave_start();
            }
            "file.sample" | "engine.script" | "file.open" | "file.new" | "file.save" => self.after_file(id, reply),
            _ if id.starts_with("solve.") => self.after_solve(id, reply),
            "results.pulse" => {
                self.views.pulse.params = crate::state::PulseParams::of(params);
                self.views.pulse.runs += 1;
                self.replies.pulse = Some(reply.clone());
                self.place = self.place.show(View::Pulse);
            }
            id if crate::state::ANALYSES.contains(&id) => {
                let run = self.replies.analysis.as_ref().map_or(0, |a| a.run + 1);
                self.replies.analysis = Some(crate::state::Analysis { command: id.to_owned(), reply: reply.clone(), run });
                self.place = self.place.show(View::Analyses);
            }
            "results.tdr" => {
                self.views.tdr.params = crate::state::TdrParams::of(params);
                self.place = self.place.show(View::Tdr);
            }
            // A measurement drawn over the S-parameter chart (a system's: its S-parameter stages).
            "results.import" | "results.import.delete" if self.session.model().analysis() == crate::steps::Solver::System => {
                self.place = self.place.show(View::System);
            }
            "results.import" | "results.import.delete" | "results.equivalent" | "circuit.cascade" => self.place = self.place.show(View::SParameters),
            id if id.starts_with("system.") => self.place = self.place.show(View::System),
            "filter.synthesize" | "results.coupling" => {
                self.replies.coupling = Some(reply.clone());
                self.place = self.place.show(View::SParameters);
            }
            "results.compare" => {
                self.replies.compare = serde_json::from_value(reply.clone()).ok();
                self.place = self.place.show(View::Compare);
            }
            "study.sweep" | "study.optimize" => {
                self.views.study.chart.clear();
                // A new study: its grid charted along its first variable, the others at their first values.
                self.views.study.slice = emcraft_engine::study::Slice::default();
                self.place = self.place.show(View::Study);
            }
            "edit.undo" | "edit.redo" => {
                self.replies.port_modes = None;
                self.replies.pcb = None;
                if self.session.model().pcb().is_some() {
                    self.place = self.place.show(View::Pcb);
                } else if self.place.view() == View::Pcb {
                    self.place = self.place.show(View::Model);
                }
            }
            "project.analysis" => {
                // Away from the PCB diagnosis its stages go; the board is the model's. A
                // system's chain is its own view's; away from it, the model's.
                if self.place.view() == View::Pcb && !View::Pcb.available(self) {
                    self.place = self.place.show(View::Model);
                }
                if self.session.model().analysis() == crate::steps::Solver::System {
                    self.place = self.place.show(View::System);
                } else if self.place.view() == View::System && !View::System.available(self) {
                    self.place = self.place.show(View::Model);
                }
            }
            _ if id.starts_with("geometry.") => {
                self.place = self.place.show(View::Model);
                self.replies.port_modes = None;
                self.replies.compare = None;
            }
            _ => {}
        }
        self.info(status::text(id, params, reply, &self.session));
        // A run started from the Run step goes on to Results.
        if steps::is_solve(id) && self.place.step() == Some(steps::Step::Run) {
            steps::open_step(self, Some(steps::Step::Results));
        }
    }

    /// A project or sample opened, a script run, a new or saved project.
    fn after_file(&mut self, id: &str, reply: &Value) {
        // Another project: its own pulse, sent again when its view opens.
        if id != "file.save" {
            self.replies.pulse = None;
            self.replies.coupling = None;
            self.views.pulse = crate::state::PulseState::default();
        }
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
            self.ui_state.page.leave_start();
        }
        // The Setup step's form: the opened project's confirmed settings, else empty.
        if matches!(id, "file.open" | "file.sample" | "file.new") {
            self.workflow.fields = steps::Fields::default();
            steps::sync_setup(self);
        }
        // An open problem's fields read best as |E| (a source's own component may vanish on
        // the planes through it); a guide's as Ey, its TE10 field.
        self.field.view.component = if self.session.model().domain.open.is_some() { Component::E } else { Component::Ey };
        self.replies.port_modes = None;
        self.replies.compare = None;
        self.place = self.place.show(results_view(&self.session));
        self.replies.pcb = None;
        if self.session.model().pcb().is_some() && self.session.model().analysis() == crate::steps::Solver::Pcb {
            self.place = self.place.show(View::Pcb);
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
                self.place = self.place.show(View::SParameters);
                self.pick_frequency();
                // A board's route: the field on the plane halfway up the substrate (the model's
                // y is the board's height), where the line's E stands between trace and
                // reference.
                if let Some(board) = self.session.model().pcb()
                    && let Some(r) = board.region
                {
                    let thickness = board.heights().into_iter().fold(0.0, f64::max);
                    self.field.view.plane = Plane::H;
                    self.field.view.component = Component::Ey;
                    self.field.at = Some(self.session.display().length.from_si(r.below + 0.5 * thickness));
                }
            }
            "solve.dispersion" => self.place = self.place.show(View::Dispersion),
            "solve.eigenmodes" => {
                self.place = self.place.show(View::Eigenmodes);
                self.field.view.mode = Some(0);
            }
            "solve.radiation" => {
                self.place = self.place.show(View::FarField);
                self.field.view.mode = None;
                if self.field.view.component == Component::Ey {
                    self.field.view.component = Component::E;
                }
                if let Some(r) = &results.radiation {
                    self.field.view.frequency = r.result.frequency;
                }
                // The pattern's cut through the strongest direction (a patch's broadside lies
                // off its own plane, which φ = 0 cuts along).
                if let Some(phi) = emcraft_engine::open::strongest_azimuth(&self.session) {
                    self.views.pattern.angle = phi;
                }
            }
            "solve.time_domain" => {
                self.place = self.place.show(match &results.td_radiation {
                    Some(_) => View::FarField,
                    None => View::SParameters,
                });
            }
            "solve.port_modes" => {
                self.replies.port_modes = serde_json::from_value(reply.clone()).ok();
                // The Excitation step tabulates them in its panel, beside the 3D view.
                if self.place.step() != Some(steps::Step::Excitation) {
                    self.place = self.place.show(View::PortModes);
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
