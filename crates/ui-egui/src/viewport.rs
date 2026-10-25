//! The viewport card: the model's outline (H-plane and E-plane views of the guide or the
//! solid's bounding views, with materials, apertures, ports and sources), and the results — the
//! S-parameter chart against the exact curves, fields on a plane with a phase animation, the
//! port-mode and eigenmode tables, the far-field pattern and the comparison with theory. Charts
//! and fields are `emcraft-render`'s CPU pictures, shown as textures.

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, pos2, vec2};
use emcraft_engine::view::{self, Plane};
use serde_json::{Value, json};

use crate::theme::Tokens;
use crate::{EmcraftApp, Picture, View, scenario};

/// The central card.
pub fn show(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE.fill(t.viewport_top).inner_margin(egui::Margin { left: 6, right: 6, top: 0, bottom: 6 }))
        .show(ui, |ui| {
            let r = ui.max_rect();
            // The viewport's gradient ground.
            let mut mesh = egui::Mesh::default();
            mesh.colored_vertex(r.left_top(), t.viewport_top);
            mesh.colored_vertex(r.right_top(), t.viewport_top);
            mesh.colored_vertex(r.right_bottom(), t.viewport_bottom);
            mesh.colored_vertex(r.left_bottom(), t.viewport_bottom);
            mesh.add_triangle(0, 1, 2);
            mesh.add_triangle(0, 2, 3);
            ui.painter().add(egui::Shape::mesh(mesh));
            ui.painter().rect_stroke(r, t.radius_lg, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
            app.viewport_rect = Some(r);
            scenario::publish_rect("viewport", r);
            let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(r.shrink(10.0)));
            view_tabs(app, &mut inner);
            inner.add_space(4.0);
            match app.view {
                View::Model => model(app, &mut inner),
                View::SParameters => sparameters(app, &mut inner),
                View::Field => field(app, &mut inner),
                View::PortModes => port_modes(app, &mut inner),
                View::FarField => far_field(app, &mut inner),
                View::Compare => compare(app, &mut inner),
                View::Eigenmodes => eigenmodes(app, &mut inner),
            }
        });
}

/// The views as a row of pills.
fn view_tabs(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    ui.horizontal(|ui| {
        for v in View::ALL {
            let on = app.view == v;
            let text = egui::RichText::new(v.label()).size(t.font_body).color(if on { t.selected_ink } else { t.text });
            let r = ui.add(egui::Button::new(text).fill(if on { t.accent_soft } else { Color32::TRANSPARENT }).corner_radius(12));
            scenario::publish_target(&format!("view:{}", v.id()), r.rect);
            if r.clicked() {
                crate::ribbon::show_view(app, v);
            }
        }
    });
}

fn empty(ui: &mut egui::Ui, text: &str) {
    let t = Tokens::get();
    ui.centered_and_justified(|ui| {
        ui.label(egui::RichText::new(text).size(t.font_heading).color(t.text_dim));
    });
}

/// A picture in the rest of the card, scaled down to fit; the image's response.
fn picture(ui: &mut egui::Ui, p: &mut Picture, handle: &str) -> egui::Response {
    let size = p.size();
    let avail = ui.available_size();
    let scale = (avail.x / size.x).min(avail.y / size.y).clamp(0.1, 1.0);
    let id = p.texture(ui.ctx());
    let r = ui.add(egui::Image::new((id, size * scale)).sense(Sense::click()));
    scenario::publish_target(handle, r.rect);
    r
}

/// A width for pictures, rounded so resizing does not redraw on every pixel.
fn picture_width(ui: &egui::Ui) -> usize {
    (((ui.available_width().max(400.0)) / 50.0).floor() * 50.0).min(2400.0) as usize
}

// ---- model ---------------------------------------------------------------------------------

fn material_color(i: usize, eps: f64) -> Color32 {
    if eps <= 1.0 + 1e-12 {
        return Color32::from_rgba_unmultiplied(160, 190, 230, 50);
    }
    const PALETTE: [(u8, u8, u8); 6] = [(214, 150, 60), (90, 170, 110), (170, 110, 200), (60, 150, 190), (200, 100, 110), (150, 150, 70)];
    let (r, g, b) = PALETTE[i % PALETTE.len()];
    Color32::from_rgba_unmultiplied(r, g, b, 120)
}

