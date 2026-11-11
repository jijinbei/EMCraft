//! The kinds of analysis: as cards on the start page (a small picture of what the kind models
//! and gives, in its own colour, its name and a line in plain words) and as the switch at the
//! head of the step bar (its glyph and name, the kinds listed on a click).

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
    pub name: crate::lang::Text,
    pub plain: crate::lang::Text,
    pub detail: crate::lang::Text,
}

/// The card of a kind of analysis.
pub fn kind_card(kind: Solver) -> KindCard {
    let (glyph, name, plain, detail) = match kind {
        Solver::Frequency => (
            Glyph::KindSweep,
            t!("S-parameters"),
            t!("What passes and what reflects, over the band"),
            t!("S-parameters between ports, swept over the band (solve.frequency)"),
        ),
        Solver::Eigenmodes => (
            Glyph::KindResonator,
            t!("Eigenmodes"),
            t!("The resonances of a closed cavity and their Q"),
            t!("A closed resonator's modes and Q; nothing drives it (solve.eigenmodes)"),
        ),
        Solver::Radiation => (
            Glyph::KindAntenna,
            t!("Radiation"),
            t!("How an antenna radiates: its far-field pattern"),
            t!("An antenna or a scatterer in an open boundary: fields and far field (solve.radiation)"),
        ),
        Solver::Pcb => (
            Glyph::Chart,
            t!("PCB signal and return path"),
            t!("Signal quality and ground continuity on a routed net"),
            t!("PCB geometry screening and ideal transmission-line estimates (pcb.inspect)"),
        ),
        Solver::TimeDomain => (
            Glyph::KindPulse,
            t!("Time domain"),
            t!("A pulse sent through, and what comes back in time"),
            t!("A pulse over the band: S-parameters by finite integration (solve.time_domain)"),
        ),
        Solver::System => (
            Glyph::Table,
            t!("System budget"),
            t!("Gain, noise and sensitivity along a receiver chain"),
            t!("An antenna, amplifiers, filters and lines in a chain: its budget stage by stage, no field solved (solve.system)"),
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
        let pic = Rect::from_min_max(r.min + vec2(10.0, 10.0), pos2(r.right() - 10.0, r.bottom() - 72.0));
        p.rect_filled(pic, 6.0, colours(kind)[1]);
        picture(&p, pic.shrink(8.0), kind);
        glyphs::paint(&p, Rect::from_min_size(pos2(r.left() + 10.0, pic.bottom() + 8.0), vec2(20.0, 20.0)), glyph, t.icon, colours(kind)[0]);
        p.text(pos2(r.left() + 36.0, pic.bottom() + 18.0), Align2::LEFT_CENTER, tr(name), FontId::proportional(t.font_body), t.text);
        // Two lines at most, cut with `…` (the whole on hover).
        let line = crate::ui_kit::wrapped_galley(ui, tr(plain), t.font_small, t.text_dim, r.width() - 20.0, 2);
        p.galley(pos2(r.left() + 10.0, pic.bottom() + 30.0), line, t.text_dim);
    } else {
        glyphs::paint(&p, Rect::from_center_size(pos2(r.center().x, r.top() + 28.0), vec2(34.0, 34.0)), glyph, t.icon, colours(kind)[0]);
        p.text(pos2(r.center().x, r.bottom() - 14.0), Align2::CENTER_CENTER, tr(name), FontId::proportional(t.font_body), name_ink);
    }
    if look.on && look.auto {
        p.text(r.right_top() + vec2(-8.0, 6.0), Align2::RIGHT_TOP, tr(t!("auto")), FontId::proportional(t.font_small - 1.0), t.text_dim);
    }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, look.enabled, tr(name)));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).hint(format!("{}\n{}", tr(name), tr(detail)))
}

/// A kind's own colours: its ink and its picture's ground.
pub fn colours(kind: Solver) -> [egui::Color32; 2] {
    let t = &Tokens::get();
    match kind {
        Solver::Frequency => t.kind_frequency,
        Solver::Eigenmodes => t.kind_eigenmodes,
        Solver::Radiation => t.kind_radiation,
        Solver::TimeDomain => t.kind_time_domain,
        Solver::Pcb => [t.success, t.panel],
        Solver::System => [t.accent, t.panel],
    }
}

