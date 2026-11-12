//! The 3D view's tools drawn over the picture, as the shared rules find them
//! ([`emcraft_ui_core::tools`]): the face under the pointer lit with its outline and corners, the
//! port tool's tip or the snap's marker, the points picked and a dashed line to the pointer, the
//! measurement's dimension (its legs along x, y and z in the axes' colours) and its card, the
//! selected port's reference plane with its grip and distance, the selected body's grips, and the band saying which tool is
//! on. A canvas laid over the shader widget; it lets every event through to the view under it.

use emcraft_model::units::Length;
use emcraft_render::view3d::{Camera, Scene};
use emcraft_ui_core::geom::Vector;
use emcraft_ui_core::lang::tr;
use emcraft_ui_core::tools::grips::{Grip, Mark};
use emcraft_ui_core::tools::{self, Hover, Ink, Measured, PortTarget, RefPlane, Snap, SnapKind, Tool, Tools};
use iced::mouse::Cursor;
use iced::widget::canvas::{self, Frame, Geometry, LineDash, Path, Stroke};
use iced::{Color, Pixels, Point, Rectangle, Renderer, Size, Theme, alignment};

use crate::app::Message;
use crate::geom::point;
use crate::look::Tokens;

/// Most triangles of the face under the pointer lit.
const MAX_LIT: usize = 20_000;
/// A character's width as a share of the font's size (the canvas does not measure text).
const CHAR_W: f32 = 0.56;

/// What the tools draw from: their state and the tool on, the camera, the scene drawn (its
/// triangles lit), the model (the ports' names), the reference plane, the display lengths and
/// the theme's tokens.
pub struct Marks<'a> {
    pub tools: &'a Tools,
    pub tool: Option<Tool>,
    pub camera: Camera,
    pub scene: Option<&'a Scene>,
    pub model: &'a emcraft_model::Model,
    pub reference: Option<RefPlane>,
    /// The selected body's grips.
    pub grips: Vec<Mark>,
    pub length: Length,
    pub tokens: Tokens,
}

/// Points on the view of `size` through `camera`.
struct On {
    camera: Camera,
    size: Vector,
}

impl On {
    fn at(&self, p: [f64; 3]) -> Option<Point> {
        tools::screen(&self.camera, self.size, p).map(point)
    }
}

fn stroke(width: f32, ink: Color) -> Stroke<'static> {
    Stroke::default().with_width(width).with_color(ink)
}

fn dashed(frame: &mut Frame, a: Point, b: Point, width: f32, ink: Color) {
    let s = Stroke { line_dash: LineDash { segments: &[6.0, 4.0], offset: 0 }, ..stroke(width, ink) };
    frame.stroke(&Path::line(a, b), s);
}

fn polygon(pts: &[Point]) -> Path {
    Path::new(|b| {
        if let Some((first, rest)) = pts.split_first() {
            b.move_to(*first);
            rest.iter().for_each(|p| b.line_to(*p));
            b.close();
        }
    })
}

/// `text` on a pill centred at `at`: the accent's when `ok`, else the panel's.
fn tip(frame: &mut Frame, t: Tokens, at: Point, text: &str, ok: bool) {
    if text.is_empty() {
        return;
    }
    let size = t.font_small;
    let ground = Size::new(text.chars().count() as f32 * size * CHAR_W + 12.0, size + 8.0);
    let corner = Point::new(at.x - ground.width / 2.0, at.y - ground.height / 2.0);
    frame.fill(&Path::rounded_rectangle(corner, ground, 8.0.into()), if ok { t.accent } else { Color { a: 0.92, ..t.panel } });
    frame.fill_text(canvas::Text {
        content: text.to_owned(),
        position: at,
        color: if ok { t.on_accent } else { t.text },
        size: Pixels(size),
        align_x: iced::widget::text::Alignment::Center,
        align_y: alignment::Vertical::Center,
        ..canvas::Text::default()
    });
}

/// A snap's marker: a square on a corner, a triangle on an edge's middle, a ringed dot on a
/// centre, a diamond on an edge, a cross on a face.
fn marker(frame: &mut Frame, t: Tokens, q: Point, kind: SnapKind) {
    let s = stroke(1.6, t.accent);
    let r = 5.5;
    let at = |dx: f32, dy: f32| Point::new(q.x + dx, q.y + dy);
    match kind {
        SnapKind::Vertex => frame.stroke(&Path::rectangle(at(-r, -r), Size::new(2.0 * r, 2.0 * r)), s),
        SnapKind::Midpoint => frame.stroke(&polygon(&[at(0.0, -r), at(r, r * 0.8), at(-r, r * 0.8)]), s),
        SnapKind::Centre => {
            frame.stroke(&Path::circle(q, r), s);
            frame.fill(&Path::circle(q, 1.8), t.accent);
        }
        SnapKind::Edge => frame.stroke(&polygon(&[at(0.0, -r), at(r, 0.0), at(0.0, r), at(-r, 0.0)]), s),
        SnapKind::Face => {
            frame.stroke(&Path::line(at(-r, 0.0), at(r, 0.0)), s);
            frame.stroke(&Path::line(at(0.0, -r), at(0.0, r)), s);
        }
    }
}