/// A drawing frame: model coordinates (horizontal, vertical in metres) onto a screen rectangle,
/// one scale for both.
struct Frame2 {
    rect: Rect,
    h0: f64,
    v0: f64,
    scale: f64,
    height: f64,
}

impl Frame2 {
    /// `scale` pixels per metre, centred in `rect`.
    fn with_scale(rect: Rect, h: (f64, f64), v: (f64, f64), scale: f64) -> Frame2 {
        let (dh, dv) = ((h.1 - h.0).max(1e-9), (v.1 - v.0).max(1e-9));
        let (w, hgt) = (dh * scale, dv * scale);
        let left = rect.center().x as f64 - w / 2.0;
        let top = rect.center().y as f64 - hgt / 2.0;
        Frame2 { rect: Rect::from_min_size(pos2(left as f32, top as f32), vec2(w as f32, hgt as f32)), h0: h.0, v0: v.0, scale, height: dv }
    }

    fn at(&self, h: f64, v: f64) -> Pos2 {
        pos2(self.rect.left() + ((h - self.h0) * self.scale) as f32, self.rect.top() + ((self.height - (v - self.v0)) * self.scale) as f32)
    }

    fn rect(&self, h0: f64, v0: f64, h1: f64, v1: f64) -> Rect {
        Rect::from_two_pos(self.at(h0, v0), self.at(h1, v1))
    }
}

