//! The active step's panel: its checks, its settings and its commands.

use super::bar::mark;
use super::*;

/// The active step's panel at the right (with the open dialog docked in it).
pub fn panel(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    let Some(step) = app.workflow.step else { return };
    let t = Tokens::get();
    let states = statuses(app);
    let Some(state) = states.get(step.index()).cloned() else { return };
    egui::Panel::right("emcraft_step_panel")
        .exact_size(t.form_card_width + 40.0)
        .show_separator_line(false)
        .frame(egui::Frame::NONE.fill(t.viewport_top).inner_margin(egui::Margin { left: 0, right: 6, top: 0, bottom: 6 }))
        .show(ui, |ui| {
            let h = ui.available_height();
            t.card(t.panel).inner_margin(egui::Margin::symmetric(12, 10)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_min_height(h - 22.0);
                scenario::publish_rect("step_panel", ui.max_rect());
                header(ui, &state, &t);
                let dialog_h = if app.dialog.is_some() { (h * 0.58).max(260.0) } else { 0.0 };
                egui::ScrollArea::vertical()
                    .id_salt("step_panel_scroll")
                    .max_height((h - dialog_h - 70.0).max(120.0))
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        checks(app, ui, &state, &t);
                        body(app, ui, step, &states, &t);
                    });
                if app.dialog.is_some() {
                    ui.add_space(6.0);
                    t.card(t.dialog_body).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        crate::dialog_ui::embedded(app, ui, dialog_h - 10.0);
                    });
                }
            });
        });
}

fn header(ui: &mut egui::Ui, s: &StepState, t: &Tokens) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
        mark(ui.painter(), r.center(), s.status, t);
        ui.label(egui::RichText::new(format!("Step {} · {}", s.step.index(), s.step.label())).size(t.font_heading).strong().color(t.text));
        ui.label(egui::RichText::new(s.status.id()).size(t.font_small).color(status_color(s.status, t)));
    });
    ui.add_space(4.0);
}

/// The step's checks, each with its fix.
fn checks(app: &mut EmcraftApp, ui: &mut egui::Ui, s: &StepState, t: &Tokens) {
    if s.messages.is_empty() {
        ui.label(egui::RichText::new("Nothing to fix here.").size(t.font_small).color(t.success));
        ui.add_space(6.0);
        return;
    }
    let mut chosen = None;
    for (i, m) in s.messages.iter().enumerate() {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(&m.text).size(t.font_small).color(status_color(s.status, t)));
            // A fix that only leads back to this step is left out.
            let here = m.command.as_deref().is_some_and(|c| c.starts_with("solve.") && Step::of_command(c) == Some(s.step));
            if let Some(c) = m.command.as_ref().filter(|_| !here) {
                let r = ui.small_button(fix_label(c)).on_hover_text(format!("{c}: {}", crate::label(c)));
                scenario::publish_target(&format!("step:fix:{i}"), r.rect);
                if r.clicked() {
                    chosen = Some(c.clone());
                }
            }
        });
    }
    ui.add_space(6.0);
    if let Some(c) = chosen {
        fix(app, &c);
    }
}

fn fix_label(command: &str) -> String {
    match Step::of_command(command) {
        Some(step) if command.starts_with("solve.") && command != "solve.port_modes" => format!("Go to {}", step.label()),
        Some(Step::Intent) if command == "project.frequency" => "Set the band".into(),
        _ => format!("Fix: {}", crate::ribbon::button_label(command)),
    }
}

/// A one-line text field with its error under it; `true` when Enter was pressed in it.
fn field(ui: &mut egui::Ui, label: &str, text: &mut String, unit: &str, error: Option<&str>, handle: &str) -> bool {
    let t = Tokens::get();
    let mut enter = false;
    ui_kit::row(ui, label, false, |ui| {
        let mut edit = egui::TextEdit::singleline(text).desired_width((ui.available_width() - 44.0).max(60.0)).char_limit(64);
        if error.is_some() {
            edit = edit.background_color(t.form_error_fill).text_color(t.form_error);
        }
        let r = ui.add(edit);
        scenario::publish_target(handle, r.rect);
        enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if !unit.is_empty() {
            ui.label(egui::RichText::new(unit).size(t.font_small).color(t.text_dim));
        }
    });
    if let Some(e) = error {
        ui.horizontal(|ui| {
            ui.add_space(t.form_label_width + 6.0);
            ui.label(egui::RichText::new(e).size(t.font_small).color(t.form_error));
        });
    }
    enter
}

fn kv(ui: &mut egui::Ui, k: &str, v: String, t: &Tokens) {
    ui.horizontal(|ui| {
        ui.add_sized(vec2(t.form_label_width, 16.0), egui::Label::new(egui::RichText::new(k).size(t.font_small).color(t.text_dim)));
        ui.label(egui::RichText::new(v).size(t.font_small).color(t.text));
    });
}

