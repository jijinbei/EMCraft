//! Text in replies and errors: input-derived text clipped before it is echoed, and the
//! "names nothing (there are …)" errors.

use std::fmt::Display;

/// At most `n` characters of `s`: text from hostile input is not echoed in full.
///
/// ```
/// assert_eq!(emcraft_engine::text::clip("Wellenleiter", 5), "Welle");
/// assert_eq!(emcraft_engine::text::clip("Ω", 5), "Ω"); // characters, not bytes
/// ```
pub fn clip(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// `what` (a name that names nothing), then in brackets `label` and the names there are:
/// "no port `3` (there are 1, 2)".
pub(crate) fn unknown<S: AsRef<str>>(what: impl Display, label: &str, known: impl IntoIterator<Item = S>) -> String {
    let names: Vec<S> = known.into_iter().collect();
    let names: Vec<&str> = names.iter().map(AsRef::as_ref).collect();
    format!("{what} ({label}{})", names.join(", "))
}

/// At most `max` of `names`, each clipped to 40 characters, then "… and n more" for the rest:
/// a list from a model or a file is not echoed in full either.
///
/// ```
/// let names = ["a", "b", "c"];
/// assert_eq!(emcraft_engine::text::listed(&names, 2), ["a", "b", "… and 1 more"]);
/// ```
pub fn listed<S: AsRef<str>>(names: &[S], max: usize) -> Vec<String> {
    let mut out: Vec<String> = names.iter().take(max).map(|n| clip(n.as_ref(), 40)).collect();
    if names.len() > max {
        out.push(format!("… and {} more", names.len() - max));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipped_and_listed() {
        assert_eq!(clip("αβγδ", 2), "αβ");
        assert_eq!(unknown("no port `3`", "there are ", ["1", "2"]), "no port `3` (there are 1, 2)");
    }
}
