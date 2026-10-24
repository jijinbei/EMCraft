//! EMCraft's desktop UI. The left panel holds the model (a rectangular waveguide in sections,
//! its mesh, ports and sweep) and the view (frequency, plane, component, arrows, phase); the
//! centre shows the fields on the chosen plane above the S-parameter chart. Solving runs on a
//! background thread through the same engine commands as scripts; pictures are drawn by
//! `emcraft-render` (the same as `emcraft-cli`'s) and shown as textures.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::sync::mpsc::{Receiver, channel};
use std::time::Instant;

use emcraft_engine::Session;
use emcraft_engine::view::{self, FieldView, Plane, PlaneSamples};
use emcraft_model::units::{Frequency::Gigahertz as GHZ, Length::Millimetre as MM};
use serde_json::{Value, json};

/// One section of the waveguide being edited: length (mm), relative permittivity and, for an
/// iris or a step, the width of its centred aperture (mm).
#[derive(Clone, Debug, PartialEq)]
pub struct SectionEdit {
    pub length: f64,
    pub eps_r: f64,
    pub aperture: Option<f64>,
    /// Dielectric loss tangent.
    pub tan_d: f64,
}

impl SectionEdit {
    fn new(length: f64, eps_r: f64) -> SectionEdit {
        SectionEdit { length, eps_r, aperture: None, tan_d: 0.0 }
    }

    fn iris(length: f64, aperture: f64) -> SectionEdit {
        SectionEdit { length, eps_r: 1.0, aperture: Some(aperture), tan_d: 0.0 }
    }
}

/// The model as the left panel edits it, in millimetres and gigahertz.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelEdit {
    pub width: f64,
    pub height: f64,
    pub sections: Vec<SectionEdit>,
    pub size: f64,
    /// Element size at iris and step edges, as a fraction of `size`.
    pub edge: f64,
    /// One element layer across the narrow wall (exact for H-plane devices).
    pub one_layer_y: bool,
    pub order: u8,
    pub modes: u32,
    pub start: f64,
    pub stop: f64,
    pub points: u32,
    /// Wall conductivity (S/m); perfect conductors when `None`.
    pub conductivity: Option<f64>,
    /// The analysis to run.
    pub analysis: Analysis,
}

/// What Solve runs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Analysis {
    /// A frequency sweep with ports at both ends.
    Sweep,
    /// The eigenmodes of the guide closed into a cavity: `count` modes near `near` GHz.
    Eigen { count: u32, near: f64 },
}

impl Default for ModelEdit {
    /// The high-pass filter sample.
    fn default() -> Self {
        ModelEdit {
            width: 22.86,
            height: 10.16,
            sections: vec![SectionEdit::new(15.0, 2.0), SectionEdit::new(20.0, 1.0), SectionEdit::new(15.0, 2.0)],
            size: 3.0,
            edge: 0.25,
            one_layer_y: false,
            order: 2,
            modes: 4,
            start: 5.0,
            stop: 9.0,
            points: 17,
            conductivity: None,
            analysis: Analysis::Sweep,
        }
    }
}

impl ModelEdit {
    /// The inductive-iris band-pass filter sample.
    pub fn bandpass() -> ModelEdit {
        let cavity = |l: f64| SectionEdit::new(l, 1.0);
        ModelEdit {
            sections: vec![
                cavity(15.0),
                SectionEdit::iris(2.0, 12.75),
                cavity(15.0),
                SectionEdit::iris(2.0, 9.25),
                cavity(16.75),
                SectionEdit::iris(2.0, 9.25),
                cavity(15.0),
                SectionEdit::iris(2.0, 12.75),
                cavity(15.0),
            ],
            size: 3.0,
            edge: 0.1,
            one_layer_y: true,
            start: 9.0,
            stop: 11.0,
            points: 21,
            ..ModelEdit::default()
        }
    }

    /// The copper cavity sample: its four lowest modes.
    pub fn cavity() -> ModelEdit {
        ModelEdit {
            sections: vec![SectionEdit::new(30.0, 1.0)],
            conductivity: Some(5.8e7),
            analysis: Analysis::Eigen { count: 4, near: 12.0 },
            ..ModelEdit::default()
        }
    }

