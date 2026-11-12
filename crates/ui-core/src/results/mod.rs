//! The result pages as every window shows them: what each page's chart shows besides the
//! session (choices the window keeps), its tables as rows of text, and the lines and actions
//! round it. Each window draws them; the engine draws the charts.

pub mod fields;
pub mod sparameters;
pub mod study;
pub mod views;

/// A table as text: its caption, its headers, its rows (as many cells as headers), and which
/// rows are marked (a fast sweep's rows solved in full), when any are.
#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    pub caption: Option<String>,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub marks: Option<Vec<bool>>,
}
