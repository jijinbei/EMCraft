//! `cargo xtask style`: the rules of docs/style.md a machine can check, over every Rust source
//! of the workspace (production code and tests).
//!
//! - **cited**: a test in a physics crate that checks a closed form — its code calls
//!   `emcraft_theory::`, or its comments speak of a closed form or an exact solution, cascade,
//!   line, transform or series — names its source in a comment: a work of
//!   `docs/solvers/literature.md` (an author's surname found there), or says `Derived here`. A
//!   test whose comments say it has no closed form, or that it is a cross-check, is not one.
//! - **catalogued**: a citation in a comment — `(Surname, …)`, `(Surname & …)`, `(Surname et al …)`
//!   with a year, a section, an equation, a table or a chapter — names a work
//!   `docs/solvers/literature.md` lists.
//! - **no debugging left**: no `dbg!` anywhere; no `println!` or `eprintln!` in a library's
//!   production code (tests, binaries, examples, build scripts, the apps, the oracle runner and
//!   xtask print by design; the window's notices in [`PRINTING_FILES`] too).
//! - **no dead code**: no comment that is a line of code (`// let x = …;`).
//! - **width**: no comment line wider than rustfmt's 150 columns (rustfmt leaves comments alone);
//!   a doc comment's code block (between ` ``` ` fences) is code, not prose, and left out.

use std::path::{Path, PathBuf};

/// The crates whose tests check physics against closed forms.
const PHYSICS: &[&str] = &["crates/theory/", "crates/fem/", "crates/solver/", "crates/fdtd/", "crates/engine/", "crates/testkit/"];

/// Words in a test's comments that say it checks a closed form.
const CLOSED_FORM_WORDS: &[&str] =
    &["closed form", "closed-form", "exact solution", "exact cascade", "exact line", "exact transform", "exact series"];

/// Words that say it does not.
const NOT_CLOSED_FORM_WORDS: &[&str] = &["no closed form", "without a closed form", "cross-check"];

/// What a derivation made here says instead of a citation.
const DERIVED: &str = "Derived here";

/// Sources named by an organisation rather than an author.
const ORGANISATIONS: &[&str] = &["EIA", "IEC", "IBIS", "DLMF"];

/// Library files whose prints are the window's notices on stderr (no log facade yet): the GPU
/// failing under a window's 3D view, a picture of the iced window that could not be written.
const PRINTING_FILES: &[&str] = &[
    "crates/ui-egui/src/lib.rs",
    "crates/ui-egui/src/gpu3d.rs",
    "crates/gpu3d/src/headless.rs",
    "crates/ui-iced/src/app.rs",
    "crates/ui-iced/src/view3d/shader.rs",
];

/// The widest comment line (rustfmt's `max_width`).
const MAX_COMMENT_WIDTH: usize = 150;

/// One breach of a rule: where, which rule and what.
#[derive(Debug, PartialEq)]
pub struct Breach {
    pub path: String,
    pub line: usize,
    pub rule: &'static str,
    pub what: String,
}

/// The surnames (and organisations) of the works `literature` lists: a capitalised word after
/// initials (`D. M. Pozar`), or before them (`Pozar, D. M.`).
pub fn surnames(literature: &str) -> Vec<String> {
    let words: Vec<&str> = literature.split_whitespace().collect();
    let initial = |w: &str| {
        let w = w.trim_end_matches([',', ';']);
        let b = w.as_bytes();
        b.len() >= 2 && b[0].is_ascii_uppercase() && w.ends_with('.') && w[..w.len() - 1].split(['.', '-']).all(|p| p.len() <= 1)
    };
    let name = |w: &str| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'' && c != '’' && c != '-').to_owned();
    let capitalised = |w: &str| w.chars().next().is_some_and(char::is_uppercase) && w.chars().skip(1).any(char::is_lowercase);
    let after = words.windows(2).filter(|p| initial(p[0]) && !initial(p[1])).map(|p| name(p[1]));
    let before = words.windows(2).filter(|p| p[0].ends_with(',') && initial(p[1])).map(|p| name(p[0]));
    let mut all: Vec<String> = after.chain(before).filter(|w| capitalised(w)).chain(ORGANISATIONS.iter().map(|o| (*o).to_owned())).collect();
    all.sort();
    all.dedup();
    all
}

