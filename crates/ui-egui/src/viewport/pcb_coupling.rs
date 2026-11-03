//! Original conceptual coupling diagrams. No field samples, amplitudes or noise voltages are synthesized.
use super::*;
use crate::gpu3d::{Curve, CurveKind};
use emcraft_engine::pcb::CouplingMode as Mode;
use emcraft_render::view3d::{Camera, Scene};
#[derive(Clone)]
struct Diagram {
    scene: Scene,
    curves: Vec<Curve>,
    // Source, receiver and reference-copper anchors for the ordinary egui callouts.
    anchors: [[f64; 3]; 3],
    reference_layer: String,
    route_layer: String,
}

impl Diagram {
    fn bounds(&self) -> Option<([f64; 3], [f64; 3])> {
        let (mut lo, mut hi) = self.scene.bounds()?;
        for c in &self.curves {
            for i in 0..3 {
                let reach = if matches!(c.kind, CurveKind::Loop) { (c.b[i] - c.a[i]).abs() } else { 0. };
                lo[i] = lo[i].min(c.a[i].min(c.b[i]) - c.lift[i].abs() - reach);
                hi[i] = hi[i].max(c.a[i].max(c.b[i]) + c.lift[i].abs() + reach);
            }
        }
        Some((lo, hi))
    }
}

fn curve_colour(mode: Mode) -> emcraft_render::Rgb {
    // Match the existing 3D E/H glyphs (scene3d::field); the flat-plot E pen can be black.
    let scene = crate::theme::scene();
    match mode {
        Mode::Electric => scene.port_input,
        Mode::Magnetic => scene.port_output,
        Mode::Return => crate::theme::palette().series[3],
    }
}

fn callouts(ui: &egui::Ui, rect: Rect, camera: &Camera, diagram: &Diagram, names: [&str; 3], mode: Mode) {
    let [source, target, ground] = names;
    let t = Tokens::get();
    let labels = [
        (
            format!("{}: {source} ({})", tr("Source"), diagram.route_layer),
            t.warning,
            pos2(rect.left() + 12., rect.top() + 12.),
            egui::Align2::LEFT_TOP,
        ),
        (
            format!("{}: {target} ({})", tr("Receiver"), diagram.route_layer),
            t.accent,
            pos2(rect.right() - 12., rect.top() + 42.),
            egui::Align2::RIGHT_TOP,
        ),
        (format!("{ground} ({})", diagram.reference_layer), t.success, pos2(rect.left() + 12., rect.bottom() - 12.), egui::Align2::LEFT_BOTTOM),
    ];
    let painter = ui.painter().with_clip_rect(rect);
    for (i, (label, color, pos, align)) in labels.into_iter().enumerate() {
        let galley = painter.layout_no_wrap(label, FontId::proportional(t.font_body), color);
        let box_rect = align.anchor_size(pos, galley.size());
        if let Some(q) = camera.project(diagram.anchors[i], f64::from(rect.width()), f64::from(rect.height())) {
            let anchor = rect.min + vec2(q[0] as f32, q[1] as f32);
            if rect.contains(anchor) {
                painter.line_segment([box_rect.center(), anchor], Stroke::new(1., color));
                painter.circle_filled(anchor, 3., color);
            }
        }
        painter.rect_filled(box_rect.expand(4.), t.radius_sm, t.panel);
        painter.galley(box_rect.min, galley, color);
    }
    if let Some(c) = diagram.curves.first() {
        let anchor = match c.kind {
            CurveKind::Arch => std::array::from_fn(|i| (c.a[i] + c.b[i]) * 0.5 + c.lift[i]),
            CurveKind::Loop => std::array::from_fn(|i| c.a[i] + c.lift[i]),
            CurveKind::Line => std::array::from_fn(|i| (c.a[i] + c.b[i]) * 0.5),
        };
        let color = crate::theme::color32(c.color);
        let galley = painter.layout_no_wrap(shown(mode).into(), FontId::proportional(t.font_body), color);
        let box_rect = egui::Align2::RIGHT_BOTTOM.anchor_size(pos2(rect.right() - 12., rect.bottom() - 12.), galley.size());
        if let Some(q) = camera.project(anchor, f64::from(rect.width()), f64::from(rect.height())) {
            let at = rect.min + vec2(q[0] as f32, q[1] as f32);
            if rect.contains(at) {
                painter.line_segment([box_rect.center(), at], Stroke::new(1., color));
            }
        }
        painter.rect_filled(box_rect.expand(4.), t.radius_sm, t.panel);
        painter.galley(box_rect.min, galley, color);
    }
}

