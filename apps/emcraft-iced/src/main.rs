//! EMCraft's window in iced: `emcraft-iced [--sample NAME]`. A sample is opened (and its chain
//! read); without one the window starts on an empty project. So far it shows a system's node
//! editor (docs/adr/0007-iced-window.md).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let sample = match args.as_slice() {
        [] => None,
        [flag, name] if flag == "--sample" => Some(name.clone()),
        _ => {
            eprintln!("usage: emcraft-iced [--sample NAME]");
            return ExitCode::FAILURE;
        }
    };
    match emcraft_ui_iced::run(sample) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("emcraft-iced: {e}");
            ExitCode::FAILURE
        }
    }
}