/// Whether `text` holds `word` as a whole word.
fn has_word(text: &str, word: &str) -> bool {
    text.match_indices(word).any(|(i, _)| {
        let before = text[..i].chars().next_back();
        let after = text[i + word.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

/// A comment's text (after `//`, `///` or `//!`), or none for a line of code.
fn comment(line: &str) -> Option<&str> {
    let t = line.trim_start();
    t.strip_prefix("//").map(|c| c.trim_start_matches(['/', '!']).trim())
}

/// Whether a parenthesised group's text carries a citation's evidence: a year, a section, an
/// equation, a table or a chapter.
fn evidence(group: &str) -> bool {
    let year =
        group.split(|c: char| !c.is_ascii_digit()).any(|d| d.len() == 4 && (d.starts_with("18") || d.starts_with("19") || d.starts_with("20")));
    year || group.contains('§')
        || ["eq. ", "eq.", "Table ", "ch. "].iter().any(|k| group.split(k).skip(1).any(|r| r.starts_with(|c: char| c.is_ascii_digit())))
}

/// The citations of a comment: each `(Surname, …)`-like group with evidence, by its surname.
fn citations(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    for (open, _) in text.match_indices('(') {
        let rest = &text[open + 1..];
        // The group to its matching bracket.
        let mut depth = 1;
        let Some(end) = rest.char_indices().find_map(|(i, c)| {
            depth += match c {
                '(' => 1,
                ')' => -1,
                _ => 0,
            };
            (depth == 0).then_some(i)
        }) else {
            continue;
        };
        let group = &rest[..end];
        let word_end = group.find(|c: char| !(c.is_alphabetic() || c == '\'' || c == '’' || c == '-')).unwrap_or(group.len());
        let (word, after) = group.split_at(word_end);
        let starts_citation = word.chars().next().is_some_and(char::is_uppercase)
            && word.chars().skip(1).any(char::is_lowercase)
            && [",", " &", " and ", " et al"].iter().any(|s| after.starts_with(s));
        if starts_citation && evidence(group) {
            out.push(word);
        }
    }
    out
}

/// A line of code written as a comment.
fn dead_code(text: &str) -> bool {
    let keyword = ["let ", "fn ", "use ", "if ", "for ", "while ", "match ", "return ", "pub ", "impl ", "struct ", "enum ", "mod "]
        .iter()
        .any(|k| text.starts_with(k));
    let ends = text.ends_with(';') || text.ends_with('{');
    let call =
        text.ends_with(");") && text.split('(').next().is_some_and(|f| !f.is_empty() && f.chars().all(|c| c.is_alphanumeric() || "_.:".contains(c)));
    let assign = text.ends_with(';')
        && text.split_once(" = ").is_some_and(|(l, _)| !l.is_empty() && l.chars().all(|c| c.is_alphanumeric() || "_.[]".contains(c)));
    (keyword && ends) || call || assign || text == "}"
}

/// The columns a line takes (wide characters two).
fn width(line: &str) -> usize {
    line.chars().map(|c| if ('\u{1100}'..='\u{115F}').contains(&c) || ('\u{2E80}'..='\u{FFDC}').contains(&c) { 2 } else { 1 }).sum()
}

/// A test function: its name, the line of its `fn`, its comments (those above it and inside)
/// and its code (strings emptied, comments cut).
struct Test {
    name: String,
    line: usize,
    comments: String,
    code: String,
}

/// The test functions of a file's lines.
fn tests(lines: &[&str]) -> Vec<Test> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let t = lines[i].trim_start();
        if !(t.starts_with("#[test]") || t.starts_with("#[rstest")) {
            i += 1;
            continue;
        }
        let above: Vec<&str> =
            lines[..i].iter().rev().take_while(|l| l.trim_start().starts_with("//") || l.trim_start().starts_with("#[")).copied().collect();
        let Some(f) = (i..lines.len()).find(|k| lines[*k].contains("fn ")) else { break };
        let name = lines[f].split("fn ").nth(1).map(|r| r.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect()).unwrap_or_default();
        // The body: to the brace that closes it, strings emptied.
        let mut depth = 0i64;
        let mut opened = false;
        let mut state = crate::colours::State::Code;
        let mut end = f;
        let mut code = String::new();
        for (k, line) in lines.iter().enumerate().skip(f) {
            let (c, next) = crate::colours::code_of(line, state);
            state = next;
            depth += c.matches('{').count() as i64 - c.matches('}').count() as i64;
            opened |= c.contains('{');
            code.push_str(&c);
            code.push('\n');
            end = k;
            if opened && depth <= 0 {
                break;
            }
        }
        let comments = above.iter().rev().chain(&lines[f..=end]).filter_map(|l| comment(l)).collect::<Vec<_>>().join("\n");
        out.push(Test { name, line: f + 1, comments, code });
        i = end + 1;
    }
    out
}

/// Whether a test checks a closed form (see the module docs).
fn checks_closed_form(t: &Test) -> bool {
    let lower = t.comments.to_lowercase();
    let says = CLOSED_FORM_WORDS.iter().any(|w| lower.contains(w));
    let denies = NOT_CLOSED_FORM_WORDS.iter().any(|w| lower.contains(w));
    (t.code.contains("emcraft_theory::") || says) && !denies
}

/// The breaches of one file (`path` relative to the workspace, `/`-separated).
pub fn scan(path: &str, text: &str, surnames: &[String]) -> Vec<Breach> {
    let lines: Vec<&str> = text.lines().collect();
    let breach = |line: usize, rule: &'static str, what: String| Breach { path: path.to_owned(), line, rule, what };
    let known = |w: &str| surnames.iter().any(|s| s == w);
    let mut out = Vec::new();
    // Per line: catalogued citations, dead code, width, prints.
    let test_file = path.contains("/tests/") || path.ends_with("tests.rs") || path.contains("/examples/") || path.ends_with("build.rs");
    let printing = test_file
        || path.starts_with("apps/")
        || path.starts_with("xtask/")
        || path.starts_with("crates/testkit/")
        || path.contains("/src/bin/")
        || PRINTING_FILES.contains(&path);
    let mut state = crate::colours::State::Code;
    let mut in_tests = false;
    let mut fenced = false;
    for (i, line) in lines.iter().enumerate() {
        in_tests |= state == crate::colours::State::Code && line.trim_start().starts_with("#[cfg(test)]");
        let (code, next) = crate::colours::code_of(line, state);
        let was_code = state == crate::colours::State::Code;
        state = next;
        if let Some(c) = comment(line).filter(|_| was_code) {
            out.extend(
                citations(c)
                    .into_iter()
                    .filter(|w| !known(w))
                    .map(|w| breach(i + 1, "catalogued", format!("`{w}` is cited but not in docs/solvers/literature.md"))),
            );
            if !line.trim_start().starts_with("///") && !line.trim_start().starts_with("//!") && dead_code(c) {
                out.push(breach(i + 1, "dead code", format!("a line of code as a comment: `{c}`")));
            }
            fenced ^= c.starts_with("```");
            if !fenced && !c.starts_with("```") && width(line) > MAX_COMMENT_WIDTH {
                out.push(breach(i + 1, "width", format!("{} columns (at most {MAX_COMMENT_WIDTH})", width(line))));
            }
        }
        if code.contains("dbg!(") {
            out.push(breach(i + 1, "debugging", "`dbg!` left in".into()));
        }
        if !printing && !in_tests && (code.contains("println!(") || code.contains("eprintln!(")) {
            out.push(breach(i + 1, "debugging", "a print in library code (a reply or an error says it instead)".into()));
        }
    }
    // Per test: a closed form cited.
    if PHYSICS.iter().any(|p| path.starts_with(p)) {
        for t in tests(&lines).iter().filter(|t| checks_closed_form(t)) {
            let cited = t.comments.contains(DERIVED) || surnames.iter().any(|s| has_word(&t.comments, s));
            if !cited {
                out.push(breach(
                    t.line,
                    "cited",
                    format!("`{}` checks a closed form: cite its source (docs/style.md §8) or say `{DERIVED}`", t.name),
                ));
            }
        }
    }
    out
}

/// Every `.rs` file under `dir`, sorted.
fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let mut entries: Vec<PathBuf> =
        std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            if p.file_name().is_some_and(|n| n != "target" && !n.to_string_lossy().starts_with('.')) {
                rust_files(&p, out)?;
            }
        } else if p.extension().is_some_and(|e| e == "rs") {
            out.push(p);
        }
    }
    Ok(())
}