/// What `kind` models and gives, drawn flat in `r` in its own colour, each composed apart:
/// a filter between two ports with the wave coming in, reflected (S11) and passed on (S21); a
/// closed cavity holding a standing wave, its resonance beside it; an antenna radiating
/// wavefronts into space over its pattern; a pulse running down a guide, its echo off a
/// discontinuity coming back.
pub fn picture(p: &egui::Painter, r: Rect, kind: Solver) {
    let t = &Tokens::get();
    let [ink, _] = colours(kind);
    let pic = Pic { r, ink, metal: t.border, edge: t.text_dim, dim: t.text_dim };
    match kind {
        Solver::Frequency => pic.sparameters(p),
        Solver::Eigenmodes => pic.cavity(p),
        Solver::Radiation => pic.antenna(p),
        Solver::TimeDomain => pic.pulse(p),
        Solver::Pcb => pic.pcb(p),
        Solver::System => pic.chain(p),
    }
}

/// A picture's frame and colours; `at` places a point by its share of the width and height
/// from the top left.
struct Pic {
    r: Rect,
    ink: egui::Color32,
    metal: egui::Color32,
    edge: egui::Color32,
    dim: egui::Color32,
}

impl Pic {
    fn pcb(&self, p: &egui::Painter) {
        for (a, b) in [(0.04, 0.46), (0.54, 0.96)] {
            p.rect_filled(Rect::from_min_max(self.at(a, 0.12), self.at(b, 0.88)), 3., self.ink.gamma_multiply(0.2));
        }
        p.line_segment([self.at(0.12, 0.42), self.at(0.88, 0.42)], Stroke::new(3., self.edge));
        p.line_segment([self.at(0.12, 0.65), self.at(0.88, 0.65)], Stroke::new(1.5, self.ink));
        p.circle_stroke(self.at(0.5, 0.42), self.r.height() * 0.14, Stroke::new(2., self.ink));
        for x in [0.12, 0.88] {
            p.circle_filled(self.at(x, 0.42), 3., self.edge);
        }
    }
    /// An antenna feeding three blocks in a row (an amplifier, a filter, a receiver), the
    /// signal's arrows between them and a gain's steps rising under them.
    fn chain(&self, p: &egui::Painter) {
        let mid = 0.4;
        // The antenna: a mast and a V.
        p.line_segment([self.at(0.07, mid), self.at(0.07, 0.15)], Stroke::new(2., self.edge));
        p.line_segment([self.at(0.07, 0.3), self.at(0.02, 0.12)], Stroke::new(2., self.edge));
        p.line_segment([self.at(0.07, 0.3), self.at(0.12, 0.12)], Stroke::new(2., self.edge));
        let mut x = 0.07;
        for (k, x0) in [0.18, 0.46, 0.74].into_iter().enumerate() {
            let arrow = Stroke::new(1.5, self.ink);
            p.line_segment([self.at(x, mid), self.at(x0, mid)], arrow);
            p.line_segment([self.at(x0, mid), self.at(x0 - 0.03, mid - 0.06)], arrow);
            p.line_segment([self.at(x0, mid), self.at(x0 - 0.03, mid + 0.06)], arrow);
            let r = Rect::from_min_max(self.at(x0, mid - 0.18), self.at(x0 + 0.2, mid + 0.18));
            p.rect(r, 3., self.ink.gamma_multiply(0.2), Stroke::new(1.5, self.ink), egui::StrokeKind::Inside);
            if k == 0 {
                // The amplifier's triangle.
                p.add(egui::Shape::convex_polygon(
                    vec![self.at(x0 + 0.05, mid - 0.1), self.at(x0 + 0.15, mid), self.at(x0 + 0.05, mid + 0.1)],
                    self.ink,
                    Stroke::NONE,
                ));
            }
            x = x0 + 0.2;
        }
        // The gain along the chain: a step up, a small step down, a step up.
        let steps = [(0.07, 0.95), (0.18, 0.95), (0.18, 0.75), (0.46, 0.75), (0.46, 0.8), (0.74, 0.8), (0.74, 0.62), (0.94, 0.62)];
        let pts: Vec<Pos2> = steps.iter().map(|(x, y)| self.at(*x, *y)).collect();
        p.add(egui::Shape::line(pts, Stroke::new(1.5, self.edge)));
    }

    fn at(&self, x: f32, y: f32) -> Pos2 {
        pos2(self.r.left() + x * self.r.width(), self.r.top() + y * self.r.height())
    }