fn fmt(v: f64) -> String {
    let s = format!("{v:.6}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// The step's own settings and information.
fn body(app: &mut EmcraftApp, ui: &mut egui::Ui, step: Step, states: &[StepState], t: &Tokens) {
    let fu = app.session.display.frequency.symbol();
    let lu = app.session.display.length.symbol();
    // The band fields follow the session's band when it changes.
    let b = band(app);
    if app.workflow.fields.band_seen != b {
        if let Some((a, z)) = b {
            app.workflow.fields.start = fmt(a);
            app.workflow.fields.stop = fmt(z);
        }
        app.workflow.fields.band_seen = b;
    }
    match step {
        Step::Intent => intent(app, ui, fu, t),
        Step::Geometry => {
            ui_kit::section(ui, "Bodies");
            let tree = crate::browser::tree(app);
            for (_, rows) in tree.iter().filter(|(s, _)| *s == "Geometry") {
                for r in rows {
                    ui.label(egui::RichText::new(&r.text).size(t.font_small).color(t.text));
                }
            }
            if let Some(emcraft_model::Aabb { min: lo, max: hi }) = app.session.model.bounds() {
                let l = app.session.display.length;
                kv(
                    ui,
                    "extent",
                    format!("{} × {} × {} {lu}", fmt(l.from_si(hi.x - lo.x)), fmt(l.from_si(hi.y - lo.y)), fmt(l.from_si(hi.z - lo.z))),
                    t,
                );
            }
        }
        Step::Materials => {
            ui_kit::section(ui, "Materials");
            let m = &app.session.model;
            for (i, mat) in m.materials.iter().enumerate() {
                ui.horizontal(|ui| {
                    let c = crate::scene3d::material_rgb(m, i);
                    let (r, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                    ui.painter().rect_filled(r, 2.0, Color32::from_rgb(c.0, c.1, c.2));
                    ui.label(egui::RichText::new(format!("{}: {}", mat.name, mat.summary())).size(t.font_small).color(t.text));
                });
            }
        }
        Step::Excitation => excitation(app, ui, fu, t),
        Step::Boundaries => {
            ui_kit::section(ui, "Boundaries");
            let m = &app.session.model;
            kv(ui, "walls", m.wall_conductivity.map_or("perfect conductor".into(), |s| format!("σ = {s:.3e} S/m")), t);
            let l = app.session.display.length;
            let kind = |o: &emcraft_model::Open| if o.shape == emcraft_model::OpenShape::Sphere { "spherical PML" } else { "PML" };
            kv(ui, "open", m.open.map_or("closed (no PML)".into(), |o| format!("{} {} {lu} thick", kind(&o), fmt(l.from_si(o.thickness)))), t);
            let planes: Vec<String> = m
                .symmetry
                .iter()
                .map(|s| {
                    let wall = if s.kind == emcraft_model::SymmetryKind::Electric { "electric" } else { "magnetic" };
                    format!("{} = {} {lu}, {wall}", if s.axis == emcraft_model::Axis::X { "x" } else { "y" }, fmt(l.from_si(s.at)))
                })
                .collect();
            kv(ui, "symmetry", if planes.is_empty() { "none (the whole model)".into() } else { planes.join("; ") }, t);
        }
        Step::Mesh => mesh(app, ui, lu, t),
        Step::Setup => setup(app, ui, fu, t),
        Step::Run => run_panel(app, ui, states, t),
        Step::Results => results(app, ui, states, t),
    }
}

fn intent(app: &mut EmcraftApp, ui: &mut egui::Ui, fu: &str, t: &Tokens) {
    ui_kit::section(ui, "Frequency band");
    ui_kit::caption(ui, "Set once, early: it sizes the mesh (elements per wavelength at its top), the sweep and the port-mode preview.");
    let f = &mut app.workflow.fields;
    let (e0, e1) = (f.error("start").map(str::to_owned), f.error("stop").map(str::to_owned));
    let mut go = field(ui, "start", &mut f.start, fu, e0.as_deref(), "intent:start");
    go |= field(ui, "stop", &mut f.stop, fu, e1.as_deref(), "intent:stop");
    let busy = app.busy();
    go |= ui_kit::button(ui, "Apply", true, !busy, "intent:apply").clicked();
    if go && !busy {
        apply_band(app);
    }
    ui_kit::section(ui, "Problem");
    let kind = match Solver::infer(&app.session.model) {
        Solver::Eigenmodes => "a resonator (no excitation: its eigenmodes)",
        Solver::Radiation => "an antenna or a scatterer (an open problem)",
        _ => "a guided-wave component (S-parameters between ports)",
    };
    ui.label(egui::RichText::new(kind).size(t.font_small).color(t.text));
    let d = app.session.display;
    kv(ui, "units", format!("{}, {}", d.length.symbol(), d.frequency.symbol()), t);
}

/// Checks the band's fields one by one and sets the band (`project.frequency`).
pub fn apply_band(app: &mut EmcraftApp) {
    let f = &mut app.workflow.fields;
    let (a, z) = (positive(&f.start), positive(&f.stop));
    f.set_error("start", a.as_ref().err().cloned());
    f.set_error("stop", z.as_ref().err().cloned());
    let (Ok(a), Ok(z)) = (a, z) else { return };
    if z < a {
        f.set_error("stop", Some("must not be below the start".into()));
        return;
    }
    if let Err(e) = app.run("project.frequency", json!({"start": a, "stop": z})) {
        app.workflow.fields.set_error("stop", Some(e.to_string()));
    }
}

fn excitation(app: &mut EmcraftApp, ui: &mut egui::Ui, fu: &str, t: &Tokens) {
    ui_kit::section(ui, "Ports and sources");
    let m = &app.session.model;
    if m.ports.is_empty() && m.lumped.is_empty() && m.sources.is_empty() {
        ui.label(egui::RichText::new("none yet").size(t.font_small).color(t.text_dim));
    }
    for p in &m.ports {
        let at = match p.face {
            Some(_) => "on a face".to_owned(),
            None => format!("{} end", if p.end == emcraft_model::End::Start { "start" } else { "far" }),
        };
        ui.label(egui::RichText::new(format!("Port {}: {at}, {} modes", p.name, p.modes)).size(t.font_small).color(t.text));
    }
    for p in &m.lumped {
        ui.label(egui::RichText::new(format!("Lumped port {}: {} Ω", p.name, p.impedance)).size(t.font_small).color(t.text));
    }
    if !m.sources.is_empty() {
        ui.label(egui::RichText::new(format!("{} source(s)", m.sources.len())).size(t.font_small).color(t.text));
    }
    if m.ports.is_empty() {
        return;
    }
    ui_kit::section(ui, "Port modes");
    let ready = m.mesh.is_some();
    let at = band(app).map(|(_, z)| format!(" at {} {fu}", fmt(z))).unwrap_or_default();
    ui_kit::caption(ui, &format!("Each port's modes{at}: propagation, effective permittivity, impedance (seconds, no full solve)."));
    let busy = app.busy();
    if ui_kit::button(ui, "Preview port modes", true, ready && !busy, "excitation:preview")
        .on_hover_text(if ready { "" } else { "needs a mesh (step 5)" })
        .clicked()
    {
        let r =
            if app.session.model.band.is_some() { app.run("solve.port_modes", json!({})).map(|_| ()) } else { app.open_dialog("solve.port_modes") };
        if let Err(e) = r {
            app.message(e, true);
        }
    }
    let Some(reply) = app.port_modes.clone() else { return };
    let f = reply.get("frequency").and_then(Value::as_f64).unwrap_or(f64::NAN);
    ui.label(egui::RichText::new(format!("at {} {fu}", fmt(f))).size(t.font_small).color(t.text_dim));
    mode_table(app, ui, &reply, t);
    if ui_kit::button(ui, "Show the mode patterns", false, true, "excitation:patterns").clicked() {
        app.view = View::PortModes;
    }
}

/// The port-mode preview: a row per mode. Hovering a row lights its port in the 3D view;
/// clicking it opens the Port modes view at that mode.
fn mode_table(app: &mut EmcraftApp, ui: &mut egui::Ui, reply: &Value, t: &Tokens) {
    let unit = reply.get("length_unit").and_then(Value::as_str).unwrap_or("m");
    let w = ui.available_width();
    let widths = [w * 0.34, w * 0.24, w * 0.2, w * 0.2];
    let cell = |ui: &mut egui::Ui, width: f32, text: egui::RichText| {
        ui.allocate_ui_with_layout(vec2(width, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.set_min_width(width);
            ui.add_space(4.0);
            ui.add(egui::Label::new(text).truncate());
        });
    };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let per = format!("β (rad/{unit})");
        for (h, width) in ["port / mode", per.as_str(), "εeff", "Z (Ω)"].into_iter().zip(widths) {
            cell(ui, width, egui::RichText::new(h).size(t.font_small).color(t.text_dim));
        }
    });
    let mut row = 0usize;
    let mut hovered = None;
    let mut clicked = None;
    for p in reply.get("ports").and_then(Value::as_array).into_iter().flatten() {
        let name = p.get("name").and_then(Value::as_str).unwrap_or("?");
        let port = app.session.model.ports.iter().position(|q| q.name == name);
        for (k, mode) in p.get("modes").and_then(Value::as_array).into_iter().flatten().take(8).enumerate() {
            let n = |key: &str| mode.get(key).and_then(Value::as_f64);
            let txt = |s: String| egui::RichText::new(s).size(t.font_small).color(t.text);
            let back = ui.painter().add(egui::Shape::Noop);
            let r = ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                cell(ui, widths[0], txt(format!("{name} · {}", mode.get("name").and_then(Value::as_str).unwrap_or(""))));
                if mode.get("propagating").and_then(Value::as_bool) == Some(true) {
                    cell(ui, widths[1], txt(n("beta").map_or("—".into(), |v| format!("{v:.4}"))));
                    cell(ui, widths[2], txt(n("eps_eff").map_or("—".into(), |v| format!("{v:.4}"))));
                    cell(ui, widths[3], txt(n("impedance").map_or("—".into(), |v| format!("{v:.2}"))));
                } else {
                    let a = n("attenuation_db").unwrap_or(f64::NAN);
                    ui.label(egui::RichText::new(format!("cut off, {a:.1} dB/{unit}")).size(t.font_small).color(t.text_dim));
                }
            });
            let rect = Rect::from_min_size(r.response.rect.min, vec2(w, r.response.rect.height()));
            let resp = ui.interact(rect, ui.id().with(("port_mode_row", row)), Sense::click());
            let resp = resp.on_hover_text("Lights this port in the 3D view; click for the mode in the Port modes view");
            scenario::publish_target(&format!("portmode:row:{row}"), rect);
            let focused = app.workflow.mode_focus.as_ref().is_some_and(|f| f.port == name && f.mode == k);
            let fill = if resp.hovered() {
                t.hover
            } else if focused {
                t.accent_soft
            } else if row % 2 == 1 {
                t.text.gamma_multiply(0.04)
            } else {
                Color32::TRANSPARENT
            };
            ui.painter().set(back, egui::Shape::rect_filled(rect, 3.0, fill));
            if resp.hovered() {
                hovered = port;
            }
            if resp.clicked() {
                clicked = Some((name.to_owned(), k));
            }
            row += 1;
        }
    }
    if let Some(i) = hovered {
        app.view3d.hover = Some(crate::scene3d::Pick::Port(i));
    }
    if let Some((port, mode)) = clicked {
        app.workflow.mode_focus = Some(ModeFocus { port, mode, scroll: true });
        app.view = View::PortModes;
    }
}

