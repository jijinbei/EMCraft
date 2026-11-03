//! Original conceptual coupling diagrams, drawn in the board's 3D view: arches between the
//! source and receiving traces (electric), loops round the source (magnetic) or the reference
//! under the route (return). No field samples, amplitudes or noise voltages are synthesized.
use super::*;
use crate::gpu3d::{Curve, CurveKind};
use crate::scene3d::Label;
use emcraft_engine::pcb::CouplingMode as Mode;
use emcraft_engine::pcb::{Inspection, PairRow};

fn curve_colour(mode: Mode) -> emcraft_render::Rgb {
    // Match the existing 3D E/H glyphs (scene3d::field); the flat-plot E pen can be black.
    let scene = crate::theme::scene();
    match mode {
        Mode::Electric => scene.port_input,
        Mode::Magnetic => scene.port_output,
        Mode::Return => crate::theme::palette().series[3],
    }
}

#[derive(Default)]
pub(crate) struct IllustrationUi {
    pub index: usize,
    /// The illustration the 3D view was last brought to (receiver, stretch), so it frames a
    /// newly illustrated stretch once and then leaves the camera to the user.
    framed: Option<(String, usize)>,
    /// The clearance and parallel length (m) kept for comparison, with the source and receiver.
    baseline: Option<(f64, f64, String, String)>,
}

fn action(out: &mut Actions, value: Value) {
    out.push(Action::run("view.pcb", value));
}

/// The receiving trace of the coupling to illustrate: another routed net (the route is the
/// noise source).
pub(super) fn select(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    let c = &app.session.views().pcb_illustration;
    ui.horizontal_wrapped(|ui| {
        ui.label(tr("Receiving trace"));
        egui::ComboBox::from_id_salt("pcb_receiver").selected_text(c.victim.as_deref().unwrap_or("—")).show_ui(ui, |ui| {
            let route = app.session.views().pcb_net.as_deref();
            let nets: std::collections::BTreeSet<_> = app
                .session
                .model()
                .pcb
                .iter()
                .flat_map(|b| b.segments.iter())
                .map(|s| s.net.as_str())
                .filter(|n| Some(*n) != route && *n != app.views.pcb.ground)
                .collect();
            for net in nets {
                let r = ui.selectable_label(c.victim.as_deref() == Some(net), net);
                scenario::publish_target(Handle::Choice("pcb_receiver", net), r.rect);
                if r.clicked() {
                    action(out, json!({"victim": net}));
                }
            }
        });
    });
}

/// A stretch's ends in metres (the report gives display lengths).
fn si(ends: [[f64; 2]; 2], unit: emcraft_model::units::Length) -> [[f64; 2]; 2] {
    ends.map(|p| p.map(|x| unit.to_si(x)))
}

/// The report's coupling pairs with the receiving trace chosen.
fn pairs(app: &EmcraftApp) -> Vec<&PairRow> {
    let Some(victim) = app.session.views().pcb_illustration.victim.as_deref() else { return Vec::new() };
    let Some(report) = app.replies.pcb.as_ref() else { return Vec::new() };
    report.coupling_pairs.iter().filter(|p| p.target == victim).collect()
}

/// Whether the board's view can illustrate the coupling: a receiving trace chosen and a
/// parallel stretch of it in the report.
pub(super) fn ready(app: &EmcraftApp) -> bool {
    !pairs(app).is_empty()
}

