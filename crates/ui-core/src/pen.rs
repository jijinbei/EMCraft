//! What a board's 3D view draws routes with.

/// What the board's 3D view draws with: a net's traces on a layer, `width` wide (m), from the
/// last point clicked (m) once there is one.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Pen {
    pub net: String,
    pub layer: String,
    pub width: f64,
    pub from: Option<[f64; 2]>,
}

impl Pen {
    /// A pen for `board`: the route's net, layer and width (the first routed net's when none is
    /// chosen), else its first layer and 0.2 mm.
    pub fn for_board(board: &emcraft_model::pcb::Board, route: Option<&str>) -> Pen {
        let first = board.segments.iter().find(|g| Some(g.net.as_str()) == route).or_else(|| board.segments.first());
        match first {
            Some(g) => Pen { net: g.net.clone(), layer: g.layer.clone(), width: g.width, from: None },
            None => Pen { net: "NET".into(), layer: board.layers.first().map(|l| l.name.clone()).unwrap_or_default(), width: 0.2e-3, from: None },
        }
    }
}