    /// The command script that builds and solves this model.
    pub fn script(&self) -> Value {
        let mut steps = Vec::new();
        let mut names = Vec::new();
        for s in &self.sections {
            if s.eps_r == 1.0 && s.tan_d == 0.0 {
                names.push("vacuum".to_owned());
                continue;
            }
            let name = if s.tan_d == 0.0 { format!("eps {}", s.eps_r) } else { format!("eps {} tan {}", s.eps_r, s.tan_d) };
            if !names.contains(&name) {
                steps.push(json!({"command": "model.material.create", "params": {"name": name, "eps_r": s.eps_r, "loss_tangent": s.tan_d}}));
            }
            names.push(name);
        }
        let sections: Vec<Value> = self
            .sections
            .iter()
            .zip(&names)
            .map(|(s, n)| match s.aperture {
                Some(w) => json!({"length": s.length, "material": n, "width": w}),
                None => json!({"length": s.length, "material": n}),
            })
            .collect();
        steps.push(json!({"command": "geometry.waveguide", "params": {"width": self.width, "height": self.height, "sections": sections}}));
        if let Some(sigma) = self.conductivity {
            steps.push(json!({"command": "model.walls", "params": {"conductivity": sigma}}));
        }
        let mut mesh = json!({"size": self.size, "edge": self.edge});
        if self.one_layer_y {
            mesh["size_y"] = json!(self.height);
        }
        steps.push(json!({"command": "mesh.generate", "params": mesh}));
        match self.analysis {
            Analysis::Sweep => {
                steps.push(json!({"command": "model.port.create", "params": {"name": "1", "end": "start", "modes": self.modes}}));
                steps.push(json!({"command": "model.port.create", "params": {"name": "2", "end": "end", "modes": self.modes}}));
                steps.push(json!({"command": "solve.frequency", "params": {"start": self.start, "stop": self.stop, "points": self.points, "order": self.order}}));
            }
            Analysis::Eigen { count, near } => {
                steps.push(json!({"command": "solve.eigenmodes", "params": {"near": near, "count": count, "order": self.order}}));
            }
        }
        Value::Array(steps)
    }
}

/// Maps a click at fraction `fx` across the S-parameter chart image to a frequency (display
/// units), with the chart's layout (`emcraft_render::chart::draw`: plot from x = 90 to w − 30).
pub fn chart_frequency(fx: f64, width_px: f64, f0: f64, f1: f64) -> Option<f64> {
    let (x0, pw) = (90.0, width_px - 120.0);
    let t = (fx * width_px - x0) / pw;
    (pw > 0.0 && (0.0..=1.0).contains(&t)).then_some(f0 + t * (f1 - f0))
}

/// The solved frequency (Hz) nearest to `f` (Hz).
pub fn nearest(freqs: &[f64], f: f64) -> Option<f64> {
    freqs.iter().copied().min_by(|a, b| (a - f).abs().total_cmp(&(b - f).abs()))
}

pub struct EmcraftApp {
    session: Session,
    edit: ModelEdit,
    view: FieldView,
    /// The plane position as the slider holds it (mm); `None` = the middle.
    at_mm: Option<f64>,
    samples: Option<PlaneSamples>,
    field: Option<(egui::TextureHandle, [usize; 2])>,
    chart: Option<(egui::TextureHandle, [usize; 2])>,
    field_key: Option<String>,
    chart_width: usize,
    animate: bool,
    last_frame: Option<Instant>,
    solving: Option<(Receiver<Result<Session, String>>, Instant)>,
    status: String,
}

impl EmcraftApp {
    /// The app with a sample's model (`highpass` or `iris-bandpass`), solved at once.
    pub fn new(sample: Option<&str>) -> EmcraftApp {
        let mut app = EmcraftApp {
            session: Session::new(),
            edit: ModelEdit::default(),
            view: FieldView::new(8.0e9, Plane::H),
            at_mm: None,
            samples: None,
            field: None,
            chart: None,
            field_key: None,
            chart_width: 0,
            animate: false,
            last_frame: None,
            solving: None,
            status: "Edit the waveguide and press Solve.".into(),
        };
        if let Some(name) = sample {
            if name == "iris-bandpass" {
                app.edit = ModelEdit::bandpass();
                app.view.frequency = 10.0e9;
            }
            if name == "cavity" {
                app.edit = ModelEdit::cavity();
            }
            app.solve();
        }
        app
    }

