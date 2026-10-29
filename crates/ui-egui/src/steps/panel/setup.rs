//! The Setup step: the solver, its settings over a diagram of the band, and the device it runs on.

use super::*;

pub(super) fn setup(app: &mut EmcraftApp, ui: &mut egui::Ui, fu: &str, t: &Tokens) {
    ui_kit::section(ui, "Solver");
    let inferred = Solver::infer(&app.session.model);
    let f = &mut app.workflow.fields;
    let mut solver = f.solver.unwrap_or(inferred);
    for s in Solver::ALL {
        let r = ui.radio_value(&mut solver, s, egui::RichText::new(s.label()).size(t.font_small));
        scenario::publish_target(format_args!("setup:solver:{}", s.id()), r.rect);
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
            scenario::publish_target(format_args!("setup:device:{name}"), r.rect);
            if r.clicked() && current != name {
                // The precision set so far where the device runs it, else its first.
                let p = if precisions.iter().any(|p| p == precision) { Some(precision.to_owned()) } else { precisions.first().cloned() };
                chosen = Some((name.clone(), p.unwrap_or_else(|| precision.to_owned())));
            }
        }
    });
    if let Some(e) = list.get("error").and_then(Value::as_str) {
        ui_kit::small(ui, e, t.form_error);
    }
    // The CPU's threads run in double; a device runs the precisions it lists.
    let runs: Vec<String> = devices.iter().find(|d| d.0 == current).map(|d| d.2.clone()).unwrap_or_default();
    ui_kit::row(ui, "precision", false, |ui| {
        for p in ["single", "double"] {
            let on = if current == "cpu" { p == "double" } else { precision == p };
            let enabled = current != "cpu" && runs.iter().any(|x| x == p) && !busy;
            let r = ui.add_enabled(enabled, egui::RadioButton::new(on, egui::RichText::new(p).size(t.font_small)));
            let r = r.on_disabled_hover_text(if current == "cpu" { "the CPU's threads run in double" } else { "this device does not run it" });
            scenario::publish_target(format_args!("setup:precision:{p}"), r.rect);
            if r.clicked() && !on {
                chosen = Some((current.to_owned(), p.to_owned()));
            }
        }
    });
    if let Some(crate::Failure { error: e, .. }) = app.failure.as_ref().filter(|f| f.command == "solve.device") {
        ui.horizontal_wrapped(|ui| {
            ui.add_space(t.form_label_width + 6.0);
            ui_kit::small(ui, e, t.form_error);
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
    ui_kit::small(ui, next, t.text);
    if let Some((device, precision)) = chosen
        && let Err(e) = app.run("solve.device", json!({"device": device, "precision": precision}))
    {
        app.failure = Some(crate::Failure { command: "solve.device".into(), error: e.to_string() });
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
        ui_kit::small(ui, "band: not set (step 0)", t.error);
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
