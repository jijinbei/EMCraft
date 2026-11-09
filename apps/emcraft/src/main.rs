//! The EMCraft desktop app: `emcraft [PROJECT.emcraft] [--sample NAME] [--control PORT]`. A
//! project file is opened; `--sample` opens a sample and solves it; with neither the start page
//! shows. `--control` listens for an agent's JSON-line requests on 127.0.0.1:PORT
//! (docs/control-protocol.md). `--help` and `--version` print and exit.
//!
//! On Windows the release build is a window program with no console (a file opened from
//! Explorer opens no terminal); what it prints shows only when its output is redirected.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod control_auth;
mod control_server;
mod picker;

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let env_port = std::env::var("EMCRAFT_CONTROL_PORT").ok().and_then(|p| p.parse().ok());
    let Cli { sample, project, replay, control } = match parse_args(&args, env_port) {
        Parsed::Run(cli) => cli,
        Parsed::Print(text) => {
            println!("{text}");
            return ExitCode::SUCCESS;
        }
        Parsed::Usage => {
            eprintln!("{}", usage_text());
            return ExitCode::FAILURE;
        }
    };
    let host = emcraft_ui_egui::Host {
        picker: Some(std::sync::Arc::new(picker::SystemPicker)),
        prefs: emcraft_ui_egui::prefs::default_path(),
        themes: emcraft_ui_egui::theme::default_dir(),
        open: project,
        autosave: emcraft_ui_egui::autosave::default_dir(),
        crashes: emcraft_ui_egui::crash::default_dir(),
        replay,
    };
    // The token file lives as long as the app.
    let mut token_file = None;
    let result = emcraft_ui_egui::run_with(sample, host, |ctx| {
        let port = control?;
        let ctx = ctx.clone();
        match control_server::start(port, move || ctx.request_repaint()) {
            Ok((rx, guard)) => {
                token_file = guard;
                Some(rx)
            }
            Err(e) => {
                eprintln!("emcraft: control channel not started: {e}");
                None
            }
        }
    });
    drop(token_file);
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            if control.is_none() {
                show_start_error(&e.to_string());
            }
            ExitCode::FAILURE
        }
    }
}

/// The command line's choices: a sample to open and solve, a project file, a crash report or
/// recording to replay, and the control channel's port.
#[derive(Debug, Default, PartialEq)]
struct Cli {
    sample: Option<String>,
    project: Option<String>,
    replay: Option<std::path::PathBuf>,
    control: Option<u16>,
}

/// What the command line asks: run with its choices, print a text and stop (help, version),
/// or it is wrong (print the usage and fail).
#[derive(Debug, PartialEq)]
enum Parsed {
    Run(Cli),
    Print(String),
    Usage,
}

/// The usage line with the samples' names.
fn usage_text() -> String {
    format!(
        "usage: emcraft [PROJECT.emcraft] [--sample NAME] [--control PORT] [--replay REPORT]   (samples: {})",
        emcraft_engine::templates::names().join(", ")
    )
}

/// Reads the arguments (without the program's name); `control` is the port from the
/// environment, which `--control` overrides.
fn parse_args(args: &[String], control: Option<u16>) -> Parsed {
    let mut cli = Cli { control, ..Cli::default() };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-h" | "--help" => return Parsed::Print(usage_text()),
            "-V" | "--version" => return Parsed::Print(format!("emcraft {}", env!("CARGO_PKG_VERSION"))),
            "--sample" => match it.next() {
                Some(name) if emcraft_engine::templates::find(name).is_some() => cli.sample = Some(name.clone()),
                _ => return Parsed::Usage,
            },
            // A crash report's folder (or a recording): played back in the window.
            "--replay" => match it.next() {
                Some(path) => cli.replay = Some(std::path::PathBuf::from(path)),
                None => return Parsed::Usage,
            },
            "--control" => match it.next().map(|p| p.parse::<u16>()) {
                Some(Ok(p)) => cli.control = Some(p),
                _ => return Parsed::Usage,
            },
            path if !path.starts_with('-') && cli.project.is_none() => cli.project = Some(path.to_owned()),
            _ => return Parsed::Usage,
        }
    }
    Parsed::Run(cli)
}

/// The window could not open (typically no OpenGL 2 driver: a virtual machine, a remote
/// desktop). A window program on Windows has no console for the error, so it is shown in a
/// message box; elsewhere the terminal shows it. Not with `--control`: an agent reads stderr.
#[cfg(all(windows, not(debug_assertions)))]
fn show_start_error(error: &str) {
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("EMCraft")
        .set_description(format!(
            "EMCraft could not open its window.\n\n{error}\n\nEMCraft draws with OpenGL 2 or later: install or update the graphics driver."
        ))
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

#[cfg(not(all(windows, not(debug_assertions))))]
fn show_start_error(_error: &str) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str], env: Option<u16>) -> Parsed {
        parse_args(&args.iter().map(ToString::to_string).collect::<Vec<_>>(), env)
    }

    #[test]
    fn arguments_choose_what_opens_and_bad_ones_give_the_usage() {
        assert_eq!(parse(&[], Some(7)), Parsed::Run(Cli { control: Some(7), ..Cli::default() }));
        let Parsed::Run(cli) = parse(&["model.emcraft", "--sample", "highpass", "--control", "9000"], Some(7)) else { panic!("not run") };
        assert_eq!(cli, Cli { sample: Some("highpass".into()), project: Some("model.emcraft".into()), replay: None, control: Some(9000) });
        assert!(matches!(parse(&["--version"], None), Parsed::Print(v) if v.starts_with("emcraft ")));
        assert!(matches!(parse(&["--help"], None), Parsed::Print(u) if u.starts_with("usage:")));
        for bad in [&["--sample", "no-such"][..], &["--control", "x"], &["--control"], &["a", "b"], &["--what"], &["--replay"]] {
            assert_eq!(parse(bad, None), Parsed::Usage, "{bad:?}");
        }
    }
}
