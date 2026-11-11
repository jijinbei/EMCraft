//! What follows a command's success in this window: the shared rules
//! ([`emcraft_ui_core::after`], [`emcraft_ui_core::replies`]) say which view to show and which
//! replies to keep; here the window's own state follows — its cached pictures let go, the view
//! settings a command resets, its message, and the workflow's next step.

use emcraft_engine::view::{Component, Plane};
use emcraft_ui_core::after;
use serde_json::Value;

use crate::{EmcraftApp, files, status, steps};

pub(crate) use emcraft_ui_core::after::results_view;

impl EmcraftApp {
    /// What follows a command's success: the replies kept, the view it calls for, the window's
    /// own settings, its message.
    pub(crate) fn after(&mut self, id: &str, params: &Value, reply: &Value) {
        let (id, params) = after::ran(id, params, reply);
        // Commands that change the machine's or the window's state, not the model.
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
        self.replies = std::mem::take(&mut self.replies).after(id, params, reply, &self.session);
        if let Some(v) = after::next_view(id, &self.session, &self.replies, self.place.view(), self.place.step()) {
            self.place = self.place.show(v);
        }
        if after::leaves_start(id) {
            self.ui_state.page.leave_start();
        }
        self.settings_after(id, params, reply);
        self.info(status::text(id, params, reply, &self.session));
        // A run started from the Run step goes on to Results.
        if steps::is_solve(id) && self.place.step() == Some(steps::Step::Run) {
            steps::open_step(self, Some(steps::Step::Results));
        }
    }

    /// The window's own settings a command resets: the PCB form from an inspection, a pulse's
    /// or a TDR's settings, a new study's slice, a project's or a solve's field view.
    fn settings_after(&mut self, id: &str, params: &Value, reply: &Value) {
        match id {
            "pcb.inspect" => {
                if let Some(r) = &self.replies.pcb {
                    self.views.pcb.ground = r.ground.clone();
                    self.views.pcb.reference = r.reference.to_string();
                    self.views.pcb.stop = r.stop.to_string();
                }
            }
            "file.sample" | "engine.script" | "file.open" | "file.new" | "file.save" => self.after_file(id, reply),
            _ if id.starts_with("solve.") => self.after_solve(id),
            "results.pulse" => {
                self.views.pulse.params = crate::state::PulseParams::of(params);
                self.views.pulse.runs += 1;
            }
            "results.tdr" => self.views.tdr.params = crate::state::TdrParams::of(params),
            "study.sweep" | "study.optimize" => {
                self.views.study.chart.clear();
                // A new study: its grid charted along its first variable, the others at their first values.
                self.views.study.slice = emcraft_engine::study::Slice::default();
            }
            _ => {}
        }
    }

    /// A project or sample opened, a script run, a new or saved project.
    fn after_file(&mut self, id: &str, reply: &Value) {
        // Another project: its own pulse, sent again when its view opens.
        if id != "file.save" {
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
        // The Setup step's form: the opened project's confirmed settings, else empty.
        if matches!(id, "file.open" | "file.sample" | "file.new") {
            self.workflow.fields = steps::Fields::default();
            steps::sync_setup(self);
        }
        // An open problem's fields read best as |E| (a source's own component may vanish on
        // the planes through it); a guide's as Ey, its TE10 field.
        self.field.view.component = if self.session.model().domain.open.is_some() { Component::E } else { Component::Ey };
        self.pick_frequency();
        self.view3d.fit_next = true;
        self.workflow.check = None;
        self.workflow.refusal = None;
    }

    /// A solve: the field view on what it solved.
    fn after_solve(&mut self, id: &str) {
        match id {
            "solve.frequency" => {
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
            "solve.eigenmodes" => self.field.view.mode = Some(0),
            "solve.radiation" => {
                self.field.view.mode = None;
                if self.field.view.component == Component::Ey {
                    self.field.view.component = Component::E;
                }
                if let Some(r) = &self.session.results().radiation {
                    self.field.view.frequency = r.result.frequency;
                }
                // The pattern's cut through the strongest direction (a patch's broadside lies
                // off its own plane, which φ = 0 cuts along).
                if let Some(phi) = emcraft_engine::open::strongest_azimuth(&self.session) {
                    self.views.pattern.angle = phi;
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
