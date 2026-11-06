//! EMCraft's translations checked when a crate compiles. Every text the window shows is written
//! in English where it is used, through a macro that finds it in the translations
//! (`locales/<language>.json`, at the workspace's root, their `strings`) and expands to the same
//! string literal:
//!
//! - `text!("no mesh yet")`: a text, its placeholders (if any) filled elsewhere.
//! - `template!("waits for {steps}", steps)`: a text and the names of the values given with it,
//!   exactly its placeholders.
//!
//! A text missing from a complete language (`"complete": true` in its file), a translation whose
//! placeholders differ from the English, names that are not the text's, or an argument that is
//! not a string literal stop the build there, pointing at the text. A language still being
//! translated is not checked: its missing texts show in English.
//!
//! The macros only check: the expansion is the literal, so nothing is generated or embedded.
//! A crate using them names the translation files in an `include_bytes!` of its own, so cargo
//! builds it again when they change.
//!
//! ```
//! let t: &str = emcraft_i18n::text!("Cancel");
//! assert_eq!(t, "Cancel");
//! let t: &str = emcraft_i18n::template!("Step {n} · {step}", n, step);
//! assert_eq!(t, "Step {n} · {step}");
//! ```
//!
//! A text no translation has, or the wrong names, does not compile:
//!
//! ```compile_fail
//! let _ = emcraft_i18n::text!("a text nobody translated");
//! ```
//!
//! ```compile_fail
//! let _ = emcraft_i18n::text!("Cancel", n);
//! ```
//!
//! ```compile_fail
//! let _ = emcraft_i18n::template!("Step {n} · {step}", n);
//! ```

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use proc_macro::{Delimiter, Group, Ident, Literal, Punct, Spacing, Span, TokenStream, TokenTree};

/// A text, checked: expands to its string literal.
#[proc_macro]
pub fn text(input: TokenStream) -> TokenStream {
    expand(input, false)
}

/// A text with placeholders and their names, checked: expands to its string literal.
#[proc_macro]
pub fn template(input: TokenStream) -> TokenStream {
    expand(input, true)
}

fn expand(input: TokenStream, named: bool) -> TokenStream {
    let mut tokens = input.into_iter().flat_map(transparent);
    let Some(TokenTree::Literal(lit)) = tokens.next() else {
        return error(Span::call_site(), "the text must be a string literal");
    };
    let span = lit.span();
    let Some(english) = unquote(&lit.to_string()) else {
        return error(span, "the text must be a string literal");
    };
    let mut names = BTreeSet::new();
    let mut expect_comma = true;
    for t in tokens {
        match (t, expect_comma) {
            (TokenTree::Punct(p), true) if p.as_char() == ',' => expect_comma = false,
            (TokenTree::Ident(i), false) if named => {
                names.insert(i.to_string());
                expect_comma = true;
            }
            (t, _) => return error(t.span(), if named { "expected `, name`" } else { "text! takes the text alone (template! takes names)" }),
        }
    }
    let wanted = placeholders(&english);
    if named && wanted != names {
        return error(span, &format!("the text's placeholders are {{{}}}, the names given {{{}}}", join(&wanted), join(&names)));
    }
    // Nothing to translate: empty, or placeholders and signs only (`{name}: {value}`).
    if !needs_words(&english) {
        let mut out = Literal::string(&english);
        out.set_span(span);
        return TokenStream::from(TokenTree::Literal(out));
    }
    match catalog() {
        Err(why) => return error(span, why),
        Ok(c) => {
            for lang in c.iter().filter(|l| l.complete) {
                match lang.strings.get(&english) {
                    None => return error(span, &format!("no {} for {english:?} in {}", lang.name, lang.path.display())),
                    Some(t) if placeholders(t) != wanted => {
                        return error(span, &format!("the {} for {english:?} has the placeholders {{{}}}", lang.name, join(&placeholders(t))));
                    }
                    Some(_) => {}
                }
            }
        }
    }
    let mut out = Literal::string(&english);
    out.set_span(span);
    TokenStream::from(TokenTree::Literal(out))
}

/// A token, or the tokens of an invisible group (a `macro_rules!` fragment passed on).
fn transparent(t: TokenTree) -> Vec<TokenTree> {
    match t {
        TokenTree::Group(g) if g.delimiter() == Delimiter::None => g.stream().into_iter().flat_map(transparent).collect(),
        t => vec![t],
    }
}

/// A translation file read.
struct Language {
    name: String,
    path: PathBuf,
    complete: bool,
    strings: BTreeMap<String, String>,
}