fn mesh(app: &mut EmcraftApp, ui: &mut egui::Ui, lu: &str, t: &Tokens) {
    ui_kit::section(ui, "Mesh");
    let l = app.session.display.length;
    let Some(m) = app.session.model.mesh.as_ref() else {
        ui.label(egui::RichText::new("not meshed yet").size(t.font_small).color(t.text_dim));
        return;
    };
    kv(ui, "nodes", m.nodes.len().to_string(), t);
    kv(ui, "tetrahedra", m.tets.len().to_string(), t);
    kv(ui, "boundary faces", m.boundary.len().to_string(), t);
    let volume: f64 = (0..m.tets.len()).filter_map(|i| m.volume(i)).sum();
    let h = (6.0 * std::f64::consts::SQRT_2 * volume / m.tets.len().max(1) as f64).cbrt();
    kv(ui, "mean element", format!("{:.3} {lu}", l.from_si(h)), t);
    if let Some(b) = app.session.model.band {
        let index = app.session.model.materials.iter().map(|x| (x.eps_r * x.mu_r).sqrt()).fold(1.0, f64::max);
        let lambda = emcraft_model::units::C0 / (b.stop * index);
        kv(ui, "per wavelength", format!("{:.1} at the band's top", lambda / h.max(1e-300)), t);
    }
    ui_kit::section(ui, "Estimate");
    match estimate(app) {
        Ok(e) => {
            let n = |k: &str| e.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN);
            kv(ui, "unknowns", format!("{:.0} (order {})", n("unknowns"), e.get("order").and_then(Value::as_u64).unwrap_or(2)), t);
            kv(ui, "memory", format!("{:.0} MB", n("memory_mb")), t);
            let measured = e.get("from_this_session").and_then(Value::as_bool) == Some(true);
            kv(ui, "time", format!("{:.2} s per frequency{}", n("seconds_per_frequency"), if measured { "" } else { " (reference machine)" }), t);
        }
        Err(e) => {
            ui.label(egui::RichText::new(e).size(t.font_small).color(t.text_dim));
        }
    }
}

