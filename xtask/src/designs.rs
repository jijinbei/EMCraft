//! `cargo xtask designs`: the SolidCraft designs built from their sources
//! (docs/adr/0010-geometry-from-solidcraft.md). Each design `<dir>/<name>.solidcraft` — the
//! templates' in `crates/templates/designs`, an oracle case's in its folder — comes from a
//! SolidCraft command script beside it, `<name>.script.json`, run by SolidCraft's own command
//! line (`solidcraft-cli run <script> --save <name>.json`: the design as JSON, no evaluated cache)
//! at the commit EMCraft pins (the workspace's Cargo.toml): `$SOLIDCRAFT_CLI`, else
//! `solidcraft-cli` on the PATH. An oracle case holding a template's design by the same file name
//! holds the same bytes. `--check` builds them aside and fails when one differs from its file.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The SolidCraft command line to run.
fn cli() -> String {
    std::env::var("SOLIDCRAFT_CLI").ok().filter(|p| !p.trim().is_empty()).unwrap_or_else(|| "solidcraft-cli".to_owned())
}

/// The entries of `dir`, sorted (none when it cannot be read).
fn entries(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().filter_map(Result::ok).map(|e| e.path()).collect();
    out.sort();
    out
}

/// The design scripts in `dir`: each design's name and script.
fn scripts(dir: &Path) -> Vec<(String, PathBuf)> {
    entries(dir)
        .into_iter()
        .filter_map(|p| {
            let name = p.file_name()?.to_str()?.strip_suffix(".script.json")?.to_owned();
            Some((name, p))
        })
        .collect()
}

/// The design built from `script` (named `name`), its bytes, built in `aside`.
fn build(cli: &str, script: &Path, name: &str, aside: &Path) -> Result<Vec<u8>, String> {
    let out = aside.join(format!("{name}.json"));
    let done = Command::new(cli).arg("run").arg(script).args(["--quiet", "--save"]).arg(&out).output().map_err(|e| {
        format!("`{cli}` did not run ({e}): build it at the commit EMCraft pins (cargo build -p solidcraft-cli in SolidCraft) and set SOLIDCRAFT_CLI")
    })?;
    if !done.status.success() {
        return Err(format!("{}: {}", script.display(), String::from_utf8_lossy(&done.stderr).trim()));
    }
    std::fs::read(&out).map_err(|e| format!("{}: {e}", out.display()))
}

/// Where the designs are built aside: the cargo target directory's `xtask-designs`.
fn aside(root: &Path) -> PathBuf {
    let target = std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from);
    let target = if target.is_absolute() { target } else { root.join(target) };
    target.join("xtask-designs")
}

/// A design to build: its name, its script and the files it is written to (its own and, for a
/// template's, the oracle cases' copies).
struct Design {
    name: String,
    script: PathBuf,
    files: Vec<PathBuf>,
}

/// Every design with a script: the templates' and the oracle cases'.
fn designs(root: &Path) -> Vec<Design> {
    let templates = root.join("crates/templates/designs");
    let cases: Vec<PathBuf> = entries(&root.join("tests/oracle")).into_iter().filter(|p| p.is_dir()).collect();
    std::iter::once(&templates)
        .chain(&cases)
        .flat_map(|dir| scripts(dir).into_iter().map(move |(name, script)| (dir, name, script)))
        .map(|(dir, name, script)| {
            let file = format!("{name}.solidcraft");
            let copies = cases.iter().map(|c| c.join(&file)).filter(|p| *dir == templates && p.exists());
            let files = std::iter::once(dir.join(&file)).chain(copies).collect();
            Design { name, script, files }
        })
        .collect()
}

pub fn run(root: &Path, args: &[&str]) -> Result<(), String> {
    let check = args.contains(&"--check");
    let cli = cli();
    let aside = aside(root);
    std::fs::create_dir_all(&aside).map_err(|e| format!("{}: {e}", aside.display()))?;
    let shown = |p: &Path| p.strip_prefix(root).unwrap_or(p).display().to_string();
    let mut differ = Vec::new();
    let mut count = 0;
    for d in designs(root) {
        let bytes = build(&cli, &d.script, &d.name, &aside)?;
        for file in &d.files {
            if !check {
                std::fs::write(file, &bytes).map_err(|e| format!("{}: {e}", shown(file)))?;
                println!("{}", shown(file));
            } else if std::fs::read(file).ok().as_deref() != Some(bytes.as_slice()) {
                differ.push(shown(file));
            }
            count += 1;
        }
    }
    if !differ.is_empty() {
        return Err(format!("built differently from the files: {} (cargo xtask designs writes them)", differ.join(", ")));
    }
    println!("{count} designs {}", if check { "match their scripts" } else { "written" });
    Ok(())
}
