//! The canvas: the ground, the links and the ports drawn under the cards (a canvas program that
//! also reports the pointer), each card pinned where the layout puts it, a mark on each link
//! that takes it away and a grip on each output that starts a link.

use emcraft_ui_core::geom::{Point, Vector};
use emcraft_ui_core::system::gesture::Command;
use emcraft_ui_core::system::layout::{Kind, Layout};
use iced::mouse::{self, Cursor};
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke};
use iced::widget::{Stack, container, mouse_area, pin, text};
use iced::{Element, Event, Fill, Rectangle, Renderer, Theme, Widget};
use serde_json::json;

use super::card;
use crate::app::{App, Message, Pointer};
use crate::geom::{point, shared};
use crate::look::{self, Tokens};

/// The grid's spacing, a port's radius and the room around an output the pointer may press
/// (points).
const GRID: f32 = 24.0;
const PORT_R: f32 = 6.0;
const PORT_REACH: f32 = 12.0;

/// The canvas' least and greatest height (points).
const CANVAS_MIN: f32 = 220.0;
const CANVAS_MAX: f32 = 520.0;

/// What the canvas draws from: the layout, the pan, and while a link is dragged its start and
/// the pointer.
struct Ground {
    layout: Layout,
    tokens: Tokens,
    pan: Vector,
    dragging: Option<(Point, Point)>,
}

/// A link from an output `a` to an input `b`, bowed out horizontally.
fn curve(a: Point, b: Point) -> Path {
    let bow = ((b.x - a.x).abs() * 0.5).max(40.0);
    Path::new(|p| {
        p.move_to(point(a));
        p.bezier_curve_to(point(a + Vector::new(bow, 0.0)), point(b - Vector::new(bow, 0.0)), point(b));
    })
}

impl canvas::Program<Message> for Ground {
    type State = ();

    /// The pointer as the gestures read it: a press on the canvas, and every move and release
    /// (outside it too, so a drag let go there ends).
    fn update(&self, _: &mut (), event: &Event, bounds: Rectangle, cursor: Cursor) -> Option<canvas::Action<Message>> {
        let at = cursor.position().map(|p| shared(p) - shared(bounds.position()));
        let at = Point::ORIGIN + at?;
        let pointer = match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) if cursor.is_over(bounds) => Pointer::Press(at),
            Event::Mouse(mouse::Event::CursorMoved { .. }) => Pointer::Move(at),
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => Pointer::Release(at),
            _ => return None,
        };
        Some(canvas::Action::publish(Message::Pointer(pointer)))
    }

    fn draw(&self, _: &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, _: Cursor) -> Vec<Geometry> {
        let t = &self.tokens;
        let mut frame = Frame::new(renderer, bounds.size());
        // Every GRID points from the first line inside the canvas, moved with the pan.
        let lines = |shift: f32, to: f32| (0..).map(move |k| shift.rem_euclid(GRID) + k as f32 * GRID).take_while(move |x| *x < to);
        let dot = t.border;
        for y in lines(self.pan.y, bounds.height) {
            for x in lines(self.pan.x, bounds.width) {
                frame.fill(&Path::circle(iced::Point::new(x, y), 0.8), dot);
            }
        }
        for &(a, b) in &self.layout.links {
            let (Some(from), Some(to)) = (self.layout.nodes.get(a), self.layout.nodes.get(b)) else { continue };
            let ink = if from.on_chain && to.on_chain { t.accent } else { t.text_dim };
            frame.stroke(&curve(from.output(), to.input()), Stroke::default().with_width(2.0).with_color(ink));
        }
        if let Some((a, b)) = self.dragging {
            frame.stroke(&curve(a, b), Stroke::default().with_width(2.0).with_color(t.accent));
        }
        let accent = t.accent;
        for n in &self.layout.nodes {
            if n.kind != Kind::Source {
                frame.fill(&Path::circle(point(n.input()), PORT_R), t.panel);
                frame.stroke(&Path::circle(point(n.input()), PORT_R), Stroke::default().with_width(1.5).with_color(accent));
            }
            if n.kind != Kind::Receiver {
                frame.fill(&Path::circle(point(n.output()), PORT_R), accent);
            }
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(&self, _: &(), bounds: Rectangle, cursor: Cursor) -> mouse::Interaction {
        if self.dragging.is_some() && cursor.is_over(bounds) { mouse::Interaction::Crosshair } else { mouse::Interaction::default() }
    }
}

/// The canvas with its cards, marks and grips over it.
pub(super) fn board(app: &App) -> Element<'_, Message> {
    let layout = app.layout();
    let t = app.look.tokens;
    let height = (layout.depth() + 56.0).clamp(CANVAS_MIN, CANVAS_MAX);
    let dragging = app.canvas.linking().and_then(|from| Some((layout.node(from)?.output(), app.pointer?)));
    let marks = layout.links.iter().filter_map(|&(a, b)| {
        let (from, to) = (layout.nodes.get(a)?, layout.nodes.get(b)?);
        let mid = from.output().lerp(to.input(), 0.5);
        let unlink = Command { id: "system.unlink", params: json!({"from": from.name, "to": to.name}) };
        let mark = crate::parts::close(t, t!("Unlink"), Message::Run(unlink));
        Some(pin(mark).x(mid.x - 8.0).y(mid.y - 9.0).boxed())
    });
    let cards = layout.nodes.iter().map(|n| pin(card::card(app, n)).x(n.rect.left()).y(n.rect.top()).boxed());
    let grips = layout.nodes.iter().filter(|n| n.kind != Kind::Receiver).map(|n| {
        let grip = mouse_area(container(text("")).width(2.0 * PORT_REACH).height(2.0 * PORT_REACH))
            .on_press(Message::StartLink(n.name.clone()))
            .interaction(mouse::Interaction::Crosshair);
        let at = n.output();
        pin(grip).x(at.x - PORT_REACH).y(at.y - PORT_REACH).boxed()
    });
    let ground = canvas::Canvas::new(Ground { layout: layout.clone(), tokens: t, pan: app.canvas.pan, dragging }).width(Fill).height(Fill);
    let layers = std::iter::once(ground.boxed()).chain(cards).chain(marks).chain(grips);
    container(Stack::with_children(layers.collect::<Vec<Element<Message>>>())).width(Fill).height(height).clip(true).style(look::ground(t)).boxed()
}
