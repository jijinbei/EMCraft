//! The 3D view's tools by name, what each shows and does: a face port, a gap port, a measure.

use crate::glyph::Glyph;

emcraft_macros::named_enum! {
    /// A tool of the 3D view.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Tool {
        /// A waveguide port on the face clicked.
        Port = "port" => "Face port",
        /// A lumped port across two points clicked.
        Lumped = "lumped" => "Gap port",
        /// The distance between two things clicked.
        Measure = "measure" => "Measure",
    }
}
emcraft_macros::named_serde!(Tool);

impl Tool {
    pub fn glyph(self) -> Glyph {
        match self {
            Tool::Port => Glyph::PickFace,
            Tool::Lumped => Glyph::PickGap,
            Tool::Measure => Glyph::Ruler,
        }
    }

    /// What it does, on hover.
    pub fn hint(self) -> crate::lang::Text {
        match self {
            Tool::Port => t!("Click a flat face: a port there"),
            Tool::Lumped => t!("Click two points: a lumped port between them"),
            Tool::Measure => t!("Click two points, edges or faces: the distance (Esc clears)"),
        }
    }

    /// Whether it makes ports (the Excitation step's tools).
    pub fn ports(self) -> bool {
        matches!(self, Tool::Port | Tool::Lumped)
    }
}

emcraft_macros::named_enum! {
    /// What a pick snapped to.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum SnapKind {
        Vertex = "vertex",
        Midpoint = "midpoint",
        Centre = "centre",
        /// A point on a straight edge, with its direction.
        Edge = "edge",
        /// A point on a face, with its normal.
        Face = "face",
    }
}
emcraft_macros::named_serde!(SnapKind);

/// A picked point (m), what it snapped to and the edge's direction or the face's normal.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Snap {
    pub at: [f64; 3],
    pub kind: SnapKind,
    pub dir: Option<[f64; 3]>,
}
