//! EMCraft's second window, in iced (docs/adr/0007-iced-window.md), built on the same shared
//! rules as the egui one ([`emcraft_ui_core`]): what a view holds, what each gesture and field
//! ends in and the texts it shows come from there; this crate only lays them out and draws
//! them. It grows alongside the egui window until it does as much.
//!
//! An iced program is its state, the messages that change it and the view drawn from it:
//! [`app::App`], [`app::Message`], [`app::App::update`] and [`app::App::view`]. Every change to
//! the project is an engine command.
//!
//! - [`app`]: the window's state and messages; [`run`]: commands on the worker; [`ribbon`]:
//!   every command by its tab and panel;
//! - [`browser`]: the model and its results as a tree beside the 3D view; [`dialog`]: a
//!   command's parameters over the window;
//! - [`model`]: the model in 3D ([`view3d`], drawn by the GPU in iced's shader widget), meshed
//!   and solved from its bar; [`sparameters`]: the sweep's chart;
//! - [`system`]: a system's node editor — the band, the canvas, the chart and the budget;
//! - [`charts`]: the engine's charts, made again only when they change; [`chart`]: one drawn
//!   as vectors by the GPU;
//! - [`look`]: the theme's tokens as iced's styles;
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
pub mod browser;
pub mod chart;
pub mod charts;
pub mod dialog;
pub mod geom;
pub mod look;
pub mod model;
pub mod ribbon;
pub mod run;
pub mod sparameters;
pub mod system;
pub mod view3d;

use emcraft_ui_core::{lang, prefs, theme};

/// Runs the window, on sample `sample` when one is named, in the preferences the egui window
/// keeps: its language, its theme (the user's themes too), the font the language is shown with
/// and the 3D view's mouse mapping and drawing. With `shoot`, a picture of the window is written
/// there once it has drawn (a running job ended first) and the window closes.
pub fn run(sample: Option<String>, shoot: Option<std::path::PathBuf>) -> iced::Result {
    let prefs = prefs::default_path().map(|p| prefs::Prefs::load(&p)).unwrap_or_default();
    lang::set(prefs.language());
    let theme = theme::Themes::load(theme::default_dir().as_deref()).get(&prefs.theme);
    let font = prefs.font_file.as_deref().and_then(|f| std::fs::read(f).ok());
    let start = move || app::App::new(sample.as_deref()).dressed(theme.clone()).with_prefs(&prefs).shooting(shoot.clone());
    iced::application(start, app::App::react, app::App::view)
        .subscription(app::App::subscription)
        .title(app::App::title)
        .theme(app::App::theme)
        // The language's font, and the pictures' face the charts' labels are set in.
        .fonts(font.map(std::borrow::Cow::Owned).into_iter().chain([std::borrow::Cow::Borrowed(emcraft_render::text::face_bytes())]))
        .window_size((1400.0, 900.0))
        .run()
}

#[cfg(test)]
mod tests;