fn model(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let m = &app.session.model;
    let Some((lo, hi)) = m.bounds() else {
        empty(ui, "No geometry yet: MODEL › Waveguide or Solid, or open a sample.");
        return;
    };
    let l = app.session.display.length;
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let p = ui.painter().with_clip_rect(area);
    let half = (area.height() - 40.0) / 2.0;
    let views = [("H-plane (x across z)", 0usize), ("E-plane (y across z)", 1usize)];
    // One scale for both views (the smaller of what each band allows), so z lines up.
    let band_size = |k: usize| vec2(area.width() - 120.0, half - 52.0 + 0.0 * k as f32);
    let fit = |k: usize| {
        let (v0, v1) = if k == 0 { (lo.x, hi.x) } else { (lo.y, hi.y) };
        let s = band_size(k);
        (s.x as f64 / (hi.z - lo.z).max(1e-9)).min(s.y as f64 / (v1 - v0).max(1e-9))
    };
    let scale = fit(0).min(fit(1));
    let mat_index = |name: &str| m.materials.iter().position(|x| x.name == name).unwrap_or(0);
    let eps = |name: &str| m.material(name).map_or(1.0, |(_, x)| x.eps_r);
    for (k, (title, axis)) in views.into_iter().enumerate() {
        let band = Rect::from_min_size(pos2(area.left(), area.top() + k as f32 * (half + 40.0)), vec2(area.width(), half + 40.0));
        p.text(band.left_top() + vec2(4.0, 2.0), Align2::LEFT_TOP, title, FontId::proportional(t.font_label), t.text_dim);
        let pad = m.open.as_ref().map_or(0.0, |o| o.thickness * 0.0);
        let (v0, v1) = if axis == 0 { (lo.x, hi.x) } else { (lo.y, hi.y) };
        let fr = Frame2::with_scale(band.shrink2(vec2(60.0, 26.0)).translate(vec2(0.0, 8.0)), (lo.z - pad, hi.z + pad), (v0, v1), scale);
        let outline = Stroke::new(1.5, t.text);
        if let Some(w) = &m.waveguide {
            let mut z = 0.0;
            for (i, s) in w.sections.iter().enumerate() {
                let (a0, a) = if axis == 0 { w.aperture(i).unwrap_or((0.0, w.width)) } else { w.aperture_y(i).unwrap_or((0.0, w.height)) };
                let full = if axis == 0 { w.width } else { w.height };
                // Metal outside the aperture.
                let metal = t.text_dim.gamma_multiply(0.55);
                if a0 > 0.0 {
                    p.rect_filled(fr.rect(z, 0.0, z + s.length, a0), 0.0, metal);
                }
                if a0 + a < full - 1e-12 {
                    p.rect_filled(fr.rect(z, a0 + a, z + s.length, full), 0.0, metal);
                }
                p.rect_filled(fr.rect(z, a0, z + s.length, a0 + a), 0.0, material_color(mat_index(&s.material), eps(&s.material)));
                p.vline(fr.at(z, 0.0).x, fr.at(z, full).y..=fr.at(z, 0.0).y, Stroke::new(0.5, t.border));
                let c = fr.rect(z, a0, z + s.length, a0 + a).center();
                p.text(c, Align2::CENTER_CENTER, &s.material, FontId::proportional(t.font_small), t.text);
                z += s.length;
            }
            if axis == 1 {
                let mut y = 0.0;
                for layer in &w.layers {
                    p.rect_filled(
                        fr.rect(0.0, y, w.length(), y + layer.thickness),
                        0.0,
                        material_color(mat_index(&layer.material), eps(&layer.material)),
                    );
                    y += layer.thickness;
                }
                for st in &w.strips {
                    p.line_segment([fr.at(st.z0, st.y), fr.at(st.z1, st.y)], Stroke::new(3.0, t.warning));
                }
            } else {
                for st in &w.strips {
                    p.rect_filled(fr.rect(st.z0, st.x0, st.z1, st.x0 + st.width), 0.0, t.warning.gamma_multiply(0.7));
                }
            }
            p.rect_stroke(fr.rect(0.0, 0.0, w.length(), if axis == 0 { w.width } else { w.height }), 0.0, outline, egui::StrokeKind::Middle);
        } else if let Some(solid) = &m.solid {
            let body = fr.rect(lo.z, v0, hi.z, v1);
            p.rect_filled(body, 0.0, material_color(mat_index(&solid.material), eps(&solid.material)));
            match &solid.shape {
                emcraft_model::Shape::Sphere { .. } => {
                    p.circle_stroke(body.center(), body.width().min(body.height()) / 2.0, outline);
                }
                _ => {
                    p.rect_stroke(body, 0.0, outline, egui::StrokeKind::Middle);
                }
            }
            for inc in &m.inclusions {
                let (a, b) = inc.bounds();
                let (va, vb) = if axis == 0 { (a.x, b.x) } else { (a.y, b.y) };
                let r = fr.rect(a.z, va, b.z, vb);
                let fill = match &inc.material {
                    Some(name) => material_color(mat_index(name), eps(name)),
                    None => t.text_dim.gamma_multiply(0.6),
                };
                if matches!(inc.shape, emcraft_model::Shape::Sphere { .. }) {
                    p.circle_filled(r.center(), r.width().min(r.height()) / 2.0, fill);
                } else {
                    p.rect_filled(r, 0.0, fill);
                }
            }
            for f in &m.features {
                let (a, b) = f.bounds();
                let (va, vb) = if axis == 0 { (a.x, b.x) } else { (a.y, b.y) };
                let r = fr.rect(a.z, va, b.z, vb);
                let s = Stroke::new(1.2, if matches!(f.combine, emcraft_model::Combine::Subtract) { t.error } else { t.success });
                p.rect_stroke(r, 0.0, s, egui::StrokeKind::Middle);
            }
            if let Some(open) = &m.open {
                let inner = fr.rect(lo.z + open.thickness, v0 + open.thickness, hi.z - open.thickness, v1 - open.thickness);
                for (a, b) in [
                    (inner.left_top(), inner.right_top()),
                    (inner.right_top(), inner.right_bottom()),
                    (inner.right_bottom(), inner.left_bottom()),
                    (inner.left_bottom(), inner.left_top()),
                ] {
                    p.extend(egui::Shape::dashed_line(&[a, b], Stroke::new(1.0, t.text_dim), 6.0, 4.0));
                }
            }
        }
        // Ports at the ends.
        for port in &m.ports {
            let z = match port.end {
                emcraft_model::End::Start => lo.z,
                _ => hi.z,
            };
            let z = port.face.map_or(z, |f| f.z);
            let c = fr.at(z, (v0 + v1) / 2.0);
            let dir = if matches!(port.end, emcraft_model::End::Start) { 1.0 } else { -1.0 };
            let tail = c - vec2(dir * 34.0, 0.0);
            p.arrow(tail, vec2(dir * 26.0, 0.0), Stroke::new(2.0, t.tab_ports[0]));
            p.text(
                tail - vec2(dir * 4.0, 0.0),
                if dir > 0.0 { Align2::RIGHT_CENTER } else { Align2::LEFT_CENTER },
                &port.name,
                FontId::proportional(t.font_label),
                t.tab_ports[1],
            );
        }
        for lp in &m.lumped {
            let (a, b) = if axis == 0 { (lp.from.x, lp.to.x) } else { (lp.from.y, lp.to.y) };
            p.line_segment([fr.at(lp.from.z, a), fr.at(lp.to.z, b)], Stroke::new(3.0, t.tab_ports[0]));
        }
        for src in &m.sources {
            if let emcraft_model::Source::Dipole { at, .. } = src {
                p.circle_filled(fr.at(at.z, if axis == 0 { at.x } else { at.y }), 4.0, t.error);
            }
        }
        // Scale: the extent along z.
        let base = fr.at(lo.z, v0);
        p.text(
            pos2(base.x, fr.rect.bottom() + 12.0),
            Align2::LEFT_TOP,
            format!("z: {:.3} – {:.3} {}", l.from_si(lo.z), l.from_si(hi.z), l.symbol()),
            FontId::proportional(t.font_small),
            t.text_dim,
        );
    }
}