    /// A box of metal from (x0, y0) to (x1, y1).
    fn metal(&self, p: &egui::Painter, x0: f32, y0: f32, x1: f32, y1: f32) {
        p.rect(Rect::from_min_max(self.at(x0, y0), self.at(x1, y1)), 1.0, self.metal, Stroke::new(1.0, self.edge), egui::StrokeKind::Inside);
    }

    /// A solid iris or post from (x0, y0) to (x1, y1).
    fn post(&self, p: &egui::Painter, x0: f32, y0: f32, x1: f32, y1: f32) {
        p.rect_filled(Rect::from_min_max(self.at(x0, y0), self.at(x1, y1)), 1.0, self.edge);
    }

    /// A sine wave of 2.5 cycles along `y` from x0 to x1, `amp` of the height, ending in an
    /// arrowhead at x1.
    fn wave(&self, p: &egui::Painter, (x0, x1): (f32, f32), y: f32, amp: f32, s: Stroke) {
        let (n, cycles) = (40, 2.5);
        let pts: Vec<Pos2> = (0..=n)
            .map(|k| k as f32 / n as f32)
            .map(|u| {
                let x = x0 + (x1 - x0) * u;
                // Tapered at the head so the arrow leaves from the axis.
                let taper = (1.0 - u).min(0.15) / 0.15;
                self.at(x, y - amp * taper * (u * cycles * std::f32::consts::TAU).sin())
            })
            .collect();
        p.add(egui::Shape::line(pts, s));
        arrow_head(p, self.at(x0 + (x1 - x0) * 0.9, y), self.at(x1, y), s);
    }

    /// A filter between two ports: a guide with two irises, the wave in at port 1, a small
    /// one reflected (S11) and one passed on at port 2 (S21).
    fn sparameters(&self, p: &egui::Painter) {
        let (x0, x1) = (0.34, 0.66);
        self.metal(p, x0, 0.2, x1, 0.8);
        p.rect_filled(Rect::from_min_max(self.at(x0, 0.36), self.at(x1, 0.64)), 0.0, self.ink.gamma_multiply(0.12));
        for x in [0.45, 0.55] {
            self.post(p, x - 0.015, 0.2, x + 0.015, 0.45);
            self.post(p, x - 0.015, 0.55, x + 0.015, 0.8);
        }
        for (x, n) in [(x0, "1"), (x1, "2")] {
            p.add(egui::Shape::dashed_line(&[self.at(x, 0.1), self.at(x, 0.9)], Stroke::new(1.5, self.ink), 3.0, 2.0));
            p.text(self.at(x, 0.08), Align2::CENTER_BOTTOM, n, FontId::proportional(10.0), self.ink);
        }
        let font = FontId::proportional(10.0);
        self.wave(p, (0.02, 0.31), 0.3, 0.1, Stroke::new(2.2, self.ink));
        self.wave(p, (0.31, 0.04), 0.72, 0.05, Stroke::new(1.5, self.ink.gamma_multiply(0.6)));
        p.text(self.at(0.17, 0.98), Align2::CENTER_BOTTOM, "S11", font.clone(), self.dim);
        self.wave(p, (0.69, 0.98), 0.5, 0.09, Stroke::new(2.2, self.ink));
        p.text(self.at(0.83, 0.98), Align2::CENTER_BOTTOM, "S21", font, self.dim);
    }

    /// A closed cavity with a standing wave of two half-waves (bright lobes, a node between),
    /// and its resonance, one sharp peak, beside it.
    fn cavity(&self, p: &egui::Painter) {
        let (x0, x1, y0, y1) = (0.04, 0.64, 0.1, 0.9);
        let wall = Rect::from_min_max(self.at(x0, y0), self.at(x1, y1));
        for c in [0.19, 0.49] {
            let centre = self.at(c, 0.5);
            for k in 0..7 {
                let s = 1.0 - k as f32 / 7.0;
                let radius = vec2(0.13 * self.r.width() * s, 0.36 * self.r.height() * s);
                p.add(egui::Shape::ellipse_filled(centre, radius, self.ink.gamma_multiply(0.16)));
            }
        }
        p.add(egui::Shape::dashed_line(&[self.at(0.34, y0), self.at(0.34, y1)], Stroke::new(1.0, self.dim), 3.0, 3.0));
        p.rect_stroke(wall, 3.0, Stroke::new(3.0, self.edge), egui::StrokeKind::Middle);
        // The resonance: a Lorentzian over a baseline.
        let (a, b) = (0.72, 0.98);
        p.line_segment([self.at(a, 0.85), self.at(b, 0.85)], Stroke::new(1.0, self.dim));
        let pts: Vec<Pos2> =
            (0..=40).map(|k| a + (b - a) * k as f32 / 40.0).map(|x| self.at(x, 0.85 - 0.65 / (1.0 + ((x - 0.85) / 0.018).powi(2)))).collect();
        p.add(egui::Shape::line(pts, Stroke::new(2.0, self.ink)));
    }