/// Runs the check over the crates, the apps and xtask: prints every breach; fails when there is
/// one.
pub fn run(root: &Path) -> Result<(), String> {
    let literature = std::fs::read_to_string(root.join("docs/solvers/literature.md")).map_err(|e| format!("docs/solvers/literature.md: {e}"))?;
    let names = surnames(&literature);
    let mut files = Vec::new();
    for dir in ["crates", "apps", "xtask"] {
        rust_files(&root.join(dir), &mut files)?;
    }
    let mut found = Vec::new();
    for path in files {
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
        found.extend(scan(&rel, &text, &names));
    }
    for b in &found {
        println!("{}:{}  [{}] {}", b.path, b.line, b.rule, b.what);
    }
    if found.is_empty() {
        println!("style: ok");
        Ok(())
    } else {
        Err(format!("style: {} breach(es) of docs/style.md", found.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> Vec<String> {
        surnames("- D. M. Pozar, *Microwave Engineering*, 4th ed.; R. E. Collin, *Field Theory*; Monk, P., *FEM*.")
    }

    #[test]
    fn surnames_come_after_initials_or_before_them() {
        assert_eq!(names(), ["Collin", "DLMF", "EIA", "IBIS", "IEC", "Monk", "Pozar"]);
    }

    #[test]
    fn a_closed_form_test_needs_a_source() {
        let bare = "#[test]\nfn te10() {\n    // Against the closed form.\n    let x = emcraft_theory::f();\n}\n";
        let found = scan("crates/solver/tests/a.rs", bare, &names());
        assert_eq!(found.iter().map(|b| b.rule).collect::<Vec<_>>(), ["cited"]);
        // Cited, derived, or not a closed form at all: fine.
        for ok in [
            "#[test]\nfn te10() {\n    // TE10 (Pozar, Microwave Engineering, §3.3).\n    let x = emcraft_theory::f();\n}\n",
            "#[test]\nfn q() {\n    // Derived here from ω₀/√(1 − j tan δ): the closed form.\n}\n",
            "#[test]\nfn x() {\n    // A cross-check: no closed form.\n    let x = emcraft_theory::f();\n}\n",
        ] {
            assert!(scan("crates/solver/tests/a.rs", ok, &names()).is_empty(), "{ok}");
        }
        // Outside the physics crates the rule does not apply.
        assert!(scan("crates/mesh/src/a.rs", bare, &names()).is_empty());
    }

    #[test]
    fn a_citation_names_a_listed_work() {
        let text = "// The step (Pozar, Microwave Engineering, §4.4) and the post (Lewin, Theory of Waveguides, 1975).\n";
        let found = scan("crates/theory/src/a.rs", text, &names());
        assert_eq!(found.len(), 1);
        assert!(found[0].what.contains("Lewin"));
        // A bracket without evidence is no citation: (None, the default), (Wexler 15(9):508–517) nests.
        assert!(scan("crates/theory/src/a.rs", "// (None, the default) (Edit, then Undo)\n", &names()).is_empty());
        assert_eq!(citations("(Wexler, IEEE Trans. MTT 15(9):508–517, 1967)"), ["Wexler"]);
    }

    #[test]
    fn debugging_and_dead_code_and_width_are_found() {
        let text = format!("fn f() {{\n    dbg!(1);\n    println!(\"x\");\n    // let y = 2;\n    // {}\n}}\n", "w".repeat(160));
        let rules: Vec<&str> = scan("crates/solver/src/a.rs", &text, &names()).iter().map(|b| b.rule).collect();
        assert_eq!(rules, ["debugging", "debugging", "dead code", "width"]);
        // Tests, binaries and xtask may print; a print in a string is no print.
        assert!(scan("crates/solver/tests/a.rs", "fn f() { println!(\"x\"); }\n", &names()).is_empty());
        assert!(scan("xtask/src/a.rs", "fn f() { println!(\"x\"); }\n", &names()).is_empty());
        assert!(scan("crates/solver/src/a.rs", "const S: &str = \"println!(\";\n", &names()).is_empty());
        // A doc example's code is code: its width is not a comment's.
        let doc = format!("/// ```\n/// let x = {};\n/// ```\n", "1 + ".repeat(60));
        assert!(scan("crates/solver/src/a.rs", &doc, &names()).is_empty());
        // Prose ending in a semicolon is not code.
        assert!(scan("crates/solver/src/a.rs", "// The walls (PEC); the ports.\n", &names()).is_empty());
    }
}
