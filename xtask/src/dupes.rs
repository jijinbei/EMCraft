//! `cargo xtask dupes`: copied code across the workspace's production sources.
//!
//! Each file's lines are normalised (comments and surrounding blanks dropped, lines of only
//! punctuation such as `}` or `);` skipped), and every run of `min` consecutive normalised lines
//! is hashed. A run found in two places, not overlapping, is a copy; neighbouring runs of the same
//! pair of places are joined into one range. Tests (`tests/` directories, `tests.rs` files and
//! everything from a `#[cfg(test)]` line to the end of a file) are left out: repeated set-ups in
//! tests are fine. So are the ported tetmesh crate and declarative tables (`SKIP_CRATES`,
//! `SKIP_FILES`).
//!
//! With `loose`, each line is reduced to its shape before matching: names become `v`, numbers `0`
//! and string literals `s`, keywords and punctuation stay. That also finds a routine copied with
//! its variables renamed or its scalar type changed (`f64` → `C`), at the price of more look-alike
//! lines that are not worth sharing.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Ported code kept close to its upstream, and so left out (ATTRIBUTION.md).
const SKIP_CRATES: &[&str] = &["tetmesh"];

/// Declarative tables, whose entries look alike by design.
const SKIP_FILES: &[&str] = &["crates/engine/src/cmd/table.rs"];

/// The default length of a copy, in normalised lines.
pub const DEFAULT_MIN: usize = 10;

/// One copy: the same `lines` normalised lines at `a` and at `b` (file, first and last line).
#[derive(Debug, PartialEq)]
pub struct Copy {
    pub a: (PathBuf, usize, usize),
    pub b: (PathBuf, usize, usize),
    pub lines: usize,
}

/// A file's normalised lines with their line numbers (1-based).
struct Source {
    path: PathBuf,
    lines: Vec<(usize, String)>,
}

/// The normalised form of a line, or `None` when it carries nothing worth matching.
fn normalise(line: &str) -> Option<String> {
    let code = match line.find("//") {
        // A `//` inside a string literal is rare enough in this code base to ignore.
        Some(i) => &line[..i],
        None => line,
    };
    let code = code.trim();
    if code.chars().all(|c| matches!(c, '{' | '}' | '(' | ')' | '[' | ']' | ';' | ',' | ' ')) {
        return None;
    }
    Some(code.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// Rust's keywords, kept by `shape` (everything else that looks like a name becomes `v`).
const KEYWORDS: &[&str] = &[
    "as", "break", "const", "continue", "crate", "else", "enum", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move",
    "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where", "while", "dyn",
];

/// `line` reduced to its shape: names `v`, numbers `0`, string literals `s`, tokens spaced.
fn shape(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut tokens: Vec<String> = Vec::new();
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        let start = i;
        i += 1;
        if c.is_alphabetic() || c == '_' {
            while chars.get(i).is_some_and(|d| d.is_alphanumeric() || *d == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            tokens.push(if KEYWORDS.contains(&word.as_str()) { word } else { "v".into() });
        } else if c.is_ascii_digit() {
            // A number, its suffix and a decimal point followed by a digit (`2.5`, not `0..n`).
            while let Some(&d) = chars.get(i) {
                let decimal = d == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit);
                if !(d.is_alphanumeric() || d == '_' || decimal) {
                    break;
                }
                i += 1;
            }
            tokens.push("0".into());
        } else if c == '"' {
            let mut escaped = false;
            while let Some(&d) = chars.get(i) {
                i += 1;
                match (escaped, d) {
                    (false, '\\') => escaped = true,
                    (false, '"') => break,
                    _ => escaped = false,
                }
            }
            tokens.push("s".into());
        } else if !c.is_whitespace() {
            tokens.push(c.to_string());
        }
    }
    tokens.join(" ")
}

fn source(path: PathBuf, text: &str, loose: bool) -> Source {
    let mut lines = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("#[cfg(test)]") {
            break;
        }
        if let Some(n) = normalise(line) {
            lines.push((i + 1, if loose { shape(&n) } else { n }));
        }
    }
    Source { path, lines }
}

