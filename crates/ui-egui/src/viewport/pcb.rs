//! Guided PCB diagnosis: board and stackup, route, conditions, diagnosis and results.
use super::*;
use crate::lang::{Raw, args};
use crate::ui_kit;
use emcraft_engine::pcb::{Stage, near};
use emcraft_model::units::{Time, Unit};
use std::collections::BTreeSet;

fn go(out: &mut Actions, stage: Stage) {
    out.push(Action::run("view.pcb", json!({"stage":stage.id()})));
}
fn report<'a>(app: &'a EmcraftApp, state: &crate::state::PcbUi, net: &str) -> Option<&'a Value> {
    app.replies.pcb.as_ref().filter(|r| {
        r["net"].as_str() == Some(net)
            && r["ground"].as_str() == Some(state.ground.trim())
            && r["reference"].as_f64() == state.reference.parse::<f64>().ok()
            && r["stop"].as_f64() == state.stop.parse::<f64>().ok()
            && r["frequency_unit"] == app.session.display().frequency.symbol()
    })
}

/// The PCB-specific workflow stays above the workspace, as the FEM workflow does.
pub(crate) fn bar(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    let current = emcraft_engine::pcb::stage(&app.session);
    let loaded = app.session.model().pcb.is_some();
    let net =
        app.session.views().pcb_net.as_deref().filter(|n| app.session.model().pcb.as_ref().is_some_and(|b| b.segments.iter().any(|s| s.net == *n)));
    let ready = net.is_some_and(|n| report(app, &app.views.pcb, n).is_some());
    egui::Panel::top("pcb_steps").show_separator_line(false).frame(egui::Frame::NONE.fill(Tokens::get().viewport_top).inner_margin(6)).show(
        ui,
        |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.selectable_label(app.view == View::Home, tr("Home")).clicked() {
                    out.push(Action::ShowView(View::Home));
                }
                for (i, stage) in Stage::ALL.into_iter().enumerate() {
                    let enabled = match stage {
                        Stage::Board => true,
                        Stage::Net => loaded,
                        Stage::Settings | Stage::Inspect => loaded && net.is_some(),
                        Stage::Results => loaded && ready,
                    };
                    let done = match stage {
                        Stage::Board => loaded,
                        Stage::Net => net.is_some(),
                        _ => ready,
                    };
                    let t = Tokens::get();
                    let label = format!("{} {}", i + 1, shown(stage));
                    let galley =
                        ui.painter().layout_no_wrap(label.clone(), FontId::proportional(t.font_body), if enabled { t.text } else { t.text_dim });
                    let (rect, _) = ui.allocate_exact_size(vec2(galley.size().x + 46., 30.), Sense::hover());
                    let r = ui.interact(
                        rect,
                        egui::Id::new(("pcb_step", stage.id())),
                        if enabled && !app.busy() { Sense::click() } else { Sense::hover() },
                    );
                    let active = app.view == View::Pcb && stage == current;
                    ui.painter().rect(
                        rect,
                        12.,
                        if active {
                            t.accent_soft
                        } else if r.hovered() {
                            t.hover
                        } else {
                            Color32::TRANSPARENT
                        },
                        Stroke::new(1., if active { t.accent } else { t.border }),
                        egui::StrokeKind::Inside,
                    );
                    crate::steps::mark(
                        ui.painter(),
                        pos2(rect.left() + 14., rect.center().y),
                        if done {
                            crate::steps::Status::Done
                        } else if enabled {
                            crate::steps::Status::Todo
                        } else {
                            crate::steps::Status::Waiting
                        },
                        &t,
                    );
                    ui.painter().galley(pos2(rect.left() + 28., rect.center().y - galley.size().y / 2.), galley, t.text);
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label.clone()));
                    scenario::publish_target(Handle::Choice("pcb_step", stage.id()), rect);
                    if r.clicked() {
                        go(out, stage);
                    }
                }
            });
        },
    );
}

