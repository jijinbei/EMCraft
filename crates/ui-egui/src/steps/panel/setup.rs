//! The Setup step: the solver, its settings over a diagram of the band, and the device it runs on.

use super::*;
use crate::Hint;
use crate::lang::args;
use emcraft_engine::{Device, Precision};

pub(super) fn setup(app: &EmcraftApp, f: &mut Fields, ui: &mut egui::Ui, out: &mut Actions) {
    let fu = app.session.display().frequency.symbol();
    // Home is the sole authority for the analysis, including its inferred (auto) choice.
    let solver = app.session.model().analysis();
    f.fill_defaults();
    ui_kit::section(ui, "Settings");
    let solves = solve_frequencies(app, f, solver);
    f.diagram = band_diagram(app, f, ui, &solves);
    let e = |f: &Fields, k: &str| f.error(k).map(str::to_owned);
    let lu = app.session.display().length.symbol();
    match solver {
        Solver::Frequency | Solver::TimeDomain => {
            let (es, ez) = (e(f, "start"), e(f, "stop"));
            field(ui, "start", &mut f.start, fu, es.as_deref(), Fixed::SetupStart);
            field(ui, "stop", &mut f.stop, fu, ez.as_deref(), Fixed::SetupStop);
            let ep = e(f, "points");
            field(ui, "Frequency points", &mut f.points, "", ep.as_deref(), Fixed::SetupPoints);
            if solver == Solver::TimeDomain {
                let ec = e(f, "cell");
                field(ui, "cell", &mut f.cell, lu, ec.as_deref(), Fixed::SetupCell);
            }
        }
        Solver::Eigenmodes => {
            let ec = e(f, "count");
            field(ui, "modes", &mut f.count, "", ec.as_deref(), Fixed::SetupCount);
            let ef = e(f, "frequency");
            field(ui, "near", &mut f.frequency, fu, ef.as_deref(), Fixed::SetupFrequency);
        }
        Solver::Pcb => {}
        Solver::Radiation => {
            let ef = e(f, "frequency");
            field(ui, "frequency", &mut f.frequency, fu, ef.as_deref(), Fixed::SetupFrequency);
        }
    }
    match solver {
        Solver::TimeDomain | Solver::Pcb => {}
        Solver::Frequency | Solver::Eigenmodes | Solver::Radiation => {
            let eo = e(f, "order");
            field(ui, "Element order", &mut f.order, "", eo.as_deref(), Fixed::SetupOrder);
            ui_kit::caption(
                ui,
                "Order 1 uses less memory; order 2 usually improves accuracy on the same mesh. Compare refined meshes to check convergence.",
            );
        }
    }
    if let Some(e) = run_estimate(app) {
        super::super::estimate_line(ui, e, Fixed::SetupEstimate);
    }
    // The fields' errors as typed.
    let ok = setup_params(&app.session, f).is_ok();
    confirm(app, ui, ok, out);
    device(app, ui, solver, out);
}

/// The button that confirms the form (`project.setup`): the accent while the settings are not
/// confirmed as they stand — the step, and Run after it, wait for it — and a check once they are.
fn confirm(app: &EmcraftApp, ui: &mut egui::Ui, ok: bool, out: &mut Actions) {
    let t = &Tokens::get();
    ui.add_space(8.0);
    let pending = super::super::unconfirmed(app);
    if pending {
        let label = format!("✔ {}", tr("Confirm settings"));
        let r = ui.act(ok && !app.busy(), ui_kit::styled(Raw(&label), true), Fixed::SetupConfirm.into(), "", || Action::ConfirmSetup, out);
        let _ = r.hint_disabled("put the marked fields right first");
        ui_kit::small(ui, "Not confirmed: the mesh and Run wait for these settings.", t.warning);
    } else {
        let r = ui.label(egui::RichText::new(format!("✔ {}", tr("Settings confirmed"))).size(t.font_body).color(t.success));
        scenario::publish_target(Fixed::SetupConfirm, r.rect);
    }
}