fn setup(app: &mut EmcraftApp, ui: &mut egui::Ui, fu: &str, t: &Tokens) {
    ui_kit::section(ui, "Solver");
    let inferred = Solver::infer(&app.session.model);
    let f = &mut app.workflow.fields;
    let mut solver = f.solver.unwrap_or(inferred);
    for s in Solver::ALL {
        let r = ui.radio_value(&mut solver, s, egui::RichText::new(s.label()).size(t.font_small));
        scenario::publish_target(&format!("setup:solver:{}", s.id()), r.rect);
    }
    if solver != f.solver.unwrap_or(inferred) || f.solver.is_some() {
        f.solver = Some(solver);
    }
    if f.points.is_empty() {
        f.points = "11".into();
    }
    if f.order.is_empty() {
        f.order = "2".into();
    }
    if f.count.is_empty() {
        f.count = "5".into();
    }
    ui_kit::section(ui, "Settings");
    let solves = solve_frequencies(app, solver);
    band_diagram(app, ui, &solves, t);
    let e = |f: &Fields, k: &str| f.error(k).map(str::to_owned);
    match solver {
        Solver::Frequency | Solver::TimeDomain => {
            let f = &mut app.workflow.fields;
            let ep = e(f, "points");
            field(ui, "points", &mut f.points, "", ep.as_deref(), "setup:points");
        }
        Solver::Eigenmodes => {
            let f = &mut app.workflow.fields;
            let ec = e(f, "count");
            field(ui, "modes", &mut f.count, "", ec.as_deref(), "setup:count");
            let ef = e(f, "frequency");
            field(ui, "near", &mut f.frequency, fu, ef.as_deref(), "setup:frequency");
        }
        Solver::Radiation => {
            let f = &mut app.workflow.fields;
            let ef = e(f, "frequency");
            field(ui, "frequency", &mut f.frequency, fu, ef.as_deref(), "setup:frequency");
        }
    }
    if solver != Solver::TimeDomain {
        let f = &mut app.workflow.fields;
        let eo = e(f, "order");
        field(ui, "order", &mut f.order, "", eo.as_deref(), "setup:order");
    }
    device(app, ui, solver, t);
    ui_kit::caption(ui, "Run (step 7) starts this solve after checking the model.");
    // The fields' errors as typed.
    let _ = run_params(app);
}