pub(super) fn show(app: &EmcraftApp, st: &mut Parts, ui: &mut egui::Ui, out: &mut Actions) {
    let stage = emcraft_engine::pcb::stage(&app.session);
    let state = &mut st.views.pcb;
    let board = app.session.model().pcb.as_ref();
    let selected = app.session.views().pcb_net.as_deref().filter(|n| board.is_some_and(|b| b.segments.iter().any(|s| s.net == *n))).unwrap_or("");
    let result = report(app, state, selected);
    ui.heading(shown(stage));
    ui_kit::caption(
        ui,
        match stage {
            Stage::Board => "1. Import a PCB or open a template, then check its copper layers and substrate thickness.",
            Stage::Net => {
                "2. Click the trace in the board view, or choose its net from the list. Orange is the selected route; green is reference copper."
            }
            Stage::Settings => "3. Set the reference net, source/load impedance and maximum frequency. The stackup below comes from the board.",
            Stage::Inspect => "4. Review the selected route and conditions, then run the diagnosis.",
            Stage::Results => "5. Red markers locate missing reference copper. Review the findings before reading the ideal line estimates.",
        },
    );
    ui.horizontal_wrapped(|ui| {
        if let Some(previous) = match stage {
            Stage::Board => None,
            Stage::Net => Some(Stage::Board),
            Stage::Settings => Some(Stage::Net),
            Stage::Inspect => Some(Stage::Settings),
            Stage::Results => Some(Stage::Inspect),
        } {
            let r = ui.button(tr("Back"));
            scenario::publish_target(Handle::Choice("pcb_control", "back"), r.rect);
            if r.clicked() {
                go(out, previous);
            }
        }
        let next = match stage {
            Stage::Board if board.is_some() => Some(Stage::Net),
            Stage::Net if !selected.is_empty() => Some(Stage::Settings),
            Stage::Settings => Some(Stage::Inspect),
            _ => None,
        };
        if let Some(next) = next {
            let r = ui.button(trf("Next: {step}", args!(step = shown(next))));
            scenario::publish_target(Handle::Choice("pcb_control", "next"), r.rect);
            if r.clicked() {
                go(out, next);
            }
        }
    });
    if stage == Stage::Board {
        ui.horizontal_wrapped(|ui| {
            if ui.button(tr("Import PCB…")).clicked() {
                out.push(Action::File(crate::files::FileAction::PickPart));
            }
            ui.menu_button(tr("Templates"), |ui| {
                for template in emcraft_engine::templates::made_for(crate::steps::Solver::Pcb) {
                    if ui.button(crate::lang::sample_label(template.name)).clicked() {
                        out.push(Action::Template(Some(template.name.into())));
                        ui.close();
                    }
                }
            });
        });
    }
    let Some(board) = board else {
        ui_kit::caption(ui, "Import a PCB or open an example, then click a routed trace.");
        return;
    };
    if stage == Stage::Net {
        let nets: BTreeSet<_> = board.segments.iter().map(|s| s.net.as_str()).collect();
        let r = egui::ComboBox::from_id_salt("pcb_net")
            .selected_text(if selected.is_empty() { tr("Choose a routed net") } else { selected })
            .show_ui(ui, |ui| {
                for net in nets {
                    let r = ui.selectable_label(net == selected, net);
                    scenario::publish_target(Handle::Choice("pcb_net", net), r.rect);
                    if r.clicked() {
                        out.push(Action::run("view.pcb", json!({"net":net,"stage":"net"})));
                    }
                }
            });
        scenario::publish_target(Handle::Choice("pcb_control", "net"), r.response.rect);
    }
    if stage == Stage::Settings {
        for (label, field) in
            [("Reference net", &mut state.ground), ("Reference impedance (Ω)", &mut state.reference), ("Maximum frequency", &mut state.stop)]
        {
            ui.horizontal(|ui| {
                ui.label(tr(label));
                let r = ui.add(egui::TextEdit::singleline(field).desired_width(110.));
                scenario::publish_target(Handle::Choice("pcb_control", label), r.rect);
            });
        }
        ui.small(app.session.display().frequency.symbol());
    }
    if stage == Stage::Inspect {
        ui.label(format!(
            "{} → {}  |  {} Ω  |  {} {}",
            selected,
            state.ground,
            state.reference,
            state.stop,
            app.session.display().frequency.symbol()
        ));
        let r = ui.add_enabled(!app.busy(), egui::Button::new(tr("Inspect route")));
        scenario::publish_target(Handle::Choice("pcb_control", "inspect"), r.rect);
        if r.clicked() {
            out.push(Action::run("pcb.inspect", state.params(selected)));
        }
    }
    if stage == Stage::Results {
        if let Some(result) = result {
            let r = ui.button(tr("Keep for comparison"));
            scenario::publish_target(Handle::Choice("pcb_control", "keep"), r.rect);
            if r.clicked() {
                state.baseline = Some(result.clone());
            }
        } else if app.replies.pcb.is_some() {
            ui_kit::caption(ui, "Settings changed; inspect the route again.");
        }
        if app.replies.pcb.is_none() && state.attempted.as_ref() != Some(&(app.revision, selected.to_owned())) {
            state.attempted = Some((app.revision, selected.to_owned()));
            out.push(Action::run("pcb.inspect", state.params(selected)));
        }
    }
    if let Some(failure) = app.failure.as_ref().filter(|f| f.command == "pcb.inspect" || f.command == "view.pcb") {
        ui.colored_label(Tokens::get().error, &failure.error);
    }
    let wide = ui.available_width() > 900.;
    let map_width = ui.available_width() * if wide { 0.52 } else { 1. };
    let layout = if wide { egui::Layout::left_to_right(egui::Align::Min) } else { egui::Layout::top_down(egui::Align::Min) };
    ui.with_layout(layout, |ui| {
        ui.allocate_ui_with_layout(vec2(map_width, 310.), egui::Layout::top_down(egui::Align::Min), |ui| {
            draw_board(app, state, ui, out, selected, &state.ground.clone(), result)
        });
        ui.vertical(|ui| match stage {
            Stage::Results => draw_report(app, state, ui, result),
            _ => stackup(app, ui, selected, &state.ground),
        });
    });
}

