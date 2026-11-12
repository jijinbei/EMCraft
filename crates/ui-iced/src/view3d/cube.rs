//! The view cube in the 3D view's bottom-left corner, drawn as SolidCraft draws its navigation
//! cube (`assets/icons/ui/source/navigation.rs`, MIT OR Apache-2.0; ATTRIBUTION.md) from the
//! shared rules ([`emcraft_ui_core::cube`]): the patches filled as one opaque surface shaded by
//! their tilt, only the cube's own edges outlined, the target under the pointer (a face, or a
//! corner's three squares) lit and ringed in the accent, each face's name laid on it; round it,
//! while the pointer is near, the quarter-turn arrows, Home and Fit, and the axes' triad. A
//! click on a patch looks from it, a drag on the cube orbits the view (Shift snaps). A canvas
//! laid over the view: it takes what lands on the cube and its controls and lets the rest
//! through.

use emcraft_render::view3d::Camera;
use emcraft_ui_core::cube::{self, Control, DESIGN, Facet, HALF, Kind, SIZE};
use emcraft_ui_core::geom::Vector;
use emcraft_ui_core::lang::lookup;
use iced::mouse::{self, Cursor};
use iced::widget::canvas::{self, Action, Frame, Geometry, Path, Stroke};
use iced::{Color, Pixels, Point, Rectangle, Renderer, Theme, alignment, keyboard};

use super::Nav;
use crate::app::Message;
use crate::look::Tokens;

/// The cube's square's gap from the view's corner (points).
const MARGIN: f32 = 8.0;
/// How far a press may move before it is a drag (points).
const SLOP: f32 = 3.0;

/// The cube seen through `camera`, in the theme's tokens.
pub struct ViewCube {
    pub camera: Camera,
    pub tokens: Tokens,
}

/// The pointer between events: a press on the cube (where it was last, and whether it has
/// become an orbit), and Shift held.
#[derive(Default)]
pub struct Hold {
    pressed: Option<(Point, bool)>,
    shift: bool,
}

/// What the pointer is over: a patch (by its view) or a control.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Target {
    Patch([i8; 3]),
    Control(Control),
}

/// The colours the cube is drawn in: faded toward the view's ground while the pointer is away.
struct Inks {
    face: Color,
    edge: Color,
    text: Color,
    hover: Color,
    accent: Color,
    axes: [Color; 3],
}

fn mix(a: Color, b: Color, t: f32) -> Color {
    Color { r: a.r + (b.r - a.r) * t, g: a.g + (b.g - a.g) * t, b: a.b + (b.b - a.b) * t, a: 1.0 }
}

fn inks(t: Tokens, active: bool) -> Inks {
    let fade = |c: Color| if active { c } else { mix(c, t.viewport_top, 0.55) };
    Inks {
        face: fade(t.panel),
        edge: fade(t.border),
        text: fade(t.text),
        hover: t.accent_soft,
        accent: t.accent,
        axes: [fade(t.axis_x), fade(t.axis_y), fade(t.axis_z)],
    }
}