/// Every translation, read once per compiler process.
fn catalog() -> Result<&'static [Language], &'static str> {
    static CATALOG: OnceLock<Result<Vec<Language>, String>> = OnceLock::new();
    CATALOG.get_or_init(read_all).as_deref().map_err(String::as_str)
}

fn read_all() -> Result<Vec<Language>, String> {
    let start = std::env::var("CARGO_MANIFEST_DIR").map_err(|_| "CARGO_MANIFEST_DIR is not set".to_owned())?;
    let dir = Path::new(&start)
        .ancestors()
        .map(|a| a.join("locales"))
        .find(|d| d.is_dir())
        .ok_or_else(|| format!("no locales folder at or above {start}"))?;
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|path| {
            let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let v: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
            let strings = v
                .get("strings")
                .and_then(serde_json::Value::as_object)
                .ok_or_else(|| format!("{}: no `strings` object", path.display()))?
                .iter()
                .filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_owned())))
                .collect();
            let name = v.get("name").and_then(serde_json::Value::as_str).unwrap_or("translation").to_owned();
            let complete = v.get("complete").and_then(serde_json::Value::as_bool).unwrap_or(false);
            Ok(Language { name, path, complete, strings })
        })
        .collect()
}

/// The `{name}` placeholders of a text (a name: a letter or `_`, then letters, digits, `_`).
fn placeholders(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut rest = text;
    while let Some(i) = rest.find('{') {
        rest = &rest[i + 1..];
        let Some(j) = rest.find('}') else { break };
        let name = &rest[..j];
        let ok =
            name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if ok {
            out.insert(name.to_owned());
        }
    }
    out
}

/// Whether a text has words outside its placeholders: letters a translation would change.
fn needs_words(text: &str) -> bool {
    let mut rest = text.to_owned();
    for p in placeholders(text) {
        rest = rest.replace(&format!("{{{p}}}"), "");
    }
    rest.chars().any(char::is_alphabetic)
}

fn join(names: &BTreeSet<String>) -> String {
    names.iter().cloned().collect::<Vec<_>>().join(", ")
}

/// A string literal's value: `"…"` with its escapes, or `r"…"` / `r#"…"#`.
fn unquote(lit: &str) -> Option<String> {
    if let Some(raw) = lit.strip_prefix('r') {
        let hashes = raw.len() - raw.trim_start_matches('#').len();
        let body = raw.get(hashes..raw.len().checked_sub(hashes)?)?;
        return body.strip_prefix('"')?.strip_suffix('"').map(str::to_owned);
    }
    let body = lit.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next()? {
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'r' => out.push('\r'),
            '0' => out.push('\0'),
            '\\' => out.push('\\'),
            '"' => out.push('"'),
            '\'' => out.push('\''),
            'u' => {
                let rest: String = chars.by_ref().take_while(|c| *c != '}').collect();
                out.push(char::from_u32(u32::from_str_radix(rest.strip_prefix('{')?, 16).ok()?)?);
            }
            'x' => {
                let hex: String = chars.by_ref().take(2).collect();
                out.push(char::from(u8::from_str_radix(&hex, 16).ok()?));
            }
            '\n' => {
                // A line continuation: the next line's leading whitespace is dropped.
                let rest: String = chars.collect();
                out.push_str(&unquote(&format!("\"{}\"", rest.trim_start()))?);
                return Some(out);
            }
            _ => return None,
        }
    }
    Some(out)
}

/// `compile_error!(why)` at `span`.
fn error(span: Span, why: &str) -> TokenStream {
    let mut msg = Literal::string(why);
    msg.set_span(span);
    let mut bang = Punct::new('!', Spacing::Alone);
    bang.set_span(span);
    let mut args = Group::new(Delimiter::Parenthesis, TokenStream::from(TokenTree::Literal(msg)));
    args.set_span(span);
    [TokenTree::Ident(Ident::new("compile_error", span)), TokenTree::Punct(bang), TokenTree::Group(args)].into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_are_names_in_braces() {
        let p = placeholders("Step {n} · {step}, {\"json\": 1}, {}");
        assert_eq!(p.into_iter().collect::<Vec<_>>(), ["n", "step"]);
    }

    #[test]
    fn literals_unquote() {
        assert_eq!(unquote(r#""a \"b\" \\ \u{3b1}""#).as_deref(), Some("a \"b\" \\ α"));
        assert_eq!(unquote(r##"r#"x "y""#"##).as_deref(), Some("x \"y\""));
        assert_eq!(unquote("\"a\\\n      b\"").as_deref(), Some("ab"));
        assert_eq!(unquote("'c'"), None);
    }
}
