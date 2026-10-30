//! Fields on a plane: the picture, the probe and its readout, and the controls.

use super::*;
use crate::lang::args;

pub(crate) fn plane_span_of(app: &EmcraftApp, plane: Plane) -> Option<(f64, &'static str)> {
    let emcraft_model::Aabb { min: lo, max: hi } = app.session.model().bounds()?;
    let l = app.session.display().length;
    Some(match plane {
        Plane::H => (l.from_si(hi.y - lo.y), "y"),
        Plane::E => (l.from_si(hi.x - lo.x), "x"),
        Plane::Cross => (l.from_si(hi.z - lo.z), "z"),
    })
}

/// The field view's plot: its plane sampled once (kept in `field.samples`), then drawn at its
/// component and phase.
pub(crate) fn field_plot(app: &EmcraftApp, field: &mut FieldState) -> Result<emcraft_render::plot::FieldPlot, String> {
    let l = app.session.display().length;
    field.view.at = field.at.map(|v| l.to_si(v));
    let at = plane_span_of(app, field.view.plane).map_or(0.0, |(span, _)| l.to_si(field.at.unwrap_or(span / 2.0).clamp(0.0, span)));
    if !field.samples.as_ref().is_some_and(|s| s.serves(&field.view, at)) {
        field.samples = Some(view::sample_plane(&app.session, &field.view).map_err(|e| e.to_string())?);
    }
    match field.samples.as_ref().map(|s| s.plot(&field.view)) {
        Some(r) => r.map_err(|e| e.to_string()),
        None => Err("no field samples".into()),
    }
}

/// The field view's plot and those of the planes kept for the 3D view, each with its plane and
/// its position along the normal (m from the model's low side). The kept planes share the
/// view's frequency, component, phase and excitation.
pub(crate) fn field_plots(app: &EmcraftApp, field: &mut FieldState) -> Result<Vec<(emcraft_render::plot::FieldPlot, Plane, f64)>, String> {
    let plot = field_plot(app, field)?;
    let (plane, at) = field.samples.as_ref().map_or((field.view.plane, 0.0), |s| (s.plane, s.at));
    let mut out = vec![(plot, plane, at)];
    let l = app.session.display().length;
    let base = field.view.clone();
    for kept in &mut field.kept {
        let Some((span, _)) = plane_span_of(app, kept.plane) else { continue };
        let at = l.to_si(kept.at.clamp(0.0, span));
        let view = emcraft_engine::view::FieldView { plane: kept.plane, at: Some(at), ..base.clone() };
        if !kept.samples.as_ref().is_some_and(|s| s.serves(&view, at)) {
            kept.samples = Some(view::sample_plane(&app.session, &view).map_err(|e| e.to_string())?);
        }
        if let Some(s) = &kept.samples {
            out.push((s.plot(&view).map_err(|e| e.to_string())?, kept.plane, s.at));
        }
    }
    Ok(out)
}

pub(super) fn field(app: &EmcraftApp, st: &mut Parts, ui: &mut egui::Ui, out: &mut Actions) {
    field_controls(app, &mut st.field, ui);
    // The model in 3D on the left when asked, then the picture and, beside it, the side column
    // about it (where, which frequency, the readout, saving).
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let gap = 10.0;
    let rest = if st.field.beside_3d {
        let split = area.left() + (area.width() * 0.36).round();
        let left = Rect::from_min_max(area.min, pos2(split - gap / 2.0, area.max.y));
        crate::view3d::show(app, &mut st.view3d, &mut st.field, ui, left, crate::scene3d::Overlay::Field, out);
        Rect::from_min_max(pos2(split + gap / 2.0, area.min.y), area.max)
    } else {
        area
    };
    let side_rect = Rect::from_min_max(pos2(rest.max.x - field_side::SIDE, rest.min.y), rest.max);
    let pic_rect = Rect::from_min_max(rest.min, pos2(side_rect.min.x - gap, rest.max.y));
    let mut pic_ui = ui.new_child(egui::UiBuilder::new().max_rect(pic_rect));
    picture_column(app, &mut st.field, &mut pic_ui);
    let mut side_ui = ui.new_child(egui::UiBuilder::new().max_rect(side_rect));
    egui::ScrollArea::vertical().id_salt("field:side").show(&mut side_ui, |ui| field_side::side(app, &mut st.field, ui, out));
}

