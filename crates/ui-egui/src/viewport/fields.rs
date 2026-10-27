//! Fields on a plane: the picture, the probe and its readout, and the controls.

use super::*;

pub(super) fn solved(app: &EmcraftApp) -> bool {
    app.session.results.sweep.is_some() || app.session.results.eigen.is_some() || app.session.results.radiation.is_some()
}

/// The span of the plane's position (display units) and its axis name.
pub(super) fn plane_span(app: &EmcraftApp) -> Option<(f64, &'static str)> {
    plane_span_of(app, app.field.view.plane)
}

pub(super) fn plane_span_of(app: &EmcraftApp, plane: Plane) -> Option<(f64, &'static str)> {
    let emcraft_model::Aabb { min: lo, max: hi } = app.session.model.bounds()?;
    let l = app.session.display.length;
    Some(match plane {
        Plane::H => (l.from_si(hi.y - lo.y), "y"),
        Plane::E => (l.from_si(hi.x - lo.x), "x"),
        Plane::Cross => (l.from_si(hi.z - lo.z), "z"),
    })
}

/// The field view's plot: its plane sampled once (kept in `app.field.samples`), then drawn
/// at its component and phase.
pub(crate) fn field_plot(app: &mut EmcraftApp) -> Result<emcraft_render::plot::FieldPlot, String> {
    let l = app.session.display.length;
    app.field.view.at = app.field.at.map(|v| l.to_si(v));
    let at = plane_span(app).map_or(0.0, |(span, _)| l.to_si(app.field.at.unwrap_or(span / 2.0).clamp(0.0, span)));
    if !app.field.samples.as_ref().is_some_and(|s| s.serves(&app.field.view, at)) {
        app.field.samples = Some(view::sample_plane(&app.session, &app.field.view).map_err(|e| e.to_string())?);
    }
    match app.field.samples.as_ref().map(|s| s.plot(&app.field.view)) {
        Some(r) => r.map_err(|e| e.to_string()),
        None => Err("no field samples".into()),
    }
}

/// The field view's plot and those of the planes kept for the 3D view, each with its plane and
/// its position along the normal (m from the model's low side). The kept planes share the
/// view's frequency, component, phase and excitation.
pub(crate) fn field_plots(app: &mut EmcraftApp) -> Result<Vec<(emcraft_render::plot::FieldPlot, Plane, f64)>, String> {
    let plot = field_plot(app)?;
    let (plane, at) = app.field.samples.as_ref().map_or((app.field.view.plane, 0.0), |s| (s.plane, s.at));
    let mut out = vec![(plot, plane, at)];
    let l = app.session.display.length;
    app.field.kept_samples.resize_with(app.field.kept.len(), || None);
    for k in 0..app.field.kept.len() {
        let (plane, at_display) = app.field.kept[k];
        let Some((span, _)) = plane_span_of(app, plane) else { continue };
        let at = l.to_si(at_display.clamp(0.0, span));
        let view = emcraft_engine::view::FieldView { plane, at: Some(at), ..app.field.view.clone() };
        if !app.field.kept_samples[k].as_ref().is_some_and(|s| s.serves(&view, at)) {
            app.field.kept_samples[k] = Some(view::sample_plane(&app.session, &view).map_err(|e| e.to_string())?);
        }
        if let Some(s) = &app.field.kept_samples[k] {
            out.push((s.plot(&view).map_err(|e| e.to_string())?, plane, s.at));
        }
    }
    Ok(out)
}

