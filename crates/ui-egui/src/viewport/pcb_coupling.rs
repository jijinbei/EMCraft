//! Original conceptual coupling diagrams. No field samples, amplitudes or noise voltages are synthesized.
use super::*;
use emcraft_engine::pcb::CouplingMode as Mode;
use emcraft_render::view3d::{Camera, Scene};
#[derive(Default)]
pub(crate) struct IllustrationUi {
    pub index: usize,
    camera: Camera,
    key: Option<String>,
    picture: Option<Picture>,
    baseline: Option<(Scene, f64, f64, Mode, bool, bool, String, String)>,
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
                action(out, json!({"pick_victim":role}));
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
                    action(out, json!({"victim":net}));
                }
            }
        });
        let r = ui.button(tr("Swap source / receiver"));
        scenario::publish_target(Handle::Choice("pcb_coupling", "swap"), r.rect);
        if r.clicked()
            && let (Some(a), Some(b)) = (&app.session.views().pcb_net, &c.victim)
        {
            action(out, json!({"net":b,"victim":a,"stage":"net"}));
        }
    });
    ui.small(tr("Source is orange; receiver is blue. Select each role before clicking a trace."));
}
fn points(v: &Value, unit: emcraft_model::units::Length) -> Option<[[f64; 2]; 2]> {
    Some([[unit.to_si(v[0][0].as_f64()?), unit.to_si(v[0][1].as_f64()?)], [unit.to_si(v[1][0].as_f64()?), unit.to_si(v[1][1].as_f64()?)]])
}
fn scene(app: &EmcraftApp, pair: &Value, mode: Mode, exaggerate: bool, section: bool, alpha: u8) -> Option<Scene> {
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
    let extent = if section { height * 8. + offset[0].hypot(offset[1]) * 2. } else { half.max(offset[0].hypot(offset[1]) * 2.) };
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
                        130,
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
        scene.quad([q(-span, -1.), q(span, -1.), q(span, 1.), q(-span, 1.)], rgb(colour), 255);
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
    for k in 0..if section || mode == Mode::Return { 1 } else { 5 } {
        let x = if section { 0. } else { half * (k as f64 / 4. - 0.5) };
        let a = p(x);
        let b = [a[0] + offset[0], a[1] + offset[1]];
        let mut curve = Vec::new();
        for j in 0..=24 {
            let f = j as f64 / 24.;
            let point = match mode {
                Mode::Electric => {
                    let q = [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f];
                    let mut v = at(q, layer.z);
                    v[1] += height * depth * 0.7 * (std::f64::consts::PI * f).sin();
                    v
                }
                Mode::Magnetic => {
                    let angle = std::f64::consts::TAU * f;
                    let radius = height * 5.;
                    let mut v = at([a[0] + n[0] * radius * angle.cos(), a[1] + n[1] * radius * angle.cos()], layer.z);
                    v[1] += height * 0.4 * depth * angle.sin();
                    v
                }
                Mode::Return => at(p(-half + f * 2. * half), reference.z),
            };
            curve.push(point);
        }
        if mode == Mode::Return && (bad || !(0..=24).all(|j| covered(p(-half + 2. * half * j as f64 / 24.)))) {
            continue;
        }
        for v in curve.windows(2) {
            scene.line(v[0], v[1], rgb(if mode == Mode::Electric { t.warning } else { t.accent }), alpha);
        }
    }
    Some(scene)
}

