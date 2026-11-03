//! PCB workspace: select real routed nets, highlight reference gaps, and keep geometry
//! screening distinct from the explicitly idealized transmission-line result.
use super::*;
use crate::lang::{Raw, args};
use crate::ui_kit;
use emcraft_engine::pcb::near;
use emcraft_model::units::{Time, Unit};
use std::collections::BTreeSet;

pub(super) fn show(app: &EmcraftApp, st: &mut Parts, ui: &mut egui::Ui, out: &mut Actions) {
    ui.horizontal_wrapped(|ui| {
        if ui.button(tr("Import PCB…")).clicked() {
            out.push(Action::File(crate::files::FileAction::PickPart));
        }
        for (name, split) in [("Example: split ground", true), ("Example: continuous ground", false)] {
            let r = ui.button(tr(name));
            scenario::publish_target(Handle::Choice("pcb_example", if split { "split" } else { "continuous" }), r.rect);
            if r.clicked() {
                out.push(Action::run("pcb.sample", json!({"split":split})));
            }
        }
    });
    let Some(board) = app.session.model().pcb.as_ref() else {
        ui_kit::caption(ui, "Import a PCB or open an example, then click a routed trace.");
        return;
    };
    let state = &mut st.views.pcb;
    let selected = app.session.views().pcb_net.as_deref().or_else(|| board.segments.first().map(|s| s.net.as_str())).unwrap_or("");
    let mut net = selected.to_owned();
    let nets: BTreeSet<_> = board.segments.iter().map(|s| s.net.as_str()).collect();
    ui.horizontal_wrapped(|ui| {
        let r = egui::ComboBox::from_id_salt("pcb_net").selected_text(&net).show_ui(ui, |ui| {
            for n in nets {
                let r = ui.selectable_label(net == n, n);
                scenario::publish_target(Handle::Choice("pcb_net", n), r.rect);
                if r.clicked() {
                    net = n.to_owned();
                }
            }
        });
        scenario::publish_target(Handle::Choice("pcb_control", "net"), r.response.rect);
        for (name, field) in
            [("Reference net", &mut state.ground), ("Reference impedance (Ω)", &mut state.reference), ("Maximum frequency", &mut state.stop)]
        {
            ui.small(tr(name));
            let r = ui.add(egui::TextEdit::singleline(field).desired_width(65.0));
            scenario::publish_target(Handle::Choice("pcb_control", name), r.rect);
        }
        ui.small(app.session.display().frequency.symbol());
    });
    let report = app.replies.pcb.as_ref().filter(|r| {
        r["net"].as_str() == Some(net.as_str())
            && r["ground"].as_str() == Some(state.ground.trim())
            && r["reference"].as_f64() == state.reference.parse::<f64>().ok()
            && r["stop"].as_f64() == state.stop.parse::<f64>().ok()
            && r["frequency_unit"] == app.session.display().frequency.symbol()
    });
    let mut analyze = net != selected || app.replies.pcb.is_none();
    if report.is_none() && app.replies.pcb.is_some() {
        ui_kit::caption(ui, "Settings changed; inspect the route again.");
    }
    ui.horizontal_wrapped(|ui| {
        let r = ui.button(tr("Inspect route"));
        scenario::publish_target(Handle::Choice("pcb_control", "inspect"), r.rect);
        if r.clicked() {
            state.attempted = None;
            analyze = true;
        }
        let keep = ui.button(tr("Keep for comparison"));
        scenario::publish_target(Handle::Choice("pcb_control", "keep"), keep.rect);
        if report.is_some() && keep.clicked() {
            state.baseline = report.cloned();
        }
    });
    let ground = state.ground.trim().to_owned();
    let reference = state.reference.parse::<f64>();
    let stop = state.stop.parse::<f64>();
    let key = (app.revision, net.clone());
    if analyze && state.attempted.as_ref() != Some(&key) {
        state.attempted = Some(key);
        match (reference, stop) {
            (Ok(reference), Ok(stop)) if reference.is_finite() && stop.is_finite() => out.push(Action::run("pcb.inspect", state.params(&net))),
            _ => ui_kit::caption(ui, "Enter finite numeric impedance and frequency values."),
        }
    }
    // An explicit retry after an error is allowed without generating a request every frame.
    if ui.ctx().input(|i| i.key_pressed(egui::Key::Enter)) {
        state.attempted = None;
    }
    ui_kit::caption(ui, "Copper support is a geometry check, not a solved return-current map or an SI/EMC pass verdict.");
    ui_kit::caption(ui, "Top view: selected net and projected reference copper; section rows identify the reference layers.");
    let wide = ui.available_width() > 900.0;
    let map_width = ui.available_width() * if wide { 0.52 } else { 1.0 };
    let layout = if wide { egui::Layout::left_to_right(egui::Align::Min) } else { egui::Layout::top_down(egui::Align::Min) };
    ui.with_layout(layout, |ui| {
        ui.allocate_ui_with_layout(vec2(map_width, 310.0), egui::Layout::top_down(egui::Align::Min), |ui| {
            draw_board(app, state, ui, out, &net, &ground, report);
        });
        ui.vertical(|ui| draw_report(app, state, ui, report));
    });
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
        out.push(Action::run("pcb.inspect", state.params(n)));
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
