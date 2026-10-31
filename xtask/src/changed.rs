//! The packages a change touches: those owning a changed file, and every workspace package that
//! depends on one of them (normal, dev or build dependency, through any number of steps), so
//! `cargo xtask test --changed` runs only the tests the change can break. A change to the
//! workspace's own files (its manifest, lock file, toolchain, test runner's configuration)
//! touches everything; files outside any package that no build reads (docs, the oracle's cases)
//! touch nothing.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::Value;

/// Files at the root whose change rebuilds or reruns everything.
const EVERYTHING: [&str; 5] = ["Cargo.toml", "Cargo.lock", "rust-toolchain.toml", ".cargo/", ".config/"];

/// What a change touches.
#[derive(Debug, PartialEq)]
pub enum Affected {
    All,
    /// These packages, by name (none for a change to docs alone).
    Some(BTreeSet<String>),
}

/// The packages `changed` (paths relative to the workspace root, `/`-separated) touches, from
/// `cargo metadata`'s packages (`root` the workspace's absolute path).
pub fn affected(meta: &Value, root: &Path, changed: &[String]) -> Result<Affected, String> {
    if changed.iter().any(|f| EVERYTHING.iter().any(|e| f == e || (e.ends_with('/') && f.starts_with(e)))) {
        return Ok(Affected::All);
    }
    let crates = crate::layers::from_metadata(meta)?;
    // Each package's directory, relative to the root, longest first so a nested package wins.
    let mut dirs: Vec<(String, String)> = meta["packages"]
        .as_array()
        .ok_or("metadata: no packages array")?
        .iter()
        .filter_map(|p| {
            let manifest = Path::new(p["manifest_path"].as_str()?);
            let dir = manifest.parent()?.strip_prefix(root).ok()?;
            Some((format!("{}/", dir.to_str()?), p["name"].as_str()?.to_owned()))
        })
        .collect();
    dirs.sort_by_key(|(d, _)| std::cmp::Reverse(d.len()));
    let mut hit: BTreeSet<String> = changed.iter().filter_map(|f| dirs.iter().find(|(d, _)| f.starts_with(d.as_str())).map(|(_, n)| n.clone())).collect();
    // Who depends on whom, within the workspace.
    let mut users: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for c in &crates {
        for d in c.deps.iter().filter(|d| d.workspace) {
            users.entry(d.name.as_str()).or_default().push(c.name.as_str());
        }
    }
    let mut next: Vec<String> = hit.iter().cloned().collect();
    while let Some(name) = next.pop() {
        for user in users.get(name.as_str()).into_iter().flatten() {
            if hit.insert((*user).to_owned()) {
                next.push((*user).to_owned());
            }
        }
    }
    Ok(Affected::Some(hit))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn meta() -> Value {
        let pkg = |name: &str, dir: &str, deps: &[(&str, &str)]| {
            json!({"name": name, "manifest_path": format!("/w/{dir}/Cargo.toml"),
                   "dependencies": deps.iter().map(|(n, k)| json!({"name": n, "kind": if k.is_empty() { Value::Null } else { json!(k) }, "path": "/w"})).collect::<Vec<_>>()})
        };
        json!({"packages": [
            pkg("model", "crates/model", &[]),
            pkg("solver", "crates/solver", &[("model", "")]),
            pkg("engine", "crates/engine", &[("solver", "")]),
            pkg("ui", "crates/ui", &[("engine", "")]),
            pkg("testkit", "crates/testkit", &[("model", "dev")]),
            pkg("xtask", "xtask", &[]),
        ]})
    }

    fn names(a: Affected) -> Vec<String> {
        match a {
            Affected::All => vec!["*".into()],
            Affected::Some(s) => s.into_iter().collect(),
        }
    }

    #[test]
    fn a_change_reaches_its_package_and_everything_built_on_it() {
        let m = meta();
        let at = |files: &[&str]| names(affected(&m, Path::new("/w"), &files.iter().map(|f| f.to_string()).collect::<Vec<_>>()).unwrap());
        assert_eq!(at(&["crates/engine/src/lib.rs"]), ["engine", "ui"]);
        // Through dev dependencies too.
        assert_eq!(at(&["crates/model/src/x.rs"]), ["engine", "model", "solver", "testkit", "ui"]);
        assert_eq!(at(&["crates/ui/tests/scenarios/a.json", "xtask/src/main.rs"]), ["ui", "xtask"]);
        // Docs and the oracle's cases: nothing; the workspace's own files: everything.
        assert!(at(&["docs/a.md", "tests/oracle/x/recipe.json", "README.md"]).is_empty());
        assert_eq!(at(&["Cargo.lock"]), ["*"]);
        assert_eq!(at(&[".config/nextest.toml"]), ["*"]);
    }
}
