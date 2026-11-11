//! The window's layers, top first: what Escape closes and what takes the pointer before what
//! is under it.

emcraft_macros::named_enum! {
    /// A layer of the window, top first.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Layer {
        /// A question before work is lost, or a prompt for a path or a file's name.
        Ask = "ask",
        /// A template's card.
        Template = "template",
        /// The crash reports' card.
        Reports = "reports",
        /// The command search, open.
        Search = "search",
        /// The open dialog (floating, docked or in the step's panel).
        Dialog = "dialog",
        /// The settings page, shown.
        Settings = "settings",
        /// A route being drawn on the board (the pen).
        Pen = "pen",
        /// The 3D view's selection box, being dragged.
        Box = "box",
        /// The 3D view's tool, with what it picked.
        Tool = "tool",
        /// What is selected (in the 3D view and the browser).
        Selection = "selection",
    }
}
emcraft_macros::named_serde!(Layer);