impl Marks<'_> {
    /// The face under the pointer lit, its outline and corners, and the port tool's tip or the
    /// snap's marker.
    fn hovered(&self, frame: &mut Frame, on: &On, tool: Tool, h: &Hover) {
        let t = self.tokens;
        let ok = !matches!(h.port, Some(PortTarget::No(_)));
        let ink = if ok { t.accent } else { t.text_dim };
        if let Some(scene) = self.scene.filter(|_| h.face.tris.len() <= MAX_LIT) {
            let lit = Color { a: 0.25, ..ink };
            for tri in h.face.tris.iter().filter_map(|i| scene.tris.get(*i)) {
                if let (Some(a), Some(b), Some(c)) = (on.at(tri.p[0]), on.at(tri.p[1]), on.at(tri.p[2])) {
                    frame.fill(&polygon(&[a, b, c]), lit);
                }
            }
        }
        for (a, b) in &h.face.outline {
            if let (Some(a), Some(b)) = (on.at(*a), on.at(*b)) {
                frame.stroke(&Path::line(a, b), stroke(2.0, ink));
            }
        }
        // The corners, to aim at.
        for q in h.face.corners.iter().take(64).filter_map(|c| on.at(*c)) {
            frame.fill(&Path::circle(q, 2.5), ink);
        }
        match tool {
            Tool::Port => {
                if let Some(q) = on.at(h.face.centre) {
                    tip(frame, t, q, &tools::port_text(self.model, h.port), ok);
                }
            }
            _ => {
                if let Some(q) = on.at(h.snap.at) {
                    marker(frame, t, q, h.snap.kind);
                }
            }
        }
    }

    /// The measurement: its legs along x, y and z in the axes' colours, the line between the
    /// picks with arrow ends, the distance on it and a card with the components, the gap and the
    /// angle in the view's top right corner.
    fn dimension(&self, frame: &mut Frame, on: &On, m: &Measured, width: f32) {
        let t = self.tokens;
        let [a, b]: [Snap; 2] = m.picks;
        let axes = [t.axis_x, t.axis_y, t.axis_z];
        // The legs: x, then y, then z.
        let _ = axes.iter().enumerate().fold(a.at, |q, (k, ink)| {
            let mut next = q;
            next[k] = b.at[k];
            if let (Some(s), Some(e)) = (on.at(q), on.at(next)) {
                dashed(frame, s, e, 1.2, *ink);
            }
            next
        });
        let (Some(sa), Some(sb)) = (on.at(a.at), on.at(b.at)) else { return };
        frame.stroke(&Path::line(sa, sb), stroke(2.0, t.accent));
        let (dx, dy) = (sb.x - sa.x, sb.y - sa.y);
        let length = dx.hypot(dy);
        if length > 1.0 {
            let (ux, uy) = (dx / length, dy / length);
            let (nx, ny) = (-uy, ux);
            for (q, s) in [(sa, 1.0), (sb, -1.0)] {
                let base = Point::new(q.x + s * ux * 9.0, q.y + s * uy * 9.0);
                frame.fill(
                    &polygon(&[q, Point::new(base.x + nx * 3.5, base.y + ny * 3.5), Point::new(base.x - nx * 3.5, base.y - ny * 3.5)]),
                    t.accent,
                );
            }
        }
        for q in [sa, sb] {
            frame.fill(&Path::circle(q, 3.0), t.accent);
        }
        let middle = Point::new((sa.x + sb.x) / 2.0, (sa.y + sb.y) / 2.0 - 14.0);
        tip(frame, t, middle, &tools::measure_distance(&m.reply), true);
        // The card.
        let lines = tools::measure_card(&m.reply);
        let size = t.font_small;
        let w = lines.iter().map(|(s, _)| s.chars().count()).max().unwrap_or(0) as f32 * size * CHAR_W + 20.0;
        let h = lines.len() as f32 * (size + 4.0) + 12.0;
        let corner = Point::new(width - w - 12.0, 12.0);
        let card = Path::rounded_rectangle(corner, Size::new(w, h), 6.0.into());
        frame.fill(&card, Color { a: 0.94, ..t.panel });
        frame.stroke(&card, stroke(1.0, t.accent));
        for (i, (s, ink)) in lines.into_iter().enumerate() {
            let colour = match ink {
                Ink::Axis(k) => axes.get(k).copied().unwrap_or(t.text),
                Ink::Text => t.text,
            };
            frame.fill_text(canvas::Text {
                content: s,
                position: Point::new(corner.x + 10.0, corner.y + 6.0 + i as f32 * (size + 4.0)),
                color: colour,
                size: Pixels(size),
                ..canvas::Text::default()
            });
        }
    }

    /// The reference plane: its outline dashed inside the model, a line from the port's face to
    /// it and its grip with the distance.
    fn reference(&self, frame: &mut Frame, on: &On, r: &RefPlane) {
        let t = self.tokens;
        let pts: Vec<Point> = r.at().iter().filter_map(|c| on.at(*c)).collect();
        if pts.len() == 4 {
            frame.fill(&polygon(&pts), Color { a: 0.18, ..t.accent });
            for k in 0..4 {
                dashed(frame, pts[k], pts[(k + 1) % 4], 1.6, t.accent);
            }
        }
        let (Some(a), Some(g)) = (on.at(emcraft_ui_core::scene3d::quad_centre(&r.face)), on.at(r.centre())) else { return };
        frame.stroke(&Path::line(a, g), stroke(1.2, t.accent));
        frame.fill(&Path::circle(g, 6.0), t.panel);
        frame.stroke(&Path::circle(g, 6.0), stroke(2.0, t.accent));
        frame.fill(&Path::circle(g, 2.5), t.accent);
        self.beside(frame, g, &r.label(self.length));
    }

    /// The selected body's grips: squares on its faces, arrows from its middle that move it in
    /// the axes' colours, a ring that turns it; the dragged one's value beside it.
    fn grips(&self, frame: &mut Frame, on: &On) {
        let t = self.tokens;
        let axes = [t.axis_x, t.axis_y, t.axis_z];
        for m in &self.grips {
            let Some(s) = on.at(m.at) else { continue };
            match m.grip {
                Grip::Move { axis } => {
                    let c = axes.get(axis).copied().unwrap_or(t.accent);
                    if let Some(f) = m.from.and_then(|f| on.at(f)) {
                        frame.stroke(&Path::line(f, s), stroke(2.0, c));
                    }
                    frame.fill(&Path::circle(s, 5.5), c);
                    frame.stroke(&Path::circle(s, 5.5), stroke(1.5, t.panel));
                }
                Grip::Turn => {
                    frame.stroke(&Path::circle(s, 9.0), stroke(2.0, t.accent));
                    frame.fill(&Path::circle(s, 3.0), t.accent);
                }
                _ => {
                    let square = Path::rounded_rectangle(Point::new(s.x - 4.5, s.y - 4.5), Size::new(9.0, 9.0), 2.0.into());
                    frame.fill(&square, t.panel);
                    frame.stroke(&square, stroke(2.0, t.accent));
                }
            }
            if let Some(label) = &m.label {
                self.beside(frame, s, label);
            }
        }
    }

    /// `label` on a ground beside the point `at` (above and to its right).
    fn beside(&self, frame: &mut Frame, at: Point, label: &str) {
        let t = self.tokens;
        let size = t.font_small;
        let ground = Size::new(label.chars().count() as f32 * size * CHAR_W + 10.0, size + 6.0);
        let corner = Point::new(at.x + 10.0, at.y - ground.height - 8.0);
        frame.fill(&Path::rounded_rectangle(corner, ground, 4.0.into()), Color { a: 0.9, ..t.panel });
        frame.fill_text(canvas::Text {
            content: label.to_owned(),
            position: Point::new(corner.x + 5.0, corner.y + 3.0),
            color: t.text,
            size: Pixels(size),
            ..canvas::Text::default()
        });
    }

    /// The band saying which tool is on, what it does and how to leave it.
    fn banner(&self, frame: &mut Frame, bounds: Size, tool: Tool) {
        let t = self.tokens;
        let text = format!("{}  ·  {}", tr(tool.hint()), tr(t!("Escape or a click on empty space ends it")));
        tip(frame, t, Point::new(bounds.width / 2.0, bounds.height - 62.0), &text, true);
    }
}

