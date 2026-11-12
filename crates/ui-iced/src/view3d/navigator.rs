//! The axes navigator in the 3D view's bottom-left corner, as the shared rules lay it out
//! ([`emcraft_ui_core::navigation::navigator`]): the six signed axes as tips round a disc, the
//! positive ones filled in their axis's colour with its letter, the negative ones rings; a click
//! on one turns the view to look from that side. A canvas laid over the view: it takes the clicks
//! on its tips and lets every other event through.

use emcraft_render::view3d::Camera;
use emcraft_ui_core::navigation::{Goal, NAVIGATOR_TIP, navigator, tip_at};
use iced::mouse::{self, Cursor};
use iced::widget::canvas::{self, Action, Frame, Geometry, Path, Stroke};
use iced::{Color, Pixels, Point, Rectangle, Renderer, Theme, alignment};

use super::Nav;
use crate::app::Message;
use crate::look::Tokens;

/// The navigator's middle, from the view's bottom-left corner (points), and its disc's radius.
const INSET: f32 = 52.0;
const DISC: f32 = 44.0;

/// The navigator seen through `camera`, in the theme's tokens.
pub struct Navigator {
    pub camera: Camera,
    pub tokens: Tokens,
}

/// The navigator's middle in a view of `bounds`.
fn middle(bounds: Rectangle) -> Point {
    Point::new(INSET, bounds.height - INSET)
}

impl Navigator {
    /// The tip under the pointer, if any.
    fn under(&self, bounds: Rectangle, cursor: Cursor) -> Option<emcraft_ui_core::navigation::Tip> {
        let at = cursor.position_in(bounds)?;
        let c = middle(bounds);
        tip_at(&navigator(&self.camera), emcraft_ui_core::geom::Vector::new(at.x - c.x, at.y - c.y))
    }
}

impl canvas::Program<Message> for Navigator {
    type State = ();

    fn update(&self, _: &mut (), event: &iced::Event, bounds: Rectangle, cursor: Cursor) -> Option<Action<Message>> {
        match event {
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let tip = self.under(bounds, cursor)?;
                Some(Action::publish(Message::View3d(Nav::Goal(Goal::View(tip.view)))).and_capture())
            }
            _ => None,
        }
    }

    fn draw(&self, _: &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, cursor: Cursor) -> Vec<Geometry> {
        let t = self.tokens;
        let mut frame = Frame::new(renderer, bounds.size());
        let c = middle(bounds);
        frame.fill(&Path::circle(c, DISC), Color { a: 0.75, ..t.panel });
        let hovered = self.under(bounds, cursor);
        let axes = [t.axis_x, t.axis_y, t.axis_z];
        for tip in navigator(&self.camera) {
            let at = Point::new(c.x + tip.offset.x, c.y + tip.offset.y);
            let colour = axes.get(tip.axis).copied().unwrap_or(t.accent);
            let hover = hovered == Some(tip);
            if tip.positive {
                frame.stroke(&Path::line(c, at), Stroke::default().with_width(2.0).with_color(colour));
                frame.fill(&Path::circle(at, if hover { NAVIGATOR_TIP + 1.0 } else { NAVIGATOR_TIP }), colour);
                frame.fill_text(canvas::Text {
                    content: tip.name().chars().skip(1).collect(),
                    position: at,
                    color: t.axis_label,
                    size: Pixels(t.font_small),
                    align_x: iced::widget::text::Alignment::Center,
                    align_y: alignment::Vertical::Center,
                    ..canvas::Text::default()
                });
            } else {
                frame.fill(&Path::circle(at, 7.0), t.panel);
                frame.stroke(&Path::circle(at, if hover { 8.0 } else { 7.0 }), Stroke::default().with_width(2.0).with_color(colour));
            }
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(&self, _: &(), bounds: Rectangle, cursor: Cursor) -> mouse::Interaction {
        match self.under(bounds, cursor) {
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

    /// From the front, the +x tip stands 30 points right of the middle and turns the view to
    /// look from the right; a press away from the tips is left to the view under it.
    #[test]
    fn a_tip_turns_the_view_from_its_side() {
        let mut camera = Camera::default();
        camera.standard(StandardView::Front);
        let tokens = crate::app::App::new(None).look.tokens;
        let nav = Navigator { camera, tokens };
        let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(800.0, 600.0));
        let tip = nav.under(bounds, Cursor::Available(Point::new(INSET + 30.0, 600.0 - INSET)));
        assert_eq!(tip.map(|t| (t.axis, t.positive, t.view)), Some((0, true, StandardView::Right)));
        let press = iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        assert!(nav.update(&mut (), &press, bounds, Cursor::Available(Point::new(INSET + 30.0, 600.0 - INSET))).is_some());
        assert!(nav.update(&mut (), &press, bounds, Cursor::Available(Point::new(400.0, 300.0))).is_none());
        assert_eq!(nav.mouse_interaction(&(), bounds, Cursor::Available(Point::new(400.0, 300.0))), mouse::Interaction::None);
    }
}