// ---- S-parameters --------------------------------------------------------------------------

fn sparameters(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    if app.session.sweep.is_none() && app.session.time_domain.is_none() {
        empty(ui, "No S-parameters yet: SIMULATE › Frequency sweep.");
        return;
    }
    let width = picture_width(ui);
    let key = format!("s|{}|{width}|{}", app.revision, crate::theme::is_dark());
    if app.chart.as_ref().is_none_or(|c| c.key != key) {
        match view::sparameter_chart(&app.session) {
            Ok(chart) => {
                let height = (ui.available_height() * 0.62).clamp(260.0, 700.0) as usize;
                app.chart = Some(Picture::new(key, &emcraft_render::chart::draw(&chart, width, height)));
            }
            Err(e) => {
                empty(ui, &e.to_string());
                return;
            }
        }
    }
    let freqs = app.frequencies();
    if let Some(p) = app.chart.as_mut() {
        let r = picture(ui, p, "chart:sparameters");
        // A click picks the nearest solved frequency for the field view.
        if r.clicked()
            && let (Some(pos), Some(f0), Some(f1)) = (r.interact_pointer_pos(), freqs.first(), freqs.last())
        {
            let fx = ((pos.x - r.rect.min.x) / r.rect.width()) as f64;
            if let Some(f) = chart_frequency(fx, p.size().x as f64, *f0, *f1).and_then(|f| crate::nearest(&freqs, f)) {
                app.field.view.frequency = f;
                app.field.view.mode = None;
                app.view = View::Field;
            }
        }
    }
    ui.label(egui::RichText::new("Click the chart to open the fields at the nearest solved frequency.").size(t.font_small).color(t.text_dim));
    // The table.
    let Some(sw) = app.session.sweep.as_ref() else { return };
    let fu = app.session.display.frequency;
    egui::ScrollArea::vertical().id_salt("s_table").show(ui, |ui| {
        egui::Grid::new("s_grid").striped(true).num_columns(5).show(ui, |ui| {
            for h in [format!("f ({})", fu.symbol()), "|S11| dB".into(), "arg S11 (°)".into(), "|S21| dB".into(), "arg S21 (°)".into()] {
                ui.label(egui::RichText::new(h).strong());
            }
            ui.end_row();
            for r in &sw.results {
                ui.label(format!("{:.4}", fu.from_si(r.frequency)));
                let s11 = r.s.first().and_then(|row| row.first()).copied();
                let s21 = r.s.get(1).and_then(|row| row.first()).copied();
                for c in [s11, s21] {
                    match c {
                        Some(c) => {
                            ui.label(format!("{:.3}", emcraft_model::units::decibels(c.norm())));
                            ui.label(format!("{:.2}", c.arg().to_degrees()));
                        }
                        None => {
                            ui.label("–");
                            ui.label("–");
                        }
                    }
                }
                ui.end_row();
            }
        });
    });
}

/// Maps a click at fraction `fx` across the S-parameter chart to a frequency, with the chart's
/// layout (`emcraft_render::chart::draw`: the plot runs from x = 90 to w − 30).
pub fn chart_frequency(fx: f64, width_px: f64, f0: f64, f1: f64) -> Option<f64> {
    let (x0, pw) = (90.0, width_px - 120.0);
    let t = (fx * width_px - x0) / pw;
    (pw > 0.0 && (0.0..=1.0).contains(&t)).then_some(f0 + t * (f1 - f0))
}