    /// A dipole radiating: its pattern faint behind, wavefronts leaving it into space.
    fn antenna(&self, p: &egui::Painter) {
        let c = self.at(0.12, 0.5);
        let reach = 0.86 * self.r.width();
        // A directive pattern (a four-element array's, its back lobe lowered) to the right.
        let pts: Vec<Pos2> = (0..=144)
            .map(|k| k as f32 / 144.0 * std::f32::consts::TAU)
            .map(|a| {
                let psi = std::f32::consts::PI * a.sin();
                let af = if psi.abs() < 1e-4 { 1.0 } else { ((2.0 * psi).sin() / (4.0 * (psi / 2.0).sin())).abs() };
                let g = 0.03 + 0.97 * af * (0.2 + 0.8 * (1.0 + a.cos()) / 2.0);
                c + vec2(a.cos(), -a.sin() * 0.9) * reach * 0.55 * g
            })
            .collect();
        let mut fan = egui::Mesh::default();
        let faint = self.ink.gamma_multiply(0.18);
        fan.colored_vertex(c, faint);
        for q in &pts {
            fan.colored_vertex(*q, faint);
        }
        for k in 1..pts.len() as u32 {
            fan.add_triangle(0, k, k + 1);
        }
        p.add(egui::Shape::mesh(fan));
        // Wavefronts: arcs about the antenna, fainter as they go.
        let clip = p.with_clip_rect(self.r.expand(4.0));
        for (k, radius) in [0.2f32, 0.4, 0.6, 0.8].iter().enumerate() {
            let rad = radius * reach;
            let half = 0.75f32;
            let arc: Vec<Pos2> = (0..=24).map(|j| -half + 2.0 * half * j as f32 / 24.0).map(|a| c + vec2(a.cos(), a.sin()) * rad).collect();
            clip.add(egui::Shape::line(arc, Stroke::new(2.0, self.ink.gamma_multiply(1.0 - 0.2 * k as f32))));
        }
        // The dipole: two arms and its feed.
        let arm = 0.36 * self.r.height();
        p.line_segment([c - vec2(0.0, 4.0), c - vec2(0.0, arm)], Stroke::new(4.0, self.edge));
        p.line_segment([c + vec2(0.0, 4.0), c + vec2(0.0, arm)], Stroke::new(4.0, self.edge));
        p.circle_filled(c, 3.5, self.ink);
    }

    /// A pulse running down a guide (fainter where it was), its echo off an iris coming back
    /// and what passes on; time along the bottom.
    fn pulse(&self, p: &egui::Painter) {
        let (top, bottom, mid) = (0.22, 0.7, 0.46);
        for y in [top, bottom] {
            p.line_segment([self.at(0.0, y), self.at(1.0, y)], Stroke::new(2.5, self.edge));
        }
        self.post(p, 0.645, top, 0.675, 0.37);
        self.post(p, 0.645, 0.55, 0.675, bottom);
        let bump = |x: f32, h: f32, ink: egui::Color32| {
            let pts: Vec<Pos2> =
                (0..=30).map(|k| x - 0.09 + 0.18 * k as f32 / 30.0).map(|u| self.at(u, mid - h * (-((u - x) / 0.035).powi(2)).exp())).collect();
            p.add(egui::Shape::line(pts, Stroke::new(2.2, ink)));
        };
        p.line_segment([self.at(0.0, mid), self.at(1.0, mid)], Stroke::new(1.0, self.dim.gamma_multiply(0.5)));
        for (x, fade) in [(0.12, 0.25), (0.22, 0.5)] {
            bump(x, 0.2, self.ink.gamma_multiply(fade));
        }
        bump(0.34, 0.2, self.ink);
        arrow(p, self.at(0.27, 0.12), self.at(0.41, 0.12), Stroke::new(1.6, self.ink));
        bump(0.53, -0.1, self.ink.gamma_multiply(0.65));
        arrow(p, self.at(0.59, 0.8), self.at(0.47, 0.8), Stroke::new(1.6, self.ink.gamma_multiply(0.65)));
        bump(0.85, 0.12, self.ink);
        arrow(p, self.at(0.79, 0.12), self.at(0.91, 0.12), Stroke::new(1.6, self.ink));
        arrow(p, self.at(0.02, 0.95), self.at(0.3, 0.95), Stroke::new(1.2, self.dim));
        p.text(self.at(0.32, 0.95), Align2::LEFT_CENTER, "t", FontId::proportional(10.0), self.dim);
    }
}

