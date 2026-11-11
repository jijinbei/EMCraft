//! What a click in the 3D view or the browser asks of the selection.

use crate::pick::Pick;

/// A click in the 3D view while no tool is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Click {
    /// One click on `pick` (`None`: empty space); `add`: Shift or Ctrl held.
    Single { pick: Option<Pick>, add: bool },
    /// A double click.
    Double { pick: Option<Pick> },
}
