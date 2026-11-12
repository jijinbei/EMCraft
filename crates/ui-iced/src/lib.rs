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
//!   command's parameters over the window; [`files`]: projects opened and saved;
//! - [`model`]: the model in 3D ([`view3d`], drawn by the GPU in iced's shader widget), meshed
//!   and solved from its bar; [`results`]: the result pages that are charts;
//!   [`eigenmodes`]: the modes a cavity resonates in; [`port_modes`]: each port's modes;
//!   [`pcb`]: a board's guided diagnosis;
//! - [`system`]: a system's node editor — the band, the canvas, the chart and the budget;
//! - [`charts`]: the engine's charts, made again only when they change; [`chart`]: one drawn
//!   as vectors by the GPU;
//! - [`start`]: the start page, the templates drawn; [`settings`]: the preferences both windows
//!   share;
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
pub mod band;
pub mod browser;
pub mod chart;
pub mod charts;
pub mod control;
pub mod dialog;
pub mod eigenmodes;
pub mod escape;
pub mod field_panel;
pub mod files;
pub mod geom;
pub mod icons;
pub mod import;
pub mod look;
pub mod messages;
pub mod model;
pub mod pcb;
pub mod port_modes;
pub mod record;
pub mod reports;
pub mod results;
pub mod ribbon;
pub mod run;
pub mod settings;
pub mod sparameters;
pub mod start;
pub mod step_panel;
pub mod steps;
pub mod style_card;
pub mod system;
pub mod table;
pub mod template_dialog;
pub mod tools;
pub mod view3d;

use emcraft_ui_core::{lang, prefs, theme};

/// Runs the window, on sample `sample` when one is named, in the preferences the egui window
/// keeps: its language, its theme (the user's themes too), the font the language is shown with
/// and the 3D view's mouse mapping and drawing. With `shoot`, a picture of the window is written
/// there once it has drawn (a running job ended first) and the window closes. `picker`: the
/// host's file dialog; `control`: the control channel's requests (docs/control-protocol.md);
/// `replay`: a recording or a crash report's folder played back once the window is up. The
/// desktop window keeps crash reports (crate::record) unless it takes a picture.
pub fn run(
    sample: Option<String>,
    shoot: Option<std::path::PathBuf>,
    replay: Option<std::path::PathBuf>,
    picker: Option<std::sync::Arc<dyn emcraft_ui_core::files::Picker>>,
    control: Option<std::sync::mpsc::Receiver<emcraft_control::ControlRequest>>,
) -> iced::Result {
    // The control channel's queue goes to the first window made.
    let control = std::sync::Mutex::new(control);
    let prefs = prefs::default_path().map(|p| prefs::Prefs::load(&p)).unwrap_or_default();
    lang::set(prefs.language());
    let themes = theme::Themes::load(theme::default_dir().as_deref());
    let theme = themes.get(&prefs.theme);
    let font = prefs.font_file.as_deref().and_then(|f| std::fs::read(f).ok());
    // A picture of the window is taken without copying or offering work.
    let autosave = emcraft_ui_core::autosave::default_dir().filter(|_| shoot.is_none());
    let crashes = emcraft_ui_core::crash::default_dir().filter(|_| shoot.is_none());
    // On the start page the templates' pictures are drawn at once.
    let start = move || {
        let mut app = app::App::new(sample.as_deref())
            .with_themes(themes.clone())
            .dressed(theme.clone())
            .with_prefs(&prefs, prefs::default_path())
            .with_autosave(autosave.clone())
            .shooting(shoot.clone())
            .with_picker(picker.clone())
            .serving(control.lock().ok().and_then(|mut c| c.take()))
            .with_crash_reports(crashes.clone(), true);
        if let Some(path) = &replay
            && let Err(e) = emcraft_ui_core::record::Recording::read(path).and_then(|r| app.play(r))
        {
            app.fail(e);
        }
        let thumbs = if app.home { app.draw_thumbs() } else { iced::Task::none() };
        (app, thumbs)
    };
    iced::application(start, app::App::react, app::App::view)
        .subscription(app::App::subscription)
        // Closing asks about unsaved changes first (App::subscription).
        .exit_on_close_request(false)
        .title(app::App::title)
        .theme(app::App::theme)
        // The language's font, and the pictures' face the charts' labels are set in.
        .fonts(font.map(std::borrow::Cow::Owned).into_iter().chain([std::borrow::Cow::Borrowed(emcraft_render::text::face_bytes())]))
        .window_size((1400.0, 900.0))
        .run()
}

#[cfg(test)]
mod scenario;
#[cfg(test)]
mod tests;
