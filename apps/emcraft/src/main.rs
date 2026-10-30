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
    let usage_text =
        || format!("usage: emcraft [PROJECT.emcraft] [--sample NAME] [--control PORT]   (samples: {})", emcraft_engine::templates::names().join(", "));
    let usage = || {
        eprintln!("{}", usage_text());
        ExitCode::FAILURE
    };
    let mut sample = None;
    let mut project = None;
    let mut control: Option<u16> = std::env::var("EMCRAFT_CONTROL_PORT").ok().and_then(|p| p.parse().ok());
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-h" | "--help" => {
                println!("{}", usage_text());
                return ExitCode::SUCCESS;
            }
            "-V" | "--version" => {
                println!("emcraft {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            "--sample" => match it.next() {
                Some(name) if emcraft_engine::templates::find(name).is_some() => sample = Some(name.clone()),
                _ => return usage(),
            },
            "--control" => match it.next().map(|p| p.parse::<u16>()) {
                Some(Ok(p)) => control = Some(p),
                _ => return usage(),
            },
            path if !path.starts_with('-') && project.is_none() => project = Some(path.to_owned()),
            _ => return usage(),
        }
    }
    let host = emcraft_ui_egui::Host {
        picker: Some(std::sync::Arc::new(picker::SystemPicker)),
        prefs: emcraft_ui_egui::prefs::default_path(),
        themes: emcraft_ui_egui::theme::default_dir(),
        open: project,
        autosave: emcraft_ui_egui::autosave::default_dir(),
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
            ExitCode::FAILURE
        }
    }
}