/// The illustration of the selected pair in the board's 3D view (`frame` its layout, `motion`
/// the explanatory motion's step): the mode's curves and the callouts naming source and
/// receiver. Nothing when the
/// view picks routes, or when the geometry gives no reference for it.
pub(crate) fn curves(app: &EmcraftApp, frame: &crate::scene3d::board::Frame, motion: u64, out: &mut crate::scene3d::Built) {
    let config = &app.session.views().pcb_illustration;
    let pairs = pairs(app);
    let results = emcraft_engine::pcb::stage(&app.session) == emcraft_engine::pcb::Stage::Results;
    let index = app.views.pcb.illustration.index.min(pairs.len().saturating_sub(1));
    let (Some(board), Some(mode), true, Some(pair)) = (app.session.model().pcb.as_ref(), config.illustrated, results, pairs.get(index)) else {
        return;
    };
    let u = app.session.display().length;
    let (a, b) = (si(pair.source_points, u), si(pair.target_points, u));
    let Some(layer) = board.layers.iter().find(|l| l.name == pair.layer) else { return };
    let ground = &app.views.pcb.ground;
    let Some(reference) = board
        .layers
        .iter()
        .filter(|l| l.name != layer.name && board.copper.iter().any(|c| c.layer == l.name && c.net == *ground))
        .min_by(|a, b| (a.z - layer.z).abs().total_cmp(&(b.z - layer.z).abs()))
    else {
        return;
    };
    let height = (reference.z - layer.z).abs() * frame.stretch;
    let d = [a[1][0] - a[0][0], a[1][1] - a[0][1]];
    let length = d[0].hypot(d[1]);
    if !(length > 0.0) {
        return;
    }
    let along = [d[0] / length, d[1] / length];
    let n = [-along[1], along[0]];
    let centre = [(a[0][0] + a[1][0]) / 2., (a[0][1] + a[1][1]) / 2.];
    let offset = [b[0][0] - a[0][0], b[0][1] - a[0][1]];
    let half = length / 2.;
    let at = |p: [f64; 2], z: f64| frame.point(p, z, board);
    let p = |x: f64| [centre[0] + along[0] * x, centre[1] + along[1] * x];
    let covered = |q: [f64; 2]| {
        board
            .copper
            .iter()
            .filter(|c| c.net == *ground && c.layer == reference.name)
            .any(|c| emcraft_render::pcb::spans(&c.polygon, q[1]).iter().any(|v| q[0] >= v[0] && q[0] <= v[1]))
    };
    let bad = app.replies.pcb.iter().flat_map(|r| &r.findings).any(|f| f.code.reference());
    let emphasis = if config.playing { (motion % 8 + 1) as f32 * 0.785 } else { 0. };
    for k in 0..if mode == Mode::Return { 1 } else { 5 } {
        let x = half * (k as f64 / 4. - 0.5);
        let a = p(x);
        let b = [a[0] + offset[0], a[1] + offset[1]];
        if mode == Mode::Return && (bad || !(0..=24).all(|j| covered(p(-half + 2. * half * j as f64 / 24.)))) {
            continue;
        }
        let (start, end, lift, kind) = match mode {
            Mode::Electric => (at(a, layer.z), at(b, layer.z), [0., height * 0.7, 0.], CurveKind::Arch),
            Mode::Magnetic => (
                at(a, layer.z),
                at([a[0] + n[0] * height / frame.stretch * 5., a[1] + n[1] * height / frame.stretch * 5.], layer.z),
                [0., height * 0.4, 0.],
                CurveKind::Loop,
            ),
            Mode::Return => {
                let (mut start, mut end) = (at(p(-half), reference.z), at(p(half), reference.z));
                start[1] += height * 0.06;
                end[1] += height * 0.06;
                (start, end, [0.; 3], CurveKind::Line)
            }
        };
        out.curves.push(Curve { a: start, b: end, lift, radius: height * 0.022, kind, color: curve_colour(mode), emphasis });
    }
    let t = Tokens::get();
    let source = app.session.views().pcb_net.clone().unwrap_or_default();
    let target = config.victim.clone().unwrap_or_default();
    let span = half * 0.8;
    let r = p(span);
    let label = |at: [f64; 3], text: String, color: Color32| Label { at, text, color: crate::theme::rgb(color), facing: None, anchor: Some(at) };
    out.labels.push(label(at(p(-span), layer.z), format!("{}: {source}", tr("Noise source")), t.warning));
    out.labels.push(label(at([r[0] + offset[0], r[1] + offset[1]], layer.z), format!("{}: {target}", tr("Receiving trace")), t.accent));
    out.legend.push((shown(mode).into(), curve_colour(mode)));
    if mode == Mode::Return && out.curves.is_empty() {
        out.legend.push((tr("Reference copper is missing; the return path is not drawn.").into(), crate::theme::rgb(t.warning)));
    }
}