/// The `.rs` files under `dir`, sorted, without tests.
fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let mut entries: Vec<PathBuf> =
        std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for p in entries {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if p.is_dir() {
            if name != "tests" {
                rust_files(&p, out)?;
            }
        } else if name.ends_with(".rs") && name != "tests.rs" {
            out.push(p);
        }
    }
    Ok(())
}

/// The `.rs` files of every crate under `root/crates` and of the apps and xtask, sorted, without
/// tests (`tests/` directories, `tests.rs` files).
pub fn production_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut dirs = Vec::new();
    for parent in ["crates", "apps"] {
        let Ok(rd) = std::fs::read_dir(root.join(parent)) else { continue };
        let mut found: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
        found.sort();
        dirs.extend(found);
    }
    dirs.push(root.join("xtask"));
    let mut files = Vec::new();
    for d in dirs {
        let src = d.join("src");
        if src.is_dir() {
            rust_files(&src, &mut files)?;
        }
    }
    Ok(files)
}

/// The production sources of every crate under `root/crates` and of the apps and xtask.
fn sources(root: &Path, loose: bool) -> Result<Vec<Source>, String> {
    let skipped = |p: &Path| SKIP_CRATES.iter().any(|c| p.starts_with(root.join("crates").join(c)));
    production_files(root)?
        .into_iter()
        .filter(|p| !skipped(p))
        .filter(|p| !SKIP_FILES.iter().any(|f| p.strip_prefix(root).is_ok_and(|r| r == Path::new(f))))
        .map(|p| {
            let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            let rel = p.strip_prefix(root).map(Path::to_path_buf).unwrap_or(p);
            Ok(source(rel, &text, loose))
        })
        .collect()
}

/// The copies of at least `min` normalised lines among `sources`, longest first.
fn copies(sources: &[Source], min: usize) -> Vec<Copy> {
    let min = min.max(2);
    // Every window of `min` lines, by its text, with where it starts (file, index).
    let mut windows: BTreeMap<String, Vec<(usize, usize)>> = BTreeMap::new();
    for (f, s) in sources.iter().enumerate() {
        for i in 0..s.lines.len().saturating_sub(min - 1) {
            let window = &s.lines[i..i + min];
            // A table of look-alike lines (match arms, struct literals) is not a copy.
            if window.iter().map(|(_, l)| l).collect::<BTreeSet<_>>().len() * 2 < min {
                continue;
            }
            let key = window.iter().map(|(_, l)| l.as_str()).collect::<Vec<_>>().join("\n");
            windows.entry(key).or_default().push((f, i));
        }
    }
    // Matching starts, as pairs (a < b), grouped into runs along both files.
    let mut pairs: BTreeSet<((usize, usize), (usize, usize))> = BTreeSet::new();
    for places in windows.values().filter(|p| p.len() > 1) {
        for (k, a) in places.iter().enumerate() {
            for b in &places[k + 1..] {
                // A pattern repeating within reach of itself is not a copy.
                if a.0 == b.0 && b.1 < a.1 + min {
                    continue;
                }
                pairs.insert((*a, *b));
            }
        }
    }
    let mut out = Vec::new();
    for &(a, b) in &pairs {
        if a.1 > 0 && b.1 > 0 && pairs.contains(&((a.0, a.1 - 1), (b.0, b.1 - 1))) {
            continue; // inside a run that started earlier
        }
        let mut n = 1;
        while pairs.contains(&((a.0, a.1 + n), (b.0, b.1 + n))) {
            n += 1;
        }
        let len = n + min - 1;
        let span = |f: usize, i: usize| {
            let s = &sources[f];
            (s.path.clone(), s.lines[i].0, s.lines[i + len - 1].0)
        };
        out.push(Copy { a: span(a.0, a.1), b: span(b.0, b.1), lines: len });
    }
    out.sort_by(|x, y| y.lines.cmp(&x.lines).then_with(|| (&x.a, &x.b).cmp(&(&y.a, &y.b))));
    out
}