/// Where the solve runs: the devices `solve.devices` lists (those this machine cannot run
/// disabled, with the reason on hover) and the time domain's precision, set through
/// `solve.device` (the ribbon's SIMULATE › Device sets the same).
fn device(app: &mut EmcraftApp, ui: &mut egui::Ui, solver: Solver, t: &Tokens) {
    ui_kit::section(ui, "Compute device");
    let list = match &app.workflow.devices {
        Some(v) => v.clone(),
        None => {
            let v = app.session.execute("solve.devices", &json!({})).unwrap_or_else(|e| json!({"error": e.to_string()}));
            app.workflow.devices = Some(v.clone());
            v
        }
    };
    let current = app.session.compute.device();
    let precision = app.session.compute.precision.name();
    let busy = app.busy();
    let devices: Vec<(String, bool, Vec<String>, Option<String>)> = list
        .get("devices")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|d| {
            let s = |k: &str| d.get(k).and_then(Value::as_str).map(str::to_owned);
            let precisions =
                d.get("precisions").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect();
            (s("name").unwrap_or_default(), d.get("available").and_then(Value::as_bool) == Some(true), precisions, s("problem"))
        })
        .collect();
    let mut chosen: Option<(String, String)> = None;
    ui_kit::row(ui, "device", false, |ui| {
        for (name, available, precisions, problem) in &devices {
            let r = ui.add_enabled(*available && !busy, egui::RadioButton::new(current == name, egui::RichText::new(name).size(t.font_small)));
            let r = match (available, problem) {
                (false, Some(p)) => r.on_disabled_hover_text(format!("{name} cannot run here: {p}")),
                (false, None) => r.on_disabled_hover_text(format!("{name} cannot run here")),
                _ => r.on_hover_text(match name.as_str() {
                    "cpu" => "the CPU's threads",
                    "cuda" => "an NVIDIA GPU",
                    _ => "the GPU's kernels on the CPU (to check them without a GPU)",
                }),
            };
            scenario::publish_target(&format!("setup:device:{name}"), r.rect);
            if r.clicked() && current != name {
                // The precision set so far where the device runs it, else its first.
                let p = if precisions.iter().any(|p| p == precision) { Some(precision.to_owned()) } else { precisions.first().cloned() };
                chosen = Some((name.clone(), p.unwrap_or_else(|| precision.to_owned())));
            }
        }
    });
    if let Some(e) = list.get("error").and_then(Value::as_str) {
        ui.label(egui::RichText::new(e).size(t.font_small).color(t.form_error));
    }
    // The CPU's threads run in double; a device runs the precisions it lists.
    let runs: Vec<String> = devices.iter().find(|d| d.0 == current).map(|d| d.2.clone()).unwrap_or_default();
    ui_kit::row(ui, "precision", false, |ui| {
        for p in ["single", "double"] {
            let on = if current == "cpu" { p == "double" } else { precision == p };
            let enabled = current != "cpu" && runs.iter().any(|x| x == p) && !busy;
            let r = ui.add_enabled(enabled, egui::RadioButton::new(on, egui::RichText::new(p).size(t.font_small)));
            let r = r.on_disabled_hover_text(if current == "cpu" { "the CPU's threads run in double" } else { "this device does not run it" });
            scenario::publish_target(&format!("setup:precision:{p}"), r.rect);
            if r.clicked() && !on {
                chosen = Some((current.to_owned(), p.to_owned()));
            }
        }
    });
    if let Some((_, e)) = app.failure.as_ref().filter(|f| f.0 == "solve.device") {
        ui.horizontal_wrapped(|ui| {
            ui.add_space(t.form_label_width + 6.0);
            ui.label(egui::RichText::new(e).size(t.font_small).color(t.form_error));
        });
    }
    let pending = app.job.as_ref().is_some_and(|j| j.command == "solve.device");
    let next = match solver {
        _ if pending => "Switching the device…".to_owned(),
        Solver::TimeDomain if current != "cpu" => format!("The next solve runs on {current} in {precision} precision."),
        Solver::TimeDomain => "The next solve runs on the CPU's threads (double precision).".to_owned(),
        Solver::Frequency | Solver::Eigenmodes | Solver::Radiation if current != "cpu" => {
            format!("The next solve runs on the CPU, its factorization's large fronts on {current} (complex double).")
        }
        _ => "The next solve runs on the CPU's threads.".to_owned(),
    };
    ui.label(egui::RichText::new(next).size(t.font_small).color(t.text));
    if let Some((device, precision)) = chosen
        && let Err(e) = app.run("solve.device", json!({"device": device, "precision": precision}))
    {
        app.failure = Some(("solve.device".into(), e.to_string()));
    }
}

