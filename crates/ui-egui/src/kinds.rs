//! The four kinds of analysis as cards, drawn alike on the start page (a glyph and a name) and
//! on the project's Home (a small picture of what the kind gives, its name and a line in plain
//! words): an S-parameter chart, resonance peaks, a radiation pattern, a pulse and its echo.

use egui::{Align2, FontId, Pos2, Rect, Sense, Stroke, pos2, vec2};

use crate::glyphs::{self, Glyph};
use crate::handle::Handle;
use crate::lang::tr;
use crate::steps::Solver;
use crate::theme::Tokens;
use crate::{Hint, UiExt};

/// A kind of analysis as its card shows it: its glyph, its name, what it gives in plain words
/// and what it does (on hover; all to be translated).
#[derive(Clone, Copy, Debug)]
pub struct KindCard {
    pub glyph: Glyph,
    pub name: &'static str,
    pub plain: &'static str,
    pub detail: &'static str,
}

/// The card of a kind of analysis.
pub fn kind_card(kind: Solver) -> KindCard {
    let (glyph, name, plain, detail) = match kind {
        Solver::Frequency => (
            Glyph::KindSweep,
            "S-parameters",
            "What passes and what reflects, over the band",
            "S-parameters between ports, swept over the band (solve.frequency)",
        ),
        Solver::Eigenmodes => (
            Glyph::KindResonator,
            "Eigenmodes",
            "The resonances of a closed cavity and their Q",
            "A closed resonator's modes and Q; nothing drives it (solve.eigenmodes)",
        ),
        Solver::Radiation => (
            Glyph::KindAntenna,
            "Radiation",
            "How an antenna radiates: its far-field pattern",
            "An antenna or a scatterer in an open boundary: fields and far field (solve.radiation)",
        ),
        Solver::TimeDomain => (
            Glyph::KindPulse,
            "Time domain",
            "A pulse sent through, and what comes back in time",
            "A pulse over the band: S-parameters by finite integration (solve.time_domain)",
        ),
    };
    KindCard { glyph, name, plain, detail }
}

/// How a card is drawn: `large` with its picture and plain line (else its glyph and name),
/// `on` the job's kind, `auto` while that is inferred, `enabled` unless a job runs.
#[derive(Clone, Copy, Debug)]
pub struct Look {
    pub large: bool,
    pub on: bool,
    pub auto: bool,
    pub enabled: bool,
}

/// The card of `kind`, `size` points, published as `handle`; clicked when its response is.
pub fn tile(ui: &mut egui::Ui, kind: Solver, size: egui::Vec2, look: Look, handle: Handle) -> egui::Response {
    let t = &Tokens::get();
    let (r, _) = ui.allocate_exact_size(size, Sense::hover());
    let resp = ui.hit(r, handle, if look.enabled { Sense::click() } else { Sense::hover() });
    let KindCard { glyph, name, plain, detail } = kind_card(kind);
    let hovered = resp.hovered() && look.enabled;
    let fill = match (look.on, hovered) {
        (true, _) => t.accent_soft,
        (false, true) => t.hover,
        (false, false) => t.panel_header,
    };
    let stroke =
        if look.on { Stroke::new(2.0, t.accent) } else { Stroke::new(if hovered { 1.5 } else { 1.0 }, if hovered { t.accent } else { t.border }) };
    let p = ui.painter().with_clip_rect(r);
    p.rect(r, 10.0, fill, stroke, egui::StrokeKind::Inside);
    let name_ink = if look.on { t.text } else { t.text_dim };
    if look.large {
        let pic = Rect::from_min_max(r.min + vec2(10.0, 10.0), pos2(r.right() - 10.0, r.bottom() - 58.0));
        p.rect_filled(pic, 6.0, t.panel);
        picture(&p, pic.shrink(8.0), kind, t);
        glyphs::paint(&p, Rect::from_min_size(pos2(r.left() + 10.0, pic.bottom() + 8.0), vec2(20.0, 20.0)), glyph, t.icon, t.accent);
        p.text(pos2(r.left() + 36.0, pic.bottom() + 18.0), Align2::LEFT_CENTER, tr(name), FontId::proportional(t.font_body), t.text);
        let line = p.layout(tr(plain).to_owned(), FontId::proportional(t.font_small), t.text_dim, r.width() - 20.0);
        p.galley(pos2(r.left() + 10.0, pic.bottom() + 30.0), line, t.text_dim);
    } else {
        glyphs::paint(&p, Rect::from_center_size(pos2(r.center().x, r.top() + 28.0), vec2(34.0, 34.0)), glyph, t.icon, t.accent);
        p.text(pos2(r.center().x, r.bottom() - 14.0), Align2::CENTER_CENTER, tr(name), FontId::proportional(t.font_body), name_ink);
    }
    if look.on && look.auto {
        p.text(r.right_top() + vec2(-8.0, 6.0), Align2::RIGHT_TOP, tr("auto"), FontId::proportional(t.font_small - 1.0), t.text_dim);
    }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, look.enabled, tr(name)));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).hint(format!("{}\n{}", tr(name), tr(detail)))
}

