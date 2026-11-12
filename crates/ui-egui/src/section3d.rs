//! What the 3D view's section view shows besides the cut itself (what the 2D outline view used
//! to draw): dimensions on the cut — the cut outline's overall extents, a guide's section and
//! iris apertures — drawn as labels with extension and dimension lines, ports marked where
//! they meet the cut with their name and size, the presets for a guide's H-plane (y = b/2) and
//! E-plane (x = a/2) cuts, and the camera turned straight at the cut, orthographic.
//!
//! Everything here is placed in the model (m) on the cut plane and projected with the view's
//! camera each frame, so it follows orbiting, panning and zooming.

use egui::{FontId, Pos2, Rect, Stroke, vec2};

use crate::handle::Handle;
use crate::theme::Tokens;
use crate::{EmcraftApp, scenario};

pub use emcraft_ui_core::scene3d::section::{Dim, PortMark, Preset, flat, look, notes, outline};

/// A dimension's label sits this far (points) beyond its arrow's tip, its half size and a
/// margin more.
const LABEL_OFFSET: f32 = 10.0;
const LABEL_MARGIN: f32 = 4.0;

/// What a section bar's button shows.
#[derive(Clone, Copy)]
pub enum Mark {
    Text(&'static str),
    Glyph(crate::glyphs::Glyph),
}

/// A button of the section bar in `r`, lit when `on`, `tip` on hover; whether it was clicked.
pub fn button(ui: &mut egui::Ui, r: Rect, handle: crate::handle::Fixed, mark: Mark, tip: crate::lang::Text, on: bool) -> bool {
    use crate::UiExt;
    let t = &Tokens::get();
    let p = &ui.painter_at(r);
    let tip = crate::lang::tr(tip);
    let resp = ui.hit(r, handle.into(), egui::Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tip));
    if on || resp.hovered() {
        p.rect_filled(r, 5.0, if on { t.accent_soft } else { t.hover });
    }
    match mark {
        Mark::Text(s) => {
            p.text(r.center(), egui::Align2::CENTER_CENTER, s, FontId::proportional(t.font_label), if on { t.accent } else { t.text });
        }
        Mark::Glyph(g) => crate::glyphs::paint(p, r.shrink(3.0), g, t.text, t.accent),
    }
    resp.on_hover_text(tip).clicked()
}

/// Draws the cut's dimensions and port marks over the view `v` in `rect`, when it shows them;
/// each label is published as `view3d:section:dim:<i>` and `view3d:section:port:<i>`.
pub fn annotate(app: &EmcraftApp, v: &crate::view3d::View3d, p: &egui::Painter, rect: Rect, bounds: ([f64; 3], [f64; 3])) {
    let (Some(cut), true, Some(b)) = (v.section, v.section_dims, v.shown()) else { return };
    let (scene, cam) = (&b.scene, &v.camera);
    let t = &Tokens::get();
    let l = app.session.display().length;
    let (dims, ports) = notes(app.session.model(), l, app.session.driven_port(), cut, outline(scene, cut, bounds));
    let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
    let screen = |q: [f64; 3]| cam.project(q, w, h).map(|s| rect.min + vec2(s[0] as f32, s[1] as f32));
    let font = FontId::proportional(t.font_small);
    let ink = t.text;
    let thin = Stroke::new(1.0, ink.gamma_multiply(0.7));
    for (i, d) in dims.iter().enumerate() {
        let shift = |q: [f64; 3]| [q[0] + d.off[0], q[1] + d.off[1], q[2] + d.off[2]];
        let (Some(a), Some(b), Some(a2), Some(b2)) = (screen(d.a), screen(d.b), screen(shift(d.a)), screen(shift(d.b))) else { continue };
        if (b2 - a2).length() < 12.0 {
            continue;
        }
        if d.off != [0.0; 3] {
            p.line_segment([a, a2], thin);
            p.line_segment([b, b2], thin);
        }
        p.line_segment([a2, b2], Stroke::new(1.2, ink));
        arrowhead(p, a2, b2, ink);
        arrowhead(p, b2, a2, ink);
        let r = label(p, a2 + (b2 - a2) / 2.0, &d.text, ink, t.border);
        scenario::publish_target(Handle::SectionNote("dim", i), r);
    }
    let sc = crate::theme::scene();
    for (i, m) in ports.iter().enumerate() {
        // The role's colour, as the browser's pills and the 3D view's port faces have it.
        let port = crate::theme::color32(if m.input { sc.port_input } else { sc.port_output });
        let (Some(a), Some(b)) = (screen(m.a), screen(m.b)) else { continue };
        p.line_segment([a, b], Stroke::new(4.0, port));
        p.circle_filled(a, 3.5, port);
        p.circle_filled(b, 3.5, port);
        // The label past the segment's upper (else right) end, on its line: clear of the
        // dimensions, which run beside the cut.
        let (first, last) = if b.y < a.y - 0.5 || (b.y - a.y).abs() <= 0.5 && b.x > a.x { (a, b) } else { (b, a) };
        let dir = (last - first).normalized();
        let dir = if dir.is_finite() { dir } else { vec2(0.0, -1.0) };
        let g = p.layout_no_wrap(m.text.clone(), font.clone(), t.text);
        let reach = LABEL_OFFSET + 0.5 * (g.size().x * dir.x.abs() + g.size().y * dir.y.abs()) + LABEL_MARGIN;
        let at = last + dir * reach;
        p.line_segment([last, last + dir * 8.0], Stroke::new(1.0, port));
        let r = label(p, at, &m.text, t.text, port);
        scenario::publish_target(Handle::SectionNote("port", i), r);
    }
}

/// A filled arrowhead at `tip`, pointing away from `from`.
fn arrowhead(p: &egui::Painter, tip: Pos2, from: Pos2, ink: egui::Color32) {
    let d = (tip - from).normalized();
    if !d.is_finite() {
        return;
    }
    let n = vec2(-d.y, d.x);
    let back = tip - d * 7.0;
    p.add(egui::Shape::convex_polygon(vec![tip, back + n * 3.0, back - n * 3.0], ink, Stroke::NONE));
}

/// `text` in `ink` on a small card edged in `edge` centred at `at`; its rectangle.
fn label(p: &egui::Painter, at: Pos2, text: &str, ink: egui::Color32, edge: egui::Color32) -> Rect {
    let t = Tokens::get();
    let fill = t.panel;
    let g = p.layout_no_wrap(text.to_owned(), FontId::proportional(t.font_small), ink);
    let r = Rect::from_center_size(at, g.size() + vec2(8.0, 4.0));
    p.rect_filled(r, 3.0, fill.gamma_multiply(0.92));
    p.rect_stroke(r, 3.0, Stroke::new(1.0, edge), egui::StrokeKind::Inside);
    p.galley(r.min + vec2(4.0, 2.0), g, ink);
    r
}
