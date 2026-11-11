//! What EMCraft's windows share, whatever draws them (docs/architecture.md): no GUI toolkit is
//! below this crate, so the egui window and any other (iced) build on the same texts and the
//! same rules for what the window shows. The modules:
//!
//! - [`lang`]: the languages, the texts looked up by their English (`t!`), the commands' and
//!   samples' translations;
//! - [`chart_words`]: the engine's charts' words in the window's language;
//! - [`param_labels`]: the names dialogs show for command parameters;
//! - the window's names for what it shows: [`glyph`]s, [`page`]s and settings sections, the
//!   [`ribbon`]'s tabs and panels, the 3D view's [`tools`], the [`view`]s and where each shows,
//!   and the [`handle`]s scenarios and the control channel find things on screen by;
//! - [`geom`]: points, offsets and rectangles on screen, each window converting them to its own;
//! - [`system`]: a system's stages as the window shows them (their kinds, the settings a new one
//!   starts from, a stage's numbers in a line, the chain's reading at its output), its canvas'
//!   layout and the pointer's gestures on it.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

/// An English text of the window, checked against every complete translation when the crate
/// compiles (`emcraft_i18n::text!`): `t!("Mesh")`.
#[macro_export]
macro_rules! t {
    ($english:literal) => {
        $crate::lang::Text::checked(::emcraft_i18n::text!($english))
    };
}

pub mod chart_words;
pub mod geom;
pub mod glyph;
pub mod handle;
pub mod lang;
pub mod page;
pub mod param_labels;
pub mod ribbon;
pub mod system;
pub mod tools;
pub mod view;