/// Where the solve runs: the devices `solve.devices` lists (those this machine cannot run
/// disabled, with the reason on hover) and the time domain's precision, set through
/// `solve.device`.
fn device(app: &EmcraftApp, ui: &mut egui::Ui, solver: Solver, out: &mut Actions) {
    let t = &Tokens::get();
    ui_kit::section(ui, "Compute device");
    let none = json!({});
    let list = app.workflow.devices.as_ref().unwrap_or(&none);
    let current = app.session.compute().device();
    let precision = app.session.compute().precision;
    let busy = app.busy();
    let devices = super::super::device_rows(list);
    let mut chosen: Option<(Device, Precision)> = None;
    ui_kit::row(ui, "device", false, |ui| {
        for d in &devices {
            let name = d.device.id();
            let radio = egui::RadioButton::new(current == d.device, egui::RichText::new(name).size(t.font_small));
            let r = ui.named_enabled(d.available && !busy, radio, Handle::Device(d.device));
            let r = match (d.available, &d.problem) {
                (false, Some(p)) => r.hint_disabled(trf("{name} cannot run here: {problem}", args!(name = name, problem = p))),
                (false, None) => r.hint_disabled(trf("{name} cannot run here", args!(name = name))),
                (true, _) => r.hint(match d.device {
                    Device::Cpu => "the CPU's threads",
                    Device::Cuda => "an NVIDIA GPU",
                    Device::CubeclCpu => "the GPU's kernels on the CPU (to check them without a GPU)",
                }),
            };
            if r.clicked() && current != d.device {
                // The precision set so far where the device runs it, else its first.
                let p = if d.runs(precision) { precision } else { d.precisions.first().copied().unwrap_or(precision) };
                chosen = Some((d.device, p));
            }
        }
    });
    if let Some(e) = list.get("error").and_then(Value::as_str) {
        ui_kit::small(ui, Raw(e), t.form_error);
    }
    // The CPU's threads run in double; a device runs the precisions it lists.
    let on_cpu = current == Device::Cpu;
    let runs = |p: Precision| devices.iter().any(|d| d.device == current && d.runs(p));
    ui_kit::row(ui, "precision", false, |ui| {
        for p in Precision::ALL {
            let on = if on_cpu { p == Precision::Double } else { precision == p };
            let enabled = !on_cpu && runs(p) && !busy;
            let radio = egui::RadioButton::new(on, egui::RichText::new(tr(p.id())).size(t.font_small));
            let r = ui.named_enabled(enabled, radio, Handle::Precision(p));
            let r = r.hint_disabled(if on_cpu { "the CPU's threads run in double" } else { "this device does not run it" });
            if r.clicked() && !on {
                chosen = Some((current, p));
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
    let template = match solver {
        _ if pending => "Switching the device…",
        Solver::TimeDomain if !on_cpu => "The next solve runs on {device} in {precision} precision.",
        Solver::TimeDomain => "The next solve runs on the CPU's threads (double precision).",
        Solver::Frequency | Solver::Eigenmodes | Solver::Radiation if !on_cpu => {
            "The next solve runs on the CPU, its factorization's large fronts on {device} (complex double)."
        }
        _ => "The next solve runs on the CPU's threads.",
    };
    let next = trf(template, args!(device = current.id(), precision = tr(precision.id())));
    ui_kit::small(ui, next, t.text);
    if let Some((device, precision)) = chosen {
        out.push(Action::SetDevice { device, precision });
    }
}

/// The frequencies (display units) the solve will work at, as the panel's fields stand: the
/// sweep's points over the band, or the one frequency of an eigenmode or radiation solve.
fn solve_frequencies(app: &EmcraftApp, f: &Fields, solver: Solver) -> Vec<f64> {
    match solver {
        Solver::Frequency | Solver::TimeDomain => {
            let Some((a, z)) = form_band(f) else { return Vec::new() };
            let Ok(n) = integer(&f.points, 1, app.session.limits().frequencies as u64) else { return Vec::new() };
            (0..n).map(|k| if n == 1 { a } else { a + (z - a) * k as f64 / (n - 1) as f64 }).collect()
        }
        Solver::Pcb => Vec::new(),
        Solver::Eigenmodes | Solver::Radiation => positive(&f.frequency).map(|v| vec![v]).unwrap_or_default(),
    }
}

/// Most ticks the diagram draws for the solve's frequencies (beyond, they fill the band).
const MAX_SOLVE_TICKS: usize = 400;

/// The band as a small diagram: a frequency axis over the band (shaded), the cut-offs of the
/// ports' modes and the guide's sections near it as dashed lines (a port's first mode in the
/// warning colour: below it no power enters), and the frequencies the solve will work at as
/// ticks. What it drew, for `ui.inspect`.
fn band_diagram(app: &EmcraftApp, f: &Fields, ui: &mut egui::Ui, solves: &[f64]) -> Option<Value> {
    let t = &Tokens::get();
    let unit = app.session.display().frequency;
    let fu = unit.symbol();
    let (a, z) = form_band(f).or_else(|| solves.first().map(|x| (*x, *x)))?;
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
    scenario::publish_target(Fixed::SetupBand, rect);
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
    let template = if n == 1 {
        "band {a} to {z} {unit}; the solve works at {n} frequency"
    } else {
        "band {a} to {z} {unit}; the solve works at {n} frequencies"
    };
    let mut tip = trf(template, args!(a = fmt(a), z = fmt(z), unit = fu, n = n));
    for (f, label, _) in &cuts {
        tip.push('\n');
        tip.push_str(&trf("cut-off {mode}: {f} {unit}", args!(mode = label, f = fmt((f * 1e4).round() / 1e4), unit = fu)));
    }
    resp.on_hover_text(tip);
    Some(json!({"cutoffs": cuts.iter().map(|c| c.1.clone()).collect::<Vec<_>>(), "solves": n}))
}

/// The band (display units) the form's start and stop give, when they do.
fn form_band(f: &Fields) -> Option<(f64, f64)> {
    let (a, z) = (positive(&f.start).ok()?, positive(&f.stop).ok()?);
    (a <= z).then_some((a, z))
}