fn point(p: emcraft_ui_core::geom::Point) -> Point {
    Point::new(p.x, p.y)
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

impl ViewCube {
    /// The cube's middle in a view of `bounds` and the design's scale.
    fn place(bounds: Rectangle) -> (emcraft_ui_core::geom::Point, f32) {
        (emcraft_ui_core::geom::Point::new(MARGIN + SIZE / 2.0, bounds.height - MARGIN - SIZE / 2.0), SIZE / DESIGN)
    }

    fn facets(&self, bounds: Rectangle) -> Vec<Facet> {
        let (c, scale) = Self::place(bounds);
        cube::facets(&self.camera, c, HALF * scale)
    }

    /// What lies under the pointer.
    fn target(bounds: Rectangle, faces: &[Facet], cursor: Cursor) -> Option<Target> {
        let at = cursor.position_in(bounds)?;
        let shared = emcraft_ui_core::geom::Point::new(at.x, at.y);
        if let Some(i) = cube::hit(faces, shared) {
            return faces.get(i).map(|f| Target::Patch(f.dir));
        }
        let (c, scale) = Self::place(bounds);
        let design = Vector::new((at.x - c.x) / scale, (at.y - c.y) / scale);
        cube::control_at(&cube::controls(faces), design).map(Target::Control)
    }

    /// Whether the pointer is near enough for the controls to show.
    fn near(bounds: Rectangle, cursor: Cursor) -> bool {
        let (c, _) = Self::place(bounds);
        cursor.position_in(bounds).is_some_and(|p| (p.x - c.x).abs() <= SIZE * 0.6 && (p.y - c.y).abs() <= SIZE * 0.6)
    }

    /// The patches as one shaded surface, the cube's edges, the lit target and its rim, and the
    /// faces' names laid on them.
    fn paint(&self, frame: &mut Frame, faces: &[Facet], lit: Option<[i8; 3]>, k: &Inks, scale: f32) {
        let mut edges: Vec<(Point, Point, usize, bool)> = Vec::new();
        for f in faces {
            let pts: Vec<Point> = f.points.iter().copied().map(point).collect();
            let on = lit == Some(f.dir);
            let s = cube::shade(f.depth);
            let fill = if on { k.hover } else { Color { r: k.face.r * s, g: k.face.g * s, b: k.face.b * s, a: 1.0 } };
            frame.fill(&polygon(&pts), fill);
            for (j, outer) in f.outer.iter().enumerate() {
                if !on && !outer {
                    continue;
                }
                let (a, b) = (pts[j], pts[(j + 1) % pts.len()]);
                let same = |p: Point, q: Point| p.distance(q) < 1e-3;
                match edges.iter_mut().find(|(p, q, _, _)| (same(*p, a) && same(*q, b)) || (same(*p, b) && same(*q, a))) {
                    Some(e) => {
                        e.2 += usize::from(on);
                        e.3 |= outer;
                    }
                    None => edges.push((a, b, usize::from(on), *outer)),
                }
            }
        }
        // Two lit patches side by side share a seam, not the target's rim.
        for (a, b, lit, outer) in edges.into_iter().filter(|(_, _, lit, outer)| *outer || *lit == 1) {
            let ink = if lit > 0 && (lit == 1 || outer) { k.accent } else { k.edge };
            frame.stroke(&Path::line(a, b), Stroke::default().with_width(1.0).with_color(ink));
        }
        let size = self.tokens.font_heading * scale;
        for (f, (name, r, down)) in faces.iter().filter_map(|f| f.label.map(|l| (f, l))) {
            let opacity = cube::label_opacity(f.depth);
            let length = r.x.hypot(r.y);
            if opacity <= 0.0 || length < 1e-3 {
                continue;
            }
            // The text along the face's own axis, its height foreshortened as the face turns.
            let height = ((r.x * down.y - r.y * down.x) / length).abs() * 1.35;
            frame.with_save(|g| {
                g.translate(iced::Vector::new(f.centre.x, f.centre.y));
                g.rotate(r.y.atan2(r.x));
                g.scale_nonuniform(iced::Vector::new(length, height.max(0.05)));
                g.fill_text(canvas::Text {
                    content: lookup(name).to_owned(),
                    position: Point::ORIGIN,
                    color: Color { a: opacity, ..k.text },
                    size: Pixels(size),
                    align_x: iced::widget::text::Alignment::Center,
                    align_y: alignment::Vertical::Center,
                    ..canvas::Text::default()
                });
            });
        }
    }

    /// A control: an arrow, Home's house or Fit's ring, lit while the pointer is on it.
    fn control(frame: &mut Frame, c: Point, scale: f32, ctl: &Control, hover: bool, k: &Inks) {
        let at = Point::new(c.x + ctl.at.x * scale, c.y + ctl.at.y * scale);
        let (fill, rim) = if hover { (k.hover, k.accent) } else { (k.face, k.edge) };
        let stroke = Stroke::default().with_width(1.0).with_color(rim);
        match ctl.kind {
            Kind::Arrow(d) => {
                let (side, s) = (Vector::new(-d.y, d.x), scale);
                let p = |a: f32, b: f32| Point::new(at.x + (d.x * a + side.x * b) * s, at.y + (d.y * a + side.y * b) * s);
                let tip = polygon(&[p(7.0, 0.0), p(-5.0, 7.0), p(-5.0, -7.0)]);
                frame.fill(&tip, fill);
                frame.stroke(&tip, stroke);
            }
            Kind::Home => {
                let s = ctl.size * scale * 0.32;
                let p = |x: f32, y: f32| Point::new(at.x + x * s, at.y + y * s);
                let house = polygon(&[p(0.0, -1.1), p(1.1, -0.1), p(0.75, -0.1), p(0.75, 0.95), p(-0.75, 0.95), p(-0.75, -0.1), p(-1.1, -0.1)]);
                frame.fill(&house, fill);
                frame.stroke(&house, stroke);
            }
            Kind::Fit => {
                let ring = Path::circle(at, 6.0 * scale);
                frame.fill(&ring, fill);
                frame.stroke(&ring, stroke);
            }
        }
    }
}

impl canvas::Program<Message> for ViewCube {
    type State = Hold;

    fn update(&self, hold: &mut Hold, event: &iced::Event, bounds: Rectangle, cursor: Cursor) -> Option<Action<Message>> {
        match event {
            iced::Event::Keyboard(keyboard::Event::ModifiersChanged(m)) => {
                hold.shift = m.shift();
                None
            }
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let faces = self.facets(bounds);
                let target = Self::target(bounds, &faces, cursor)?;
                let at = cursor.position_in(bounds)?;
                hold.pressed = Some((at, false));
                let orbit = matches!(target, Target::Patch(_)).then(|| {
                    let from = emcraft_ui_core::geom::Point::new(at.x, at.y);
                    Message::View3d(Nav::OrbitFrom(from, emcraft_ui_core::navigation::OrbitAround::ViewCentre))
                });
                Some(orbit.map_or_else(Action::capture, |m| Action::publish(m).and_capture()))
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { position }) => {
                let (last, dragging) = hold.pressed?;
                let at = Point::new(position.x - bounds.x, position.y - bounds.y);
                if !dragging && last.distance(at) <= SLOP {
                    return Some(Action::capture());
                }
                hold.pressed = Some((at, true));
                let (dx, dy) = (at.x - last.x, at.y - last.y);
                let nav = if hold.shift { Nav::OrbitSnapped(dx, dy) } else { Nav::Orbit(dx, dy) };
                Some(Action::publish(Message::View3d(nav)).and_capture())
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                let (_, dragging) = hold.pressed.take()?;
                if dragging {
                    return Some(Action::request_redraw().and_capture());
                }
                let faces = self.facets(bounds);
                let goal = match Self::target(bounds, &faces, cursor)? {
                    Target::Patch(dir) => cube::goal(dir),
                    Target::Control(c) => c.goal,
                };
                Some(Action::publish(Message::View3d(Nav::Goal(goal))).and_capture())
            }
            _ => None,
        }
    }

    fn draw(&self, hold: &Hold, renderer: &Renderer, _: &Theme, bounds: Rectangle, cursor: Cursor) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let faces = self.facets(bounds);
        let dragging = hold.pressed.is_some_and(|(_, d)| d);
        let active = dragging || Self::near(bounds, cursor);
        let k = inks(self.tokens, active);
        let target = if dragging { None } else { Self::target(bounds, &faces, cursor) };
        let lit = match target {
            Some(Target::Patch(dir)) => Some(dir),
            _ => None,
        };
        let (c, scale) = Self::place(bounds);
        self.paint(&mut frame, &faces, lit, &k, scale);
        let c = point(c);
        if active {
            for ctl in cube::controls(&faces) {
                let hover = matches!(target, Some(Target::Control(t)) if t.name == ctl.name);
                Self::control(&mut frame, c, scale, &ctl, hover, &k);
            }
        }
        // The axes' triad below the cube.
        let (origin, tips) = cube::triad(&self.camera);
        let o = Point::new(c.x + origin.x * scale, c.y + origin.y * scale);
        for ((tip, ink), name) in tips.iter().zip(k.axes).zip(["X", "Y", "Z"]) {
            let end = Point::new(o.x + tip.x * scale, o.y + tip.y * scale);
            frame.stroke(&Path::line(o, end), Stroke::default().with_width(1.5).with_color(ink));
            let n = tip.x.hypot(tip.y).max(1e-3);
            frame.fill_text(canvas::Text {
                content: name.to_owned(),
                position: Point::new(end.x + tip.x / n * 5.0, end.y + tip.y / n * 5.0),
                color: ink,
                size: Pixels(self.tokens.font_small * 0.85),
                align_x: iced::widget::text::Alignment::Center,
                align_y: alignment::Vertical::Center,
                ..canvas::Text::default()
            });
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(&self, hold: &Hold, bounds: Rectangle, cursor: Cursor) -> mouse::Interaction {
        if hold.pressed.is_some_and(|(_, d)| d) {
            return mouse::Interaction::Grabbing;
        }
        match Self::target(bounds, &self.facets(bounds), cursor) {
            Some(_) => mouse::Interaction::Pointer,
            None => mouse::Interaction::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use emcraft_render::view3d::StandardView;
    use iced::widget::canvas::Program;

    /// From the front the cube's middle is the FRONT face: a click there looks from +z, a
    /// press and drag orbits instead, and a press away from the cube and its controls is left to
    /// the view under it.
    #[test]
    fn a_click_looks_from_a_patch_and_a_drag_orbits() {
        let mut camera = Camera::default();
        camera.standard(StandardView::Front);
        let tokens = crate::app::App::new(None).look.tokens;
        let view = ViewCube { camera, tokens };
        let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(800.0, 600.0));
        let (c, _) = ViewCube::place(bounds);
        let middle = Cursor::Available(Point::new(c.x, c.y));
        let faces = view.facets(bounds);
        assert_eq!(ViewCube::target(bounds, &faces, middle), Some(Target::Patch([0, 0, 1])));
        let mut hold = Hold::default();
        let press = iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        let release = iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
        assert!(view.update(&mut hold, &press, bounds, middle).is_some());
        assert!(view.update(&mut hold, &release, bounds, middle).is_some());
        assert!(hold.pressed.is_none());
        let away = Cursor::Available(Point::new(400.0, 300.0));
        assert!(view.update(&mut hold, &press, bounds, away).is_none());
        assert_eq!(view.mouse_interaction(&hold, bounds, away), mouse::Interaction::None);
        // Pressed and moved past the slop: an orbit.
        view.update(&mut hold, &press, bounds, middle);
        let moved = iced::Event::Mouse(mouse::Event::CursorMoved { position: Point::new(c.x + 20.0, c.y) });
        assert!(view.update(&mut hold, &moved, bounds, middle).is_some());
        assert_eq!(hold.pressed.map(|(_, d)| d), Some(true));
    }
}