/// The coupling's illustrations over the 3D view's top edge, once there are results to draw
/// them from: none (the whole board), electric, magnetic, return, and the explanatory motion.
pub(super) fn mode_bar(app: &EmcraftApp, ui: &mut egui::Ui, view: Rect, out: &mut Actions) {
    let config = &app.session.views().pcb_illustration;
    let bar = Rect::from_center_size(pos2(view.center().x, view.top() + 22.), vec2(view.width().min(560.), 32.));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(bar).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let ui = &mut child;
    egui::Frame::NONE.fill(Tokens::get().panel).corner_radius(Tokens::get().radius_lg).inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
        ui.horizontal(|ui| {
            let r = ui.selectable_label(config.illustrated.is_none(), tr("Whole board"));
            scenario::publish_target(Handle::Choice("pcb_mode", "select"), r.rect);
            if r.clicked() {
                action(out, json!({"coupling": false}));
            }
            for mode in Mode::ALL {
                let r = ui.selectable_label(config.illustrated == Some(mode), shown(mode));
                scenario::publish_target(Handle::Choice("pcb_mode", mode.id()), r.rect);
                if r.clicked() {
                    action(out, json!({"coupling": true, "mode": mode.id()}));
                }
            }
            if config.illustrated.is_some() {
                let r =
                    ui.selectable_label(config.playing, tr("Explain motion")).on_hover_text(tr("Motion is explanatory emphasis, not physical time."));
                scenario::publish_target(Handle::Choice("pcb_coupling", "playing"), r.rect);
                if r.clicked() {
                    action(out, json!({"playing": !config.playing}));
                }
            }
        });
    });
}

/// Brings the 3D view to the illustrated stretch when it is new (its curves with room round
/// them), and back to the whole board when the view returns to picking routes.
pub(super) fn frame_view(app: &EmcraftApp, illustrated: bool, state: &mut IllustrationUi, view: &mut crate::view3d::View3d, rect: Rect) {
    let config = &app.session.views().pcb_illustration;
    if !(illustrated && config.illustrated.is_some()) {
        if state.framed.take().is_some() {
            view.standard(emcraft_render::view3d::StandardView::Iso);
        }
        return;
    }
    let key = (config.victim.clone().unwrap_or_default(), state.index);
    if state.framed.as_ref() == Some(&key) {
        return;
    }
    let Some(built) = view.shown() else { return };
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for c in &built.curves {
        for p in [c.a, c.b] {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k] - c.lift[k].abs());
                hi[k] = hi[k].max(p[k] + c.lift[k].abs());
            }
        }
    }
    if !lo.iter().chain(&hi).all(|v| v.is_finite()) {
        return;
    }
    let mut camera = view.camera;
    camera.fit(lo, hi, f64::from(rect.aspect_ratio()));
    camera.distance *= 1.4;
    view.set_camera(camera);
    state.framed = Some(key);
}