/// The field's picture, as large as its column allows, with the probe on it.
fn picture_column(app: &EmcraftApp, field: &mut FieldState, ui: &mut egui::Ui) {
    let width = picture_width(ui);
    let l = app.session.display().length;
    field.view.at = field.at.map(|v| l.to_si(v));
    let key = PictureKey::new(app.revision, width, field.view.clone());
    if !field.picture.is_fresh(&key) {
        let plot = match field_plot(app, field) {
            Ok(p) => p,
            Err(e) => {
                field.picture.clear();
                return empty(ui, &e);
            }
        };
        field.layout = Some(emcraft_render::plot::layout_of(&plot, width));
        field.picture.set(key, &emcraft_render::plot::draw(&plot, width, &crate::theme::palette()));
    }
    let Some(p) = field.picture.get_mut() else { return };
    let r = picture(ui, p, Fixed::PictureField);
    probe_picture(field, ui, &r);
}

/// Most planes kept for the 3D view.
pub(super) const MAX_KEPT: usize = 8;

/// The pointer over the field picture: the fields under it, a click pins a point (a right click
/// lets it go); both are marked on the picture.
pub(super) fn probe_picture(field: &mut FieldState, ui: &egui::Ui, r: &egui::Response) {
    field.hover = None;
    let (Some(layout), Some(samples)) = (field.layout, field.samples.as_ref()) else { return };
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
        field.hover = samples.probe(q);
    }
    if r.clicked()
        && let Some(q) = r.interact_pointer_pos().and_then(to_plane)
    {
        field.pinned = samples.probe(q).map(|p| p.at);
    }
    if r.secondary_clicked() {
        field.pinned = None;
    }
    let painter = ui.painter_at(rect);
    let t = &Tokens::get();
    if let Some(pin) = field.pinned.and_then(|q| samples.probe(q)) {
        let c = to_screen(pin.at);
        painter.circle_stroke(c, 7.0, Stroke::new(3.0, crate::theme::color32(crate::theme::palette().halo)));
        painter.circle_stroke(c, 7.0, Stroke::new(1.6, t.accent));
        painter.circle_filled(c, 2.0, t.accent);
        // The value beside the pin, on the side with room.
        let comp = field.view.component;
        let (now, amp) = pin.value(comp, field.view.phase);
        let fmt = emcraft_render::axes::format;
        let unit = if comp.is_magnetic() { "A/m" } else { "V/m" };
        let text = trf(
            "{component} {now} {unit} (amp. {amplitude} {unit})",
            args!(component = comp.label(), now = fmt(now), amplitude = fmt(amp), unit = unit),
        );
        let galley = painter.layout_no_wrap(text, FontId::proportional(t.font_small), t.text);
        let size = galley.size() + vec2(10.0, 6.0);
        let right = c.x + 12.0 + size.x < rect.right();
        let min = if right { pos2(c.x + 12.0, c.y - size.y - 6.0) } else { pos2(c.x - 12.0 - size.x, c.y - size.y - 6.0) };
        let callout = Rect::from_min_size(pos2(min.x, min.y.max(rect.top() + 2.0)), size);
        painter.rect_filled(callout, t.radius_sm, t.panel.gamma_multiply(0.95));
        painter.rect_stroke(callout, t.radius_sm, Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
        painter.galley(callout.min + vec2(5.0, 3.0), galley, t.text);
    }
    if let Some(h) = &field.hover {
        let c = to_screen(h.at);
        let l = crate::theme::palette().line;
        let ink = Stroke::new(1.0, Color32::from_rgba_unmultiplied(l.0, l.1, l.2, 160));
        let [x0, y0, pw, ph] = layout.plane;
        let (left, top) = (to_screen(layout.to_axes([x0, y0 + ph]).unwrap_or(h.at)), to_screen(layout.to_axes([x0 + pw, y0]).unwrap_or(h.at)));
        painter.line_segment([pos2(left.x, c.y), pos2(top.x, c.y)], ink);
        painter.line_segment([pos2(c.x, top.y), pos2(c.x, left.y)], ink);
    }
}