#[derive(Default)]
pub(crate) struct IllustrationUi {
    pub index: usize,
    camera: Camera,
    key: Option<String>,
    picture: Option<Picture>,
    baseline: Option<(Diagram, f64, f64, Mode, bool, bool, String, String)>,
    compare: Vec<Picture>,
    baseline_palette: [emcraft_render::Rgb; 3],
}

fn action(out: &mut Actions, value: Value) {
    out.push(Action::run("view.pcb", value));
}

pub(super) fn select(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    let c = &app.session.views().pcb_illustration;
    ui.horizontal_wrapped(|ui| {
        for (label, role) in [("Pick source", false), ("Pick receiver", true)] {
            let r = ui.selectable_label(c.pick_victim == role, tr(label));
            scenario::publish_target(Handle::Choice("pcb_coupling", if role { "pick_receiver" } else { "pick_source" }), r.rect);
            if r.clicked() {
                action(out, json!({"pick_victim": role}));
            }
        }
        ui.label(tr("Receiver"));
        egui::ComboBox::from_id_salt("pcb_receiver").selected_text(c.victim.as_deref().unwrap_or("—")).show_ui(ui, |ui| {
            let nets: std::collections::BTreeSet<_> = app
                .session
                .model()
                .pcb
                .iter()
                .flat_map(|b| b.segments.iter())
                .filter(|s| Some(s.net.as_str()) != app.session.views().pcb_net.as_deref() && s.net != app.views.pcb.ground)
                .map(|s| s.net.as_str())
                .collect();
            for net in nets {
                let r = ui.selectable_label(c.victim.as_deref() == Some(net), net);
                scenario::publish_target(Handle::Choice("pcb_receiver", net), r.rect);
                if r.clicked() {
                    action(out, json!({"victim": net}));
                }
            }
        });
        let r = ui.button(tr("Swap source / receiver"));
        scenario::publish_target(Handle::Choice("pcb_coupling", "swap"), r.rect);
        if r.clicked()
            && let (Some(a), Some(b)) = (&app.session.views().pcb_net, &c.victim)
        {
            action(out, json!({"net": b, "victim": a, "stage": "net"}));
        }
    });
    ui.small(tr("Source is orange; receiver is blue. Select each role before clicking a trace."));
}

fn points(v: &Value, unit: emcraft_model::units::Length) -> Option<[[f64; 2]; 2]> {
    Some([[unit.to_si(v[0][0].as_f64()?), unit.to_si(v[0][1].as_f64()?)], [unit.to_si(v[1][0].as_f64()?), unit.to_si(v[1][1].as_f64()?)]])
}