/// The frequencies (display units) the solve will work at, as the panel's fields stand: the
/// sweep's points over the band, or the one frequency of an eigenmode or radiation solve.
fn solve_frequencies(app: &EmcraftApp, solver: Solver) -> Vec<f64> {
    let Some((a, z)) = band(app) else { return Vec::new() };
    let f = &app.workflow.fields;
    match solver {
        Solver::Frequency | Solver::TimeDomain => {
            let Ok(n) = integer(&f.points, 1, app.session.limits.frequencies as u64) else { return Vec::new() };
            (0..n).map(|k| if n == 1 { a } else { a + (z - a) * k as f64 / (n - 1) as f64 }).collect()
        }
        Solver::Eigenmodes | Solver::Radiation => {
            if f.frequency.trim().is_empty() {
                vec![0.5 * (a + z)]
            } else {
                positive(&f.frequency).map(|v| vec![v]).unwrap_or_default()
            }
        }
    }
}

/// Most ticks the diagram draws for the solve's frequencies (beyond, they fill the band).
const MAX_SOLVE_TICKS: usize = 400;

/// The band as a small diagram: a frequency axis over the band (shaded), the cut-offs of the
/// ports' modes and the guide's sections near it as dashed lines (a port's first mode in the
/// warning colour: below it no power enters), and the frequencies the solve will work at as
/// ticks.
fn band_diagram(app: &mut EmcraftApp, ui: &mut egui::Ui, solves: &[f64], t: &Tokens) {
    let unit = app.session.display.frequency;
    let fu = unit.symbol();
    let Some((a, z)) = band(app) else {
        ui.label(egui::RichText::new("band: not set (step 0)").size(t.font_small).color(t.error));
        app.workflow.diagram = None;
        return;
    };
    let span = (z - a).max(z.abs() * 1e-3).max(1e-12);
    let cuts: Vec<(f64, String, bool)> = emcraft_engine::workflow::cutoffs(&app.session)
        .into_iter()
        .map(|c| (unit.from_si(c.frequency), c.label, c.first))
        .filter(|(f, _, _)| *f >= a - 0.35 * span && *f <= z + 0.35 * span)
        .collect();
    let lo = cuts.iter().map(|c| c.0).fold(a, f64::min);
    let hi = cuts.iter().map(|c| c.0).fold(z, f64::max);
    let pad = 0.04 * (hi - lo).max(span);
    let (lo, hi) = (lo - pad, hi + pad);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 64.0), Sense::hover());
    scenario::publish_target("setup:band", rect);
    let p = ui.painter_at(rect);
    let (left, right) = (rect.left() + 4.0, rect.right() - 4.0);
    let x = |f: f64| left + (((f - lo) / (hi - lo)) as f32).clamp(0.0, 1.0) * (right - left);
    let base = rect.top() + 46.0;
    let small = FontId::proportional(t.font_small);
    // The band, shaded, with the solve's frequencies as ticks in it.
    let band_rect = Rect::from_min_max(pos2(x(a), base - 16.0), pos2(x(z).max(x(a) + 1.0), base));
    p.rect_filled(band_rect, 2.0, t.accent_soft);
    if solves.len() > MAX_SOLVE_TICKS {
        p.rect_filled(band_rect.shrink2(vec2(0.0, 3.0)), 0.0, t.accent.gamma_multiply(0.6));
    } else {
        for f in solves.iter().filter(|f| f.is_finite()) {
            p.vline(x(*f), (base - 13.0)..=base, Stroke::new(1.0, t.accent));
        }
    }
    // The axis and its ticks.
    p.hline(left..=right, base, Stroke::new(1.0, t.text_dim));
    let raw = (hi - lo) / 6.0;
    let mag = 10f64.powf(raw.log10().floor());
    let step = [1.0, 2.0, 5.0, 10.0].into_iter().map(|k| k * mag).find(|s| *s >= raw).unwrap_or(10.0 * mag);
    let mut v = (lo / step).ceil() * step;
    for _ in 0..12 {
        if v > hi || !v.is_finite() {
            break;
        }
        p.vline(x(v), base..=(base + 3.0), Stroke::new(1.0, t.text_dim));
        p.text(pos2(x(v), base + 4.0), egui::Align2::CENTER_TOP, fmt((v / step).round() * step), small.clone(), t.text_dim);
        v += step;
    }
    p.text(pos2(right, base + 4.0), egui::Align2::RIGHT_TOP, fu, small.clone(), t.text_dim);
    // The cut-offs, labelled on two staggered rows.
    for (i, (f, label, first)) in cuts.iter().enumerate() {
        let color = if *first { t.warning } else { t.text_dim };
        let top = rect.top() + 1.0 + 12.0 * (i % 2) as f32;
        let xf = x(*f);
        p.extend(egui::Shape::dashed_line(&[pos2(xf, top + 2.0), pos2(xf, base)], Stroke::new(1.2, color), 3.0, 2.0));
        let (anchor, at) = if xf > rect.center().x { (egui::Align2::RIGHT_TOP, xf - 3.0) } else { (egui::Align2::LEFT_TOP, xf + 3.0) };
        p.text(pos2(at, top), anchor, label, FontId::proportional(t.font_small - 1.0), color);
    }
    let n = solves.len();
    let mut tip = format!("band {} to {} {fu}; the solve works at {n} frequenc{}", fmt(a), fmt(z), if n == 1 { "y" } else { "ies" });
    for (f, label, _) in &cuts {
        tip.push_str(&format!("\ncut-off {label}: {} {fu}", fmt((f * 1e4).round() / 1e4)));
    }
    resp.on_hover_text(tip);
    app.workflow.diagram = Some(json!({"cutoffs": cuts.iter().map(|c| c.1.clone()).collect::<Vec<_>>(), "solves": n}));
}

