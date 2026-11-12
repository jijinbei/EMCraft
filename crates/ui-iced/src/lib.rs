//! EMCraft's window, in iced (docs/adr/0007-iced-window.md, docs/adr/0008-retire-egui.md),
//! built on the shared rules ([`emcraft_ui_core`]): what a view holds, what each gesture and
//! field ends in and the texts it shows come from there; this crate only lays them out and draws
//! them.
//!
//! An iced program is its state, the messages that change it and the view drawn from it:
//! [`app::App`], [`app::Message`], [`app::App::update`] and [`app::App::view`]. Every change to
//! the project is an engine command.
//!
//! - [`app`]: the window's state and messages; [`run`]: commands on the worker; [`search`]:
//!   every command by its name;
//! - [`browser`]: the model tree, the workflow's steps its nodes, each with what it adds (＋)
//!   and runs (▶); [`dialog`]: a command's parameters in the properties panel; [`files`]: projects opened and saved;
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
pub mod designs;
pub mod dialog;
pub mod eigenmodes;
pub mod escape;
pub mod field_panel;
pub mod files;
pub mod geom;
pub mod headless;
pub mod icons;
pub mod import;
pub mod language;
pub mod look;
pub mod messages;
pub mod model;
pub mod pcb;
pub mod port_modes;
pub mod record;
pub mod reports;
pub mod results;
pub mod run;
pub mod search;
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

use emcraft_ui_core::{prefs, theme};

/// The desktop's id for the window: a dock shows the icon of `emcraft.desktop`
/// (`cargo xtask desktop`).
pub const APP_ID: &str = "emcraft";

/// The window: its size, the smallest it may be, its icon and (on Linux) its app id.
fn window_settings() -> iced::window::Settings {
    let icon = iced::window::icon::from_rgba(emcraft_render::icon::rgba(64), 64, 64).ok();
    #[cfg(target_os = "linux")]
    let platform_specific = iced::window::settings::PlatformSpecific { application_id: APP_ID.into(), ..Default::default() };
    #[cfg(not(target_os = "linux"))]
    let platform_specific = Default::default();
    iced::window::Settings {
        size: iced::Size::new(1400.0, 900.0),
        min_size: Some(iced::Size::new(900.0, 600.0)),
        icon,
        platform_specific,
        exit_on_close_request: false,
        ..Default::default()
    }
}

/// What the window opens on and with.
#[derive(Default)]
pub struct Launch {
    /// A sample to open, or a project file (`.emcraft`); with neither, the start page.
    pub sample: Option<String>,
    pub project: Option<String>,
    /// Where a picture of the window is written once it has drawn (a running job ended first);
    /// the window then closes.
    pub shoot: Option<std::path::PathBuf>,
    /// A recording or a crash report's folder, played back once the window is up.
    pub replay: Option<std::path::PathBuf>,
    /// The host's file dialog.
    pub picker: Option<std::sync::Arc<dyn emcraft_ui_core::files::Picker>>,
    /// The control channel's requests (docs/control-protocol.md).
    pub control: Option<std::sync::mpsc::Receiver<emcraft_control::ControlRequest>>,
}

/// Runs the window as `launch` asks, in the preferences it keeps: its language, its theme (the
/// user's themes too), the font the language is shown with and the 3D view's mouse mapping and
/// drawing. It keeps crash reports (crate::record) and copies of unsaved work unless it takes a
/// picture.
pub fn run(launch: Launch) -> iced::Result {
    let Launch { sample, project, shoot, replay, picker, control } = launch;
    // The control channel's queue goes to the first window made.
    let control = std::sync::Mutex::new(control);
    let prefs = prefs::default_path().map(|p| prefs::Prefs::load(&p)).unwrap_or_default();
    // The language shown when a font shows it (else English), the pictures' text with its face.
    let (font_state, found) = language::chosen(prefs.language(), prefs.font_file.as_deref());
    let font = found.filter(|_| prefs.font_file.is_some()).map(|f| f.bytes);
    let themes = theme::Themes::load(theme::default_dir().as_deref());
    let theme = themes.get(&prefs.theme);
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
            .with_crash_reports(crashes.clone(), true)
            .with_font(font_state.clone());
        if let Some(path) = &project {
            app.open_path(path);
        }
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
        .window(window_settings())
        .run()
}

#[cfg(test)]
mod lang_tests;
#[cfg(test)]
mod scenario;
#[cfg(test)]
mod tests;