fn stackup(app: &EmcraftApp, ui: &mut egui::Ui, selected: &str, ground: &str) {
    let Some(board) = app.session.model().pcb.as_ref() else {
        return;
    };
    ui.heading(tr("Layer stackup (section)"));
    ui_kit::caption(ui, "The layout is a top view. Copper depth and dielectric thickness are included in the line estimate.");
    let t = Tokens::get();
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width().min(540.), 180.), Sense::hover());
    scenario::publish_target(Handle::Choice("pcb_control", "stackup"), r);
    let painter = ui.painter_at(r);
    painter.rect_filled(r, 4., t.panel);
    let first = board.layers.first().map_or(0., |l| l.z);
    let last = board.layers.last().map_or(1., |l| l.z);
    let y = |z: f64| r.top() + 28. + ((z - first) / (last - first).max(1e-12)) as f32 * (r.height() - 62.);
    for pair in board.layers.windows(2) {
        let a = y(pair[0].z);
        let b = y(pair[1].z);
        painter.rect_filled(Rect::from_min_max(pos2(r.left() + 12., a), pos2(r.right() - 12., b)), 0., t.accent_soft);
        let h = app.session.display().length.from_si(pair[1].z - pair[0].z);
        painter.text(
            pos2(r.center().x, (a + b) / 2.),
            egui::Align2::CENTER_CENTER,
            format!("εr = {}   ·   {:.3} {}", pair[0].eps_below, h, app.session.display().length.symbol()),
            FontId::proportional(12.),
            t.text,
        );
    }
    for layer in &board.layers {
        let chosen = board.segments.iter().any(|s| s.net == selected && s.layer == layer.name);
        let reference = board.copper.iter().any(|c| c.net == ground && c.layer == layer.name);
        let colour = if chosen {
            t.warning
        } else if reference {
            t.success
        } else {
            t.text_dim
        };
        let at = y(layer.z);
        painter.line_segment([pos2(r.left() + 12., at), pos2(r.right() - 12., at)], Stroke::new(3., colour));
        painter.text(pos2(r.left() + 14., at - 7.), egui::Align2::LEFT_BOTTOM, &layer.name, FontId::proportional(12.), t.text);
    }
    ui_kit::caption(ui, "Depth spacing is schematic; labels show actual dimensions.");
    ui_kit::caption(ui, "This is geometry screening and a quasi-static line estimate. 3D fields at vias, pads, bends and radiation are not solved.");
}