/// An error band with a heading and its details.
fn refusal_band(ui: &mut egui::Ui, heading: &str, t: &Tokens, details: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(4.0);
    let r = egui::Frame::NONE.fill(t.form_error_fill).corner_radius(t.radius_sm).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(egui::RichText::new(heading).size(t.font_body).strong().color(t.form_error));
        details(ui);
    });
    scenario::publish_rect("run:error", r.response.rect);
}

/// Run: the preflight's findings and the button that starts the solve.
fn run_panel(app: &mut EmcraftApp, ui: &mut egui::Ui, states: &[StepState], t: &Tokens) {
    ui_kit::section(ui, "Preflight");
    let pre = preflight(app, states);
    if pre.blocking.is_empty() && pre.warnings.is_empty() {
        ui.label(egui::RichText::new("Every step is ready.").size(t.font_small).color(t.success));
    }
    let mut go = None;
    for (list, color, what) in [(&pre.blocking, t.error, "blocks"), (&pre.warnings, t.warning, "warning")] {
        for (i, (step, text)) in list.iter().enumerate() {
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(format!("{} · {text}", step.label())).size(t.font_small).color(color));
                let r = ui.small_button(format!("Go to {}", step.label()));
                scenario::publish_target(&format!("preflight:{what}:{i}"), r.rect);
                if r.clicked() {
                    go = Some(*step);
                }
            });
        }
    }
    if let Some(s) = go {
        open_step(app, Some(s));
        return;
    }
    let solver = app.workflow.fields.solver.unwrap_or_else(|| Solver::infer(&app.session.model));
    ui.add_space(6.0);
    ui.label(egui::RichText::new(format!("Solver: {}", solver.label())).size(t.font_small).color(t.text));
    let label = if !pre.blocking.is_empty() {
        format!("Run — {} problem(s) to fix first", pre.blocking.len())
    } else if pre.warnings.is_empty() {
        "Run".to_owned()
    } else {
        format!("Run, accepting {} warning(s)", pre.warnings.len())
    };
    let busy = app.busy();
    if ui_kit::button(ui, &label, true, !busy, "run:start").clicked() {
        let _ = start_run(app);
    }
    // Why Run was refused, under the button: each blocking problem with the way to its step,
    // as long as it blocks.
    if app.workflow.fields.refused.is_some() && pre.blocking.is_empty() {
        app.workflow.fields.refused = None;
    }
    let mut go = None;
    if app.workflow.fields.refused.is_some() {
        refusal_band(ui, &format!("Run refused: {} problem(s) to fix first", pre.blocking.len()), t, |ui| {
            for (i, (step, text)) in pre.blocking.iter().enumerate() {
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(format!("• {} · {text}", step.label())).size(t.font_small).color(t.form_error));
                    let r = ui.small_button(format!("Go to {}", step.label()));
                    scenario::publish_target(&format!("run:refused:{i}"), r.rect);
                    if r.clicked() {
                        go = Some(*step);
                    }
                });
            }
        });
    }
    // A solve that started and failed says why here too.
    if !busy && let Some((_, e)) = app.failure.as_ref().filter(|f| f.0 == solver.command()) {
        refusal_band(ui, &format!("{} failed", crate::label(solver.command())), t, |ui| {
            ui.label(egui::RichText::new(e).size(t.font_small).color(t.form_error));
        });
    }
    if let Some(s) = go {
        open_step(app, Some(s));
        return;
    }
    if busy {
        ui.label(egui::RichText::new("Running — progress and Cancel below.").size(t.font_small).color(t.text_dim));
    }
    if let Some(e) = estimate(app).ok().filter(|_| solver == Solver::Frequency) {
        let n = |k: &str| e.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN);
        let points = app.workflow.fields.points.trim().parse::<f64>().unwrap_or(11.0);
        ui_kit::caption(
            ui,
            &format!(
                "About {:.0} unknowns, {:.0} MB, {:.1} s for {points} frequencies (fast sweep: less).",
                n("unknowns"),
                n("memory_mb"),
                n("seconds_per_frequency") * points
            ),
        );
    }
}

