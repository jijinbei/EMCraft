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
                View::Model => model3d(app, &mut inner),
                View::Outline => model(app, &mut inner),
                View::SParameters => sparameters(app, &mut inner),
                View::Field => field(app, &mut inner),
                View::PortModes => port_modes(app, &mut inner),
                View::FarField => far_field(app, &mut inner),
                View::Compare => compare(app, &mut inner),
                View::Eigenmodes => eigenmodes(app, &mut inner),
            }
        });
}

/// Whether view `v` has something to show: the model always, results once solved.
pub(crate) fn available(app: &EmcraftApp, v: View) -> bool {
    let r = &app.session.results;
    match v {
        View::Model | View::Outline => true,
        View::SParameters => r.sweep.is_some() || r.time_domain.is_some(),
        View::Field => r.sweep.is_some() || r.eigen.is_some() || r.radiation.is_some(),
        View::PortModes => app.port_modes.is_some() || (app.session.model.mesh.is_some() && !app.session.model.ports.is_empty()),
        View::FarField => r.radiation.is_some() || r.td_radiation.is_some(),
        View::Compare => r.sweep.is_some(),
        View::Eigenmodes => r.eigen.is_some(),
    }
}

/// The views with something to show, as a row of pills.
fn view_tabs(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    ui.horizontal(|ui| {
        let shown: Vec<View> = View::ALL.into_iter().filter(|v| *v == app.view || available(app, *v)).collect();
        for v in shown {
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

/// A picture as large as fits with `below` pixels kept free under it.
fn picture_above(ui: &mut egui::Ui, p: &mut Picture, handle: &str, below: f32) -> egui::Response {
    let size = p.size();
    let avail = ui.available_size() - vec2(0.0, below);
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

/// The model in 3D, in the active step's overlay.
fn model3d(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let mut overlay = app.workflow.step.map_or(crate::scene3d::Overlay::Plain, crate::steps::overlay);
    // In Results, the field's plane in the model.
    if app.workflow.step == Some(crate::steps::Step::Results) && solved(app) {
        overlay = crate::scene3d::Overlay::Field;
    }
    crate::view3d::show(app, ui, area, overlay);
}

fn model(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let m = &app.session.model;
    let Some(emcraft_model::Aabb { min: lo, max: hi }) = m.bounds() else {
        empty(ui, "No geometry yet: MODEL › Waveguide or Solid, or open a sample.");
        return;
    };
    let l = app.session.display.length;
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let p = ui.painter().with_clip_rect(area);
    let half = (area.height() - 40.0) / 2.0;
    // One scale for both views (the smaller of what each band allows), so z lines up.
    let room = vec2(area.width() - 120.0, half - 52.0);
    let scale = [Across::X, Across::Y]
        .map(|a| {
            let (v0, v1) = a.span(lo, hi);
            (room.x as f64 / (hi.z - lo.z).max(1e-9)).min(room.y as f64 / (v1 - v0).max(1e-9))
        })
        .into_iter()
        .fold(f64::INFINITY, f64::min);
    for (k, across) in [Across::X, Across::Y].into_iter().enumerate() {
        let band = Rect::from_min_size(pos2(area.left(), area.top() + k as f32 * (half + 20.0)), vec2(area.width(), half + 20.0));
        p.text(band.left_top() + vec2(4.0, 2.0), Align2::LEFT_TOP, across.title(), FontId::proportional(t.font_label), t.text_dim);
        let (v0, v1) = across.span(lo, hi);
        let fr = Frame2::with_scale(band.shrink2(vec2(60.0, 26.0)).translate(vec2(0.0, 8.0)), (lo.z, hi.z), (v0, v1), scale);
        let sec = Section { p: &p, fr, across, t: &t, m };
        if let Some(w) = &m.waveguide {
            sec.waveguide(w);
        } else if let Some(solid) = &m.solid {
            sec.solid(solid, lo, hi);
        }
        sec.drivers(lo, hi);
        // Scale: the extent along z.
        let base = sec.fr.at(lo.z, v0);
        p.text(
            pos2(base.x, sec.fr.rect.bottom() + 12.0),
            Align2::LEFT_TOP,
            format!("z: {:.3} – {:.3} {}", l.from_si(lo.z), l.from_si(hi.z), l.symbol()),
            FontId::proportional(t.font_small),
            t.text_dim,
        );
    }
}

/// The transverse axis a 2D outline view shows across z.
#[derive(Clone, Copy, PartialEq)]
enum Across {
    X,
    Y,
}

impl Across {
    fn title(self) -> &'static str {
        match self {
            Across::X => "H-plane (x across z)",
            Across::Y => "E-plane (y across z)",
        }
    }

    fn of(self, p: emcraft_model::Point3) -> f64 {
        match self {
            Across::X => p.x,
            Across::Y => p.y,
        }
    }

    fn span(self, lo: emcraft_model::Point3, hi: emcraft_model::Point3) -> (f64, f64) {
        (self.of(lo), self.of(hi))
    }
}

/// One outline view: the model drawn across z in `fr`.
struct Section<'a> {
    p: &'a egui::Painter,
    fr: Frame2,
    across: Across,
    t: &'a Tokens,
    m: &'a emcraft_model::Model,
}

impl Section<'_> {
    fn fill_of(&self, material: &str) -> Color32 {
        let index = self.m.materials.iter().position(|x| x.name == material).unwrap_or(0);
        material_color(index, self.m.material(material).map_or(1.0, |(_, x)| x.eps_r))
    }

    fn outline(&self) -> Stroke {
        Stroke::new(1.5, self.t.text)
    }

    /// The box `a`–`b` seen across z.
    fn rect_of(&self, a: emcraft_model::Point3, b: emcraft_model::Point3) -> Rect {
        self.fr.rect(a.z, self.across.of(a), b.z, self.across.of(b))
    }

    fn waveguide(&self, w: &emcraft_model::Waveguide) {
        let (p, fr, t) = (self.p, &self.fr, self.t);
        let full = match self.across {
            Across::X => w.width,
            Across::Y => w.height,
        };
        let metal = t.text_dim.gamma_multiply(0.55);
        let mut z = 0.0;
        for (i, s) in w.sections.iter().enumerate() {
            let emcraft_model::Span { start: a0, width: a } = match self.across {
                Across::X => w.aperture(i).unwrap_or(w.across_x()),
                Across::Y => w.aperture_y(i).unwrap_or(w.across_y()),
            };
            // Metal outside the aperture.
            if a0 > 0.0 {
                p.rect_filled(fr.rect(z, 0.0, z + s.length, a0), 0.0, metal);
            }
            if a0 + a < full - 1e-12 {
                p.rect_filled(fr.rect(z, a0 + a, z + s.length, full), 0.0, metal);
            }
            let open = fr.rect(z, a0, z + s.length, a0 + a);
            p.rect_filled(open, 0.0, self.fill_of(&s.material));
            p.vline(fr.at(z, 0.0).x, fr.at(z, full).y..=fr.at(z, 0.0).y, Stroke::new(0.5, t.border));
            p.text(open.center(), Align2::CENTER_CENTER, &s.material, FontId::proportional(t.font_small), t.text);
            z += s.length;
        }
        match self.across {
            Across::Y => {
                let mut y = 0.0;
                for layer in &w.layers {
                    p.rect_filled(fr.rect(0.0, y, w.length(), y + layer.thickness), 0.0, self.fill_of(&layer.material));
                    y += layer.thickness;
                }
                for st in &w.strips {
                    p.line_segment([fr.at(st.z0, st.y), fr.at(st.z1, st.y)], Stroke::new(3.0, t.warning));
                }
            }
            Across::X => {
                for st in &w.strips {
                    p.rect_filled(fr.rect(st.z0, st.x0, st.z1, st.x0 + st.width), 0.0, t.warning.gamma_multiply(0.7));
                }
            }
        }
        p.rect_stroke(fr.rect(0.0, 0.0, w.length(), full), 0.0, self.outline(), egui::StrokeKind::Middle);
    }

    fn solid(&self, solid: &emcraft_model::Solid, lo: emcraft_model::Point3, hi: emcraft_model::Point3) {
        let (p, t) = (self.p, self.t);
        let body = self.rect_of(lo, hi);
        p.rect_filled(body, 0.0, self.fill_of(&solid.material));
        if matches!(solid.shape, emcraft_model::Shape::Sphere { .. }) {
            p.circle_stroke(body.center(), body.width().min(body.height()) / 2.0, self.outline());
        } else {
            p.rect_stroke(body, 0.0, self.outline(), egui::StrokeKind::Middle);
        }
        for inc in &self.m.inclusions {
            let emcraft_model::Aabb { min: a, max: b } = inc.bounds();
            let r = self.rect_of(a, b);
            let fill = inc.material.as_deref().map_or(t.text_dim.gamma_multiply(0.6), |name| self.fill_of(name));
            if matches!(inc.shape, emcraft_model::Shape::Sphere { .. }) {
                p.circle_filled(r.center(), r.width().min(r.height()) / 2.0, fill);
            } else {
                p.rect_filled(r, 0.0, fill);
            }
        }
        for f in &self.m.features {
            let emcraft_model::Aabb { min: a, max: b } = f.bounds();
            let ink = if matches!(f.combine, emcraft_model::Combine::Subtract) { t.error } else { t.success };
            p.rect_stroke(self.rect_of(a, b), 0.0, Stroke::new(1.2, ink), egui::StrokeKind::Middle);
        }
        if let Some(open) = &self.m.open {
            let (v0, v1) = self.across.span(lo, hi);
            let d = open.thickness;
            let inner = self.fr.rect(lo.z + d, v0 + d, hi.z - d, v1 - d);
            let corners = [inner.left_top(), inner.right_top(), inner.right_bottom(), inner.left_bottom(), inner.left_top()];
            for pair in corners.windows(2) {
                p.extend(egui::Shape::dashed_line(pair, Stroke::new(1.0, t.text_dim), 6.0, 4.0));
            }
        }
    }

    /// Ports, lumped ports and dipoles.
    fn drivers(&self, lo: emcraft_model::Point3, hi: emcraft_model::Point3) {
        let (p, fr, t) = (self.p, &self.fr, self.t);
        let (v0, v1) = self.across.span(lo, hi);
        for port in &self.m.ports {
            let start = matches!(port.end, emcraft_model::End::Start);
            let z = port.face.map_or(if start { lo.z } else { hi.z }, |f| f.z);
            let dir = if start { 1.0 } else { -1.0 };
            let tail = fr.at(z, (v0 + v1) / 2.0) - vec2(dir * 34.0, 0.0);
            p.arrow(tail, vec2(dir * 26.0, 0.0), Stroke::new(2.0, t.tab_ports[0]));
            let align = if start { Align2::RIGHT_CENTER } else { Align2::LEFT_CENTER };
            p.text(tail - vec2(dir * 4.0, 0.0), align, &port.name, FontId::proportional(t.font_label), t.tab_ports[1]);
        }
        for lp in &self.m.lumped {
            p.line_segment([fr.at(lp.from.z, self.across.of(lp.from)), fr.at(lp.to.z, self.across.of(lp.to))], Stroke::new(3.0, t.tab_ports[0]));
        }
        for src in &self.m.sources {
            if let emcraft_model::Source::Dipole { at, .. } = src {
                p.circle_filled(fr.at(at.z, self.across.of(*at)), 4.0, t.error);
            }
        }
    }
}

// ---- S-parameters --------------------------------------------------------------------------

fn sparameters(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    if app.session.results.sweep.is_none() && app.session.results.time_domain.is_none() {
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
    let Some(sw) = app.session.results.sweep.as_ref() else { return };
    let fu = app.session.display.frequency;
    egui::ScrollArea::vertical().id_salt("s_table").show(ui, |ui| {
        egui::Grid::new("s_grid").striped(true).num_columns(5).show(ui, |ui| {
            for h in [format!("f ({})", fu.symbol()), "|S11| dB".into(), "arg S11 (°)".into(), "|S21| dB".into(), "arg S21 (°)".into()] {
                ui.label(egui::RichText::new(h).strong());
            }
            ui.end_row();
            for r in &sw.results {
                ui.label(format!("{:.4}", fu.from_si(r.frequency)));
                let s11 = r.s.get(0, 0);
                let s21 = r.s.get(1, 0);
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
    app.session.results.sweep.is_some() || app.session.results.eigen.is_some() || app.session.results.radiation.is_some()
}

/// The span of the plane's position (display units) and its axis name.
fn plane_span(app: &EmcraftApp) -> Option<(f64, &'static str)> {
    plane_span_of(app, app.field.view.plane)
}

fn plane_span_of(app: &EmcraftApp, plane: Plane) -> Option<(f64, &'static str)> {
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
const MAX_KEPT: usize = 8;

/// Room under the field picture for the probe's readout.
const READOUT_HEIGHT: f32 = 58.0;

/// The pointer over the field picture: the fields under it, a click pins a point (a right click
/// lets it go); both are marked on the picture.
fn probe_picture(app: &mut EmcraftApp, ui: &egui::Ui, r: &egui::Response) {
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
fn phasor(c: emcraft_model::Complex64) -> String {
    use emcraft_render::axes::format;
    if c.norm() == 0.0 { "0".into() } else { format!("{} ({}°)", format(c.norm()), format(c.arg().to_degrees().round())) }
}

/// The readout under the field picture: the fields at the pointer and at the pinned point —
/// the coloured component at this phase and its amplitude, and every component of E and H as
/// amplitude (phase).
fn readout(app: &mut EmcraftApp, ui: &mut egui::Ui) {
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

fn field_controls(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    let fu = app.session.display.frequency;
    ui.horizontal_wrapped(|ui| {
        if let Some(e) = app.session.results.eigen.as_ref()
            && (app.field.view.mode.is_some() || app.session.results.sweep.is_none())
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
        ui.separator();
        for (p, name) in [(Plane::H, "H-plane"), (Plane::E, "E-plane"), (Plane::Cross, "Cross-section")] {
            let r = ui.selectable_label(app.field.view.plane == p, name);
            scenario::publish_target(&format!("field:plane:{}", p.name()), r.rect);
            if r.clicked() && app.field.view.plane != p {
                app.field.view.plane = p;
                app.field.at = None;
                app.field.pinned = None;
            }
        }
        if let Some((span, axis)) = plane_span(app) {
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
    });
    // The planes kept for the 3D view, each a chip that a click lets go.
    if !app.field.kept.is_empty() {
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
    ui.horizontal_wrapped(|ui| {
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
        let ports: Vec<String> = app.session.model.ports.iter().map(|p| p.name.clone()).collect();
        if ports.len() > 1 && app.field.view.mode.is_none() {
            ui.separator();
            ui.label("excite");
            for (i, n) in ports.iter().enumerate() {
                ui.radio_value(&mut app.field.view.excitation, i, n.as_str());
            }
        }
        ui.separator();
        let mut deg = app.field.view.phase.to_degrees().rem_euclid(360.0);
        let r = crate::ui_kit::slider(ui, &mut deg, 0.0..=360.0, "ωt (°)");
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
    if app.session.results.eigen.is_none() {
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
            let r = ui.add_enabled(!app.busy() && app.session.results.sweep.is_some(), egui::Button::new("Compare…"));
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
    if app.session.results.radiation.is_none() && app.session.results.td_radiation.is_none() {
        empty(ui, "No far field yet: SIMULATE › Radiation (an open model with sources or lumped ports).");
        return;
    }
    let mut phi = app.pattern_phi;
    ui.horizontal(|ui| {
        let r = crate::ui_kit::slider(ui, &mut phi, 0.0..=180.0, "azimuth φ (°)");
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
