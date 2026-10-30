//! `cargo xtask colours`: colours written in code. A theme's colours are data
//! (`crates/engine/themes/*.json`, the user's theme files, docs/themes.md); production code
//! names them by role (a token, a scene colour, a picture pen) and never spells one out. This
//! finds the colour literals that slipped in: `Rgb(` with a number, and egui's `Color32`
//! constructors (`from_rgb`, `from_gray`, `from_rgba_unmultiplied`, `from_rgba_premultiplied`,
//! `from_white_alpha`, `from_black_alpha`) with a number first, and `from_hex` with a text.
//!
//! Tests (as `cargo xtask dupes` reads them: `tests/` directories, `tests.rs` files, everything
//! from a `#[cfg(test)]` line) and comments are left out. So are the scientific colour maps of
//! `crates/render/src/colormap.rs` (a map's colours are its meaning, the same in every theme)
//! and the two constants of `Rgb` itself ([`ALLOWED_LINES`]). `Color32::WHITE` (an image's tint)
//! and `Color32::TRANSPARENT` are no colour of a theme.

use std::path::{Path, PathBuf};

/// Files whose colours are not a theme's.
const ALLOWED_FILES: &[&str] = &["crates/render/src/colormap.rs"];

/// Lines allowed as they are: `Rgb`'s white and black.
const ALLOWED_LINES: &[(&str, &str)] = &[
    ("crates/render/src/canvas.rs", "pub const WHITE: Rgb = Rgb(255, 255, 255);"),
    ("crates/render/src/canvas.rs", "pub const BLACK: Rgb = Rgb(0, 0, 0);"),
];

/// Calls that make a colour from numbers, and what must follow them to be a literal.
const CALLS: &[(&str, Follows)] = &[
    ("Rgb(", Follows::Number),
    ("from_rgb(", Follows::Number),
    ("from_gray(", Follows::Number),
    ("from_rgba_unmultiplied(", Follows::Number),
    ("from_rgba_premultiplied(", Follows::Number),
    ("from_white_alpha(", Follows::Number),
    ("from_black_alpha(", Follows::Number),
    ("from_hex(", Follows::Text),
];

#[derive(Clone, Copy)]
enum Follows {
    Number,
    Text,
}

/// One colour literal: where and the line.
#[derive(Debug, PartialEq)]
pub struct Literal {
    pub path: PathBuf,
    pub line: usize,
    pub text: String,
}

/// Whether a line of code (its comment cut off) spells out a colour.
fn has_literal(code: &str) -> bool {
    CALLS.iter().any(|(call, follows)| {
        code.match_indices(call).any(|(i, _)| {
            // `Rgb(` is the type's own name, not the end of another (`MyRgb(`).
            let whole = !call.starts_with(char::is_uppercase) || !code[..i].ends_with(|c: char| c.is_alphanumeric() || c == '_');
            let after = code[i + call.len()..].trim_start();
            whole
                && match follows {
                    Follows::Number => after.starts_with(|c: char| c.is_ascii_digit()),
                    Follows::Text => after.starts_with('"'),
                }
        })
    })
}

/// Where a line starts: in code, or inside a string (`raw`: a raw string closed by `"` and that
/// many `#`).
#[derive(Clone, Copy, PartialEq)]
enum State {
    Code,
    Str,
    Raw(usize),
}

/// A line's code: string literals emptied (their quotes kept), the comment cut off, and the
/// state the next line starts in.
fn code_of(line: &str, mut state: State) -> (String, State) {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        match state {
            State::Str => {
                if c == '\\' {
                    i += 1;
                } else if c == '"' {
                    out.push('"');
                    state = State::Code;
                }
            }
            State::Raw(hashes) => {
                if c == '"' && (1..=hashes).all(|k| chars.get(i + k) == Some(&'#')) {
                    out.push('"');
                    i += hashes;
                    state = State::Code;
                }
            }
            State::Code => match c {
                '/' if chars.get(i + 1) == Some(&'/') => break,
                '"' => {
                    out.push('"');
                    state = State::Str;
                }
                // A raw string `r"…"`, `r#"…"#` (not the end of a name such as `bar"`).
                'r' if !(i > 0 && chars.get(i - 1).is_some_and(|p| p.is_alphanumeric() || *p == '_')) => {
                    let hashes = chars[i + 1..].iter().take_while(|h| **h == '#').count();
                    if chars.get(i + 1 + hashes) == Some(&'"') {
                        out.push('"');
                        i += 1 + hashes;
                        state = State::Raw(hashes);
                    } else {
                        out.push(c);
                    }
                }
                // A character literal (`'"'`, `'\''`), not a lifetime.
                '\'' if chars.get(i + 1) == Some(&'\\') => {
                    i += chars.get(i + 3..).and_then(|rest| rest.iter().position(|x| *x == '\'')).map_or(chars.len(), |k| k + 3);
                }
                '\'' if chars.get(i + 2) == Some(&'\'') => i += 2,
                _ => out.push(c),
            },
        }
        i += 1;
    }
    (out, state)
}

