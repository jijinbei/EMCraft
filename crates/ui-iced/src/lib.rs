//! EMCraft's second window, in iced (docs/adr/0007-iced-window.md), built on the same shared
//! rules as the egui one ([`emcraft_ui_core`]): what a view holds, what each gesture and field
//! ends in and the texts it shows come from there; this crate only lays them out and draws
//! them. It grows alongside the egui window until it does as much.
//!
//! An iced program is its state, the messages that change it and the view drawn from it:
//! [`app::App`], [`app::Message`], [`app::App::update`] and [`app::App::view`]. Every change to
//! the project is an engine command.
//!
//! - [`app`]: the window's state and messages;
//! - [`system`]: a system's node editor — the band, the canvas, the chart and the budget;
//! - [`geom`]: the shared points on iced's screen and back.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

/// A text of the window, checked against every complete translation when the crate compiles:
/// `t!("Source")` ([`emcraft_ui_core::lang::Text`]).
macro_rules! t {
    ($english:literal) => {
        ::emcraft_ui_core::lang::Text::checked(::emcraft_i18n::text!($english))
    };
}

pub mod app;
pub mod geom;
pub mod system;

/// Runs the window, on sample `sample` when one is named.
pub fn run(sample: Option<String>) -> iced::Result {
    iced::application(move || app::App::new(sample.as_deref()), app::App::update, app::App::view)
        .title(app::App::title)
        .window_size((1400.0, 900.0))
        .run()
}

#[cfg(test)]
mod tests;