pub(super) fn field(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    if !solved(app) {
        empty(ui, "No fields yet: solve a sweep, eigenmodes or an open problem first.");
        return;
    }
    field_controls(app, ui);
    let width = picture_width(ui);
    let l = app.session.display.length;
    app.field.view.at = app.field.at.map(|v| l.to_si(v));
    let key = format!("{:?}|{width}|{}", app.field.view, app.revision);
    if app.field.picture.as_ref().is_none_or(|p| p.key != key) {
        let plot = match field_plot(app) {
            Ok(p) => p,
            Err(e) => {
                app.field.picture = None;
                empty(ui, &e);
                return;
            }
        };
        let c = emcraft_render::plot::draw(&plot, width);
        app.field.layout = Some(emcraft_render::plot::Layout::of(&plot, width));
        app.field.picture = Some(Picture::new(key, &c));
    }
    let Some(p) = app.field.picture.as_mut() else { return };
    let r = picture_above(ui, p, "picture:field", READOUT_HEIGHT);
    probe_picture(app, ui, &r);
    readout(app, ui);
}

/// Most planes kept for the 3D view.
pub(super) const MAX_KEPT: usize = 8;

/// Room under the field picture for the probe's readout.
pub(super) const READOUT_HEIGHT: f32 = 58.0;

/// The pointer over the field picture: the fields under it, a click pins a point (a right click
/// lets it go); both are marked on the picture.
pub(super) fn probe_picture(app: &mut EmcraftApp, ui: &egui::Ui, r: &egui::Response) {
    app.field.hover = None;
    let (Some(layout), Some(samples)) = (app.field.layout, app.field.samples.as_ref()) else { return };
    let rect = r.rect;
    let (lw, lh) = (layout.width as f32, layout.height as f32);
    let to_plane = |pos: Pos2| {
        let f = (pos - rect.min) / rect.size();
        layout.to_axes([f64::from(f.x * lw), f64::from(f.y * lh)])
    };
    let to_screen = |q: [f64; 2]| {
        let [x, y] = layout.to_pixels(q);
        rect.min + vec2(x as f32 / lw * rect.width(), y as f32 / lh * rect.height())
    };
    if let Some(q) = r.hover_pos().and_then(to_plane) {
        app.field.hover = samples.probe(q);
    }
    if r.clicked()
        && let Some(q) = r.interact_pointer_pos().and_then(to_plane)
    {
        app.field.pinned = samples.probe(q).map(|p| p.at);
    }
    if r.secondary_clicked() {
        app.field.pinned = None;
    }
    let painter = ui.painter_at(rect);
    let t = Tokens::get();
    if let Some(pin) = app.field.pinned.and_then(|q| samples.probe(q)) {
        let c = to_screen(pin.at);
        painter.circle_stroke(c, 7.0, Stroke::new(3.0, Color32::WHITE));
        painter.circle_stroke(c, 7.0, Stroke::new(1.6, t.accent));
        painter.circle_filled(c, 2.0, t.accent);
    }
    if let Some(h) = &app.field.hover {
        let c = to_screen(h.at);
        let ink = Stroke::new(1.0, Color32::from_black_alpha(160));
        let [x0, y0, pw, ph] = layout.plane;
        let (left, top) = (to_screen(layout.to_axes([x0, y0 + ph]).unwrap_or(h.at)), to_screen(layout.to_axes([x0 + pw, y0]).unwrap_or(h.at)));
        painter.line_segment([pos2(left.x, c.y), pos2(top.x, c.y)], ink);
        painter.line_segment([pos2(c.x, top.y), pos2(c.x, left.y)], ink);
    }
}

/// A complex component as its amplitude and phase.
pub(super) fn phasor(c: emcraft_model::Complex64) -> String {
    use emcraft_render::axes::format;
    if c.norm() == 0.0 { "0".into() } else { format!("{} ({}°)", format(c.norm()), format(c.arg().to_degrees().round())) }
}