/// The colour literals of one file's text (`path` relative to the workspace).
fn scan(path: &Path, text: &str) -> Vec<Literal> {
    let shown = path.to_string_lossy().replace('\\', "/");
    if ALLOWED_FILES.contains(&shown.as_str()) {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut state = State::Code;
    for (i, line) in text.lines().enumerate() {
        if state == State::Code && line.trim_start().starts_with("#[cfg(test)]") {
            break;
        }
        let (code, next) = code_of(line, state);
        state = next;
        let code = code.trim();
        if code.is_empty() || ALLOWED_LINES.iter().any(|(f, l)| *f == shown && *l == code) {
            continue;
        }
        if has_literal(code) {
            out.push(Literal { path: path.to_path_buf(), line: i + 1, text: code.to_owned() });
        }
    }
    out
}

/// Runs the check: prints every literal; fails when there is one.
pub fn run(root: &Path) -> Result<(), String> {
    let mut found = Vec::new();
    for path in crate::dupes::production_files(root)? {
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let rel = path.strip_prefix(root).map(Path::to_path_buf).unwrap_or(path);
        found.extend(scan(&rel, &text));
    }
    for l in &found {
        println!("{}:{}  {}", l.path.display(), l.line, l.text);
    }
    if found.is_empty() {
        println!("colours: ok (no colour literals outside the themes' files)");
        Ok(())
    } else {
        Err(format!("colours: {} colour literal(s); name the colour in the themes (docs/themes.md) and use it by role", found.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(path: &str, text: &str) -> Vec<usize> {
        scan(Path::new(path), text).into_iter().map(|l| l.line).collect()
    }

    #[test]
    fn literals_are_found_and_roles_pass() {
        let text = "\
let a = Rgb(30, 90, 200);
let b = Color32::from_rgb( 1, 2, 3);
let c = egui::Color32::from_gray(110);
let d = Color32::from_rgba_unmultiplied(160, 190, 230, 50);
let e = Color32::from_rgba_premultiplied(28, 28, 28, 28);
let f = Color32::from_white_alpha(28);
let g = Color32::from_hex(\"#ff0000\");
let ok = Rgb(c.r(), c.g(), c.b());
let ok = Color32::from_rgb(c.0, c.1, c.2);
let ok = Color32::from_rgba_unmultiplied(l.0, l.1, l.2, 160);
let ok = MyRgb(1, 2, 3);
let ok = palette.pen(Pen::Series(0)); // not Rgb(1, 2, 3)
p.image(id, rect, uv, Color32::WHITE);
#[cfg(test)]
let t = Rgb(1, 2, 3);
";
        assert_eq!(lines("crates/x/src/a.rs", text), [1, 2, 3, 4, 5, 6, 7]);
    }

    #[test]
    fn strings_comments_and_char_literals_are_no_code() {
        let text = "\
const USAGE: &str = \"
  colours   Rgb(30, …) in code
\";
let s = r#\"Rgb(1, 2, 3) \"quoted\" \"#; let x = Rgb(4, 5, 6);
let q = '\"'; let y = Rgb(7, 8, 9);
let e = '\\''; let z = Color32::from_gray(1);
let url = \"http://x\"; let w = Rgb(1, 1, 1);
fn f<'a>(s: &'a str) -> Rgb { Rgb(2, 2, 2) }
";
        assert_eq!(lines("crates/x/src/a.rs", text), [4, 5, 6, 7, 8]);
        let (code, state) = code_of("let s = \"open", State::Code);
        assert_eq!((code.as_str(), state == State::Str), ("let s = \"", true));
    }

    #[test]
    fn the_allowed_places_pass() {
        assert!(lines("crates/render/src/colormap.rs", "const D: [Rgb; 1] = [Rgb(40, 80, 170)];").is_empty());
        let consts = "    pub const WHITE: Rgb = Rgb(255, 255, 255);\n    pub const BLACK: Rgb = Rgb(0, 0, 0); // black\n";
        assert!(lines("crates/render/src/canvas.rs", consts).is_empty());
        // Only those lines, only there.
        assert_eq!(lines("crates/render/src/canvas.rs", "pub const RED: Rgb = Rgb(255, 0, 0);"), [1]);
        assert_eq!(lines("crates/ui-egui/src/theme.rs", consts), [1, 2]);
    }
}
