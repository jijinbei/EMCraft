//! The EMCraft desktop app: `emcraft [--sample highpass|iris-bandpass]`.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let sample = match args.as_slice() {
        [] => None,
        [flag, name] if flag == "--sample" && (name == "highpass" || name == "iris-bandpass") => Some(name.clone()),
        _ => {
            eprintln!("usage: emcraft [--sample highpass|iris-bandpass]");
            return ExitCode::FAILURE;
        }
    };
    match emcraft_ui_egui::run(sample) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