    fn solve(&mut self) {
        let script = self.edit.script();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let mut s = Session::new();
            let r = s.run_script(&script).map(|_| s).map_err(|e| e.to_string());
            let _ = tx.send(r);
        });
        self.solving = Some((rx, Instant::now()));
        self.status = "Solving…".into();
    }

    fn poll(&mut self) {
        let Some((rx, started)) = &self.solving else { return };
        match rx.try_recv() {
            Ok(Ok(s)) => {
                let secs = started.elapsed().as_secs_f64();
                self.session = s;
                if let Some(e) = self.session.eigen.as_ref() {
                    self.view.mode = Some(0);
                    let list: Vec<String> = e.modes.iter().map(|m| format!("{:.4}", GHZ.from_si(m.frequency))).collect();
                    self.status = format!("Found {} eigenmodes in {secs:.1} s: {} GHz.", e.modes.len(), list.join(", "));
                    self.samples = None;
                    self.field_key = None;
                    self.chart = None;
                    self.solving = None;
                    return;
                }
                self.view.mode = None;
                let freqs = self.frequencies();
                if let Some(f) = nearest(&freqs, self.view.frequency) {
                    self.view.frequency = f;
                }
                let unknowns = self.session.sweep.as_ref().map_or(0, |sw| sw.driven.dofs.unknowns);
                let worst = self.session.execute("results.compare", &json!({})).ok().and_then(|c| c["worst"].as_f64());
                self.status = format!(
                    "Solved {} frequencies, {unknowns} unknowns, in {secs:.1} s. Worst |S − S_exact| = {}.",
                    freqs.len(),
                    worst.map_or("–".into(), |w| format!("{w:.2e}"))
                );
                self.samples = None;
                self.field_key = None;
                self.chart = None;
                self.solving = None;
            }
            Ok(Err(e)) => {
                self.status = format!("Solve failed: {e}");
                self.solving = None;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.status = "Solve failed: the solver stopped".into();
                self.solving = None;
            }
        }
    }

    fn frequencies(&self) -> Vec<f64> {
        self.session.sweep.as_ref().map(|s| s.results.iter().map(|r| r.frequency).collect()).unwrap_or_default()
    }

    fn controls(&mut self, ui: &mut egui::Ui) {
        ui.heading("Waveguide");
        let e = &mut self.edit;
        egui::Grid::new("guide").num_columns(2).show(ui, |ui| {
            ui.label("width a");
            ui.add(egui::DragValue::new(&mut e.width).speed(0.1).range(1.0..=500.0).suffix(" mm"));
            ui.end_row();
            ui.label("height b");
            ui.add(egui::DragValue::new(&mut e.height).speed(0.1).range(0.5..=500.0).suffix(" mm"));
            ui.end_row();
        });
        ui.label("Sections along z (length, εr, aperture):");
        let mut remove = None;
        for (i, s) in e.sections.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.label(format!("{}", i + 1));
                ui.add(egui::DragValue::new(&mut s.length).speed(0.1).range(0.5..=1000.0).suffix(" mm"));
                ui.add(egui::DragValue::new(&mut s.eps_r).speed(0.01).range(1.0..=100.0).prefix("εr "));
                ui.add(egui::DragValue::new(&mut s.tan_d).speed(0.0005).range(0.0..=1.0).prefix("tanδ ")).on_hover_text("dielectric loss tangent");
                let mut narrowed = s.aperture.is_some();
                if ui.checkbox(&mut narrowed, "").on_hover_text("narrow the broad wall to a centred aperture (iris, step)").changed() {
                    s.aperture = narrowed.then_some(s.aperture.unwrap_or(e.width / 2.0));
                }
                if let Some(w) = s.aperture.as_mut() {
                    ui.add(egui::DragValue::new(w).speed(0.05).range(0.5..=e.width).suffix(" mm"));
                }
                if ui.small_button("✕").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove
            && e.sections.len() > 1
        {
            e.sections.remove(i);
        }
        if ui.button("Add section").clicked() {
            e.sections.push(SectionEdit::new(10.0, 1.0));
        }
        ui.horizontal(|ui| {
            ui.label("walls");
            let name = |c: Option<f64>| match c {
                None => "perfect conductor".to_owned(),
                Some(s) => format!("σ = {s:.2e} S/m"),
            };
            egui::ComboBox::from_id_salt("walls").selected_text(name(e.conductivity)).show_ui(ui, |ui| {
                for (label, c) in [("perfect conductor", None), ("copper", Some(5.8e7)), ("aluminium", Some(3.8e7)), ("brass", Some(1.5e7))] {
                    ui.selectable_value(&mut e.conductivity, c, label);
                }
            });
        });
        ui.separator();
        ui.heading("Mesh and analysis");
        egui::Grid::new("solve").num_columns(2).show(ui, |ui| {
            ui.label("element size");
            ui.add(egui::DragValue::new(&mut e.size).speed(0.05).range(0.5..=50.0).suffix(" mm"));
            ui.end_row();
            ui.label("at iris edges");
            ui.add(egui::DragValue::new(&mut e.edge).speed(0.01).range(0.05..=1.0).prefix("× "));
            ui.end_row();
            ui.label("");
            ui.checkbox(&mut e.one_layer_y, "one layer across b (H-plane)");
            ui.end_row();
            ui.label("element order");
            ui.horizontal(|ui| {
                ui.radio_value(&mut e.order, 1, "1");
                ui.radio_value(&mut e.order, 2, "2");
            });
            ui.end_row();
            ui.label("port modes");
            ui.add(egui::DragValue::new(&mut e.modes).range(1..=32));
            ui.end_row();
            ui.label("analysis");
            ui.horizontal(|ui| {
                let sweep = matches!(e.analysis, Analysis::Sweep);
                if ui.radio(sweep, "S-parameters").clicked() {
                    e.analysis = Analysis::Sweep;
                }
                if ui.radio(!sweep, "eigenmodes").clicked() && sweep {
                    e.analysis = Analysis::Eigen { count: 4, near: 0.5 * (e.start + e.stop) };
                }
            });
            ui.end_row();
            if let Analysis::Eigen { count, near } = &mut e.analysis {
                ui.label("modes");
                ui.add(egui::DragValue::new(count).range(1..=50));
                ui.end_row();
                ui.label("near");
                ui.add(egui::DragValue::new(near).speed(0.05).range(0.1..=1000.0).suffix(" GHz"));
                ui.end_row();
            }
            ui.label("start");
            ui.add(egui::DragValue::new(&mut e.start).speed(0.05).range(0.1..=1000.0).suffix(" GHz"));
            ui.end_row();
            ui.label("stop");
            ui.add(egui::DragValue::new(&mut e.stop).speed(0.05).range(0.1..=1000.0).suffix(" GHz"));
            ui.end_row();
            ui.label("points");
            ui.add(egui::DragValue::new(&mut e.points).range(1..=401));
            ui.end_row();
        });
        ui.horizontal(|ui| {
            let busy = self.solving.is_some();
            if ui.add_enabled(!busy, egui::Button::new("Solve")).clicked() {
                self.solve();
            }
            if ui.button("High-pass sample").clicked() {
                self.edit = ModelEdit::default();
            }
            if ui.button("Band-pass sample").clicked() {
                self.edit = ModelEdit::bandpass();
            }
            if ui.button("Cavity sample").clicked() {
                self.edit = ModelEdit::cavity();
            }
            if busy {
                ui.spinner();
            }
        });
        ui.separator();
        ui.heading("View");
        if let Some(e) = self.session.eigen.as_ref() {
            let label = |i: usize| {
                e.modes.get(i).map_or(String::new(), |m| {
                    let q = m.q().map_or(String::new(), |q| format!(", Q {q:.0}"));
                    format!("{}: {:.4} GHz{q}", i + 1, GHZ.from_si(m.frequency))
                })
            };
            let mut idx = self.view.mode.unwrap_or(0);
            egui::ComboBox::from_label("mode").selected_text(label(idx)).show_ui(ui, |ui| {
                for i in 0..e.modes.len() {
                    ui.selectable_value(&mut idx, i, label(i));
                }
            });
            self.view.mode = Some(idx);
        }
        let freqs = self.frequencies();
        if !freqs.is_empty() && self.view.mode.is_none() {
            let mut idx = freqs.iter().position(|f| (f - self.view.frequency).abs() <= 1e-9 * f).unwrap_or(0);
            let label = |f: f64| format!("{:.4} GHz", GHZ.from_si(f));
            egui::ComboBox::from_label("frequency").selected_text(label(self.view.frequency)).show_ui(ui, |ui| {
                for (i, f) in freqs.iter().enumerate() {
                    ui.selectable_value(&mut idx, i, label(*f));
                }
            });
            if let Some(f) = freqs.get(idx) {
                self.view.frequency = *f;
            }
        }
        ui.horizontal(|ui| {
            for (p, name) in [(Plane::H, "H-plane"), (Plane::E, "E-plane"), (Plane::Cross, "Cross-section")] {
                if ui.radio_value(&mut self.view.plane, p, name).changed() {
                    self.at_mm = None;
                }
            }
        });
        if let Some(w) = self.session.model.waveguide.as_ref() {
            let (span, axis) = match self.view.plane {
                Plane::H => (w.height, "y"),
                Plane::E => (w.width, "x"),
                Plane::Cross => (w.length(), "z"),
            };
            let span_mm = MM.from_si(span);
            let mut at = self.at_mm.unwrap_or(span_mm / 2.0);
            if ui.add(egui::Slider::new(&mut at, 0.0..=span_mm).text(format!("{axis} (mm)"))).changed() {
                self.at_mm = Some(at);
            }
        }
        egui::ComboBox::from_label("colour").selected_text(self.view.component.clone()).show_ui(ui, |ui| {
            for c in ["ex", "ey", "ez", "hx", "hy", "hz", "|e|", "|h|"] {
                ui.selectable_value(&mut self.view.component, c.to_owned(), c);
            }
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.view.show_e, "E arrows");
            ui.checkbox(&mut self.view.show_h, "H arrows");
        });
        ui.horizontal(|ui| {
            ui.label("excite port");
            ui.radio_value(&mut self.view.excitation, 0, "1");
            ui.radio_value(&mut self.view.excitation, 1, "2");
        });
        let mut deg = self.view.phase.to_degrees().rem_euclid(360.0);
        if ui.add(egui::Slider::new(&mut deg, 0.0..=360.0).text("ωt (deg)")).changed() {
            self.view.phase = deg.to_radians();
        }
        ui.checkbox(&mut self.animate, "Animate over the period");
    }

    /// The field texture, redrawn when the view or the width changed.
    fn update_field(&mut self, ctx: &egui::Context, width: usize) {
        if self.session.sweep.is_none() && self.session.eigen.is_none() {
            return;
        }
        self.view.at = self.at_mm.map(|mm| MM.to_si(mm));
        let key = format!("{:?}|{width}", self.view);
        if self.field_key.as_deref() == Some(key.as_str()) {
            return;
        }
        let at = match self.session.model.waveguide.as_ref() {
            Some(w) => {
                let span = match self.view.plane {
                    Plane::H => w.height,
                    Plane::E => w.width,
                    Plane::Cross => w.length(),
                };
                self.view.at.unwrap_or(span / 2.0).clamp(0.0, span)
            }
            None => return,
        };
        if !self.samples.as_ref().is_some_and(|s| s.serves(&self.view, at)) {
            match view::sample_plane(&self.session, &self.view) {
                Ok(s) => self.samples = Some(s),
                Err(e) => {
                    self.status = e.to_string();
                    return;
                }
            }
        }
        let Some(samples) = &self.samples else { return };
        match samples.plot(&self.view) {
            Ok(plot) => {
                let c = emcraft_render::plot::draw(&plot, width);
                let image = egui::ColorImage::from_rgba_unmultiplied([c.w, c.h], &c.rgba);
                self.field = Some((ctx.load_texture("field", image, egui::TextureOptions::LINEAR), [c.w, c.h]));
                self.field_key = Some(key);
            }
            Err(e) => self.status = e.to_string(),
        }
    }

    fn update_chart(&mut self, ctx: &egui::Context, width: usize) {
        if self.session.sweep.is_none() {
            self.chart = None;
            return;
        }
        if self.chart.is_some() && self.chart_width == width {
            return;
        }
        if let Ok(chart) = view::sparameter_chart(&self.session) {
            let c = emcraft_render::chart::draw(&chart, width, 360);
            let image = egui::ColorImage::from_rgba_unmultiplied([c.w, c.h], &c.rgba);
            self.chart = Some((ctx.load_texture("chart", image, egui::TextureOptions::LINEAR), [c.w, c.h]));
            self.chart_width = width;
        }
    }
}