fn results(app: &mut EmcraftApp, ui: &mut egui::Ui, states: &[StepState], t: &Tokens) {
    let stale = states.get(Step::Results.index()).is_some_and(|s| s.status == Status::Stale);
    if stale {
        ui_kit::error_band(ui, "These results belong to an earlier state of the model.");
    }
    if !app.session.results.any() {
        ui.label(
            egui::RichText::new("No results yet: Run (step 7). The view tabs over the viewport show them.").size(t.font_small).color(t.text_dim),
        );
        return;
    }
    if app.session.results.sweep.is_none() {
        return;
    }
    ui_kit::section(ui, "Checks");
    let busy = app.busy();
    if ui_kit::button(ui, "Passivity and reciprocity", false, !busy, "results:check").clicked() {
        match app.run("results.check", json!({})) {
            Ok(v) => app.workflow.check = Some((app.revision, v)),
            Err(e) => app.message(e, true),
        }
    }
    if let Some((_, c)) = &app.workflow.check {
        let n = |k: &str| c.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN);
        let passive = c.get("passive").and_then(Value::as_bool) == Some(true);
        kv(ui, "largest σ(S)", format!("{:.6} — {}", n("largest_singular_value"), if passive { "passive" } else { "NOT passive" }), t);
        kv(ui, "max |S − S^T|", format!("{:.3e}", n("reciprocity_error")), t);
    }
    ui_kit::section(ui, "Touchstone");
    let ports = app.session.model.ports.len().max(1);
    let f = &mut app.workflow.fields;
    if f.touchstone.is_empty() {
        f.touchstone = format!("emcraft.s{ports}p");
    }
    if f.format.is_empty() {
        f.format = "ri".into();
    }
    let ep = f.error("touchstone").map(str::to_owned);
    field(ui, "path", &mut f.touchstone, "", ep.as_deref(), "results:touchstone:path");
    ui.horizontal(|ui| {
        ui.add_space(t.form_label_width + 6.0);
        for (v, label) in [("ri", "real, imaginary"), ("ma", "magnitude, angle")] {
            let r = ui.radio_value(&mut f.format, v.to_owned(), egui::RichText::new(label).size(t.font_small));
            scenario::publish_target(&format!("results:touchstone:{v}"), r.rect);
        }
    });
    if ui_kit::button(ui, "Export", true, !busy, "results:touchstone").clicked() {
        let (path, format) = (f.touchstone.trim().to_owned(), f.format.clone());
        if path.is_empty() {
            f.set_error("touchstone", Some("give a file".into()));
        } else {
            f.set_error("touchstone", None);
            if let Err(e) = app.run("results.touchstone", json!({"path": path, "format": format})) {
                app.workflow.fields.set_error("touchstone", Some(e.to_string()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_read_their_ids_and_commands() {
        for s in Step::ALL {
            assert_eq!(s.id().parse::<Step>(), Ok(s));
        }
        assert_eq!(Step::of_command("geometry.solid"), Some(Step::Geometry));
        assert_eq!(Step::of_command("solve.port_modes"), Some(Step::Excitation));
        assert_eq!(overlay(Step::Mesh), Overlay::Mesh);
    }
}