/// A complex component as its amplitude in `unit` and its phase.
pub(super) fn phasor(c: emcraft_model::Complex64, unit: &str) -> String {
    use emcraft_render::axes::format;
    if c.norm() == 0.0 { format!("0 {unit}") } else { format!("{} {unit} ∠ {}°", format(c.norm()), format(c.arg().to_degrees().round())) }
}

pub(super) fn field_controls(app: &EmcraftApp, field: &mut FieldState, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        field_choice(app, field, ui);
        ui.separator();
        plane_choice(app, field, ui);
        ui.separator();
        let r = ui.named(egui::Button::selectable(field.beside_3d, tr("Beside 3D")), Fixed::FieldBeside3d.into());
        if r.hint("The model in 3D beside the picture, the plane in it").clicked() {
            field.beside_3d = !field.beside_3d;
        }
    });
    kept_planes(app, field, ui);
    ui.horizontal_wrapped(|ui| field_display(app, field, ui));
}

/// Which field: an eigenmode, or a solved frequency.
pub(super) fn field_choice(app: &EmcraftApp, field: &mut FieldState, ui: &mut egui::Ui) {
    let fu = app.session.display().frequency;
    if let Some(e) = app.session.results().eigen.as_ref()
        && (field.view.mode.is_some() || app.session.results().sweep.is_none())
    {
        let mut idx = field.view.mode.unwrap_or(0);
        let label = |i: usize| {
            e.modes
                .get(i)
                .map_or(String::new(), |m| trf("mode {n}: {f}", args!(n = (i + 1), f = format!("{:.4} {}", fu.from_si(m.frequency), fu.symbol()))))
        };
        egui::ComboBox::from_id_salt("mode").selected_text(label(idx)).show_ui(ui, |ui| {
            for i in 0..e.modes.len() {
                ui.selectable_value(&mut idx, i, label(i));
            }
        });
        field.view.mode = Some(idx);
    }
    let freqs = app.frequencies();
    if freqs.is_empty() || field.view.mode.is_some() {
        return;
    }
    let mut idx = freqs.iter().position(|f| (f - field.view.frequency).abs() <= 1e-9 * f).unwrap_or(0);
    let label = |f: f64| format!("{:.4} {}", fu.from_si(f), fu.symbol());
    let r = egui::ComboBox::from_id_salt("frequency").selected_text(label(field.view.frequency)).show_ui(ui, |ui| {
        for (i, f) in freqs.iter().enumerate() {
            ui.selectable_value(&mut idx, i, label(*f));
        }
    });
    scenario::publish_target(Fixed::FieldFrequency, r.response.rect);
    if let Some(f) = freqs.get(idx) {
        field.view.frequency = *f;
    }
}