impl canvas::Program<Message> for Marks<'_> {
    type State = ();

    fn draw(&self, _: &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, _: Cursor) -> Vec<Geometry> {
        let t = self.tokens;
        let mut frame = Frame::new(renderer, bounds.size());
        let on = On { camera: self.camera, size: crate::geom::size(bounds.size()) };
        if let Some(r) = &self.reference {
            self.reference(&mut frame, &on, r);
        }
        self.grips(&mut frame, &on);
        if let (Some(tool), Some(h)) = (self.tool, &self.tools.hover) {
            self.hovered(&mut frame, &on, tool, h);
        }
        // The points picked, and a line from the last to the pointer.
        for q in self.tools.picks.iter().filter_map(|s| on.at(s.at)) {
            frame.fill(&Path::circle(q, 4.0), t.accent);
        }
        if let (Some(last), Some(h)) = (self.tools.picks.last(), &self.tools.hover)
            && let (Some(a), Some(b)) = (on.at(last.at), on.at(h.snap.at))
        {
            dashed(&mut frame, a, b, 1.5, t.accent);
        }
        if let Some(m) = &self.tools.measured {
            self.dimension(&mut frame, &on, m, bounds.width);
        }
        if let Some(tool) = self.tool {
            self.banner(&mut frame, bounds.size(), tool);
        }
        vec![frame.into_geometry()]
    }
}
