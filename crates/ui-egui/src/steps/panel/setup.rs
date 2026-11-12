//! The Setup step: the solver, its settings over a diagram of the band, and the device it runs on.

use super::*;
use crate::Hint;
use crate::lang::args;
use emcraft_engine::{Device, Precision};

pub(super) fn setup(app: &EmcraftApp, f: &mut Fields, ui: &mut egui::Ui, out: &mut Actions) {
    let fu = app.session.display().frequency.symbol();
    // Home is the sole authority for the analysis, including its inferred (auto) choice.
    let solver = app.session.model().analysis();
    *f = emcraft_ui_core::steps::band::with_proposal(std::mem::take(f).with_defaults(), &app.session);
    ui_kit::section(ui, t!("Settings"));
    let limit = app.session.limits().frequencies as u64;
    if let Some(d) = emcraft_ui_core::steps::band::diagram(&app.session, f, emcraft_ui_core::steps::band::solves(solver, f, limit)) {
        band_diagram(app, ui, &d);
    }
    let e = |f: &Fields, k: &str| f.shown_error(k).map(str::to_owned);
    let lu = app.session.display().length.symbol();
    match solver {
        Solver::Frequency | Solver::TimeDomain => {
            let (es, ez) = (e(f, "start"), e(f, "stop"));
            field(ui, t!("start"), &mut f.start, fu, es.as_deref(), Fixed::SetupStart);
            field(ui, t!("stop"), &mut f.stop, fu, ez.as_deref(), Fixed::SetupStop);
            let ep = e(f, "points");
            field(ui, t!("Frequency points"), &mut f.points, "", ep.as_deref(), Fixed::SetupPoints);
            if solver == Solver::TimeDomain {
                let ec = e(f, "cell");
                field(ui, t!("cell"), &mut f.cell, lu, ec.as_deref(), Fixed::SetupCell);
            }
        }
        Solver::Eigenmodes => {
            let ec = e(f, "count");
            field(ui, t!("modes"), &mut f.count, "", ec.as_deref(), Fixed::SetupCount);
            let ef = e(f, "frequency");
            field(ui, t!("near"), &mut f.frequency, fu, ef.as_deref(), Fixed::SetupFrequency);
        }
        Solver::Pcb => {}
        Solver::System => {
            let (es, ez) = (e(f, "start"), e(f, "stop"));
            field(ui, t!("start"), &mut f.start, fu, es.as_deref(), Fixed::SetupStart);
            field(ui, t!("stop"), &mut f.stop, fu, ez.as_deref(), Fixed::SetupStop);
            let ep = e(f, "points");
            field(ui, t!("Frequency points"), &mut f.points, "", ep.as_deref(), Fixed::SetupPoints);
            let ef = e(f, "frequency");
            field(ui, t!("budget at"), &mut f.frequency, fu, ef.as_deref(), Fixed::SetupFrequency);
            ui_kit::caption(ui, t!("Empty: the band's centre."));
        }
        Solver::Radiation => {
            let ef = e(f, "frequency");
            field(ui, t!("frequency"), &mut f.frequency, fu, ef.as_deref(), Fixed::SetupFrequency);
        }
    }
    match solver {
        Solver::TimeDomain | Solver::Pcb | Solver::System => {}
        Solver::Frequency | Solver::Eigenmodes | Solver::Radiation => {
            let eo = e(f, "order");
            field(ui, t!("Element order"), &mut f.order, "", eo.as_deref(), Fixed::SetupOrder);
            ui_kit::caption(ui, t!("Order 2: more accurate, more memory."));
        }
    }
    // Before the mesh there is nothing to estimate from: say when there will be. A system's
    // chain is read in a moment, on no device.
    if solver != Solver::System {
        if app.session.model().mesh.is_none() {
            ui_kit::small(ui, t!("The run's time is estimated once the model is meshed (step 6)."), Tokens::get().text_dim);
        } else if let Some(e) = run_estimate(app) {
            super::super::estimate_line(ui, e, Fixed::SetupEstimate);
        }
    }
    // The fields' errors as typed.
    let ok = setup_params(&app.session, f).is_ok();
    confirm(app, ui, ok, out);
    if solver != Solver::System {
        device(app, ui, solver, out);
    }
}

/// The button that confirms the form (`project.setup`): the accent while the settings are not
/// confirmed as they stand — the step, and Run after it, wait for it — and a check once they are.
fn confirm(app: &EmcraftApp, ui: &mut egui::Ui, ok: bool, out: &mut Actions) {
    let t = &Tokens::get();
    ui.add_space(8.0);
    let pending = super::super::unconfirmed(app);
    if pending {
        let label = format!("✔ {}", tr(t!("Confirm settings")));
        let r = ui.act(ok, ui_kit::styled(Raw(&label), true), Fixed::SetupConfirm.into(), Action::ConfirmSetup, out);
        let _ = r.hint_disabled(t!("put the marked fields right first"));
        ui_kit::small(ui, t!("Not confirmed: the mesh and Run wait for these settings."), t.warning);
    } else {
        let r = ui.label(egui::RichText::new(format!("✔ {}", tr(t!("Settings confirmed")))).size(t.font_body).color(t.success));
        scenario::publish_target(Fixed::SetupConfirm, r.rect);
    }
}