/// What `kind` gives, drawn in `r`: an S-parameter chart (|S21| rising past the cut-off, |S11|
/// falling), resonance peaks, a radiation pattern on its polar grid, a pulse and its echo.
pub fn picture(p: &egui::Painter, r: Rect, kind: Solver, t: &Tokens) {
    let at = |x: f32, y: f32| pos2(r.left() + x * r.width(), r.bottom() - y * r.height());
    let curve = |f: &dyn Fn(f32) -> f32| -> Vec<Pos2> { (0..=48).map(|k| k as f32 / 48.0).map(|x| at(x, f(x).clamp(0.0, 1.0))).collect() };
    let axes = Stroke::new(1.0, t.border);
    let first = Stroke::new(2.0, t.accent);
    let second = Stroke::new(1.6, t.info);
    match kind {
        Solver::Frequency => {
            p.line_segment([at(0.0, 0.0), at(1.0, 0.0)], axes);
            p.line_segment([at(0.0, 0.0), at(0.0, 1.0)], axes);
            let s21 = |x: f32| 0.88 / (1.0 + (-(x - 0.32) * 22.0).exp());
            p.add(egui::Shape::line(curve(&s21), first));
            let s11 = |x: f32| 0.9 - 0.75 / (1.0 + (-(x - 0.32) * 22.0).exp()) + 0.06 * (x * 18.0).sin() * (x - 0.3).max(0.0);
            p.add(egui::Shape::line(curve(&s11), second));
        }
        Solver::Eigenmodes => {
            p.line_segment([at(0.0, 0.0), at(1.0, 0.0)], axes);
            let peaks = [(0.22, 0.9), (0.5, 0.65), (0.78, 0.8)];
            let f = |x: f32| peaks.iter().map(|(c, h)| h / (1.0 + ((x - c) / 0.025).powi(2))).sum::<f32>();
            p.add(egui::Shape::line(curve(&f), first));
            for (c, _) in peaks {
                p.add(egui::Shape::dashed_line(&[at(c, 0.0), at(c, 1.0)], Stroke::new(1.0, t.border), 3.0, 3.0));
            }
        }
        Solver::Radiation => {
            let c = r.center();
            let rad = 0.5 * r.width().min(r.height());
            for k in 1..=3 {
                p.circle_stroke(c, rad * k as f32 / 3.0, axes);
            }
            p.line_segment([c - vec2(rad, 0.0), c + vec2(rad, 0.0)], axes);
            p.line_segment([c - vec2(0.0, rad), c + vec2(0.0, rad)], axes);
            // A four-element array's pattern, |sin(2ψ)/(4 sin(ψ/2))| with ψ = π sin θ, its back
            // lobe lowered by (1 + cos θ)/2: a main lobe to the right and side lobes.
            let pts: Vec<Pos2> = (0..=144)
                .map(|k| k as f32 / 144.0 * std::f32::consts::TAU)
                .map(|a| {
                    let psi = std::f32::consts::PI * a.sin();
                    let af = if psi.abs() < 1e-4 { 1.0 } else { ((2.0 * psi).sin() / (4.0 * (psi / 2.0).sin())).abs() };
                    let g = 0.04 + 0.96 * af * (0.35 + 0.65 * (1.0 + a.cos()) / 2.0);
                    c + vec2(a.cos(), -a.sin()) * rad * g
                })
                .collect();
            // Filled as a fan from the middle: the pattern is star-shaped, not convex.
            let mut fan = egui::Mesh::default();
            fan.colored_vertex(c, t.accent_soft);
            for q in &pts {
                fan.colored_vertex(*q, t.accent_soft);
            }
            for k in 1..pts.len() as u32 {
                fan.add_triangle(0, k, k + 1);
            }
            p.add(egui::Shape::mesh(fan));
            p.add(egui::Shape::closed_line(pts, first));
        }
        Solver::TimeDomain => {
            p.line_segment([at(0.0, 0.5), at(1.0, 0.5)], axes);
            let pulse = |x: f32, c: f32, h: f32| h * (-((x - c) / 0.06).powi(2)).exp() * ((x - c) * 60.0).cos();
            let incident = |x: f32| 0.5 + 0.45 * pulse(x, 0.25, 1.0);
            p.add(egui::Shape::line(curve(&incident), first));
            let echo = |x: f32| 0.5 - 0.45 * pulse(x, 0.72, 0.45);
            p.add(egui::Shape::line(curve(&echo), second));
        }
    }
}