/// A line from `a` to `b` with a head at `b`.
fn arrow(p: &egui::Painter, a: Pos2, b: Pos2, s: Stroke) {
    p.line_segment([a, b], s);
    arrow_head(p, a, b, s);
}

/// An arrowhead at `b` pointing away from `a`.
fn arrow_head(p: &egui::Painter, a: Pos2, b: Pos2, s: Stroke) {
    let d = b - a;
    let len = d.length();
    if len < 1e-3 {
        return;
    }
    let u = d / len;
    let n = vec2(-u.y, u.x);
    let head = 6.0;
    p.add(egui::Shape::convex_polygon(vec![b + u * 1.5, b - u * head + n * head * 0.55, b - u * head - n * head * 0.55], s.color, Stroke::NONE));
}

/// The switch's width, in points.
pub const SWITCH_WIDTH: f32 = 200.0;

/// The job's kind of analysis at the head of the step bar, in `r`: its glyph and name (`auto`
/// while inferred); a click lists the kinds, and choosing one runs `project.analysis`.
pub fn switch(app: &crate::EmcraftApp, ui: &mut egui::Ui, r: Rect, out: &mut crate::Actions) {
    let t = &Tokens::get();
    let m = app.session.model();
    let (current, chosen) = (m.analysis(), m.job.chosen());
    let enabled = !app.busy();
    let resp = ui.hit(r, Handle::Step(crate::steps::Step::Intent), if enabled { Sense::click() } else { Sense::hover() });
    let KindCard { glyph, name, detail, .. } = kind_card(current);
    let fill = if resp.hovered() && enabled { t.hover } else { t.accent_soft };
    let p = ui.painter().with_clip_rect(r);
    p.rect(r, 12.0, fill, Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
    glyphs::paint(&p, Rect::from_center_size(pos2(r.left() + 18.0, r.center().y), vec2(18.0, 18.0)), glyph, t.icon, colours(current)[0]);
    let label = if chosen.is_none() { format!("{} ({})", tr(name), tr(t!("auto"))) } else { tr(name).to_owned() };
    p.text(pos2(r.left() + 34.0, r.center().y), Align2::LEFT_CENTER, label, FontId::proportional(t.font_body), t.text);
    let c = pos2(r.right() - 14.0, r.center().y);
    p.add(egui::Shape::convex_polygon(vec![c + vec2(-4.0, -2.0), c + vec2(4.0, -2.0), c + vec2(0.0, 3.0)], t.text_dim, Stroke::NONE));
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, enabled, tr(name)));
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand).hint(format!("{}\n{}", tr(name), tr(detail)));
    egui::Popup::menu(&resp).show(|ui| {
        for kind in Solver::ALL {
            let card = kind_card(kind);
            // A row: the kind's glyph in its colour and its name, the job's own filled.
            let (row, _) = ui.allocate_exact_size(vec2(SWITCH_WIDTH + 40.0, 28.0), Sense::hover());
            let item = ui.hit(row, Handle::Kind(kind), if enabled { Sense::click() } else { Sense::hover() });
            let fill = match (kind == current, item.hovered() && enabled) {
                (true, _) => t.accent_soft,
                (false, true) => t.hover,
                (false, false) => egui::Color32::TRANSPARENT,
            };
            let p = ui.painter();
            p.rect_filled(row, 6.0, fill);
            glyphs::paint(p, Rect::from_center_size(pos2(row.left() + 16.0, row.center().y), vec2(18.0, 18.0)), card.glyph, t.icon, colours(kind)[0]);
            p.text(pos2(row.left() + 34.0, row.center().y), Align2::LEFT_CENTER, tr(card.name), FontId::proportional(t.font_body), t.text);
            item.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, enabled, kind == current, tr(card.name)));
            if item.on_hover_cursor(egui::CursorIcon::PointingHand).hint(card.detail).clicked() {
                if chosen != Some(kind) {
                    out.push(crate::Action::run("project.analysis", serde_json::json!({"kind": kind.id()})));
                }
                ui.close();
            }
        }
    });
}