fn scene(app: &EmcraftApp, pair: &Value, mode: Mode, exaggerate: bool, section: bool, emphasis: f32) -> Option<Diagram> {
    let board = app.session.model().pcb.as_ref()?;
    let u = app.session.display().length;
    let a = points(&pair["source_points"], u)?;
    let b = points(&pair["target_points"], u)?;
    let layer = board.layers.iter().find(|l| Some(l.name.as_str()) == pair["layer"].as_str())?;
    if board.copper.iter().map(|c| c.polygon.len()).sum::<usize>().saturating_mul(80) > 10_000_000 {
        return None;
    }
    let ground = &app.views.pcb.ground;
    let reference = board
        .layers
        .iter()
        .filter(|l| l.name != layer.name && board.copper.iter().any(|c| c.layer == l.name && c.net == *ground))
        .min_by(|a, b| (a.z - layer.z).abs().total_cmp(&(b.z - layer.z).abs()))?;
    let height = (reference.z - layer.z).abs();
    let d = [a[1][0] - a[0][0], a[1][1] - a[0][1]];
    let length = d[0].hypot(d[1]);
    let along = [d[0] / length, d[1] / length];
    let n = [-along[1], along[0]];
    let centre = [(a[0][0] + a[1][0]) / 2., (a[0][1] + a[1][1]) / 2.];
    let offset = [b[0][0] - a[0][0], b[0][1] - a[0][1]];
    let half = length / 2.;
    let depth = if exaggerate { 10. } else { 1. };
    let at = |p: [f64; 2], z: f64| [p[0], -(z - layer.z) * depth, p[1]];
    let p = |x: f64| [centre[0] + along[0] * x, centre[1] + along[1] * x];
    let t = Tokens::get();
    let rgb = crate::theme::rgb;
    let mut scene = Scene::default();
    let extent = if section { height * 2. + offset[0].hypot(offset[1]) } else { half.max(offset[0].hypot(offset[1]) * 2.) };
    // A transparent local dielectric section gives the two copper layers a readable depth.
    // Copper itself is zero-thickness in the board model; the shallow extrusion is a display aid.
    let bottom = at(centre, reference.z)[1];
    scene.cuboid(
        [centre[0] - extent, bottom.min(0.), centre[1] - extent],
        [centre[0] + extent, bottom.max(0.), centre[1] + extent],
        rgb(t.text_dim),
        22,
    );
    // Raster spans retain saved slots and holes; bounded local projection only.
    for copper in board.copper.iter().filter(|c| c.net == *ground && c.layer == reference.name) {
        for row in 0..80 {
            let y = centre[1] - extent + 2. * extent * row as f64 / 80.;
            let dy = 2. * extent / 80.;
            for [l, r] in emcraft_render::pcb::spans(&copper.polygon, y + dy / 2.) {
                let l = l.max(centre[0] - extent);
                let r = r.min(centre[0] + extent);
                if r > l {
                    if scene.tris.len() > 20000 {
                        return None;
                    }
                    scene.quad(
                        [at([l, y], reference.z), at([r, y], reference.z), at([r, y + dy], reference.z), at([l, y + dy], reference.z)],
                        rgb(t.success),
                        230,
                    );
                }
            }
        }
    }
    for (id, colour, shift) in [("source_segment", t.warning, [0., 0.]), ("target_segment", t.accent, offset)] {
        let seg = board.segments.iter().find(|s| Some(s.id.as_str()) == pair[id].as_str())?;
        let w = seg.width / 2.;
        let q = |x: f64, side: f64| {
            let q = p(x);
            at([q[0] + shift[0] + side * n[0] * w, q[1] + shift[1] + side * n[1] * w], layer.z)
        };
        let span = if section { height.max(seg.width) * 1.5 } else { half };
        let top = [q(-span, -1.), q(span, -1.), q(span, 1.), q(-span, 1.)];
        let thickness = height * depth * 0.045;
        let low = top.map(|mut v| {
            v[1] -= thickness;
            v
        });
        scene.quad(top, rgb(colour), 255);
        scene.quad(low, rgb(colour), 255);
        for i in 0..4 {
            let j = (i + 1) % 4;
            scene.quad([top[i], top[j], low[j], low[i]], rgb(colour), 255);
        }
    }
    let covered = |p: [f64; 2]| {
        board
            .copper
            .iter()
            .filter(|c| c.net == *ground && c.layer == reference.name)
            .any(|c| emcraft_render::pcb::spans(&c.polygon, p[1]).iter().any(|v| p[0] >= v[0] && p[0] <= v[1]))
    };
    let bad = app
        .replies
        .pcb
        .as_ref()?
        .get("findings")?
        .as_array()?
        .iter()
        .any(|f| ["return_gap", "reference_edge", "no_reference"].contains(&f["code"].as_str().unwrap_or("")));
    let mut curves = Vec::new();
    for k in 0..if section || mode == Mode::Return { 1 } else { 5 } {
        let x = if section { 0. } else { half * (k as f64 / 4. - 0.5) };
        let a = p(x);
        let b = [a[0] + offset[0], a[1] + offset[1]];
        if mode == Mode::Return && (bad || !(0..=24).all(|j| covered(p(-half + 2. * half * j as f64 / 24.)))) {
            continue;
        }
        let (start, end, lift, kind) = match mode {
            Mode::Electric => (at(a, layer.z), at(b, layer.z), [0., height * depth * 0.7, 0.], CurveKind::Arch),
            Mode::Magnetic => {
                (at(a, layer.z), at([a[0] + n[0] * height * 5., a[1] + n[1] * height * 5.], layer.z), [0., height * depth * 0.4, 0.], CurveKind::Loop)
            }
            Mode::Return => {
                let span = if section { height * 1.5 } else { half };
                let mut start = at(p(-span), reference.z);
                let mut end = at(p(span), reference.z);
                start[1] += height * depth * 0.06;
                end[1] += height * depth * 0.06;
                (start, end, [0.; 3], CurveKind::Line)
            }
        };
        curves.push(Curve { a: start, b: end, lift, radius: height * depth * 0.022, kind, color: curve_colour(mode), emphasis });
    }
    let span = if section { height * 1.5 } else { half * 0.8 };
    let source_anchor = p(-span);
    let receiver_anchor = p(span);
    Some(Diagram {
        scene,
        curves,
        anchors: [
            at(source_anchor, layer.z),
            at([receiver_anchor[0] + offset[0], receiver_anchor[1] + offset[1]], layer.z),
            at([centre[0] - extent * 0.5, centre[1] + extent * 0.5], reference.z),
        ],
        reference_layer: reference.name.clone(),
        route_layer: layer.name.clone(),
    })
}

