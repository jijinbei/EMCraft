//! Where everything sits on the canvas, from the model alone: each node's box, its ports, the
//! links between them. No drawing and no input: the wires, the cards and the gestures read it.

use egui::{Pos2, Rect, Vec2, pos2, vec2};
use emcraft_model::system::{Chain, Part, RECEIVER, SOURCE};

/// A node's width, its header's height, a field's row and the gap along the chain (points).
pub(super) const NODE_W: f32 = 184.0;
pub(super) const HEAD_H: f32 = 24.0;
pub(super) const ROW_H: f32 = 22.0;
const STEP_X: f32 = 230.0;

/// What a node is: the source, a stage (its index) or the receiver.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Kind {
    Source,
    Stage(usize),
    Receiver,
}

/// A node on the canvas: what it is, its name in links and commands (`@source`, `@receiver`, a
/// stage's), its box and whether the chain reads it.
#[derive(Clone, Debug)]
pub(super) struct NodeBox {
    pub kind: Kind,
    pub name: String,
    pub rect: Rect,
    pub on_chain: bool,
}

impl NodeBox {
    pub fn output(&self) -> Pos2 {
        self.rect.right_top() + vec2(0.0, HEAD_H / 2.0)
    }

    pub fn input(&self) -> Pos2 {
        self.rect.left_top() + vec2(0.0, HEAD_H / 2.0)
    }

    /// Its header, the part dragged to move it.
    pub fn header(&self) -> Rect {
        Rect::from_min_size(self.rect.min, vec2(self.rect.width(), HEAD_H))
    }

    /// Its `k`th row of fields.
    pub fn row(&self, k: usize) -> Rect {
        Rect::from_min_size(
            pos2(self.rect.left() + 10.0, self.rect.top() + HEAD_H + 4.0 + k as f32 * ROW_H),
            vec2(self.rect.width() - 20.0, ROW_H - 2.0),
        )
    }
}

/// The rows of fields a node's card shows.
pub(super) fn rows(part: Option<&Part>) -> usize {
    match part {
        None => 3,
        Some(Part::Amplifier { .. }) => 4,
        Some(Part::Data { .. } | Part::Attenuator { .. }) => 1,
        Some(Part::Line { .. } | Part::Element { .. }) => 3,
    }
}

/// A node's height: its header, its rows and the reading under them.
fn height(rows: usize) -> f32 {
    HEAD_H + 6.0 + rows as f32 * ROW_H + ROW_H + 6.0
}

/// The canvas as laid out: its origin on screen, the nodes (the source first, the receiver
/// last) and the links by node index.
pub(super) struct Layout {
    pub origin: Pos2,
    pub nodes: Vec<NodeBox>,
    pub links: Vec<(usize, usize)>,
}

impl Layout {
    /// `chain` laid out from `origin`: a stage where it was put, else by its place along the
    /// chain (those off it under); the node `moving` (by name) shifted by its drag.
    pub fn of(chain: &Chain, origin: Pos2, moving: Option<(&str, Vec2)>) -> Layout {
        let (order, _) = chain.order();
        let shift = |name: &str| moving.filter(|(n, _)| *n == name).map_or(Vec2::ZERO, |(_, d)| d);
        let mut off = 0.0;
        let mut nodes = vec![NodeBox {
            kind: Kind::Source,
            name: SOURCE.into(),
            rect: Rect::from_min_size(origin + vec2(0.0, 20.0), vec2(NODE_W, height(3))),
            on_chain: true,
        }];
        for (i, s) in chain.stages.iter().enumerate() {
            let place = order.iter().position(|k| *k == i);
            let at = match (s.at, place) {
                (Some([x, y]), _) => vec2(x as f32, y as f32),
                (None, Some(k)) => vec2(STEP_X * (k + 1) as f32, 20.0),
                (None, None) => {
                    off += 1.0;
                    vec2(STEP_X * off, 200.0)
                }
            };
            let rect = Rect::from_min_size(origin + at + shift(&s.name), vec2(NODE_W, height(rows(Some(&s.part)))));
            nodes.push(NodeBox { kind: Kind::Stage(i), name: s.name.clone(), rect, on_chain: place.is_some() });
        }
        let right = nodes.iter().map(|n| n.rect.left() - origin.x).fold(0.0f32, f32::max);
        let last_y = order.last().map_or(20.0, |&i| nodes[i + 1].rect.top() - origin.y);
        nodes.push(NodeBox {
            kind: Kind::Receiver,
            name: RECEIVER.into(),
            rect: Rect::from_min_size(origin + vec2(right + STEP_X, last_y), vec2(NODE_W, height(3))),
            on_chain: true,
        });
        // A chain written without links reads in its order: drawn so.
        let named: Vec<(String, String)> = if chain.links.is_empty() {
            let names: Vec<&str> =
                std::iter::once(SOURCE).chain(order.iter().map(|&i| chain.stages[i].name.as_str())).chain(std::iter::once(RECEIVER)).collect();
            names.windows(2).map(|w| (w[0].to_owned(), w[1].to_owned())).collect()
        } else {
            chain.links.iter().map(|l| (l.from.clone(), l.to.clone())).collect()
        };
        let index = |n: &str| nodes.iter().position(|b| b.name == n);
        let links = named.iter().filter_map(|(a, b)| Some((index(a)?, index(b)?))).collect();
        Layout { origin, nodes, links }
    }

    /// The node named `name`.
    pub fn node(&self, name: &str) -> Option<&NodeBox> {
        self.nodes.iter().find(|n| n.name == name)
    }

    /// The node whose input port is within `reach` of `p`, but `except`.
    pub fn input_near(&self, p: Pos2, reach: f32, except: &str) -> Option<&NodeBox> {
        self.nodes.iter().find(|n| n.kind != Kind::Source && n.name != except && n.input().distance(p) <= reach)
    }

    /// How far below the origin the lowest node reaches.
    pub fn depth(&self) -> f32 {
        self.nodes.iter().map(|n| n.rect.bottom() - self.origin.y).fold(0.0, f32::max)
    }
}