pub(super) fn show(app: &EmcraftApp, state: &mut crate::state::PcbUi, ui: &mut egui::Ui, report: &Value, out: &mut Actions) {
    let config = &app.session.views().pcb_illustration;
    let Some(victim) = &config.victim else {
        return;
    };
    ui.heading(tr("Coupling illustration"));
    ui.colored_label(Tokens::get().warning, tr("Schematic only — fields and noise voltage are not calculated"));
    ui.horizontal(|ui| {
        ui.colored_label(Tokens::get().warning, format!("{}: {}", tr("Source"), report["net"].as_str().unwrap_or("")));
        ui.colored_label(Tokens::get().accent, format!("{}: {}", tr("Receiver"), victim));
    });
    ui.small(tr("3D shows the selected parallel overlap; circles mark its centre in 2D."));
    let pairs: Vec<_> = report["coupling_pairs"].as_array().into_iter().flatten().filter(|p| p["target"].as_str() == Some(victim)).collect();
    if pairs.is_empty() {
        ui.label(tr("No supported parallel overlap for this pair. Angles, vias and interlayer coupling are not illustrated."));
        return;
    }
    if app.session.variables().contains_key("gap") {
        let r = ui.button(tr("Edit template spacing…"));
        scenario::publish_target(Handle::Choice("pcb_coupling", "spacing"), r.rect);
        if r.clicked() {
            out.push(Action::dialog_with("project.variable", json!({"name":"gap"})));
        }
    }
    let state = &mut state.illustration;
    state.index = state.index.min(pairs.len() - 1);
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
    ui.small(format!(
        "gap {:.3} {} · overlap {:.3} {}",
        pair["gap"].as_f64().unwrap_or(0.),
        app.session.display().length.symbol(),
        pair["overlap"].as_f64().unwrap_or(0.),
        app.session.display().length.symbol()
    ));
    ui.horizontal_wrapped(|ui| {
        for mode in Mode::ALL {
            let r = ui.selectable_label(config.mode == mode, shown(mode));
            scenario::publish_target(Handle::Choice("pcb_mode", mode.id()), r.rect);
            if r.clicked() {
                action(out, json!({"mode":mode.id()}));
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
                action(out, json!({key:!value}));
            }
        }
    });
    ui.small(tr("Motion is explanatory emphasis, not physical time. Drag to orbit; scroll to zoom."));
    ui.small(tr(match config.mode {
        Mode::Electric => "Electric coupling: schematic connections between conductors; no field strength is implied.",
        Mode::Magnetic => "Magnetic coupling: symbolic loops around the source current and receiver; no flux is calculated.",
        Mode::Return => "Return reference only, not a solved current. Missing ground is never bridged.",
    }));
    let frame = if config.playing { (ui.input(|i| i.time) * 5.) as u64 % 8 } else { 0 };
    if config.playing {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
    }
    let Some(scene) = scene(app, pair, config.mode, config.exaggerate, config.section, if config.playing { 100 + (frame * 18) as u8 } else { 220 })
    else {
        ui.label(tr("No supported reference geometry for a 3D illustration."));
        return;
    };
    let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), 280.), Sense::drag());
    scenario::publish_target(Handle::Choice("pcb_coupling", "scene"), rect);
    let fitkey = format!("{}:{}:{}:{}", pair["source_segment"], pair["target_segment"], config.exaggerate, config.section);
    if state.key.as_ref().is_none_or(|s| !s.starts_with(&fitkey))
        && let Some((lo, hi)) = scene.bounds()
    {
        state.camera.fit(lo, hi, rect.aspect_ratio() as f64);
        state.camera.distance *= 0.7;
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
        match crate::gpu3d::scene_picture(&scene, &state.camera, [rect.width().min(1000.) as u32, 280], (bg, bg)) {
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
            ui.small(format!(
                "gap {:.3} → {:.3}; overlap {:.3} → {:.3} {}",
                app.session.display().length.from_si(*gap),
                pair["gap"].as_f64().unwrap_or(0.),
                app.session.display().length.from_si(*length),
                pair["overlap"].as_f64().unwrap_or(0.),
                app.session.display().length.symbol()
            ));
            // Both pictures use exactly the current camera, size and depth scaling.
            if state.compare.is_empty() {
                let mut themed = old.clone();
                let t = Tokens::get();
                let rgb = crate::theme::rgb;
                let palette = [rgb(t.warning), rgb(t.accent), rgb(t.success)];
                let remap = |c| state.baseline_palette.iter().position(|p| *p == c).map_or(c, |i| palette[i]);
                for triangle in &mut themed.tris {
                    triangle.color = remap(triangle.color);
                }
                for line in &mut themed.lines {
                    line.color = remap(line.color);
                }
                for s in [&themed, &scene] {
                    let bg = crate::theme::rgb(Tokens::get().panel);
                    if let Ok(c) = crate::gpu3d::scene_picture(s, &state.camera, [220, 140], (bg, bg)) {
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
            assert_eq!(h.call("engine.execute", json!({"command":"pcb.sample","params":{"split":split}}))["ok"], true);
            assert_eq!(h.call("engine.execute", json!({"command":"pcb.inspect","params":{"net":"DATA"}}))["ok"], true);
            let report = h.app.replies.pcb.as_ref().unwrap();
            let diagram = scene(&h.app, &report["coupling_pairs"][0], Mode::Return, true, false, 220).unwrap();
            assert_eq!(diagram.lines.is_empty(), split);
            assert!(diagram.bounds().is_some());
        }
    }
}