/// Where the solve runs: the devices `solve.devices` lists (those this machine cannot run
/// disabled, with the reason on hover) and the time domain's precision, set through
/// `solve.device`.
fn device(app: &EmcraftApp, ui: &mut egui::Ui, solver: Solver, out: &mut Actions) {
    let t = &Tokens::get();
    ui_kit::section(ui, t!("Compute device"));
    let none = json!({});
    let list = app.workflow.devices.as_ref().unwrap_or(&none);
    let current = app.session.compute().device();
    let precision = app.session.compute().precision;
    let busy = app.busy();
    let devices = super::super::device_rows(list);
    let mut chosen: Option<(Device, Precision)> = None;
    ui_kit::row(ui, t!("device"), false, |ui| {
        for d in &devices {
            let name = d.device.id();
            let radio = egui::RadioButton::new(current == d.device, egui::RichText::new(name).size(t.font_small));
            let r = ui.named_enabled(d.available && !busy, radio, Handle::Device(d.device));
            let r = match (d.available, &d.problem) {
                (false, Some(p)) => r.hint_disabled(trf(t!("{name} cannot run here: {problem}"), args!(name = name, problem = p))),
                (false, None) => r.hint_disabled(trf(t!("{name} cannot run here"), args!(name = name))),
                (true, _) => r.hint(match d.device {
                    Device::Cpu => t!("the CPU's threads"),
                    Device::Cuda => t!("an NVIDIA GPU"),
                    Device::CubeclCpu => t!("the GPU's kernels on the CPU (to check them without a GPU)"),
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
    ui_kit::row(ui, t!("precision"), false, |ui| {
        for p in Precision::ALL {
            let on = if on_cpu { p == Precision::Double } else { precision == p };
            let enabled = !on_cpu && runs(p) && !busy;
            let radio = egui::RadioButton::new(on, egui::RichText::new(crate::lang::lookup(p.id())).size(t.font_small));
            let r = ui.named_enabled(enabled, radio, Handle::Precision(p));
            let r = r.hint_disabled(if on_cpu { t!("the CPU's threads run in double") } else { t!("this device does not run it") });
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
        _ if pending => t!("Switching the device…"),
        Solver::TimeDomain if !on_cpu => t!("The next solve runs on {device} in {precision} precision."),
        Solver::TimeDomain => t!("The next solve runs on the CPU's threads (double precision)."),
        Solver::Frequency | Solver::Eigenmodes | Solver::Radiation if !on_cpu => {
            t!("The next solve runs on the CPU, its factorization's large fronts on {device} (complex double).")
        }
        _ => t!("The next solve runs on the CPU's threads."),
    };
    let next = trf(template, args!(device = current.id(), precision = crate::lang::lookup(precision.id())));
    ui_kit::small(ui, next, t.text);
    if let Some((device, precision)) = chosen {
        out.push(Action::SetDevice { device, precision });
    }
}

/// Most ticks the diagram draws for the solve's frequencies (beyond, they fill the band).
const MAX_SOLVE_TICKS: usize = 400;

/// The band as a small diagram ([`emcraft_ui_core::steps::band::diagram`]): a frequency axis
/// over the band (shaded), the cut-offs near it as dashed lines (a port's first mode in the
/// warning colour: below it no power enters), and the frequencies the solve will work at as
/// ticks.
fn band_diagram(app: &EmcraftApp, ui: &mut egui::Ui, d: &emcraft_ui_core::steps::band::Diagram) {
    let t = &Tokens::get();
    let fu = app.session.display().frequency.symbol();
    let ((lo, hi), (a, z)) = (d.span, d.band);
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
    if d.solves.len() > MAX_SOLVE_TICKS {
        p.rect_filled(band_rect.shrink2(vec2(0.0, 3.0)), 0.0, t.accent.gamma_multiply(0.6));
    } else {
        for f in d.solves.iter().filter(|f| f.is_finite()) {
            p.vline(x(*f), (base - 13.0)..=base, Stroke::new(1.0, t.accent));
        }
    }
    // The axis and its ticks.
    p.hline(left..=right, base, Stroke::new(1.0, t.text_dim));
    for v in &d.ticks {
        p.vline(x(*v), base..=(base + 3.0), Stroke::new(1.0, t.text_dim));
        p.text(pos2(x(*v), base + 4.0), egui::Align2::CENTER_TOP, fmt(*v), small.clone(), t.text_dim);
    }
    p.text(pos2(right, base + 4.0), egui::Align2::RIGHT_TOP, fu, small, t.text_dim);
    // The cut-offs, labelled on two staggered rows.
    for (i, c) in d.cuts.iter().enumerate() {
        let color = if c.first { t.warning } else { t.text_dim };
        let top = rect.top() + 1.0 + 12.0 * (i % 2) as f32;
        let xf = x(c.at);
        p.extend(egui::Shape::dashed_line(&[pos2(xf, top + 2.0), pos2(xf, base)], Stroke::new(1.2, color), 3.0, 2.0));
        let (anchor, at) = if xf > rect.center().x { (egui::Align2::RIGHT_TOP, xf - 3.0) } else { (egui::Align2::LEFT_TOP, xf + 3.0) };
        p.text(pos2(at, top), anchor, &c.label, FontId::proportional(t.font_small - 1.0), color);
    }
    resp.on_hover_text(&d.tip);
}