/// The readout under the field picture: the fields at the pointer and at the pinned point —
/// the coloured component at this phase and its amplitude, and every component of E and H as
/// amplitude (phase).
pub(super) fn readout(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let Some(samples) = app.field.samples.as_ref() else { return };
    let (xa, ya) = samples.axes();
    let name = |a: &emcraft_render::Axis| a.label.split(' ').next().unwrap_or("").to_owned();
    let (hx, vy) = (name(xa), name(ya));
    let lu = app.session.display.length.symbol();
    let comp = app.field.view.component;
    let phase = app.field.view.phase;
    let unit = if comp.is_magnetic() { "A/m" } else { "V/m" };
    let rows = [("pointer", app.field.hover), ("pinned", app.field.pinned.and_then(|q| samples.probe(q)))];
    let fmt = emcraft_render::axes::format;
    let text = |s: String| egui::RichText::new(s).size(t.font_small).color(t.text);
    let dim = |s: &str| egui::RichText::new(s).size(t.font_small).color(t.text_dim);
    ui.add_space(t.space_4);
    egui::Grid::new("field:readout").num_columns(5).spacing(vec2(t.space_12, 2.0)).show(ui, |ui| {
        for (what, probe) in rows {
            ui.label(dim(what));
            match probe {
                None => {
                    ui.label(dim(if what == "pointer" {
                        "hover the picture to read the fields"
                    } else {
                        "click the picture to pin a point (right click lets it go)"
                    }));
                }
                Some(p) => {
                    let (now, amp) = p.value(comp, phase);
                    ui.label(text(format!("{hx} = {} {lu}, {vy} = {} {lu}", fmt(p.at[0]), fmt(p.at[1]))));
                    ui.label(text(format!("{} = {} {unit}, amplitude {}", comp.label(), fmt(now), fmt(amp))).strong());
                    ui.label(text(format!("E (V/m): x {}, y {}, z {}", phasor(p.e.x), phasor(p.e.y), phasor(p.e.z))));
                    ui.label(text(format!("H (A/m): x {}, y {}, z {}", phasor(p.h.x), phasor(p.h.y), phasor(p.h.z))));
                }
            }
            ui.end_row();
        }
    });
}

pub(super) fn field_controls(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        field_choice(app, ui);
        ui.separator();
        plane_choice(app, ui);
    });
    kept_planes(app, ui);
    ui.horizontal_wrapped(|ui| field_display(app, ui));
}

/// Which field: an eigenmode, or a solved frequency.
pub(super) fn field_choice(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    let fu = app.session.display.frequency;
    if let Some(e) = app.session.results.eigen.as_ref()
        && (app.field.view.mode.is_some() || app.session.results.sweep.is_none())
    {
        let mut idx = app.field.view.mode.unwrap_or(0);
        let label = |i: usize| e.modes.get(i).map_or(String::new(), |m| format!("mode {}: {:.4} {}", i + 1, fu.from_si(m.frequency), fu.symbol()));
        egui::ComboBox::from_id_salt("mode").selected_text(label(idx)).show_ui(ui, |ui| {
            for i in 0..e.modes.len() {
                ui.selectable_value(&mut idx, i, label(i));
            }
        });
        app.field.view.mode = Some(idx);
    }
    let freqs = app.frequencies();
    if freqs.is_empty() || app.field.view.mode.is_some() {
        return;
    }
    let mut idx = freqs.iter().position(|f| (f - app.field.view.frequency).abs() <= 1e-9 * f).unwrap_or(0);
    let label = |f: f64| format!("{:.4} {}", fu.from_si(f), fu.symbol());
    let r = egui::ComboBox::from_id_salt("frequency").selected_text(label(app.field.view.frequency)).show_ui(ui, |ui| {
        for (i, f) in freqs.iter().enumerate() {
            ui.selectable_value(&mut idx, i, label(*f));
        }
    });
    scenario::publish_target("field:frequency", r.response.rect);
    if let Some(f) = freqs.get(idx) {
        app.field.view.frequency = *f;
    }
}