/// The coupling's panel: which parallel stretch, its clearance and length, what the mode
/// means, and a kept geometry to compare with.
pub(super) fn show(app: &EmcraftApp, state: &mut crate::state::PcbUi, ui: &mut egui::Ui, report: &Inspection, out: &mut Actions) {
    let config = &app.session.views().pcb_illustration;
    let Some(victim) = &config.victim else {
        return;
    };
    ui.heading(tr("Coupling illustration"));
    ui.colored_label(Tokens::get().warning, tr("Schematic only — fields and noise voltage are not calculated"));
    let pairs = pairs(app);
    if pairs.is_empty() {
        ui.label(tr("No supported parallel overlap for this pair. Angles, vias and interlayer coupling are not illustrated."));
        return;
    }
    ui.small(tr("The 3D view shows it: choose a mode over the view."));
    let state = &mut state.illustration;
    state.index = state.index.min(pairs.len() - 1);
    ui.horizontal_wrapped(|ui| {
        if app.session.variables().contains_key("gap") {
            let r = ui.button(tr("Edit template spacing…"));
            scenario::publish_target(Handle::Choice("pcb_coupling", "spacing"), r.rect);
            if r.clicked() {
                out.push(Action::dialog_with("project.variable", json!({"name": "gap"})));
            }
        }
        egui::ComboBox::from_id_salt("coupling_section").selected_text(format!("{}/{}", state.index + 1, pairs.len())).show_ui(ui, |ui| {
            for (i, p) in pairs.iter().enumerate() {
                let text = format!(
                    "{}: ({}, {}) → ({}, {})",
                    i + 1,
                    p.source_points[0][0],
                    p.source_points[0][1],
                    p.source_points[1][0],
                    p.source_points[1][1]
                );
                if ui.selectable_label(i == state.index, text).clicked() {
                    state.index = i;
                }
            }
        });
    });
    let pair = pairs[state.index];
    let unit = app.session.display().length.symbol();
    ui.small(crate::lang::trf(
        "Clearance {gap} {unit} · parallel length {length} {unit}",
        &[("gap", &format!("{:.3}", pair.gap)), ("length", &format!("{:.3}", pair.overlap)), ("unit", &unit)],
    ));
    ui.small(tr(match config.illustrated.unwrap_or_default() {
        Mode::Electric => "Electric coupling: schematic connections between conductors; no field strength is implied.",
        Mode::Magnetic => "Magnetic coupling: symbolic loops around the source current and receiver; no flux is calculated.",
        Mode::Return => "Return reference only, not a solved current. Missing ground is never bridged.",
    }));
    let length = app.session.display().length;
    let source = report.net.clone();
    let keep = ui.button(tr("Keep geometry for comparison"));
    scenario::publish_target(Handle::Choice("pcb_coupling", "keep"), keep.rect);
    if keep.clicked() {
        state.baseline = Some((length.to_si(pair.gap), length.to_si(pair.overlap), source.clone(), victim.clone()));
    }
    if let Some((gap, overlap, s, v)) = &state.baseline
        && *s == source
        && v == victim
    {
        ui.label(tr("Before / current geometry — no noise reduction is predicted"));
        ui.small(crate::lang::trf(
            "Clearance {before} → {after}; parallel length {old_length} → {length} {unit}",
            &[
                ("before", &format!("{:.3}", length.from_si(*gap))),
                ("after", &format!("{:.3}", pair.gap)),
                ("old_length", &format!("{:.3}", length.from_si(*overlap))),
                ("length", &format!("{:.3}", pair.overlap)),
                ("unit", &unit),
            ],
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_ground_slot_never_gets_an_invented_return_line() {
        let mut h = crate::scenario::Harness::new();
        for split in [false, true] {
            assert_eq!(h.call("engine.execute", json!({"command": "pcb.sample", "params": {"split": split}}))["ok"], true);
            assert_eq!(
                h.call(
                    "engine.execute",
                    json!({"command": "view.pcb", "params": {"net": "DATA", "victim": "CLK", "coupling": true, "mode": "return"}})
                )["ok"],
                true
            );
            assert_eq!(h.call("engine.execute", json!({"command": "pcb.inspect", "params": {"net": "DATA"}}))["ok"], true);
            let board = h.app.session.model().pcb.clone().unwrap();
            let mut built = crate::scene3d::Built::default();
            curves(&h.app, &crate::scene3d::board::frame(&board), 0, &mut built);
            assert_eq!(built.curves.is_empty(), split);
            assert_eq!(built.labels.len(), 2);
        }
    }
}
