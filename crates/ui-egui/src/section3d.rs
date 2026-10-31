//! What the 3D view's section view shows besides the cut itself (what the 2D outline view used
//! to draw): dimensions on the cut — the cut outline's overall extents, a guide's section and
//! iris apertures — drawn as labels with extension and dimension lines, ports marked where
//! they meet the cut with their name and size, the presets for a guide's H-plane (y = b/2) and
//! E-plane (x = a/2) cuts, and the camera turned straight at the cut, orthographic.
//!
//! Everything here is placed in the model (m) on the cut plane and projected with the view's
//! camera each frame, so it follows orbiting, panning and zooming.

use egui::{FontId, Pos2, Rect, Stroke, vec2};
use emcraft_render::view3d::{Camera, Keep, Scene, StandardView};

use crate::handle::Handle;
use crate::theme::Tokens;
use crate::view3d::Cut;
use crate::{EmcraftApp, scenario};

/// Most dimensions drawn on one cut (a long iris filter's apertures beyond it are left out).
const MAX_DIMS: usize = 24;

/// A preset cut of a guide: its H-plane (across y, through the middle of the height) or its
/// E-plane (across x, through the middle of the width).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    HPlane,
    EPlane,
}

impl Preset {
    /// The preset's cut through the middle of `bounds` (m), the low side kept.
    pub fn cut(self, bounds: ([f64; 3], [f64; 3])) -> Cut {
        Cut::middle(if self == Preset::HPlane { 1 } else { 0 }, bounds)
    }

    /// Whether `cut` is this preset's.
    pub fn is(self, cut: Cut, bounds: ([f64; 3], [f64; 3])) -> bool {
        let c = self.cut(bounds);
        cut.axis == c.axis && (cut.at - c.at).abs() <= 1e-9 * (bounds.1[c.axis] - bounds.0[c.axis]).abs().max(1e-12)
    }
}

/// The two axes in the plane of a cut across `k`, in the order x, y, z.
fn across(k: usize) -> (usize, usize) {
    match k {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    }
}

/// The box (m) the cut faces fill on the plane: the scene's triangles lying on it (the caps),
/// else the model's box flattened onto the plane.
pub fn outline(scene: &Scene, cut: Cut, bounds: ([f64; 3], [f64; 3])) -> ([f64; 3], [f64; 3]) {
    let k = cut.axis.min(2);
    let size = (0..3).map(|d| (bounds.1[d] - bounds.0[d]).abs()).fold(0.0, f64::max).max(1e-12);
    let eps = 1e-7 * size;
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for t in scene.tris.iter().filter(|t| t.p.iter().all(|q| (q[k] - cut.at).abs() <= eps)) {
        for q in &t.p {
            for d in 0..3 {
                lo[d] = lo[d].min(q[d]);
                hi[d] = hi[d].max(q[d]);
            }
        }
    }
    if !(lo[0] <= hi[0]) {
        (lo, hi) = bounds;
    }
    lo[k] = cut.at;
    hi[k] = cut.at;
    (lo, hi)
}

/// The model's box flattened onto the cut's plane.
pub fn flat(cut: Cut, (mut lo, mut hi): ([f64; 3], [f64; 3])) -> ([f64; 3], [f64; 3]) {
    let k = cut.axis.min(2);
    (lo[k], hi[k]) = (cut.at, cut.at);
    (lo, hi)
}

