//! The workflow's steps as the window shows them.

/// A port mode picked in the Excitation panel: the Port modes view marks its row and scrolls
/// to it once.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ModeFocus {
    pub port: String,
    pub mode: usize,
    pub scroll: bool,
}
