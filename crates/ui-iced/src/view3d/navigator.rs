//! The axes navigator at the 3D view's top right, drawn as SolidCraft draws its signed-axes
//! compass (`crates/ui-egui/src/viewport.rs` and `assets/icons/ui/source/navigation.rs`, MIT OR
//! Apache-2.0; ATTRIBUTION.md) from the shared measures ([`emcraft_ui_core::navigation`]): a
//! disc, faint until the pointer is in it; the six signed axes round its middle, farthest first —
//! a positive end a disc in its axis's colour with its letter, a negative end a ring, a shaft
//! from the middle to each, dimmer for the negative ends and those pointing away; the origin's
//! dot between the three back axes and the three front ones; a tip under the pointer white. A
//! click on a tip looks from its side (again: from the other side), a drag anywhere on the
//! navigator orbits the view (Shift snaps), and the home chip up and left of it turns to the home
//! view. A canvas laid over the view: it takes what lands on the navigator and lets the rest
//! through.

use emcraft_render::view3d::Camera;
use emcraft_ui_core::glyph::Glyph;
use emcraft_ui_core::navigation::{
    Goal, NAVIGATOR_BOX, NAVIGATOR_DISC, NAVIGATOR_HOME, NAVIGATOR_HOME_R, NAVIGATOR_LIFT, Tip, navigator, tip_at, tip_goal,
};
use iced::mouse::{self, Cursor};
use iced::widget::canvas::{self, Action, Frame, Geometry, Path, Stroke};
use iced::{Color, Font, Pixels, Point, Rectangle, Renderer, Size, Theme, alignment, font, keyboard};

use super::Nav;
use crate::app::Message;
use crate::look::Tokens;

/// A shaft's width, a positive end's disc and a negative end's ring (radius, its line's width),
/// the origin's dot, a letter's size and the home glyph's side (points), as SolidCraft's.
const SHAFT: f32 = 3.6;
const DISC_TIP: f32 = 11.0;
const RING_TIP: f32 = 4.8;
const RING_LINE: f32 = 2.4;
const ORIGIN: f32 = 3.6;
const LETTER: f32 = 13.5;
const HOME_SIDE: f32 = 18.0;
/// How far a press may move before it is a drag (points).
const SLOP: f32 = 3.0;

/// The navigator seen through `camera`, in the theme's tokens.
pub struct Navigator {
    pub camera: Camera,
    pub tokens: Tokens,
}

/// The pointer between events: a press on the navigator (where it was last, and whether it has
/// become an orbit), and Shift held.
#[derive(Default)]
pub struct Hold {
    pressed: Option<(Point, bool)>,
    shift: bool,
}

/// What the pointer is over: a tip or the home chip.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Target {
    Tip(Tip),
    Home,
}

/// The navigator's middle in a view of `bounds`: its square's at the top right, a little higher.
fn middle(bounds: Rectangle) -> Point {
    Point::new(bounds.width - NAVIGATOR_BOX / 2.0, NAVIGATOR_BOX / 2.0 - NAVIGATOR_LIFT)
}

/// The home chip's middle.
fn home(bounds: Rectangle) -> Point {
    let c = middle(bounds);
    Point::new(c.x - NAVIGATOR_HOME, c.y - NAVIGATOR_HOME)
}

/// `c` at `alpha` of its opacity.
fn faded(c: Color, alpha: f32) -> Color {
    Color { a: c.a * alpha, ..c }
}

impl Navigator {
    /// Whether `at` (points in the view) lies in the navigator's square.
    fn owns(bounds: Rectangle, at: Point) -> bool {
        at.x >= bounds.width - NAVIGATOR_BOX && at.x <= bounds.width && at.y >= 0.0 && at.y <= NAVIGATOR_BOX
    }

    /// What the pointer is over, if anything.
    fn under(&self, bounds: Rectangle, cursor: Cursor) -> Option<Target> {
        let at = cursor.position_in(bounds)?;
        if at.distance(home(bounds)) <= NAVIGATOR_HOME_R {
            return Some(Target::Home);
        }
        let c = middle(bounds);
        tip_at(&navigator(&self.camera), emcraft_ui_core::geom::Vector::new(at.x - c.x, at.y - c.y)).map(Target::Tip)
    }

