//! The EMCraft desktop app: `emcraft [--sample NAME] [--control PORT]`. `--sample` opens a
//! sample and solves it; `--control` listens for an agent's JSON-line requests on
//! 127.0.0.1:PORT (docs/control-protocol.md).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod control_auth;
mod control_server;

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = || {
        eprintln!("usage: emcraft [--sample NAME] [--control PORT]   (samples: {})", emcraft_engine::sample::names().join(", "));
        ExitCode::FAILURE
    };
    let mut sample = None;
    let mut control: Option<u16> = std::env::var("EMCRAFT_CONTROL_PORT").ok().and_then(|p| p.parse().ok());
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match (a.as_str(), it.next()) {
            ("--sample", Some(name)) if emcraft_engine::sample::find(name).is_some() => sample = Some(name.clone()),
            ("--control", Some(port)) => match port.parse::<u16>() {
                Ok(p) => control = Some(p),
                Err(_) => return usage(),
            },
            _ => return usage(),
        }
    }
    // The token file lives as long as the app.
    let mut token_file = None;
    let result = emcraft_ui_egui::run_with(sample, |ctx| {
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