pub(super) fn show(app: &EmcraftApp, state: &mut crate::state::PcbUi, ui: &mut egui::Ui, report: &Value, out: &mut Actions) {
    let config = &app.session.views().pcb_illustration;
    let Some(victim) = &config.victim else {
        return;
    };
    ui.heading(tr("Coupling illustration"));
    ui.colored_label(Tokens::get().warning, tr("Schematic only — fields and noise voltage are not calculated"));
    ui.small(tr(if config.section {
        "3D shows a close-up at the circles in the layout."
    } else {
        "3D shows the selected parallel overlap; circles mark its centre in 2D."
    }));
    let pairs: Vec<_> = report["coupling_pairs"].as_array().into_iter().flatten().filter(|p| p["target"].as_str() == Some(victim)).collect();
    if pairs.is_empty() {
        ui.label(tr("No supported parallel overlap for this pair. Angles, vias and interlayer coupling are not illustrated."));
        return;
    }
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
                if ui
                    .selectable_label(
                        i == state.index,
                        format!(
                            "{}: ({}, {}) → ({}, {})",
                            i + 1,
                            p["source_points"][0][0],
                            p["source_points"][0][1],
                            p["source_points"][1][0],
                            p["source_points"][1][1]
                        ),
                    )
                    .clicked()
                {
                    state.index = i;
                }
            }
        });
        let pair = pairs[state.index];
        ui.small(crate::lang::trf(
            "Clearance {gap} {unit} · parallel length {length} {unit}",
            &[
                ("gap", &format!("{:.3}", pair["gap"].as_f64().unwrap_or(0.))),
                ("length", &format!("{:.3}", pair["overlap"].as_f64().unwrap_or(0.))),
                ("unit", &app.session.display().length.symbol()),
            ],
        ));
    });
    let pair = pairs[state.index];
    ui.horizontal_wrapped(|ui| {
        for mode in Mode::ALL {
            let r = ui.selectable_label(config.mode == mode, shown(mode));
            scenario::publish_target(Handle::Choice("pcb_mode", mode.id()), r.rect);
            if r.clicked() {
                action(out, json!({"mode": mode.id()}));
            }
        }
        for (label, key, value) in [
            ("Explain motion", "playing", config.playing),
            ("Depth ×10", "exaggerate", config.exaggerate),
            ("Cross section", "section", config.section),
        ] {
            let r = ui.selectable_label(value, tr(label));
            scenario::publish_target(Handle::Choice("pcb_coupling", key), r.rect);
            if r.clicked() {
                action(out, json!({key: !value}));
            }
        }
    });
    ui.small(tr(match config.mode {
        Mode::Electric => "Electric coupling: schematic connections between conductors; no field strength is implied.",
        Mode::Magnetic => "Magnetic coupling: symbolic loops around the source current and receiver; no flux is calculated.",
        Mode::Return => "Return reference only, not a solved current. Missing ground is never bridged.",
    }));
    let frame = if config.playing { (ui.input(|i| i.time) * 5.) as u64 % 8 } else { 0 };
    if config.playing {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
    }
    let Some(scene) = scene(app, pair, config.mode, config.exaggerate, config.section, if config.playing { (frame + 1) as f32 * 0.785 } else { 0. })
    else {
        ui.label(tr("No supported reference geometry for a 3D illustration."));
        return;
    };
    let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), 320.), Sense::drag());
    let r = r.on_hover_text(tr("Motion is explanatory emphasis, not physical time. Drag to orbit; scroll to zoom."));
    scenario::publish_target(Handle::Choice("pcb_coupling", "scene"), rect);
    let fitkey = format!("{}:{}:{}:{}", pair["source_segment"], pair["target_segment"], config.exaggerate, config.section);
    if state.key.as_ref().is_none_or(|s| !s.starts_with(&fitkey))
        && let Some((lo, hi)) = scene.bounds()
    {
        state.camera.fit(lo, hi, rect.aspect_ratio() as f64);
        state.camera.distance *= 0.85;
    }
    if r.dragged() {
        let d = ui.input(|i| i.pointer.delta());
        state.camera.orbit(d.x as f64, d.y as f64);
        state.picture = None;
    }
    if r.hovered() {
        let scroll = ui.input_mut(|i| {
            let y = i.smooth_scroll_delta.y;
            i.smooth_scroll_delta = egui::Vec2::ZERO;
            i.raw.events.retain(|e| !matches!(e, egui::Event::MouseWheel { .. }));
            y
        });
        if scroll != 0. {
            state.camera.zoom(scroll as f64 / 100.);
            state.picture = None;
        }
    }
    let key = format!("{fitkey}:{}:{:?}:{frame}:{:?}:{:?}", app.revision, config.mode, state.camera, crate::theme::key());
    if state.key.as_ref() != Some(&key) || state.picture.is_none() {
        state.compare.clear();
        let bg = crate::theme::rgb(Tokens::get().panel);
        match crate::gpu3d::scene_picture_with_curves(
            &scene.scene,
            &scene.curves,
            &state.camera,
            [rect.width().min(1000.) as u32, rect.height() as u32],
            (bg, bg),
        ) {
            Ok(c) => state.picture = Some(Picture::new(&c)),
            Err(e) => {
                ui.label(e.to_string());
            }
        }
        state.key = Some(key);
    }
    if let Some(p) = &mut state.picture {
        let id = p.texture(ui.ctx());
        ui.painter().image(id, rect, Rect::from_min_max(pos2(0., 0.), pos2(1., 1.)), Color32::WHITE);
        callouts(ui, rect, &state.camera, &scene, [report["net"].as_str().unwrap_or(""), victim, &app.views.pcb.ground], config.mode);
    }
    ui.small(tr("Transparent block: dielectric. Copper thickness is schematic."));
    if config.mode == Mode::Return && scene.curves.is_empty() {
        ui.colored_label(Tokens::get().warning, tr("Reference copper is missing; the return path is not drawn."));
    }
    let keep = ui.button(tr("Keep geometry for comparison"));
    scenario::publish_target(Handle::Choice("pcb_coupling", "keep"), keep.rect);
    if keep.clicked() {
        let t = Tokens::get();
        let rgb = crate::theme::rgb;
        state.baseline_palette = [rgb(t.warning), rgb(t.accent), rgb(t.success)];
        state.baseline = Some((
            scene.clone(),
            app.session.display().length.to_si(pair["gap"].as_f64().unwrap_or(0.)),
            app.session.display().length.to_si(pair["overlap"].as_f64().unwrap_or(0.)),
            config.mode,
            config.exaggerate,
            config.section,
            report["net"].as_str().unwrap_or("").into(),
            victim.clone(),
        ));
        state.compare.clear();
    }
    if let Some((old, gap, length, mode, depth, section, source, target)) = &state.baseline {
        if *mode == config.mode
            && *depth == config.exaggerate
            && *section == config.section
            && Some(source.as_str()) == report["net"].as_str()
            && target == victim
        {
            ui.label(tr("Before / current geometry — no noise reduction is predicted"));
            ui.small(crate::lang::trf(
                "Clearance {before} → {after}; parallel length {old_length} → {length} {unit}",
                &[
                    ("before", &format!("{:.3}", app.session.display().length.from_si(*gap))),
                    ("after", &format!("{:.3}", pair["gap"].as_f64().unwrap_or(0.))),
                    ("old_length", &format!("{:.3}", app.session.display().length.from_si(*length))),
                    ("length", &format!("{:.3}", pair["overlap"].as_f64().unwrap_or(0.))),
                    ("unit", &app.session.display().length.symbol()),
                ],
            ));
            // Both pictures use exactly the current camera, size and depth scaling.
            if state.compare.is_empty() {
                let mut themed = old.clone();
                let t = Tokens::get();
                let rgb = crate::theme::rgb;
                let palette = [rgb(t.warning), rgb(t.accent), rgb(t.success)];
                let remap = |c| state.baseline_palette.iter().position(|p| *p == c).map_or(c, |i| palette[i]);
                for triangle in &mut themed.scene.tris {
                    triangle.color = remap(triangle.color);
                }
                for line in &mut themed.scene.lines {
                    line.color = remap(line.color);
                }
                for curve in &mut themed.curves {
                    curve.color = curve_colour(config.mode);
                }
                for s in [&themed, &scene] {
                    let bg = crate::theme::rgb(Tokens::get().panel);
                    if let Ok(c) = crate::gpu3d::scene_picture_with_curves(&s.scene, &s.curves, &state.camera, [220, 140], (bg, bg)) {
                        state.compare.push(Picture::new(&c));
                    }
                }
            }
            ui.horizontal_wrapped(|ui| {
                for p in &mut state.compare {
                    let id = p.texture(ui.ctx());
                    ui.image((id, vec2(220., 140.)));
                }
            });
        } else {
            ui.small(tr("Use the same mode and depth scale to compare geometry."));
        }
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
            assert_eq!(h.call("engine.execute", json!({"command": "pcb.inspect", "params": {"net": "DATA"}}))["ok"], true);
            let report = h.app.replies.pcb.as_ref().unwrap();
            let diagram = scene(&h.app, &report["coupling_pairs"][0], Mode::Return, true, false, 0.).unwrap();
            assert_eq!(diagram.curves.is_empty(), split);
            assert!(diagram.bounds().is_some());
        }
    }
}
