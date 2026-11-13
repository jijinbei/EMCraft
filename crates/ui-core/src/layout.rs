//! The window's columns on a project's pages: the tree at the left, the properties panel at the
//! right and the page between them. Each column has its width when the window is wide enough; a
//! narrower window takes the room from the tree and the panel alike, down to their least
//! widths, so the page (the 3D view, a chart) keeps [`PAGE_LEAST`].

/// The tree's least and full widths (points).
pub const TREE: (f32, f32) = (220.0, 290.0);
/// The properties panel's least and full widths (points): a form, a step's panel, the field's.
pub const SIDE: (f32, f32) = (280.0, 380.0);
/// The least room the page between them keeps (points).
pub const PAGE_LEAST: f32 = 360.0;

/// The columns' widths (points).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Columns {
    pub tree: f32,
    pub side: f32,
}

/// The columns of a window `width` points wide whose gaps and margins take `gaps`, with the
/// properties panel shown or not (`side`: none then).
///
/// ```
/// use emcraft_ui_core::layout::{Columns, PAGE_LEAST, SIDE, TREE, columns};
/// // Wide: each column its full width.
/// assert_eq!(columns(1600.0, 40.0, true), Columns { tree: TREE.1, side: SIDE.1 });
/// // Narrow: both give way, the page keeps its least room.
/// let c = columns(1024.0, 40.0, true);
/// assert!(c.tree < TREE.1 && c.side < SIDE.1 && 1024.0 - 40.0 - c.tree - c.side >= PAGE_LEAST - 1e-3);
/// // Narrower still: their least widths, never less.
/// assert_eq!(columns(700.0, 40.0, true), Columns { tree: TREE.0, side: SIDE.0 });
/// ```
pub fn columns(width: f32, gaps: f32, side: bool) -> Columns {
    let (side_least, side_full) = if side { SIDE } else { (0.0, 0.0) };
    let room = width - gaps - PAGE_LEAST;
    let (least, full) = (TREE.0 + side_least, TREE.1 + side_full);
    // How far from their least widths towards their full ones both go (0 to 1).
    let share = if full > least { ((room - least) / (full - least)).clamp(0.0, 1.0) } else { 1.0 };
    let share = if share.is_finite() { share } else { 0.0 };
    Columns { tree: TREE.0 + share * (TREE.1 - TREE.0), side: side_least + share * (side_full - side_least) }
}