    /// A tip: its shaft from the middle `c`, its disc and letter (positive) or ring (negative),
    /// white while `hovered`.
    fn tip(&self, frame: &mut Frame, c: Point, tip: &Tip, hovered: bool) {
        let t = self.tokens;
        let colour = [t.axis_x, t.axis_y, t.axis_z].get(tip.axis).copied().unwrap_or(t.accent);
        let at = Point::new(c.x + tip.offset.x, c.y + tip.offset.y);
        let light = t.panel.r > 0.5;
        // A negative shaft, and any end pointing away from the eye, a dimmed tint.
        let ink = if !tip.positive || tip.toward < -0.01 { faded(colour, 0.55) } else { colour };
        let radius = if tip.positive { DISC_TIP } else { RING_TIP };
        let length = tip.offset.x.hypot(tip.offset.y);
        // A positive shaft runs under its disc; a negative one stops at its ring's outer edge.
        let stop = if tip.positive { radius } else { radius + RING_LINE / 2.0 };
        let end = if length > stop {
            let k = (length - stop) / length;
            Point::new(c.x + tip.offset.x * k, c.y + tip.offset.y * k)
        } else {
            c
        };
        frame.stroke(&Path::line(c, end), Stroke::default().with_width(SHAFT).with_color(ink));
        frame.fill(&Path::circle(c, SHAFT / 2.0), ink);
        if tip.positive {
            frame.fill(&Path::circle(at, radius), colour);
        }
        if hovered {
            frame.fill(&Path::circle(at, if tip.positive { radius } else { radius + RING_LINE / 2.0 }), t.on_accent);
            if light && tip.positive {
                frame.stroke(&Path::circle(at, radius - 1.0), Stroke::default().with_width(2.0).with_color(colour));
            }
        }
        if tip.positive {
            frame.fill_text(canvas::Text {
                content: ["X", "Y", "Z"][tip.axis.min(2)].to_owned(),
                position: at,
                color: if hovered { colour } else { t.on_accent },
                size: Pixels(LETTER),
                font: Font { weight: font::Weight::Bold, ..Font::default() },
                align_x: iced::widget::text::Alignment::Center,
                align_y: alignment::Vertical::Center,
                ..canvas::Text::default()
            });
        } else {
            let line = match (hovered, light) {
                (false, _) => ink,
                (true, true) => colour,
                (true, false) => t.on_accent,
            };
            frame.stroke(&Path::circle(at, radius), Stroke::default().with_width(RING_LINE).with_color(line));
        }
    }
}

impl canvas::Program<Message> for Navigator {
    type State = Hold;

    fn update(&self, hold: &mut Hold, event: &iced::Event, bounds: Rectangle, cursor: Cursor) -> Option<Action<Message>> {
        match event {
            iced::Event::Keyboard(keyboard::Event::ModifiersChanged(m)) => {
                hold.shift = m.shift();
                None
            }
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let at = cursor.position_in(bounds).filter(|at| Self::owns(bounds, *at) || at.distance(home(bounds)) <= NAVIGATOR_HOME_R)?;
                hold.pressed = Some((at, false));
                // A drag turns about the view's centre.
                let from = emcraft_ui_core::geom::Point::new(at.x, at.y);
                let orbit = Message::View3d(Nav::OrbitFrom(from, emcraft_ui_core::navigation::OrbitAround::ViewCentre));
                Some(Action::publish(orbit).and_capture())
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { position }) => {
                // Unpressed, the disc lights as the pointer comes over it and fades as it leaves.
                let Some((last, dragging)) = hold.pressed else { return Some(Action::request_redraw()) };
                let at = Point::new(position.x - bounds.x, position.y - bounds.y);
                if !dragging && last.distance(at) <= SLOP {
                    return Some(Action::request_redraw().and_capture());
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
                let goal = match self.under(bounds, cursor) {
                    Some(Target::Tip(tip)) => tip_goal(&self.camera, &tip),
                    Some(Target::Home) => Goal::Home,
                    None => return Some(Action::request_redraw().and_capture()),
                };
                Some(Action::publish(Message::View3d(Nav::Goal(goal))).and_capture())
            }
            _ => None,
        }
    }

