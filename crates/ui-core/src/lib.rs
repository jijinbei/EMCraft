//! What EMCraft's window decides, whatever draws it (docs/architecture.md): no GUI toolkit is
//! below this crate, so the window (iced, `emcraft-ui-iced`) only lays out and draws what these
//! texts and rules say it shows. The modules:
//!
//! - [`lang`]: the languages, the texts looked up by their English (`t!`), the commands' and
//!   samples' translations;
//! - [`chart_words`]: the engine's charts' words in the window's language;
//! - [`commands`]: each command's button label, glyph and whether it runs long;
//! - [`param_labels`]: the names dialogs show for command parameters;
//! - the window's names for what it shows: [`glyph`]s, [`page`]s and settings sections, the
//!   former [`ribbon`]'s tabs (in old recordings), the 3D view's [`tools`], the [`view`]s and
//!   where each shows, and the [`handle`]s scenarios and the control channel find things on
//!   screen by;
//! - what a frame asks of the window ([`action`]: each `Action` a click, a key or a request
//!   makes, performed by each window its own way) and what those carry: a [`pick`] in the 3D
//!   view, a [`selection`] click, the window's [`layer`]s, the [`files`] actions, a browser item
//!   and the browser's rows ([`browser`]: what each part of the model is called, and its
//!   [`math`] symbols) grouped under the workflow's steps ([`outline`]), the board's [`pen`],
//!   how the mouse and keys move the 3D view ([`navigation`]), a port mode's focus ([`steps`]);
//! - what a window keeps: its [`prefs`] between launches, copies of unsaved work ([`autosave`]), a [`record`]ing of its actions and the
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
pub mod analyses;
pub mod autosave;
pub mod browser;
pub mod chart_words;
pub mod commands;
pub mod crash;
pub mod cube;
pub mod dialogs;
pub mod files;
pub mod fonts;
pub mod geom;
pub mod glyph;
pub mod glyph_marks;
pub mod handle;
pub mod job;
pub mod lang;
pub mod layer;
pub mod layout;
pub mod math;
pub mod messages;
pub mod navigation;
pub mod number;
pub mod outline;
pub mod page;
pub mod param_labels;
pub mod pcb;
pub mod pen;
pub mod pick;
pub mod prefs;
pub mod record;
pub mod replies;
pub mod results;
pub mod ribbon;
pub mod scenario;
pub mod scene3d;
pub mod search;
pub mod selection;
pub mod shortcuts;
pub mod start;
pub mod status;
pub mod steps;
pub mod system;
pub mod template_card;
pub mod theme;
pub mod toast;
pub mod tools;
pub mod ui_state;
pub mod view;

#[cfg(test)]
mod lang_tests;
