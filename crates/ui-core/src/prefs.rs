//! The window's preferences shared by every window: what the 3D view adds to its picture.

use serde::{Deserialize, Serialize};

/// What the 3D view adds to the plain picture, each its own switch (Settings ▸ 3D view ▸ Drawing).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GpuLook {
    /// Edges where surfaces crease or end, found per pixel.
    pub edges: bool,
    /// Ambient occlusion: corners and creases darkened.
    pub ao: bool,
    /// A ring round what is selected and what is under the pointer.
    pub outline: bool,
    /// Picking reads what the GPU drew under the pointer (the CPU's rays otherwise).
    pub pick: bool,
    /// The field's colour map on its plane evaluated by the GPU, its phase stepped without
    /// building the scene again.
    pub field: bool,
}

impl Default for GpuLook {
    fn default() -> Self {
        GpuLook { edges: true, ao: false, outline: true, pick: true, field: true }
    }
}
