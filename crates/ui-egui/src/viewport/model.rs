//! The model's outline: H-plane and E-plane views across z of the guide or the solid.

use super::*;

pub(super) fn material_color(i: usize, eps: f64) -> Color32 {
    if eps <= 1.0 + 1e-12 {
        return Color32::from_rgba_unmultiplied(160, 190, 230, 50);
    }
    const PALETTE: [(u8, u8, u8); 6] = [(214, 150, 60), (90, 170, 110), (170, 110, 200), (60, 150, 190), (200, 100, 110), (150, 150, 70)];
    let (r, g, b) = PALETTE[i % PALETTE.len()];
    Color32::from_rgba_unmultiplied(r, g, b, 120)
}

/// A drawing frame: model coordinates (horizontal, vertical in metres) onto a screen rectangle,
/// one scale for both.
pub(super) struct Frame2 {
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
pub(super) fn model3d(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let mut overlay = app.workflow.step.map_or(crate::scene3d::Overlay::Plain, crate::steps::overlay);
    // In Results, the field's plane in the model.
    if app.workflow.step == Some(crate::steps::Step::Results) && View::Field.available(app) {
        overlay = crate::scene3d::Overlay::Field;
    }
    crate::view3d::show(app, ui, area, overlay);
}

pub(super) fn model(app: &mut EmcraftApp, ui: &mut egui::Ui) {
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
pub(super) enum Across {
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
pub(super) struct Section<'a> {
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