    fn draw(&self, hold: &Hold, renderer: &Renderer, _: &Theme, bounds: Rectangle, cursor: Cursor) -> Vec<Geometry> {
        let t = self.tokens;
        let mut frame = Frame::new(renderer, bounds.size());
        let c = middle(bounds);
        let dragging = hold.pressed.is_some_and(|(_, d)| d);
        let at = cursor.position_in(bounds);
        let inside = at.is_some_and(|p| p.distance(c) <= NAVIGATOR_DISC);
        let fill = if inside || dragging { 1.0 } else { 0.35 };
        let disc = Path::circle(c, NAVIGATOR_DISC);
        frame.fill(&disc, faded(t.panel, 0.94 * fill));
        frame.stroke(&disc, Stroke::default().with_width(1.0).with_color(faded(t.border, 0.75)));
        let target = if dragging { None } else { self.under(bounds, cursor) };
        for (i, tip) in navigator(&self.camera).iter().enumerate() {
            if i == 3 {
                // The origin's dot behind the three front axes and over the three back ones.
                frame.fill(&Path::circle(c, ORIGIN), faded(t.text, 0.9));
                frame.stroke(&Path::circle(c, ORIGIN), Stroke::default().with_width(1.4).with_color(t.panel));
            }
            self.tip(&mut frame, c, tip, target == Some(Target::Tip(*tip)));
        }
        // The home chip, in its own small disc.
        let h = home(bounds);
        let chip = Path::circle(h, NAVIGATOR_HOME_R);
        frame.fill(&chip, faded(t.panel, 0.94 * if target == Some(Target::Home) { 0.9 } else { 0.5 }));
        frame.stroke(&chip, Stroke::default().with_width(1.0).with_color(t.border));
        let area = Rectangle::new(Point::new(h.x - HOME_SIDE / 2.0, h.y - HOME_SIDE / 2.0), Size::new(HOME_SIDE, HOME_SIDE));
        crate::icons::Icon { glyph: Glyph::House, ink: t.icon, accent: t.accent }.paint(&mut frame, area);
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(&self, hold: &Hold, bounds: Rectangle, cursor: Cursor) -> mouse::Interaction {
        if hold.pressed.is_some_and(|(_, d)| d) {
            return mouse::Interaction::Grabbing;
        }
        match (self.under(bounds, cursor), cursor.position_in(bounds)) {
            (Some(_), _) => mouse::Interaction::Pointer,
            (None, Some(at)) if at.distance(middle(bounds)) <= NAVIGATOR_DISC => mouse::Interaction::Grab,
            _ => mouse::Interaction::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use emcraft_render::view3d::StandardView;
    use iced::widget::canvas::Program;

    /// From the front, the +x tip stands 31.92 points right of the middle (SolidCraft's 38 at
    /// 0.84) and a click on it looks from the right; a click on the front's own tip looks from
    /// the back; the home chip turns home; a press away from the navigator is the view's.
    #[test]
    fn a_tip_turns_the_view_from_its_side() {
        let mut camera = Camera::default();
        camera.standard(StandardView::Front);
        let tokens = crate::app::App::new(None).look.tokens;
        let nav = Navigator { camera, tokens };
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(800.0, 600.0));
        let c = middle(bounds);
        let press = iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        let release = iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
        let click = |at: Point| {
            let mut hold = Hold::default();
            let cursor = Cursor::Available(at);
            let pressed = nav.update(&mut hold, &press, bounds, cursor);
            (pressed.is_some(), nav.update(&mut hold, &release, bounds, cursor).and_then(|a| a.into_inner().0))
        };
        let right = Point::new(c.x + 31.92, c.y);
        assert!(matches!(click(right), (true, Some(Message::View3d(Nav::Goal(Goal::View(StandardView::Right)))))));
        assert!(matches!(click(c), (true, Some(Message::View3d(Nav::Goal(Goal::View(StandardView::Back)))))));
        assert!(matches!(click(home(bounds)), (true, Some(Message::View3d(Nav::Goal(Goal::Home))))));
        assert!(matches!(click(Point::new(400.0, 300.0)), (false, None)));
        assert_eq!(nav.mouse_interaction(&Hold::default(), bounds, Cursor::Available(Point::new(400.0, 300.0))), mouse::Interaction::None);
    }
}