impl eframe::App for EmcraftApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll();
        let ctx = ui.ctx().clone();
        let now = Instant::now();
        if self.animate {
            let dt = self.last_frame.map_or(0.0, |t| now.duration_since(t).as_secs_f64()).min(0.1);
            // One period every three seconds.
            self.view.phase = (self.view.phase + dt * std::f64::consts::TAU / 3.0).rem_euclid(std::f64::consts::TAU);
            ctx.request_repaint();
        }
        self.last_frame = Some(now);
        if self.solving.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }

        egui::Panel::top("top").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.strong("EMCraft");
                ui.label("— frequency-domain FEM (Nédélec elements) for waveguides");
            });
        });
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.label(&self.status);
        });
        egui::Panel::left("controls").default_size(340.0).resizable(true).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| self.controls(ui));
        });
        egui::CentralPanel::default().show(ui, |ui| {
            let avail = ui.available_width().max(400.0);
            // Rounded so a resize does not redraw on every pixel.
            let width = ((avail / 50.0).floor() * 50.0) as usize;
            if self.session.sweep.is_none() && self.session.eigen.is_none() {
                ui.centered_and_justified(|ui| {
                    ui.label(if self.solving.is_some() { "Solving…" } else { "No results yet: press Solve." });
                });
                return;
            }
            self.update_field(&ctx, width);
            self.update_chart(&ctx, width);
            egui::ScrollArea::vertical().show(ui, |ui| {
                if let Some((tex, [w, h])) = &self.field {
                    ui.add(egui::Image::new((tex.id(), egui::vec2(*w as f32, *h as f32))));
                }
                if let Some((tex, [w, h])) = &self.chart {
                    let r = ui.add(egui::Image::new((tex.id(), egui::vec2(*w as f32, *h as f32))).sense(egui::Sense::click()));
                    let freqs = self.frequencies();
                    if r.clicked()
                        && let (Some(pos), Some(f0), Some(f1)) = (r.interact_pointer_pos(), freqs.first(), freqs.last())
                    {
                        let fx = ((pos.x - r.rect.min.x) / r.rect.width()) as f64;
                        if let Some(f) = chart_frequency(fx, *w as f64, *f0, *f1).and_then(|f| nearest(&freqs, f)) {
                            self.view.frequency = f;
                        }
                    }
                    ui.label("Click the chart to show the fields at the nearest solved frequency.");
                }
            });
        });
    }
}

