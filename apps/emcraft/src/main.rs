//! The EMCraft desktop app: `emcraft [--sample NAME]` (`highpass`, `iris-bandpass`, `cavity` in
//! the waveguide editor; any other engine sample run as it comes).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = || {
        eprintln!("usage: emcraft [--sample NAME]   (samples: {})", emcraft_engine::sample::names().join(", "));
        ExitCode::FAILURE
    };
    let sample = match args.as_slice() {
        [] => None,
        [flag, name] if flag == "--sample" && emcraft_engine::sample::find(name).is_some() => Some(name.clone()),
        _ => return usage(),
    };
    match emcraft_ui_egui::run(sample) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