fn draw_board(
    app: &EmcraftApp,
    state: &mut crate::state::PcbUi,
    ui: &mut egui::Ui,
    out: &mut Actions,
    net: &str,
    ground: &str,
    report: Option<&Value>,
) {
    let Some(board) = app.session.model().pcb.as_ref() else {
        return;
    };
    let t = Tokens::get();
    let Some((lo, hi)) = board.bounds() else {
        return;
    };
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width().max(100.0), 310.0), Sense::click());
    scenario::publish_target(Handle::Choice("pcb_control", "board"), rect);
    let pad = rect.shrink(18.0);
    let scale = (pad.width() as f64 / (hi[0] - lo[0]).max(1e-9)).min(pad.height() as f64 / (hi[1] - lo[1]).max(1e-9));
    let to = |p: [f64; 2]| {
        pos2(pad.center().x + ((p[0] - (lo[0] + hi[0]) / 2.0) * scale) as f32, pad.center().y + ((p[1] - (lo[1] + hi[1]) / 2.0) * scale) as f32)
    };
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, t.panel);
    // Even-odd filled copper polygons, including holes/bridges, are drawn as scan spans:
    // outlines alone would make a filled plane look like an unfilled zone boundary.
    if state.copper.as_ref().is_none_or(|c| c.revision != app.revision || c.rect != rect || c.ground != ground || c.theme != crate::theme::key()) {
        let mut shapes = Vec::new();
        let mut fill = egui::Mesh::default();
        for copper in board.copper.iter().filter(|c| c.net == ground) {
            let poly: Vec<_> = copper.polygon.iter().map(|p| to(*p)).collect();
            let coords: Vec<_> = poly.iter().map(|p| [p.x as f64, p.y as f64]).collect();
            for row in 0..150 {
                let step = pad.height() / 150.0;
                let y = pad.top() + (row as f32 + 0.5) * step;
                for [a, b] in emcraft_render::pcb::spans(&coords, y as f64) {
                    let ab = [a as f32, b as f32];
                    fill.add_colored_rect(
                        Rect::from_min_max(pos2(ab[0], y - step / 2.0), pos2(ab[1], y + step / 2.0)),
                        t.success.gamma_multiply(0.18),
                    );
                }
            }
            if poly.len() > 1 {
                shapes.push(egui::Shape::closed_line(poly, Stroke::new(1.0, t.success.gamma_multiply(0.6))));
            }
        }
        shapes.insert(0, egui::Shape::mesh(fill));
        state.copper =
            Some(crate::state::PcbCopperCache { revision: app.revision, rect, ground: ground.to_owned(), theme: crate::theme::key(), shapes });
    }
    if let Some(c) = &state.copper {
        painter.extend(c.shapes.iter().cloned());
    }
    let flagged: BTreeSet<_> = report
        .and_then(|r| r["findings"].as_array())
        .into_iter()
        .flatten()
        .filter(|f| ["return_gap", "reference_edge", "no_reference"].contains(&f["code"].as_str().unwrap_or("")))
        .filter_map(|f| f["segment"].as_str())
        .collect();
    let mut nearest: Option<(f32, &str)> = None;
    for seg in &board.segments {
        let (a, b) = (to(seg.start), to(seg.end));
        let chosen = seg.net == net;
        let colour = match (chosen, flagged.contains(seg.id.as_str())) {
            (true, true) => t.error,
            (true, false) => t.warning,
            _ => t.text_dim,
        };
        painter.line_segment([a, b], Stroke::new((seg.width * scale) as f32 + if chosen { 2.0 } else { 0.5 }, colour));
        painter.circle_filled(a, 2.0, colour);
        painter.circle_filled(b, 2.0, colour);
        let target = Rect::from_two_pos(a, b).expand(3.0);
        scenario::publish_target(Handle::Choice("pcb_segment", &seg.id), target);
        if let Some(pos) = response.interact_pointer_pos() {
            let (d, _) = near([pos.x as f64, pos.y as f64], [a.x as f64, a.y as f64], [b.x as f64, b.y as f64]);
            if d < 12.0 && nearest.is_none_or(|(old, _)| d < (old as f64)) {
                nearest = Some((d as f32, &seg.net));
            }
        }
    }
    if let Some(report) = report {
        for section in report["sections"].as_array().into_iter().flatten() {
            if let Some(seg) = board.segments.iter().find(|s| s.net == net && s.id == section["id"].as_str().unwrap_or("")) {
                for gap in section["uncovered"].as_array().into_iter().flatten() {
                    if let (Some(a), Some(b)) = (gap[0].as_f64(), gap[1].as_f64()) {
                        let at = |t: f64| to([seg.start[0] + t * (seg.end[0] - seg.start[0]), seg.start[1] + t * (seg.end[1] - seg.start[1])]);
                        painter.line_segment([at(a), at(b)], Stroke::new(6.0, t.error));
                        painter.circle_stroke(at((a + b) / 2.0), 9.0, Stroke::new(2.0, t.error));
                    }
                }
            }
        }
    }
    for via in &board.vias {
        painter.circle_stroke(to(via.at), (via.diameter * scale / 2.0).max(3.0) as f32, Stroke::new(1.4, t.accent));
    }
    if response.clicked()
        && let Some((_, n)) = nearest
    {
        state.attempted = None;
        out.push(Action::run("view.pcb", json!({"net":n,"stage":"net"})));
    }
}
fn draw_report(app: &EmcraftApp, state: &crate::state::PcbUi, ui: &mut egui::Ui, report: Option<&Value>) {
    let t = Tokens::get();
    if let Some(crate::Failure { command, error }) = &app.failure
        && command == "pcb.inspect"
    {
        ui.colored_label(t.error, error);
    }
    let Some(report) = report else {
        return;
    };
    ui.label(trf(
        "Selected net: {net}; routed length {length} {unit}",
        args!(
            net = report["net"].as_str().unwrap_or(""),
            length = format!("{:.3}", report["length"].as_f64().unwrap_or(0.0)),
            unit = report["length_unit"].as_str().unwrap_or("")
        ),
    ));
    ui.heading(tr("Return-path geometry"));
    let mut shown = BTreeSet::new();
    if let Some(findings) = report["findings"].as_array() {
        if findings.is_empty() {
            ui_kit::caption(ui, "No issue found by the implemented geometry checks; this is not a compliance pass.");
        }
        for f in findings {
            if !shown.insert((f["code"].as_str(), f["detail"].as_str())) {
                continue;
            }
            let code = f["code"].as_str().unwrap_or("");
            ui.colored_label(if code == "nearby_signal" { t.warning } else { t.error }, tr(finding_label(code)));
            ui.small(f["detail"].as_str().unwrap_or(""));
        }
    }
    ui.heading(tr("Ideal transmission-line estimate"));
    ui_kit::caption(
        ui,
        "Lossless quasi-static outer microstrip only. Bends, vias, pads, copper thickness, solder mask, dispersion and coupling are excluded.",
    );
    for row in report["sections"].as_array().into_iter().flatten() {
        ui.small(format!(
            "{} → {}: {:.2} Ω, {:.1} ps",
            row["id"].as_str().unwrap_or(""),
            row["reference_layer"].as_str().unwrap_or(""),
            row["ideal_impedance"].as_f64().unwrap_or(0.0),
            Time::Picosecond.from_si(row["ideal_delay_s"].as_f64().unwrap_or(0.0))
        ));
    }
    if report["model_available"] == true {
        ui.small(trf(
            "Propagation delay: {value} ps",
            args!(value = format!("{:.1}", Time::Picosecond.from_si(report["si"]["delay_s"].as_f64().unwrap_or(0.0)))),
        ));
        let pts = report["si"]["points"].as_array().map(Vec::as_slice).unwrap_or(&[]);
        ui.small(tr("Magnitude |S| (0 to 1); frequency spans zero to the selected maximum"));
        chart(ui, pts, "frequency", &[("s11", t.error), ("s21", t.accent)], false, app.session.display().frequency.symbol());
        ui.small(tr("TDR reflection (ideal line model)"));
        let pts = report["si"]["tdr"].as_array().map(Vec::as_slice).unwrap_or(&[]);
        let horizon = 4.0 * report["si"]["delay_s"].as_f64().unwrap_or(0.0).max(report["si"]["resolution_s"].as_f64().unwrap_or(0.0));
        let end = pts.iter().position(|p| p["time_s"].as_f64().unwrap_or(0.0) > horizon).unwrap_or(pts.len()).max(2).min(pts.len());
        chart(ui, &pts[..end], "time_s", &[("reflection", t.warning)], true, "ns");
        ui.small(trf(
            "TDR window {window} ns; resolution {resolution} ps",
            args!(
                window = format!("{:.2}", Time::Nanosecond.from_si(horizon)),
                resolution = format!("{:.1}", Time::Picosecond.from_si(report["si"]["resolution_s"].as_f64().unwrap_or(0.0)))
            ),
        ));
    } else {
        ui_kit::caption(
            ui,
            "No S-parameters or TDR: the route does not meet the model assumptions. Repair the reported geometry or use a detailed field model.",
        );
    }
    if let Some(old) = &state.baseline {
        let count = |v: &Value| v["findings"].as_array().map_or(0, Vec::len);
        ui.label(trf("Comparison — geometry findings: {before} → {after}", args!(before = count(old), after = count(report))));
    }
    for note in report["import_notes"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        ui_kit::caption(ui, Raw(note));
    }
}