// ---- fields --------------------------------------------------------------------------------

fn solved(app: &EmcraftApp) -> bool {
    app.session.sweep.is_some() || app.session.eigen.is_some() || app.session.radiation.is_some()
}

/// The span of the plane's position (display units) and its axis name.
fn plane_span(app: &EmcraftApp) -> Option<(f64, &'static str)> {
    let (lo, hi) = app.session.model.bounds()?;
    let l = app.session.display.length;
    Some(match app.field.view.plane {
        Plane::H => (l.from_si(hi.y - lo.y), "y"),
        Plane::E => (l.from_si(hi.x - lo.x), "x"),
        Plane::Cross => (l.from_si(hi.z - lo.z), "z"),
    })
}

fn field(app: &mut EmcraftApp, ui: &mut egui::Ui) {
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
        let at = plane_span(app).map_or(0.0, |(span, _)| l.to_si(app.field.at.unwrap_or(span / 2.0).clamp(0.0, span)));
        if !app.field.samples.as_ref().is_some_and(|s| s.serves(&app.field.view, at)) {
            match view::sample_plane(&app.session, &app.field.view) {
                Ok(s) => app.field.samples = Some(s),
                Err(e) => {
                    app.field.picture = None;
                    empty(ui, &e.to_string());
                    return;
                }
            }
        }
        let plot = match app.field.samples.as_ref().map(|s| s.plot(&app.field.view)) {
            Some(Ok(p)) => p,
            Some(Err(e)) => {
                empty(ui, &e.to_string());
                return;
            }
            None => return,
        };
        let c = emcraft_render::plot::draw(&plot, width);
        app.field.picture = Some(Picture::new(key, &c));
    }
    if let Some(p) = app.field.picture.as_mut() {
        picture(ui, p, "picture:field");
    }
}