/// The plane, where it cuts, and a button that keeps it in the 3D view.
pub(super) fn plane_choice(app: &EmcraftApp, field: &mut FieldState, ui: &mut egui::Ui) {
    for (p, name) in [(Plane::H, "H-plane"), (Plane::E, "E-plane"), (Plane::Cross, "Cross-section")] {
        let r = ui.named(egui::Button::selectable(field.view.plane == p, tr(name)), Handle::FieldPlane(p));
        if r.clicked() && field.view.plane != p {
            field.view.plane = p;
            field.at = None;
            field.pinned = None;
        }
    }
    let Some((span, axis)) = plane_span_of(app, field.view.plane) else { return };
    let mut at = field.at.unwrap_or(span / 2.0);
    let unit = app.session.display().length.symbol();
    let r = crate::ui_kit::slider(ui, &mut at, 0.0..=span, format!("{axis} ({unit})"));
    scenario::publish_target(Fixed::FieldAt, r.rect);
    // What lies along the slider: the materials (or sections) the plane passes, as a strip under
    // its track.
    let track = Rect::from_min_size(pos2(r.rect.left() + 6.0, r.rect.bottom() - 2.0), vec2(ui.spacing().slider_width - 12.0, 4.0));
    for (from, to, colour) in field_side::bands(app, field.view.plane) {
        let band = Rect::from_min_max(
            pos2(track.left() + from as f32 * track.width(), track.top()),
            pos2(track.left() + to as f32 * track.width(), track.bottom()),
        );
        ui.painter().rect_filled(band, 0.0, colour.unwrap_or_else(|| crate::theme::color32(crate::theme::scene().metal_dark)));
    }
    if r.changed() {
        field.at = Some(at);
        field.pinned = None;
    }
    // Keep this plane in the 3D view (Results), beside the one being moved.
    let here = (field.view.plane, at);
    let (r, resp) = ui.allocate_exact_size(vec2(26.0, 22.0), Sense::click());
    scenario::publish_target(Fixed::FieldKeep, r);
    let t = &Tokens::get();
    if resp.hovered() {
        ui.painter().rect_filled(r, t.radius_sm, t.hover);
    }
    crate::glyphs::paint(ui.painter(), r.shrink(3.0), crate::glyphs::Glyph::KeepPlane, t.icon, t.accent);
    let kept = field.kept.iter().any(|k| (k.plane, k.at) == here);
    if resp.hint("Keep this plane in the 3D view").clicked() && !kept && field.kept.len() < MAX_KEPT {
        field.kept.push(crate::state::KeptPlane { plane: here.0, at: here.1, samples: None });
    }
}

/// The planes kept for the 3D view, each a chip that a click lets go.
pub(super) fn kept_planes(app: &EmcraftApp, field: &mut FieldState, ui: &mut egui::Ui) {
    if field.kept.is_empty() {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        let t = &Tokens::get();
        crate::ui_kit::small(ui, "In 3D:", t.text_dim);
        let unit = app.session.display().length.symbol();
        let mut drop = None;
        for (k, crate::state::KeptPlane { plane, at, .. }) in field.kept.iter().enumerate() {
            let name = match plane {
                Plane::H => "H-plane y",
                Plane::E => "E-plane x",
                Plane::Cross => "cross z",
            };
            let chip =
                egui::Button::new(egui::RichText::new(format!("{} = {} {unit}  ✕", tr(name), emcraft_render::axes::format(*at))).size(t.font_small));
            let r = ui.named(chip, Handle::FieldKept(k));
            if r.hint("Remove from the 3D view").clicked() {
                drop = Some(k);
            }
        }
        if let Some(k) = drop {
            field.kept.remove(k);
        }
    });
}

/// What is drawn: the coloured component, the arrows, the excited port, and the phase played
/// like a video.
pub(super) fn field_display(app: &EmcraftApp, field: &mut FieldState, ui: &mut egui::Ui) {
    let before = field.view.component;
    let r = egui::ComboBox::from_id_salt("component").selected_text(before.label()).show_ui(ui, |ui| {
        for c in emcraft_engine::view::Component::ALL {
            ui.selectable_value(&mut field.view.component, c, c.label());
        }
    });
    scenario::publish_target(Fixed::FieldComponent, r.response.rect);
    // Arrows of the field coloured; the other stays a click away.
    let now = field.view.component;
    if now != before && now.is_magnetic() != before.is_magnetic() {
        field.view.show_e = !now.is_magnetic();
        field.view.show_h = now.is_magnetic();
    }
    ui.checkbox(&mut field.view.show_e, tr("E arrows"));
    ui.checkbox(&mut field.view.show_h, tr("H arrows"));
    // Each entry of S is an excitation: a port, or a terminal of a terminal port.
    let entries = app.session.sparameter_entries();
    if entries.len() > 1 && field.view.mode.is_none() {
        ui.separator();
        ui.text(Style::Body, "excite");
        for (i, e) in entries.iter().enumerate() {
            ui.radio_value(&mut field.view.excitation, i, e.name.as_str());
        }
    }
    ui.separator();
    crate::ui_kit::player(ui, &mut field.player, &mut field.view.phase, Fixed::FieldPhase);
}
