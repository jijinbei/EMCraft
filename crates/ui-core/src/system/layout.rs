//! Where everything sits on a system's canvas, from the model alone: each node's box, its ports,
//! the links between them. No drawing and no input: a window draws it, the gestures read it.

use emcraft_model::system::{Chain, Part, RECEIVER, SOURCE, Stage};

use crate::geom::{Point, Rect, Vector};

/// A node's width, its header's height and a row of its fields (points).
pub const NODE_W: f32 = 184.0;
pub const HEAD_H: f32 = 24.0;
pub const ROW_H: f32 = 22.0;

/// The gap between nodes laid out along the chain, and where the rows of them sit (points).
const STEP_X: f32 = 230.0;
const CHAIN_Y: f32 = 20.0;
const OFF_CHAIN_Y: f32 = 200.0;

/// What a node is: the source, a stage (its index) or the receiver.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Source,
    Stage(usize),
    Receiver,
}

/// A node on the canvas: what it is, its name in links and commands (`@source`, `@receiver`, a
/// stage's), its box and whether the chain reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct NodeBox {
    pub kind: Kind,
    pub name: String,
    pub rect: Rect,
    pub on_chain: bool,
}

impl NodeBox {
    /// Its output port, on its right edge level with its header.
    pub fn output(&self) -> Point {
        self.rect.right_top() + Vector::new(0.0, HEAD_H / 2.0)
    }

    /// Its input port, on its left edge level with its header.
    pub fn input(&self) -> Point {
        self.rect.left_top() + Vector::new(0.0, HEAD_H / 2.0)
    }

    /// Its header, the part dragged to move it.
    pub fn header(&self) -> Rect {
        Rect::from_min_size(self.rect.min, Vector::new(self.rect.width(), HEAD_H))
    }

    /// Its `k`th row of fields.
    pub fn row(&self, k: usize) -> Rect {
        let at = self.rect.min + Vector::new(10.0, HEAD_H + 4.0 + k as f32 * ROW_H);
        Rect::from_min_size(at, Vector::new(self.rect.width() - 20.0, ROW_H - 2.0))
    }
}

/// The rows of fields a node's card shows: the source's and receiver's three (`None`), a stage's
/// by its kind.
pub fn rows(part: Option<&Part>) -> usize {
    match part {
        None => 3,
        Some(Part::Amplifier { .. }) => 4,
        Some(Part::Data { .. } | Part::Attenuator { .. }) => 1,
        Some(Part::Line { .. } | Part::Element { .. }) => 3,
    }
}

/// A node's size: its header, its rows and the reading under them.
fn size(rows: usize) -> Vector {
    Vector::new(NODE_W, HEAD_H + 6.0 + rows as f32 * ROW_H + ROW_H + 6.0)
}

/// The canvas as laid out: its origin on screen, the nodes (the source first, the receiver
/// last) and the links by node index.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub origin: Point,
    pub nodes: Vec<NodeBox>,
    pub links: Vec<(usize, usize)>,
}

/// A stage's place from the origin: where it was put, else by its place along the chain, else
/// (off the chain) along a row under it by its place among those off it.
fn place(stage: &Stage, along: Option<usize>, off: Option<usize>) -> Vector {
    match (stage.at, along, off) {
        (Some([x, y]), _, _) => Vector::new(x as f32, y as f32),
        (None, Some(k), _) => Vector::new(STEP_X * (k + 1) as f32, CHAIN_Y),
        (None, None, k) => Vector::new(STEP_X * (k.unwrap_or(0) + 1) as f32, OFF_CHAIN_Y),
    }
}

/// The stages' boxes from `origin`, the one `moving` (by name) shifted by its drag.
fn stage_boxes(chain: &Chain, order: &[usize], origin: Point, moving: Option<(&str, Vector)>) -> Vec<NodeBox> {
    let off: Vec<usize> = (0..chain.stages.len()).filter(|i| !order.contains(i)).collect();
    chain
        .stages
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let along = order.iter().position(|k| *k == i);
            let shift = moving.filter(|(n, _)| *n == s.name).map_or(Vector::ZERO, |(_, d)| d);
            let at = origin + place(s, along, off.iter().position(|k| *k == i)) + shift;
            NodeBox {
                kind: Kind::Stage(i),
                name: s.name.clone(),
                rect: Rect::from_min_size(at, size(rows(Some(&s.part)))),
                on_chain: along.is_some(),
            }
        })
        .collect()
}

