//! What EMCraft's windows share, whatever draws them (docs/architecture.md): no GUI toolkit is
//! below this crate, so the egui window and any other (iced) build on the same texts and the
//! same rules for what the window shows. The modules:
//!
//! - [`lang`]: the languages, the texts looked up by their English (`t!`), the commands' and
//!   samples' translations;
//! - [`chart_words`]: the engine's charts' words in the window's language;
//! - [`commands`]: each command's button label, glyph and whether it runs long;
//! - [`param_labels`]: the names dialogs show for command parameters;
//! - the window's names for what it shows: [`glyph`]s, [`page`]s and settings sections, the
//!   [`ribbon`]'s tabs and panels, the 3D view's [`tools`], the [`view`]s and where each shows,
//!   and the [`handle`]s scenarios and the control channel find things on screen by;
//! - what a frame asks of the window ([`action`]: each `Action` a click, a key or a request
//!   makes, performed by each window its own way) and what those carry: a [`pick`] in the 3D
//!   view, a [`selection`] click, the window's [`layer`]s, the [`files`] actions, a browser item
//!   and the browser's rows ([`browser`]: what each part of the model is called, and its
//!   [`math`] symbols), the board's [`pen`], how the mouse and keys move the 3D view
//!   ([`navigation`]), a port mode's focus ([`steps`]);
//! - what a window keeps: its [`prefs`] between launches, a [`record`]ing of its actions and the
//!   [`crash`] reports made of them;
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

pub mod action;
pub mod after;
pub mod browser;
pub mod chart_words;
pub mod commands;
pub mod crash;
pub mod dialogs;
pub mod files;
pub mod geom;
pub mod glyph;
pub mod handle;
pub mod job;
pub mod lang;
pub mod layer;
pub mod math;
pub mod navigation;
pub mod page;
pub mod param_labels;
pub mod pen;
pub mod pick;
pub mod prefs;
pub mod record;
pub mod replies;
pub mod ribbon;
pub mod scene3d;
pub mod selection;
pub mod steps;
pub mod system;
pub mod template_card;
pub mod theme;
pub mod tools;
pub mod ui_state;
pub mod view;