/// The plane, where it cuts, and a button that keeps it in the 3D view.
pub(super) fn plane_choice(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    for (p, name) in [(Plane::H, "H-plane"), (Plane::E, "E-plane"), (Plane::Cross, "Cross-section")] {
        let r = ui.selectable_label(app.field.view.plane == p, name);
        scenario::publish_target(&format!("field:plane:{}", p.name()), r.rect);
        if r.clicked() && app.field.view.plane != p {
            app.field.view.plane = p;
            app.field.at = None;
            app.field.pinned = None;
        }
    }
    let Some((span, axis)) = plane_span(app) else { return };
    let mut at = app.field.at.unwrap_or(span / 2.0);
    let unit = app.session.display.length.symbol();
    let r = crate::ui_kit::slider(ui, &mut at, 0.0..=span, format!("{axis} ({unit})"));
    scenario::publish_target("field:at", r.rect);
    if r.changed() {
        app.field.at = Some(at);
        app.field.pinned = None;
    }
    // Keep this plane in the 3D view (Results), beside the one being moved.
    let here = (app.field.view.plane, at);
    let (r, resp) = ui.allocate_exact_size(vec2(26.0, 22.0), Sense::click());
    let t = Tokens::get();
    if resp.hovered() {
        ui.painter().rect_filled(r, t.radius_sm, t.hover);
    }
    crate::glyphs::paint(ui.painter(), r.shrink(3.0), "keep_plane", t.icon, t.accent);
    scenario::publish_target("field:keep", r);
    if resp.on_hover_text("Keep this plane in the 3D view").clicked() && !app.field.kept.contains(&here) && app.field.kept.len() < MAX_KEPT {
        app.field.kept.push(here);
    }
}

/// The planes kept for the 3D view, each a chip that a click lets go.
pub(super) fn kept_planes(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    if app.field.kept.is_empty() {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        let t = Tokens::get();
        ui.label(egui::RichText::new("In 3D:").size(t.font_small).color(t.text_dim));
        let unit = app.session.display.length.symbol();
        let mut drop = None;
        for (k, (plane, at)) in app.field.kept.iter().enumerate() {
            let name = match plane {
                Plane::H => "H-plane y",
                Plane::E => "E-plane x",
                Plane::Cross => "cross z",
            };
            let r = ui.button(egui::RichText::new(format!("{name} = {} {unit}  ✕", emcraft_render::axes::format(*at))).size(t.font_small));
            scenario::publish_target(&format!("field:kept:{k}"), r.rect);
            if r.on_hover_text("Remove from the 3D view").clicked() {
                drop = Some(k);
            }
        }
        if let Some(k) = drop {
            app.field.kept.remove(k);
            if k < app.field.kept_samples.len() {
                app.field.kept_samples.remove(k);
            }
        }
    });
}

/// What is drawn: the coloured component, the arrows, the excited port, and the phase played
/// like a video.
pub(super) fn field_display(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    let before = app.field.view.component;
    let r = egui::ComboBox::from_id_salt("component").selected_text(before.label()).show_ui(ui, |ui| {
        for c in emcraft_engine::view::Component::ALL {
            ui.selectable_value(&mut app.field.view.component, c, c.label());
        }
    });
    scenario::publish_target("field:component", r.response.rect);
    // Arrows of the field coloured; the other stays a click away.
    let now = app.field.view.component;
    if now != before && now.is_magnetic() != before.is_magnetic() {
        app.field.view.show_e = !now.is_magnetic();
        app.field.view.show_h = now.is_magnetic();
    }
    ui.checkbox(&mut app.field.view.show_e, "E arrows");
    ui.checkbox(&mut app.field.view.show_h, "H arrows");
    let ports = &app.session.model.ports;
    if ports.len() > 1 && app.field.view.mode.is_none() {
        ui.separator();
        ui.label("excite");
        for (i, p) in ports.iter().enumerate() {
            ui.radio_value(&mut app.field.view.excitation, i, p.name.as_str());
        }
    }
    ui.separator();
    crate::ui_kit::player(ui, &mut app.field.player, &mut app.field.view.phase, "field:phase");
}
