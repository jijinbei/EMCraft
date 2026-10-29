//! What follows a command's success: the cached pictures let go, the view the command calls
//! for, the view state it resets, its message, and the workflow's next step.

use emcraft_engine::view::Component;
use serde_json::Value;

use crate::{EmcraftApp, View, files, status, steps};

impl EmcraftApp {
    /// What follows a command's success: the view it calls for, its message.
    pub(crate) fn after(&mut self, id: &str, params: &Value, reply: &Value) {
        self.revision = self.revision.wrapping_add(1);
        self.failure = None;
        self.field.forget_samples();
        self.views.chart.clear();
        match id {
            "file.sample" | "engine.script" | "file.open" | "file.new" | "file.save" => self.after_file(id, reply),
            _ if id.starts_with("solve.") => self.after_solve(id, reply),
            "results.tdr" => {
                let tdr = &mut self.views.tdr;
                tdr.port = params.get("port").and_then(Value::as_str).map(str::to_owned);
                tdr.mode = params.get("mode").and_then(|m| serde_json::from_value(m.clone()).ok());
                self.view = View::Tdr;
            }
            "results.compare" => {
                self.views.compare = Some(reply.clone());
                self.view = View::Compare;
            }
            "study.sweep" | "study.optimize" => {
                self.views.study.chart.clear();
                // A new study: its grid charted along its first variable, the others at their first values.
                self.views.study.slice = emcraft_engine::study::Slice::default();
                self.view = View::Study;
            }
            "edit.undo" | "edit.redo" => self.views.port_modes = None,
            _ if id.starts_with("geometry.") => {
                self.view = View::Model;
                self.views.port_modes = None;
                self.views.compare = None;
            }
            _ => {}
        }
        self.info(status::text(id, params, reply, &self.session));
        // A run started from the Run step goes on to Results.
        if id.starts_with("solve.") && id != "solve.port_modes" && self.workflow.step == Some(steps::Step::Run) {
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
            return;
        }
        if matches!(id, "file.new" | "file.sample") {
            self.home.open = false;
        }
        // An open problem's fields read best as |E| (a source's own component may vanish on
        // the planes through it); a guide's as Ey, its TE10 field.
        self.field.view.component = if self.session.model.open.is_some() { Component::E } else { Component::Ey };
        self.views.port_modes = None;
        self.views.compare = None;
        let results = &self.session.results;
        self.view = if results.sweep.is_some() {
            View::SParameters
        } else if results.eigen.is_some() {
            View::Eigenmodes
        } else if results.radiation.is_some() {
            View::FarField
        } else if results.dispersion.is_some() {
            View::Dispersion
        } else {
            View::Model
        };
        self.pick_frequency();
        self.view3d.fit_next = true;
        self.workflow.check = None;
    }

    /// A solve: the view of its results, the field view on what it solved.
    fn after_solve(&mut self, id: &str, reply: &Value) {
        let results = &self.session.results;
        match id {
            "solve.frequency" => {
                self.view = View::SParameters;
                self.pick_frequency();
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
                self.view = if results.td_radiation.is_some() { View::FarField } else { View::SParameters };
            }
            "solve.port_modes" => {
                self.views.port_modes = Some(reply.clone());
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
        self.field.view.mode = if self.session.results.sweep.is_none() && self.session.results.eigen.is_some() { Some(0) } else { None };
        self.field.at = None;
    }
}
