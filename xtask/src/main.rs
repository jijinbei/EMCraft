//! Workspace tooling: `cargo xtask <command>`.
//!
//! Pure Rust (std + serde_json). External tools (`cargo`) are invoked through
//! `std::process::Command`. Adapted from FieldCraft's xtask (MIT OR Apache-2.0), see
//! ATTRIBUTION.md.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod changed;
mod colours;
mod dupes;
#[cfg(feature = "fonts")]
mod fonts;
mod installer;
mod layers;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const USAGE: &str = "\
usage: cargo xtask <command>

commands:
  layers          enforce the crate dependency layering (docs/architecture.md)
  test [--heavy] [--changed [BASE]] [CARGO ARGS] [-- TEST ARGS]
                  run the workspace tests (with cargo-nextest when installed: every test in one
                  pool, .config/nextest.toml; the doctests after); --heavy runs only the ignored
                  (heavy) tests: convergence studies and fine meshes; --changed only the
                  packages changed since BASE (default main; committed or not) and those built
                  on them
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
  colours         fail on colours written in production code (Rgb(30, …), Color32::from_rgb(…)
                  with numbers): a theme's colours live in its file (docs/themes.md)
  icon            write apps/emcraft/assets/emcraft.ico (the app icon, drawn by
                  emcraft_render::icon)
  font-subset     write the embedded fonts' subsets from the full fonts beside them
                  (crates/ui-egui/fonts/full, crates/render/fonts/full), checked glyph by
                  glyph against them
  installer-windows [--out FILE]
                  build the Windows installer on Linux: release binaries cross-compiled with
                  MinGW-w64, packed by NSIS (both fetched through nix-shell when missing;
                  docs/install-windows.md); default FILE target/EMCraft-VERSION-setup.exe
  ci              fmt --check, layers, dupes --fail, colours, clippy -D warnings, the tests,
                  then the oracle
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
        Some("colours" | "colors") => colours::run(&root()),
        Some("icon") => installer::icon(&root()),
        Some("font-subset") => font_subset(),
        Some("installer-windows") => installer::windows(&root(), &rest),
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
    let (mut heavy, mut changed, mut rest) = (false, None, rest);
    loop {
        match rest {
            ["--heavy", after @ ..] => (heavy, rest) = (true, after),
            ["--changed", base, after @ ..] if !base.starts_with('-') => (changed, rest) = (Some(*base), after),
            ["--changed", after @ ..] => (changed, rest) = (Some("main"), after),
            _ => break,
        }
    }
    let (cargo_args, test_args) = rest.iter().position(|a| *a == "--").map_or((rest, &[][..]), |i| (&rest[..i], &rest[i + 1..]));
    let packages: Vec<String> = match changed.map(changed_packages).transpose()? {
        None | Some(changed::Affected::All) => vec!["--workspace".into()],
        Some(changed::Affected::Some(names)) if names.is_empty() => {
            println!("tests: nothing changed that a test builds from");
            return Ok(());
        }
        Some(changed::Affected::Some(names)) => {
            println!("tests: {}", names.iter().cloned().collect::<Vec<_>>().join(", "));
            names.into_iter().flat_map(|n| ["-p".into(), n]).collect()
        }
    };
    let what = if heavy { "heavy tests" } else { "tests" };
    if !nextest() {
        let mut c = cargo();
        c.args(["test"]).args(&packages).args(cargo_args).arg("--");
        if heavy {
            c.arg("--ignored");
        }
        c.args(test_args);
        return run(c, what);
    }
    let mut c = cargo();
    c.args(["nextest", "run", "--no-tests", "pass"]).args(&packages).args(cargo_args);
    if heavy {
        c.args(["--run-ignored", "only"]);
    }
    c.arg("--").args(test_args);
    run(c, what)?;
    // nextest leaves the doctests out; the heavy ones are none of them.
    if heavy {
        return Ok(());
    }
    let mut doc = cargo();
    doc.args(["test", "--doc"]).args(&packages).args(cargo_args).arg("--").args(test_args);
    run(doc, "doctests")
}

/// Whether cargo-nextest is installed.
fn nextest() -> bool {
    cargo().args(["nextest", "--version"]).output().is_ok_and(|o| o.status.success())
}

/// The packages the files changed since `base` (its merge base with HEAD: commits, the working
/// tree and untracked files) touch.
fn changed_packages(base: &str) -> Result<changed::Affected, String> {
    let git = |args: &[&str]| -> Result<String, String> {
        let out = Command::new("git").args(args).current_dir(root()).output().map_err(|e| format!("git: {e}"))?;
        if !out.status.success() {
            return Err(format!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    };
    let from = git(&["merge-base", base, "HEAD"])?;
    let mut files: Vec<String> = git(&["diff", "--name-only", from.trim()])?.lines().map(str::to_owned).collect();
    files.extend(git(&["ls-files", "--others", "--exclude-standard"])?.lines().map(str::to_owned));
    let out = cargo().args(["metadata", "--format-version", "1", "--no-deps"]).output().map_err(|e| format!("cargo metadata: {e}"))?;
    let meta: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("cargo metadata: {e}"))?;
    let root = meta["workspace_root"].as_str().map(PathBuf::from).unwrap_or_else(root);
    changed::affected(&meta, &root, &files)
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

#[cfg(feature = "fonts")]
fn font_subset() -> Result<(), String> {
    fonts::run(&root())
}

/// The subsets need the `fonts` feature's crates: the xtask runs again with them.
#[cfg(not(feature = "fonts"))]
fn font_subset() -> Result<(), String> {
    let mut c = cargo();
    c.args(["run", "--package", "xtask", "--features", "fonts", "--", "font-subset"]);
    run(c, "font-subset")
}

fn cmd_ci() -> Result<(), String> {
    let mut fmt = cargo();
    fmt.args(["fmt", "--all", "--check"]);
    run(fmt, "fmt")?;
    cmd_layers()?;
    dupes::run(&root(), dupes::DEFAULT_MIN, true, false)?;
    colours::run(&root())?;
    let mut clippy = cargo();
    clippy.args(["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]);
    run(clippy, "clippy")?;
    cmd_test(&[])?;
    let mut oracle = cargo();
    oracle.args(["run", "--release", "-p", "emcraft-testkit", "--bin", "oracle", "--", "--no-write"]);
    run(oracle, "oracle")
}
