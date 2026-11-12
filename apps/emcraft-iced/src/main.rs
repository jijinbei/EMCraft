//! EMCraft's window in iced: `emcraft-iced [--sample NAME] [--shot PNG] [--control PORT]
//! [--replay REPORT]`. A sample is opened
//! (and its chain read); without one the window starts on an empty project. So far it shows the
//! model in 3D (meshed and solved from its bar), the sweep's S-parameters and a system's node
//! editor (docs/adr/0007-iced-window.md). `--shot` writes a picture of the window once it has
//! drawn, then closes it; `--replay` plays back a recording or a crash report's folder.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    let known = args.chunks(2).all(|p| matches!(p, [f, _] if ["--sample", "--shot", "--control", "--replay"].contains(&f.as_str())));
    if !known {
        eprintln!("usage: emcraft-iced [--sample NAME] [--shot PNG] [--control PORT] [--replay REPORT]");
        return ExitCode::FAILURE;
    }
    // The control channel (docs/control-protocol.md); its token file lives as long as the app.
    let port = value("--control").and_then(|p| p.parse::<u16>().ok());
    let (control, _token) = match port.map(|p| emcraft_control::server::start(p, || {})) {
        Some(Ok(started)) => {
            eprintln!("emcraft-iced: {}", started.note);
            (Some(started.requests), started.token_file)
        }
        Some(Err(e)) => {
            eprintln!("emcraft-iced: control channel not started: {e}");
            (None, None)
        }
        None => (None, None),
    };
    let picker: std::sync::Arc<dyn emcraft_ui_core::files::Picker> = std::sync::Arc::new(emcraft_picker::SystemPicker);
    match emcraft_ui_iced::run(value("--sample"), value("--shot").map(Into::into), value("--replay").map(Into::into), Some(picker), control) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("emcraft-iced: {e}");
            ExitCode::FAILURE
        }
    }
}