fn finding_label(code: &str) -> &'static str {
    match code {
        "return_gap" => "Reference copper missing beneath the route",
        "reference_edge" => "Return corridor reaches a reference edge or slot",
        "no_reference" => "No filled reference plane",
        "route_topology" => "Route is branched, disconnected or closed",
        "via_transition" => "Signal via needs a transition model",
        "missing_return_via" => "Verify the return via at this layer transition",
        "nearby_signal" => "Nearby parallel signal: check coupling",
        _ => "Cross-section outside the supported line model",
    }
}
fn chart(ui: &mut egui::Ui, pts: &[Value], xkey: &str, series: &[(&str, Color32)], scalar: bool, unit: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 112.0), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 3.0, Tokens::get().panel);
    let xmax = pts.last().and_then(|v| v[xkey].as_f64()).unwrap_or(1.0).max(1e-30);
    let pad = Rect::from_min_max(rect.min + vec2(28.0, 8.0), rect.max - vec2(10.0, 20.0));
    let tokens = Tokens::get();
    let font = egui::FontId::proportional(10.0);
    for (fraction, value) in [(0.0, if scalar { "−1" } else { "0" }), (0.5, if scalar { "0" } else { "0.5" }), (1.0, "1")] {
        let y = pad.bottom() - fraction * pad.height();
        p.line_segment([pos2(pad.left(), y), pos2(pad.right(), y)], Stroke::new(0.5, tokens.border));
        p.text(pos2(pad.left() - 5.0, y), egui::Align2::RIGHT_CENTER, value, font.clone(), tokens.text_dim);
    }
    p.text(pos2(pad.left(), pad.bottom() + 5.0), egui::Align2::LEFT_TOP, "0", font.clone(), tokens.text_dim);
    let end = if scalar { Time::Nanosecond.from_si(xmax) } else { xmax };
    p.text(pos2(pad.right(), pad.bottom() + 5.0), egui::Align2::RIGHT_TOP, format!("{end:.2} {unit}"), font, tokens.text_dim);
    for &(key, colour) in series {
        let points: Vec<_> = pts
            .iter()
            .filter_map(|v| {
                let x = v[xkey].as_f64()?;
                let y = if scalar { v[key].as_f64()? } else { v[key]["mag"].as_f64()? };
                Some(pos2(
                    pad.left() + (x / xmax) as f32 * pad.width(),
                    pad.bottom()
                        - if scalar { ((y + 1.0) / 2.0).clamp(0.0, 1.0) as f32 * pad.height() } else { y.clamp(0.0, 1.0) as f32 * pad.height() },
                ))
            })
            .collect();
        if points.len() > 1 {
            p.add(egui::Shape::line(points, Stroke::new(1.5, colour)));
        }
    }
    ui.horizontal_wrapped(|ui| {
        for &(name, c) in series {
            ui.colored_label(c, name);
        }
    });
}