/// Runs the check: prints the copies; fails with `fail` when there are any.
pub fn run(root: &Path, min: usize, fail: bool, loose: bool) -> Result<(), String> {
    let found = copies(&sources(root, loose)?, min);
    let total: usize = found.iter().map(|c| c.lines).sum();
    for c in &found {
        println!("{:>4} lines  {}:{}-{}  ~  {}:{}-{}", c.lines, c.a.0.display(), c.a.1, c.a.2, c.b.0.display(), c.b.1, c.b.2);
    }
    println!("dupes: {} copies of {min}+ lines, {total} lines in all", found.len());
    if fail && !found.is_empty() {
        return Err(format!("dupes: {} copies (factor them into a shared module, docs/architecture.md)", found.len()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src(name: &str, text: &str) -> Source {
        source(PathBuf::from(name), text, false)
    }

    #[test]
    fn a_loose_match_sees_through_renamed_variables_and_types() {
        assert_eq!(shape(r#"let s: f64 = a[i] * 2.5 + f("x\"y");"#), "let v : v = v [ v ] * 0 + v ( s ) ;");
        assert_eq!(shape("for k in 0..n { y += c.conj() }"), "for v in 0 . . v { v + = v . v ( ) }");
        // The same routine on reals and on complex numbers, its names changed.
        let real = "let mut l = vec![0.0; n * n];\nfor j in 0..n {\nlet mut d = m[j * n + j];\nfor k in 0..j {\nd -= l[j * n + k] * l[j * n + k];\n}\nif d <= 0.0 {\nreturn None;\n}\nl[j * n + j] = d.sqrt();\n}\n";
        let cplx = "let mut f = vec![C::ZERO; s * s];\nfor c in 0..s {\nlet mut p = a[c * s + c];\nfor r in 0..c {\np -= f[c * s + r] * f[c * s + r].conj();\n}\nif p.re <= 0.0 {\nreturn None;\n}\nf[c * s + c] = p.sqrt();\n}\n";
        let found = copies(&[source("a.rs".into(), real, true), source("b.rs".into(), cplx, true)], 3);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(copies(&[src("a.rs", real), src("b.rs", cplx)], 3).is_empty());
    }

    #[test]
    fn a_block_copied_into_another_file_is_found_once_with_its_full_length() {
        let body: String = (0..12).map(|i| format!("let x{i} = f({i}); // note {i}\n")).collect();
        let a = src("a.rs", &format!("fn a() {{\n{body}}}\n"));
        let b = src("b.rs", &format!("// header\n\nfn b() {{\n{body}}}\n"));
        let found = copies(&[a, b], 5);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].lines, 12);
        assert_eq!(found[0].a, (PathBuf::from("a.rs"), 2, 13));
        assert_eq!(found[0].b, (PathBuf::from("b.rs"), 4, 15));
    }

    #[test]
    fn comments_blanks_braces_and_tests_are_ignored() {
        assert_eq!(normalise("    }"), None);
        assert_eq!(normalise("  );"), None);
        assert_eq!(normalise("let  a = 1; // why"), Some("let a = 1;".into()));
        let s = src("a.rs", "fn f() {}\n#[cfg(test)]\nmod tests { fn g() {} }\n");
        assert_eq!(s.lines.len(), 1);
    }

    #[test]
    fn short_or_self_overlapping_repeats_are_not_copies() {
        let rep: String = (0..15).map(|_| "x += 1;\n").collect();
        assert!(copies(&[src("a.rs", &rep)], 10).is_empty());
        let a = src("a.rs", "a();\nb();\nc();\n");
        let b = src("b.rs", "a();\nb();\nc();\n");
        assert!(copies(&[a, b], 4).is_empty());
    }
}