fn field_controls(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    let fu = app.session.display.frequency;
    ui.horizontal_wrapped(|ui| {
        if let Some(e) = app.session.eigen.as_ref()
            && (app.field.view.mode.is_some() || app.session.sweep.is_none())
        {
            let mut idx = app.field.view.mode.unwrap_or(0);
            let label =
                |i: usize| e.modes.get(i).map_or(String::new(), |m| format!("mode {}: {:.4} {}", i + 1, fu.from_si(m.frequency), fu.symbol()));
            egui::ComboBox::from_id_salt("mode").selected_text(label(idx)).show_ui(ui, |ui| {
                for i in 0..e.modes.len() {
                    ui.selectable_value(&mut idx, i, label(i));
                }
            });
            app.field.view.mode = Some(idx);
        }
        let freqs = app.frequencies();
        if !freqs.is_empty() && app.field.view.mode.is_none() {
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
        for (p, name) in [(Plane::H, "H-plane"), (Plane::E, "E-plane"), (Plane::Cross, "Cross-section")] {
            let r = ui.selectable_label(app.field.view.plane == p, name);
            scenario::publish_target(&format!("field:plane:{}", p.name()), r.rect);
            if r.clicked() {
                app.field.view.plane = p;
                app.field.at = None;
            }
        }
        if let Some((span, axis)) = plane_span(app) {
            let mut at = app.field.at.unwrap_or(span / 2.0);
            let unit = app.session.display.length.symbol();
            if ui.add(egui::Slider::new(&mut at, 0.0..=span).text(format!("{axis} ({unit})"))).changed() {
                app.field.at = Some(at);
            }
        }
    });
    ui.horizontal_wrapped(|ui| {
        egui::ComboBox::from_id_salt("component").selected_text(app.field.view.component.clone()).show_ui(ui, |ui| {
            for c in ["ex", "ey", "ez", "hx", "hy", "hz", "|e|", "|h|"] {
                ui.selectable_value(&mut app.field.view.component, c.to_owned(), c);
            }
        });
        ui.checkbox(&mut app.field.view.show_e, "E arrows");
        ui.checkbox(&mut app.field.view.show_h, "H arrows");
        let ports: Vec<String> = app.session.model.ports.iter().map(|p| p.name.clone()).collect();
        if ports.len() > 1 && app.field.view.mode.is_none() {
            ui.label("excite");
            for (i, n) in ports.iter().enumerate() {
                ui.radio_value(&mut app.field.view.excitation, i, n.as_str());
            }
        }
        let mut deg = app.field.view.phase.to_degrees().rem_euclid(360.0);
        let r = ui.add(egui::Slider::new(&mut deg, 0.0..=360.0).text("ωt (°)"));
        scenario::publish_target("field:phase", r.rect);
        if r.changed() {
            app.field.view.phase = deg.to_radians();
        }
        let r = ui.checkbox(&mut app.field.animate, "Animate");
        scenario::publish_target("field:animate", r.rect);
    });
}

// ---- tables --------------------------------------------------------------------------------

fn port_modes(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let Some(reply) = app.port_modes.clone() else {
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.label(egui::RichText::new("No port modes yet: they need a meshed model with ports.").size(t.font_heading).color(t.text_dim));
            let r = ui.add_enabled(!app.busy(), egui::Button::new("Solve port modes…"));
            scenario::publish_target("port_modes:solve", r.rect);
            if r.clicked()
                && let Err(e) = app.open_dialog("solve.port_modes")
            {
                app.message(e, true);
            }
        });
        return;
    };
    let lu = reply.get("length_unit").and_then(Value::as_str).unwrap_or("mm").to_owned();
    let fu = app.session.display.frequency.symbol();
    ui.label(egui::RichText::new(format!("Port modes at {} {fu}", num(&reply["frequency"], 4))).size(t.font_heading));
    egui::ScrollArea::vertical().id_salt("modes_table").show(ui, |ui| {
        for port in reply.get("ports").and_then(Value::as_array).into_iter().flatten() {
            crate::ui_kit::section(ui, &format!("Port {}", port["name"].as_str().unwrap_or("")));
            egui::Grid::new(("modes", port["name"].as_str().unwrap_or(""))).striped(true).num_columns(6).show(ui, |ui| {
                for h in ["Mode", "Propagates", &format!("β (rad/{lu})"), "εeff", "Z (Ω)", &format!("Attenuation (dB/{lu})")] {
                    ui.label(egui::RichText::new(h).strong());
                }
                ui.end_row();
                for m in port.get("modes").and_then(Value::as_array).into_iter().flatten() {
                    ui.label(m["name"].as_str().unwrap_or("?"));
                    ui.label(if m["propagating"] == json!(true) { "yes" } else { "no" });
                    ui.label(num(&m["beta"], 5));
                    ui.label(num(&m["eps_eff"], 5));
                    ui.label(num(&m["impedance"], 3));
                    ui.label(num(&m["attenuation_db"], 3));
                    ui.end_row();
                }
            });
        }
    });
    scenario::publish_rect("table:port_modes", ui.min_rect());
}

fn num(v: &Value, digits: usize) -> String {
    v.as_f64().map_or("–".into(), |x| format!("{x:.digits$}"))
}

fn eigenmodes(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    if app.session.eigen.is_none() {
        empty(ui, "No eigenmodes yet: SIMULATE › Eigenmodes.");
        return;
    }
    if app.eigen_table.as_ref().is_none_or(|(rev, _)| *rev != app.revision) {
        match app.session.execute("results.eigenmodes", &json!({})) {
            Ok(v) => app.eigen_table = Some((app.revision, v)),
            Err(e) => {
                empty(ui, &e.to_string());
                return;
            }
        }
    }
    let Some((_, table)) = app.eigen_table.clone() else { return };
    let fu = app.session.display.frequency.symbol();
    let mut open = None;
    egui::Grid::new("eigen_grid").striped(true).num_columns(4).show(ui, |ui| {
        for h in ["Mode", &format!("f ({fu})"), "Q", ""] {
            ui.label(egui::RichText::new(h).strong());
        }
        ui.end_row();
        for (i, m) in table.get("modes").and_then(Value::as_array).into_iter().flatten().enumerate() {
            ui.label(format!("{}", i + 1));
            ui.label(num(&m["frequency"], 5));
            ui.label(num(&m["q"], 0));
            let r = ui.button("Field");
            scenario::publish_target(&format!("eigen:field:{}", i + 1), r.rect);
            if r.clicked() {
                open = Some(i);
            }
            ui.end_row();
        }
    });
    if let Some(exact) = table.get("exact").and_then(Value::as_array) {
        crate::ui_kit::section(ui, "Closed form");
        let list: Vec<String> = exact.iter().take(8).map(|e| format!("{} {}", e["mode"].as_str().unwrap_or(""), num(&e["frequency"], 4))).collect();
        ui.label(egui::RichText::new(list.join(" · ")).size(t.font_body).color(t.text_dim));
    }
    if let Some(i) = open {
        app.field.view.mode = Some(i);
        app.view = View::Field;
    }
}

