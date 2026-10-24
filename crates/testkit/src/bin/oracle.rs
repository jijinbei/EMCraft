//! `cargo xtask oracle`: runs the verification cases (docs/verification.md).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut opts = emcraft_testkit::oracle::Options { write: true, ..Default::default() };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--case" => match it.next() {
                Some(c) => opts.cases.push(c.clone()),
                None => return usage("--case needs a name"),
            },
            "--bless" => match it.next() {
                Some(c) => opts.bless.push(c.clone()),
                None => return usage("--bless needs a name or `all`"),
            },
            "--no-write" => opts.write = false,
            other => return usage(&format!("unknown option `{other}`")),
        }
    }
    match emcraft_testkit::oracle::run(&emcraft_testkit::root(), &opts) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn usage(e: &str) -> ExitCode {
    eprintln!("error: {e}\nusage: oracle [--case NAME]... [--bless NAME|all]... [--no-write]");
    ExitCode::FAILURE
}
