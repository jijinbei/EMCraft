//! What the pointer is doing on the canvas — panning, moving a node, dragging a link — as a
//! state kept between frames, the inputs that change it and the command a gesture ends in.

use egui::{Pos2, Vec2, vec2};
use serde_json::json;

use super::layout::Layout;
use crate::Action;

/// How near the pointer must come to an input port to drop a link on it.
const PORT_REACH: f32 = 16.0;

/// A gesture under way.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) enum Gesture {
    #[default]
    Idle,
    Pan,
    /// A node, by name, and how far it has been dragged.
    Move {
        name: String,
        moved: Vec2,
    },
    /// A link from a node's output, by name.
    Link {
        from: String,
    },
}

/// What the cards, ports and canvas report of the pointer this frame.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Input {
    /// A node's header pressed to drag it.
    Grab(String),
    /// A node's output port pressed.
    StartLink(String),
    /// The canvas pressed where nothing else is.
    PressEmpty,
    /// The pointer dragged by this much.
    Drag(Vec2),
    /// The button let go here.
    Release(Pos2),
}

/// The canvas' own state between frames: its pan and the gesture under way.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Canvas {
    pub pan: Vec2,
    pub gesture: Gesture,
}

impl Default for Canvas {
    fn default() -> Canvas {
        Canvas { pan: vec2(16.0, 16.0), gesture: Gesture::Idle }
    }
}

impl Canvas {
    fn id() -> egui::Id {
        egui::Id::new("system_canvas")
    }

    /// As the last frame left it.
    pub fn load(ctx: &egui::Context) -> Canvas {
        ctx.data_mut(|d| d.get_temp::<Canvas>(Canvas::id())).unwrap_or_default()
    }

    /// Kept for the next frame.
    pub fn store(self, ctx: &egui::Context) {
        ctx.data_mut(|d| d.insert_temp(Canvas::id(), self));
    }

    /// The node being moved and by how much.
    pub fn moving(&self) -> Option<(&str, Vec2)> {
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

    /// The gesture after `input`, and the command it ends in: a node let go moves it there, a
    /// link let go on an input port links it.
    pub fn step(&mut self, input: Input, layout: &Layout) -> Option<Action> {
        match (input, &mut self.gesture) {
            (Input::Grab(name), _) => self.gesture = Gesture::Move { name, moved: Vec2::ZERO },
            (Input::StartLink(from), _) => self.gesture = Gesture::Link { from },
            (Input::PressEmpty, Gesture::Idle) => self.gesture = Gesture::Pan,
            (Input::Drag(d), Gesture::Pan) => self.pan += d,
            (Input::Drag(d), Gesture::Move { moved, .. }) => *moved += d,
            (Input::Release(p), _) => return self.release(p, layout),
            _ => {}
        }
        None
    }

    fn release(&mut self, p: Pos2, layout: &Layout) -> Option<Action> {
        match std::mem::take(&mut self.gesture) {
            Gesture::Move { name, .. } => {
                let at = layout.node(&name)?.rect.min - layout.origin;
                Some(Action::run("system.stage.edit", json!({"stage": name, "at": [at.x.round(), at.y.round()]})))
            }
            Gesture::Link { from } => {
                let to = layout.input_near(p, PORT_REACH, &from)?;
                Some(Action::run("system.link", json!({"from": from, "to": to.name})))
            }
            Gesture::Idle | Gesture::Pan => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use emcraft_model::system::{Chain, Part, RECEIVER, Stage};

    fn chain() -> Chain {
        let mut m = emcraft_model::Model::default();
        for n in ["a", "b"] {
            m.add_stage(9, Stage { name: n.into(), part: Part::Attenuator { loss: 1.0 }, at: None }).unwrap();
        }
        m.system
    }

    /// Gestures without a frame: the layout from the model, the inputs as the cards report them.
    #[test]
    fn a_node_dragged_moves_there_a_link_dropped_on_a_port_links_and_the_empty_canvas_pans() {
        let c = chain();
        let mut s = Canvas::default();
        let layout = Layout::of(&c, Pos2::ZERO, s.moving());
        assert_eq!(s.step(Input::Grab("a".into()), &layout), None);
        assert_eq!(s.step(Input::Drag(vec2(30.0, 10.0)), &layout), None);
        let moved = Layout::of(&c, Pos2::ZERO, s.moving());
        let at = moved.node("a").unwrap().rect.min;
        let done = s.step(Input::Release(at), &moved);
        assert_eq!(done, Some(Action::run("system.stage.edit", json!({"stage": "a", "at": [at.x, at.y]}))));
        assert_eq!(s.gesture, Gesture::Idle);
        // A link from b's output dropped on the receiver's input.
        s.step(Input::StartLink("b".into()), &layout);
        let to = layout.node(RECEIVER).unwrap().input();
        assert_eq!(s.step(Input::Release(to + vec2(3.0, 2.0)), &layout), Some(Action::run("system.link", json!({"from": "b", "to": RECEIVER}))));
        // Dropped nowhere: nothing.
        s.step(Input::StartLink("b".into()), &layout);
        assert_eq!(s.step(Input::Release(Pos2::new(-500.0, -500.0)), &layout), None);
        // The empty canvas dragged pans it.
        s.step(Input::PressEmpty, &layout);
        s.step(Input::Drag(vec2(5.0, -4.0)), &layout);
        assert_eq!(s.pan, vec2(21.0, 12.0));
    }
}