/// The receiver's box: a step right of the rightmost stage, level with the chain's last.
fn receiver_box(stages: &[NodeBox], order: &[usize], origin: Point) -> NodeBox {
    let right = stages.iter().map(|n| n.rect.left() - origin.x).fold(0.0f32, f32::max);
    let level = order.last().map_or(CHAIN_Y, |&i| stages[i].rect.top() - origin.y);
    let at = origin + Vector::new(right + STEP_X, level);
    NodeBox { kind: Kind::Receiver, name: RECEIVER.into(), rect: Rect::from_min_size(at, size(rows(None))), on_chain: true }
}

/// The links as drawn, by name: the chain's own, or (a chain written without links, read in
/// its order) its order's.
fn drawn_links(chain: &Chain, order: &[usize]) -> Vec<(String, String)> {
    if !chain.links.is_empty() {
        return chain.links.iter().map(|l| (l.from.clone(), l.to.clone())).collect();
    }
    let names: Vec<&str> =
        std::iter::once(SOURCE).chain(order.iter().map(|&i| chain.stages[i].name.as_str())).chain(std::iter::once(RECEIVER)).collect();
    names.windows(2).map(|w| (w[0].to_owned(), w[1].to_owned())).collect()
}

impl Layout {
    /// `chain` laid out from `origin`, the node `moving` (by name) shifted by its drag.
    ///
    /// ```
    /// use emcraft_model::system::{Chain, Part, Stage};
    /// use emcraft_ui_core::geom::Point;
    /// use emcraft_ui_core::system::layout::Layout;
    /// let pad = Stage { name: "pad".into(), part: Part::Attenuator { loss: 3.0 }, at: None };
    /// let l = Layout::of(&Chain { stages: vec![pad], ..Chain::default() }, Point::ORIGIN, None);
    /// // The source, the pad, the receiver, linked in a row.
    /// assert_eq!(l.nodes.len(), 3);
    /// assert_eq!(l.links, [(0, 1), (1, 2)]);
    /// assert!(l.nodes[2].rect.left() > l.nodes[1].rect.left());
    /// ```
    pub fn of(chain: &Chain, origin: Point, moving: Option<(&str, Vector)>) -> Layout {
        let (order, _) = chain.order();
        let stages = stage_boxes(chain, &order, origin, moving);
        let source = NodeBox {
            kind: Kind::Source,
            name: SOURCE.into(),
            rect: Rect::from_min_size(origin + Vector::new(0.0, CHAIN_Y), size(rows(None))),
            on_chain: true,
        };
        let receiver = receiver_box(&stages, &order, origin);
        let nodes: Vec<NodeBox> = std::iter::once(source).chain(stages).chain(std::iter::once(receiver)).collect();
        let index = |n: &str| nodes.iter().position(|b| b.name == n);
        let links = drawn_links(chain, &order).iter().filter_map(|(a, b)| Some((index(a)?, index(b)?))).collect();
        Layout { origin, nodes, links }
    }

    /// The node named `name`.
    pub fn node(&self, name: &str) -> Option<&NodeBox> {
        self.nodes.iter().find(|n| n.name == name)
    }

    /// The node whose input port is within `reach` of `p`, but `except`.
    pub fn input_near(&self, p: Point, reach: f32, except: &str) -> Option<&NodeBox> {
        self.nodes.iter().find(|n| n.kind != Kind::Source && n.name != except && n.input().distance(p) <= reach)
    }

    /// How far below the origin the lowest node reaches.
    pub fn depth(&self) -> f32 {
        self.nodes.iter().map(|n| n.rect.bottom() - self.origin.y).fold(0.0, f32::max)
    }
}