/// What a section bar's button shows.
#[derive(Clone, Copy)]
pub enum Mark {
    Text(&'static str),
    Glyph(crate::glyphs::Glyph),
}

/// A button of the section bar in `r`, lit when `on`, `tip` on hover; whether it was clicked.
pub fn button(ui: &mut egui::Ui, p: &egui::Painter, r: Rect, handle: crate::handle::Fixed, mark: Mark, tip: &str, on: bool) -> bool {
    use crate::UiExt;
    let t = &Tokens::get();
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

/// The camera's goal for looking straight at the cut: its box framed from the side that was
/// cut away, so the kept side lies behind the cut faces.
pub fn look(cut: Cut, outline: ([f64; 3], [f64; 3])) -> Option<crate::view3d::Goal> {
    let sign = if cut.keep == Keep::Low { 1.0 } else { -1.0 };
    let side: StandardView = crate::view3d::view_from(cut.axis.min(2), sign)?;
    // Framed with room round the cut for its dimensions and the ports' labels, and below it for
    // the view's bars.
    let k = cut.axis.min(2);
    let (mut lo, mut hi) = outline;
    let pad = 0.25 * (0..3).filter(|d| *d != k).map(|d| hi[d] - lo[d]).fold(0.0, f64::max);
    for d in (0..3).filter(|d| *d != k) {
        lo[d] -= pad;
        hi[d] += pad;
    }
    Some(crate::view3d::Goal::Face(lo, hi, side))
}

/// A dimension: from `a` to `b` (m), its line moved by `off` (m) off them, labelled `text`.
#[derive(Clone, Debug, PartialEq)]
pub struct Dim {
    pub a: [f64; 3],
    pub b: [f64; 3],
    pub off: [f64; 3],
    pub text: String,
}

/// A port where it meets the cut: the segment (m), its label and its role in the S-parameters
/// (the driven port is the input, as the browser's pills say).
#[derive(Clone, Debug, PartialEq)]
pub struct PortMark {
    pub a: [f64; 3],
    pub b: [f64; 3],
    pub text: String,
    pub input: bool,
}

/// What the cut is annotated with: its dimensions and the ports it meets.
/// `driven` is the input port's index (`Session::driven_port`).
pub fn notes(
    m: &emcraft_model::Model,
    l: emcraft_model::units::Length,
    driven: Option<usize>,
    cut: Cut,
    outline: ([f64; 3], [f64; 3]),
) -> (Vec<Dim>, Vec<PortMark>) {
    let k = cut.axis.min(2);
    let c = cut.at;
    let (u, v) = across(k);
    let (lo, hi) = outline;
    let gap = 0.08 * (hi[u] - lo[u]).abs().max((hi[v] - lo[v]).abs()).max(1e-12);
    let len = |x: f64| format!("{} {}", crate::steps::fmt(l.from_si(x)), l.symbol());
    let at = |pu: f64, pv: f64| {
        let mut p = [0.0; 3];
        (p[k], p[u], p[v]) = (c, pu, pv);
        p
    };
    let along = |d: usize, s: f64| {
        let mut p = [0.0; 3];
        p[d] = s;
        p
    };
    let mut dims = Vec::new();
    // The outline's overall extents: across u under it, across v beside it.
    if hi[u] - lo[u] > 0.0 {
        dims.push(Dim { a: at(lo[u], lo[v]), b: at(hi[u], lo[v]), off: along(v, -gap), text: len(hi[u] - lo[u]) });
    }
    if hi[v] - lo[v] > 0.0 {
        dims.push(Dim { a: at(lo[u], lo[v]), b: at(lo[u], hi[v]), off: along(u, -gap), text: len(hi[v] - lo[v]) });
    }
    // A guide's apertures where they narrow (an iris, a step), in the sections the cut crosses.
    if let Some(w) = &m.waveguide {
        let mut z = 0.0;
        for (i, s) in w.sections.iter().enumerate() {
            let (z0, z1) = (z, z + s.length.get());
            z = z1;
            let (Some(ax), Some(ay)) = (w.aperture(i), w.aperture_y(i)) else { continue };
            let inside = |span: emcraft_model::Span, x: f64| x >= span.start - 1e-12 && x <= span.start + span.width + 1e-12;
            let narrow = |span: emcraft_model::Span, full: emcraft_model::Span| (span.width - full.width).abs() > 1e-9 * full.width.max(1e-12);
            let zm = 0.5 * (z0 + z1);
            match k {
                // H-plane: the x aperture of each section the cut's y passes through.
                1 if inside(ay, c) && narrow(ax, w.across_x()) => {
                    dims.push(Dim { a: [ax.start, c, zm], b: [ax.start + ax.width, c, zm], off: [0.0; 3], text: len(ax.width) });
                }
                // E-plane: the y aperture.
                0 if inside(ax, c) && narrow(ay, w.across_y()) => {
                    dims.push(Dim { a: [c, ay.start, zm], b: [c, ay.start + ay.width, zm], off: [0.0; 3], text: len(ay.width) });
                }
                // Across the guide: the opening of the section cut.
                2 if c >= z0 && c <= z1 && (narrow(ax, w.across_x()) || narrow(ay, w.across_y())) => {
                    let (ym, xm) = (ay.start + 0.5 * ay.width, ax.start + 0.5 * ax.width);
                    dims.push(Dim { a: [ax.start, ym, c], b: [ax.start + ax.width, ym, c], off: [0.0; 3], text: len(ax.width) });
                    dims.push(Dim { a: [xm, ay.start, c], b: [xm, ay.start + ay.width, c], off: [0.0; 3], text: len(ay.width) });
                }
                _ => {}
            }
        }
    }
    dims.truncate(MAX_DIMS);
    let ports = m.bounds().map(|b| port_marks(m, b, k, c, driven, &len)).unwrap_or_default();
    (dims, ports)
}

/// The ports' faces where the plane across `k` at `c` meets them: a segment each.
fn port_marks(
    m: &emcraft_model::Model,
    b: emcraft_model::Aabb,
    k: usize,
    c: f64,
    driven: Option<usize>,
    len: &dyn Fn(f64) -> String,
) -> Vec<PortMark> {
    let (lo, hi) = (b.min.to_array(), b.max.to_array());
    let mut out = Vec::new();
    for (j, port) in m.ports.iter().enumerate().take(64) {
        // The face as a box flat across its normal: its own rectangle, or the model's end.
        let (n, flo, fhi) = match port.face {
            Some(f) => {
                let q = f.corners();
                let mut flo = [f64::INFINITY; 3];
                let mut fhi = [f64::NEG_INFINITY; 3];
                for p in q {
                    for (d, x) in p.to_array().into_iter().enumerate() {
                        flo[d] = flo[d].min(x);
                        fhi[d] = fhi[d].max(x);
                    }
                }
                (f.axis.index(), flo, fhi)
            }
            None => {
                let z = if port.end == emcraft_model::End::Start { lo[2] } else { hi[2] };
                (2, [lo[0], lo[1], z], [hi[0], hi[1], z])
            }
        };
        if n == k || c < flo[k] - 1e-12 || c > fhi[k] + 1e-12 {
            continue;
        }
        let other = 3 - n - k;
        let mut a = [0.0; 3];
        a[n] = flo[n];
        a[k] = c;
        let mut e = a;
        a[other] = flo[other];
        e[other] = fhi[other];
        let input = driven == Some(j);
        let size = len(fhi[other] - flo[other]);
        let text = if input {
            crate::lang::trf("{name} · in · {size}", crate::lang::args!(name = port.name, size = size))
        } else {
            crate::lang::trf("{name} · out · {size}", crate::lang::args!(name = port.name, size = size))
        };
        out.push(PortMark { a, b: e, text, input });
    }
    out
}

/// Draws the cut's dimensions and port marks over the view in `rect`; each label is published
/// as `view3d:section:dim:<i>` and `view3d:section:port:<i>`.
pub fn annotate(app: &EmcraftApp, scene: &Scene, cam: &Camera, p: &egui::Painter, rect: Rect, cut: Cut, bounds: ([f64; 3], [f64; 3])) {
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
        let r = label(p, a2 + (b2 - a2) / 2.0, &d.text, &font, ink, t.panel, t.border);
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
        let reach = 10.0 + 0.5 * (g.size().x * dir.x.abs() + g.size().y * dir.y.abs()) + 4.0;
        let at = last + dir * reach;
        p.line_segment([last, last + dir * 8.0], Stroke::new(1.0, port));
        let r = label(p, at, &m.text, &font, t.text, t.panel, port);
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

/// `text` on a small card centred at `at`; its rectangle.
fn label(p: &egui::Painter, at: Pos2, text: &str, font: &FontId, ink: egui::Color32, fill: egui::Color32, edge: egui::Color32) -> Rect {
    let g = p.layout_no_wrap(text.to_owned(), font.clone(), ink);
    let r = Rect::from_center_size(at, g.size() + vec2(8.0, 4.0));
    p.rect_filled(r, 3.0, fill.gamma_multiply(0.92));
    p.rect_stroke(r, 3.0, Stroke::new(1.0, edge), egui::StrokeKind::Inside);
    p.galley(r.min + vec2(4.0, 2.0), g, ink);
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// An iris filter cut in its H-plane: the overall length and width, each iris's aperture,
    /// and the two ports at its ends, sized as the guide is wide.
    #[test]
    fn a_cut_of_a_guide_carries_its_dimensions_and_ports() {
        let mut s = emcraft_engine::Session::new();
        s.execute("file.sample", &json!({"name": "iris-bandpass"})).unwrap();
        let m = s.model();
        let b = m.bounds().unwrap();
        let bounds = (b.min.to_array(), b.max.to_array());
        let cut = Preset::HPlane.cut(bounds);
        assert!(Preset::HPlane.is(cut, bounds) && !Preset::EPlane.is(cut, bounds));
        let (dims, ports) = notes(m, s.display().length, s.driven_port(), cut, outline(&Scene::default(), cut, bounds));
        let w = m.waveguide.as_ref().unwrap();
        assert!(dims.len() > 2, "{dims:?}");
        assert!(dims[0].text.starts_with(&crate::steps::fmt(w.width.get() * 1e3)), "{dims:?}");
        assert_eq!(ports.len(), 2);
        // Port 1 is driven: the input; port 2 an output.
        assert!(ports[0].input && !ports[1].input && ports[0].text.starts_with("1 · in · "), "{ports:?}");
        assert!(ports.iter().all(|p| (p.b[0] - p.a[0] - w.width.get()).abs() < 1e-12), "{ports:?}");
        // Across x through the middle (the E-plane), the irises do not narrow the height: only the
        // overall extents.
        let (dims, ports) =
            notes(m, s.display().length, s.driven_port(), Preset::EPlane.cut(bounds), outline(&Scene::default(), Preset::EPlane.cut(bounds), bounds));
        assert_eq!(dims.len(), 2);
        assert_eq!(ports.len(), 2);
        // Facing the cut: from +y, the low side kept.
        assert!(look(cut, bounds).is_some());
    }
}
