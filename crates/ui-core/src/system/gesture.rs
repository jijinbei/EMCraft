//! What the pointer is doing on a system's canvas — panning, moving a node, dragging a link — as
//! a value the window keeps between frames, the inputs that change it and the command a gesture
//! ends in. [`Canvas::step`] is pure: the state and an input in, the next state and the command
//! out.

use serde_json::{Value, json};

use super::layout::Layout;
use crate::geom::{Point, Vector};

/// How near the pointer must come to an input port to drop a link on it (points).
const PORT_REACH: f32 = 16.0;

/// Where the canvas starts panned, so the source's card clears its edge (points).
const START_PAN: Vector = Vector::new(16.0, 16.0);

/// A command a gesture ends in: its id and parameters, which the window runs.
#[derive(Clone, Debug, PartialEq)]
pub struct Command {
    pub id: &'static str,
    pub params: Value,
}

/// A gesture under way.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Gesture {
    #[default]
    Idle,
    Pan,
    /// A node, by name, and how far it has been dragged.
    Move {
        name: String,
        moved: Vector,
    },
    /// A link from a node's output, by name.
    Link {
        from: String,
    },
}

/// What a window reports of the pointer: a press on a node's header or output port or on the
/// empty canvas, a drag, a release.
#[derive(Clone, Debug, PartialEq)]
pub enum Input {
    Grab(String),
    StartLink(String),
    PressEmpty,
    Drag(Vector),
    Release(Point),
}

/// The canvas between frames: its pan and the gesture under way.
#[derive(Clone, Debug, PartialEq)]
pub struct Canvas {
    pub pan: Vector,
    pub gesture: Gesture,
}

impl Default for Canvas {
    fn default() -> Canvas {
        Canvas { pan: START_PAN, gesture: Gesture::Idle }
    }
}

/// The command a gesture ends in when let go at `p`: a node moved there, a link dropped on an
/// input port; nothing for a pan or a link dropped elsewhere.
fn ended(gesture: Gesture, p: Point, layout: &Layout) -> Option<Command> {
    match gesture {
        Gesture::Move { name, .. } => {
            let at = layout.node(&name)?.rect.min - layout.origin;
            Some(Command { id: "system.stage.edit", params: json!({"stage": name, "at": [at.x.round(), at.y.round()]}) })
        }
        Gesture::Link { from } => {
            let to = layout.input_near(p, PORT_REACH, &from)?;
            Some(Command { id: "system.link", params: json!({"from": from, "to": to.name}) })
        }
        Gesture::Idle | Gesture::Pan => None,
    }
}

impl Canvas {
    /// The node being moved and by how much.
    pub fn moving(&self) -> Option<(&str, Vector)> {
        match &self.gesture {
            Gesture::Move { name, moved } => Some((name, *moved)),
            _ => None,
        }
    }

    /// The node a link is being dragged from.
    pub fn linking(&self) -> Option<&str> {
        match &self.gesture {
            Gesture::Link { from } => Some(from),
            _ => None,
        }
    }

    /// The canvas after `input` on `layout`, and the command the gesture ends in, if it ends.
    ///
    /// ```
    /// use emcraft_model::system::{Chain, Part, Stage};
    /// use emcraft_ui_core::geom::{Point, Vector};
    /// use emcraft_ui_core::system::gesture::{Canvas, Input};
    /// use emcraft_ui_core::system::layout::Layout;
    /// let pad = Stage { name: "pad".into(), part: Part::Attenuator { loss: 3.0 }, at: None };
    /// let chain = Chain { stages: vec![pad], ..Chain::default() };
    /// let layout = Layout::of(&chain, Point::ORIGIN, None);
    /// // The empty canvas dragged pans it, and ends in no command.
    /// let (c, _) = Canvas::default().step(Input::PressEmpty, &layout);
    /// let (c, _) = c.step(Input::Drag(Vector::new(5.0, 0.0)), &layout);
    /// assert_eq!(c.step(Input::Release(Point::ORIGIN), &layout), (Canvas { pan: Vector::new(21.0, 16.0), ..Canvas::default() }, None));
    /// ```
    pub fn step(self, input: Input, layout: &Layout) -> (Canvas, Option<Command>) {
        let Canvas { pan, gesture } = self;
        let next = |gesture| (Canvas { pan, gesture }, None);
        match (input, gesture) {
            (Input::Grab(name), _) => next(Gesture::Move { name, moved: Vector::ZERO }),
            (Input::StartLink(from), _) => next(Gesture::Link { from }),
            (Input::PressEmpty, Gesture::Idle) => next(Gesture::Pan),
            (Input::Drag(d), Gesture::Pan) => (Canvas { pan: pan + d, gesture: Gesture::Pan }, None),
            (Input::Drag(d), Gesture::Move { name, moved }) => next(Gesture::Move { name, moved: moved + d }),
            (Input::Release(p), gesture) => (Canvas { pan, gesture: Gesture::Idle }, ended(gesture, p, layout)),
            (_, gesture) => next(gesture),
        }
    }
}

impl Canvas {
    /// The canvas after `inputs` in turn, and the commands their gestures end in.
    pub fn steps(self, inputs: impl IntoIterator<Item = Input>, layout: &Layout) -> (Canvas, Vec<Command>) {
        inputs.into_iter().fold((self, Vec::new()), |(canvas, mut done), input| {
            let (canvas, command) = canvas.step(input, layout);
            done.extend(command);
            (canvas, done)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use emcraft_model::system::{Part, RECEIVER, Stage};

    fn chain() -> emcraft_model::system::Chain {
        let mut m = emcraft_model::Model::default();
        for n in ["a", "b"] {
            m.add_stage(9, Stage { name: n.into(), part: Part::Attenuator { loss: 1.0 }, at: None }).unwrap();
        }
        m.system
    }

    /// Gestures without a frame: the layout from the model, the inputs as a window reports them.
    #[test]
    fn a_node_dragged_moves_there_and_a_link_dropped_on_a_port_links() {
        let c = chain();
        let layout = Layout::of(&c, Point::ORIGIN, None);
        let (s, none) = Canvas::default().step(Input::Grab("a".into()), &layout);
        let (s, _) = s.step(Input::Drag(Vector::new(30.0, 10.0)), &layout);
        assert_eq!(none, None);
        let moved = Layout::of(&c, Point::ORIGIN, s.moving());
        let at = moved.node("a").unwrap().rect.min;
        let (s, done) = s.step(Input::Release(at), &moved);
        assert_eq!(done, Some(Command { id: "system.stage.edit", params: json!({"stage": "a", "at": [at.x, at.y]}) }));
        assert_eq!(s.gesture, Gesture::Idle);
        // A link from b's output dropped on the receiver's input; dropped nowhere, nothing.
        let to = layout.node(RECEIVER).unwrap().input();
        let (s, _) = s.step(Input::StartLink("b".into()), &layout);
        let (s, linked) = s.step(Input::Release(to + Vector::new(3.0, 2.0)), &layout);
        assert_eq!(linked, Some(Command { id: "system.link", params: json!({"from": "b", "to": RECEIVER}) }));
        let (s, _) = s.step(Input::StartLink("b".into()), &layout);
        assert_eq!(s.step(Input::Release(Point::new(-500.0, -500.0)), &layout).1, None);
    }
}