fn compare(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let Some(reply) = app.compare.clone() else {
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.label(egui::RichText::new("Compare the sweep with the exact TE10 solution or mode matching.").size(t.font_heading).color(t.text_dim));
            let r = ui.add_enabled(!app.busy() && app.session.sweep.is_some(), egui::Button::new("Compare…"));
            scenario::publish_target("compare:run", r.rect);
            if r.clicked() {
                let _ = app.run("results.compare", json!({}));
            }
        });
        return;
    };
    ui.label(
        egui::RichText::new(format!("Against {} — worst |S − S_ref| = {}", reply["reference"].as_str().unwrap_or("?"), num(&reply["worst"], 6)))
            .size(t.font_heading),
    );
    egui::ScrollArea::vertical().id_salt("compare_table").show(ui, |ui| {
        egui::Grid::new("compare_grid").striped(true).num_columns(5).show(ui, |ui| {
            for h in ["f", "|S11|", "|S11| ref", "|S21|", "|S21| ref", "error S11", "error S21"] {
                ui.label(egui::RichText::new(h).strong());
            }
            ui.end_row();
            for r in reply.get("points").and_then(Value::as_array).into_iter().flatten() {
                ui.label(num(&r["frequency"], 4));
                ui.label(num(&r["s11"]["mag"], 5));
                ui.label(num(&r["exact_s11"]["mag"], 5));
                ui.label(num(&r["s21"]["mag"], 5));
                ui.label(num(&r["exact_s21"]["mag"], 5));
                ui.label(format!("{:.2e}", r["error_s11"].as_f64().unwrap_or(f64::NAN)));
                ui.label(format!("{:.2e}", r["error_s21"].as_f64().unwrap_or(f64::NAN)));
                ui.end_row();
            }
        });
    });
}

// ---- far field -----------------------------------------------------------------------------

fn far_field(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    if app.session.radiation.is_none() && app.session.td_radiation.is_none() {
        empty(ui, "No far field yet: SIMULATE › Radiation (an open model with sources or lumped ports).");
        return;
    }
    let mut phi = app.pattern_phi;
    ui.horizontal(|ui| {
        let r = ui.add(egui::Slider::new(&mut phi, 0.0..=180.0).text("azimuth φ (°)"));
        scenario::publish_target("pattern:phi", r.rect);
    });
    if (phi - app.pattern_phi).abs() > 1e-9 {
        app.pattern_phi = phi;
    }
    let width = picture_width(ui);
    let key = format!("p|{}|{width}|{:.3}", app.revision, app.pattern_phi);
    if app.chart.as_ref().is_none_or(|c| c.key != key) {
        match emcraft_engine::open::pattern_chart(&app.session, app.pattern_phi, 91) {
            Ok(chart) => {
                let height = (ui.available_height() * 0.9).clamp(260.0, 700.0) as usize;
                app.chart = Some(Picture::new(key, &emcraft_render::chart::draw(&chart, width, height)));
            }
            Err(e) => {
                empty(ui, &e.to_string());
                return;
            }
        }
    }
    if let Some(p) = app.chart.as_mut() {
        picture(ui, p, "chart:pattern");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chart_clicks_map_to_frequencies() {
        assert_eq!(chart_frequency(90.0 / 1000.0, 1000.0, 5.0, 9.0), Some(5.0));
        assert!((chart_frequency(530.0 / 1000.0, 1000.0, 5.0, 9.0).unwrap_or(0.0) - 7.0).abs() < 1e-12);
        assert_eq!(chart_frequency(0.01, 1000.0, 5.0, 9.0), None);
        assert_eq!(crate::nearest(&[5.0, 6.0, 7.0], 6.4), Some(6.0));
        assert_eq!(crate::nearest(&[], 6.4), None);
    }
}
