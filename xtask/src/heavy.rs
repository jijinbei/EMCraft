//! The heavy tests: those ignored with a reason starting "heavy" (AGENTS.md), as a nextest
//! filter. Other ignored tests (a GPU's, benchmarks, timing reports, tours to look at) are left
//! out: a runner without the hardware or a person looking cannot pass them.

use std::path::{Path, PathBuf};

/// The names of the functions under `#[ignore = "heavy…"]` in the `.rs` files under `dirs`,
/// sorted and without repeats.
pub fn names(dirs: &[PathBuf]) -> Result<Vec<String>, String> {
    let mut files = Vec::new();
    for d in dirs {
        rust_files(d, &mut files)?;
    }
    let mut out = Vec::new();
    for f in files {
        let text = std::fs::read_to_string(&f).map_err(|e| format!("{}: {e}", f.display()))?;
        out.extend(in_source(&text));
    }
    out.sort();
    out.dedup();
    Ok(out)
}

/// The heavy tests of one source: the first `fn` after each heavy ignore.
fn in_source(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        if !line.trim_start().starts_with("#[ignore = \"heavy") {
            continue;
        }
        if let Some(name) = lines.by_ref().take(8).find_map(fn_name) {
            out.push(name);
        }
    }
    out
}

/// The name a line declares a function by.
fn fn_name(line: &str) -> Option<String> {
    let rest = line.trim_start().strip_prefix("fn ").or_else(|| line.trim_start().strip_prefix("async fn "))?;
    let name: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
    (!name.is_empty()).then_some(name)
}

/// A nextest filter for exactly these tests (by their last path segment).
pub fn filter(names: &[String]) -> String {
    format!("test(/(^|::)({})$/)", names.join("|"))
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Ok(()) };
    let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            if p.file_name().is_none_or(|n| n != "target") {
                rust_files(&p, out)?;
            }
        } else if p.extension().is_some_and(|e| e == "rs") {
            out.push(p);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heavy_ignores_are_found_and_the_others_left_out() {
        let src = "#[test]\n#[ignore = \"heavy: fine meshes\"]\nfn converges() {}\n\
                   #[test]\n#[ignore = \"needs an NVIDIA GPU\"]\nfn on_cuda() {}\n\
                   #[test]\n    #[ignore = \"heavy: cargo xtask test --heavy\"]\n    #[allow(x)]\n    fn nested_one() {}\n";
        assert_eq!(in_source(src), ["converges", "nested_one"]);
        assert_eq!(filter(&["a".into(), "b_c".into()]), "test(/(^|::)(a|b_c)$/)");
    }
}
