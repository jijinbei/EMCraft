//! Workspace tooling: `cargo xtask <command>`.
//!
//! Pure Rust (std + serde_json). External tools (`cargo`) are invoked through
//! `std::process::Command`. Adapted from FieldCraft's xtask (MIT OR Apache-2.0), see
//! ATTRIBUTION.md.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod dupes;
mod layers;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const USAGE: &str = "\
usage: cargo xtask <command>

commands:
  layers          enforce the crate dependency layering (docs/architecture.md)
  test [--heavy] [CARGO ARGS] [-- TEST ARGS]
                  run the workspace tests; --heavy runs only the ignored (heavy) tests:
                  convergence studies and fine meshes
  oracle [--case NAME]... [--bless NAME|all]... [--no-write]
                  run the verification cases (tests/oracle, docs/verification.md) against their
                  references and fixtures; write docs/oracle.md after a full run; --bless
                  freezes a passing case's results in its fixture.json (release build)
  features        write docs/features.md (every command with its parameters)
  dupes [--min LINES] [--loose] [--fail]
                  list code copied between production sources (runs of LINES normalised lines,
                  default 10; tests and the ported tetmesh left out); --loose matches lines by
                  their shape (names, numbers and strings ignored); --fail exits non-zero when
                  there are copies
  ci              fmt --check, layers, dupes --fail, clippy -D warnings, the tests, then the
                  oracle
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();
    let result = match args.first().map(String::as_str) {
        Some("layers") => cmd_layers(),
        Some("test") => cmd_test(&rest),
        Some("oracle") => {
            let mut c = cargo();
            c.args(["run", "--release", "-p", "emcraft-testkit", "--bin", "oracle", "--"]).args(&rest);
            run(c, "oracle")
        }
        Some("features") => cmd_features(),
        Some("dupes") => cmd_dupes(&rest),
        Some("ci") => cmd_ci(),
        Some("-h" | "--help" | "help") | None => {
            print!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("unknown command `{other}`\n\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Workspace root (parent of the xtask crate).
pub fn root() -> PathBuf {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    here.parent().unwrap_or(here).to_path_buf()
}

pub fn cargo() -> Command {
    let mut c = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    c.current_dir(root());
    c
}

fn run(mut cmd: Command, what: &str) -> Result<(), String> {
    eprintln!("$ {what}");
    let status = cmd.status().map_err(|e| format!("{what}: failed to spawn: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("{what} failed ({status})")) }
}

fn cmd_layers() -> Result<(), String> {
    let out = cargo().args(["metadata", "--format-version", "1", "--no-deps"]).output().map_err(|e| format!("cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo metadata failed: {}", String::from_utf8_lossy(&out.stderr)));
    }
    let meta: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("cargo metadata: {e}"))?;
    let crates = layers::from_metadata(&meta)?;
    for c in &crates {
        println!("{:<20} {}", c.name, layers::describe(layers::classify(&c.name)));
    }
    let v = layers::check(&crates);
    if v.is_empty() {
        println!("layers: ok ({} crates)", crates.len());
        Ok(())
    } else {
        for x in &v {
            eprintln!("  {x}");
        }
        Err(format!("layers: {} violation(s)", v.len()))
    }
}

fn cmd_test(rest: &[&str]) -> Result<(), String> {
    let (heavy, rest) = match rest.split_first() {
        Some((&"--heavy", after)) => (true, after),
        _ => (false, rest),
    };
    let (cargo_args, test_args) = rest.iter().position(|a| *a == "--").map_or((rest, &[][..]), |i| (&rest[..i], &rest[i + 1..]));
    let mut c = cargo();
    c.args(["test", "--workspace"]).args(cargo_args).arg("--");
    if heavy {
        c.arg("--ignored");
    }
    c.args(test_args);
    run(c, if heavy { "heavy tests" } else { "tests" })
}

fn cmd_features() -> Result<(), String> {
    let out = cargo().args(["run", "-q", "-p", "emcraft-cli", "--", "features"]).output().map_err(|e| format!("features: {e}"))?;
    if !out.status.success() {
        return Err(format!("features: {}", String::from_utf8_lossy(&out.stderr)));
    }
    std::fs::write(root().join("docs/features.md"), &out.stdout).map_err(|e| format!("docs/features.md: {e}"))
}

fn cmd_dupes(rest: &[&str]) -> Result<(), String> {
    let mut min = dupes::DEFAULT_MIN;
    let mut fail = false;
    let mut loose = false;
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        match *a {
            "--fail" => fail = true,
            "--loose" => loose = true,
            "--min" => {
                let v = it.next().ok_or("dupes: --min needs a number of lines")?;
                min = v.parse().ok().filter(|n| (2..=1000).contains(n)).ok_or(format!("dupes: --min {v}: a whole number from 2 to 1000"))?;
            }
            other => return Err(format!("dupes: unknown argument `{other}`")),
        }
    }
    dupes::run(&root(), min, fail, loose)
}

fn cmd_ci() -> Result<(), String> {
    let mut fmt = cargo();
    fmt.args(["fmt", "--all", "--check"]);
    run(fmt, "fmt")?;
    cmd_layers()?;
    dupes::run(&root(), dupes::DEFAULT_MIN, true, false)?;
    let mut clippy = cargo();
    clippy.args(["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]);
    run(clippy, "clippy")?;
    cmd_test(&[])?;
    let mut oracle = cargo();
    oracle.args(["run", "--release", "-p", "emcraft-testkit", "--bin", "oracle", "--", "--no-write"]);
    run(oracle, "oracle")
}