/// Opens the desktop app; `sample` (`highpass`, `iris-bandpass`) is solved at once.
pub fn run(sample: Option<String>) -> Result<(), String> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("EMCraft").with_inner_size([1500.0, 980.0]).with_min_inner_size([900.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native("EMCraft", options, Box::new(move |_cc| Ok(Box::new(EmcraftApp::new(sample.as_deref()))))).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_edit_is_the_sample_and_solves() {
        let script = ModelEdit::default().script();
        let steps = script.as_array().unwrap();
        assert_eq!(steps.iter().filter(|s| s["command"] == "model.material.create").count(), 1);
        let mut s = Session::new();
        // Coarser and two frequencies, for speed.
        let mut quick = ModelEdit { size: 5.0, points: 2, ..ModelEdit::default() };
        quick.order = 1;
        s.run_script(&quick.script()).unwrap();
        assert_eq!(s.sweep.as_ref().unwrap().results.len(), 2);
        assert_eq!(s.model.waveguide.as_ref().unwrap().sections[1].material, "vacuum");
        // The band-pass edit builds the sample's model.
        let mut b = Session::new();
        let script = ModelEdit::bandpass().script();
        b.run_script(&Value::Array(script.as_array().unwrap()[..3].to_vec())).unwrap();
        let mut sample = Session::new();
        sample.execute("file.sample", &json!({"name": "iris-bandpass"})).unwrap();
        assert_eq!(b.model.waveguide, sample.model.waveguide);
        assert_eq!(b.model.mesh.as_ref().map(|m| m.tets.len()), sample.model.mesh.as_ref().map(|m| m.tets.len()));
        // The cavity edit runs an eigenmode analysis with copper walls.
        let script = ModelEdit::cavity().script();
        let steps = script.as_array().unwrap();
        assert!(steps.iter().any(|s| s["command"] == "solve.eigenmodes"));
        assert!(steps.iter().any(|s| s["command"] == "model.walls"));
        assert!(!steps.iter().any(|s| s["command"] == "model.port.create"));
    }

    #[test]
    fn chart_clicks_map_to_frequencies() {
        // 1000 px wide: the plot runs from x = 90 to 970.
        assert_eq!(chart_frequency(90.0 / 1000.0, 1000.0, 5.0, 9.0), Some(5.0));
        assert!((chart_frequency(530.0 / 1000.0, 1000.0, 5.0, 9.0).unwrap() - 7.0).abs() < 1e-12);
        assert_eq!(chart_frequency(0.01, 1000.0, 5.0, 9.0), None);
        assert_eq!(nearest(&[5.0, 6.0, 7.0], 6.4), Some(6.0));
        assert_eq!(nearest(&[], 6.4), None);
    }
}
