//! The window's symbols as mathematics: text it writes in plain Unicode (`εr 2.1, μr 1`, `S11`,
//! `tan δm`, `f_c`, `5.8e7`) turned into the TeX its labels are set from ([`texify`]), so a
//! label keeps its plain text for handles and tests and each window shows the rich form; a
//! material's properties and a parameter's symbol written the same way.
//!
//! ```
//! use emcraft_ui_core::math::texify;
//! // Plain text in, TeX out: the symbols the window writes become scripts.
//! assert_eq!(texify("vacuum: εr 1, μr 1"), "vacuum: $\\varepsilon_{r}$ 1, $\\mu_{r}$ 1");
//! assert_eq!(texify("|S21| at f_c"), "|$S_{21}$| at $f_{c}$");
//! // Identifiers, prices and unknown words stay as they are.
//! assert_eq!(texify("`loss_tangent` costs $5"), "`loss_tangent` costs \\$5");
//! ```

use std::borrow::Cow;
use std::fmt::Write as _;

/// Longest text [`texify`] reads (labels are short; a hostile one is cut).
const MAX_TEXT: usize = 4096;

/// The Greek letters [`texify`] gives subscripts, with their TeX names.
const GREEK: [(char, &str); 7] =
    [('ε', "\\varepsilon"), ('μ', "\\mu"), ('δ', "\\delta"), ('λ', "\\lambda"), ('β', "\\beta"), ('σ', "\\sigma"), ('χ', "\\chi")];

/// The subscripts written straight after a Greek letter that [`texify`] lowers (`εr`, `μr`,
/// `δm`, `λg`, `λc`, `εeff`), longest first.
const GREEK_SUBS: [&str; 5] = ["eff", "r", "m", "g", "c"];

/// Plain text with the window's mathematical symbols written as TeX: `εr`, `μr`, `δm`, `λg`,
/// `εeff` and `ε∞` take their subscripts, a single letter before `_` its subscript (`f_c`,
/// `δ_m`, `χ_k`), `S` and two digits is an S-parameter (`S21`), a number in e-notation a power
/// of ten (`5.8e7` → 5.8×10⁷). Words, `snake_case` names and text in backticks stay as they
/// are, and `$` becomes `\$`. Borrowed when nothing changed.
pub fn texify(plain: &str) -> Cow<'_, str> {
    if plain.len() > MAX_TEXT || !plain.chars().any(|c| c == '$' || c == '_' || c == 'S' || c == 'e' || c == '∞' || GREEK.iter().any(|g| g.0 == c))
    {
        return Cow::Borrowed(plain);
    }
    let c: Vec<char> = plain.chars().collect();
    let mut out = String::with_capacity(plain.len() + 16);
    let mut changed = false;
    let mut quoted = false;
    let mut i = 0;
    // Whether the character before `k` ends a word (so a symbol may start at `k`).
    let starts = |k: usize| k == 0 || !(c[k - 1].is_alphanumeric() || c[k - 1] == '_' || c[k - 1] == '.');
    // Whether `k` is past the end of a word.
    let ends = |k: usize| c.get(k).is_none_or(|x| !(x.is_ascii_alphanumeric() || *x == '_'));
    while i < c.len() {
        let ch = c[i];
        if ch == '`' {
            quoted = !quoted;
        }
        if ch == '$' {
            out.push_str("\\$");
            changed = true;
            i += 1;
            continue;
        }
        if quoted || !starts(i) {
            out.push(ch);
            i += 1;
            continue;
        }
        // A Greek letter and its subscript.
        if let Some((_, name)) = GREEK.iter().find(|g| g.0 == ch) {
            if c.get(i + 1) == Some(&'∞') && ends(i + 2) {
                let _ = write!(out, "${name}_{{\\infty}}$");
                (i, changed) = (i + 2, true);
                continue;
            }
            let sub = GREEK_SUBS.iter().find(|s| s.chars().enumerate().all(|(k, x)| c.get(i + 1 + k) == Some(&x)) && ends(i + 1 + s.len()));
            if let Some(sub) = sub {
                let _ = write!(out, "${name}_{{{sub}}}$");
                (i, changed) = (i + 1 + sub.len(), true);
                continue;
            }
        }
        // A single letter and `_` subscript.
        if (ch.is_ascii_alphabetic() || GREEK.iter().any(|g| g.0 == ch)) && c.get(i + 1) == Some(&'_') {
            let n = c[i + 2..].iter().take_while(|x| x.is_ascii_alphanumeric()).count();
            if (1..=4).contains(&n) && ends(i + 2 + n) {
                let base = GREEK.iter().find(|g| g.0 == ch).map_or_else(|| ch.to_string(), |g| g.1.to_owned());
                let sub: String = c[i + 2..i + 2 + n].iter().collect();
                let _ = write!(out, "${base}_{{{sub}}}$");
                (i, changed) = (i + 2 + n, true);
                continue;
            }
        }
        // An S-parameter: S and two digits.
        if ch == 'S' && c.get(i + 1).is_some_and(char::is_ascii_digit) && c.get(i + 2).is_some_and(char::is_ascii_digit) && ends(i + 3) {
            let _ = write!(out, "$S_{{{}{}}}$", c[i + 1], c[i + 2]);
            (i, changed) = (i + 3, true);
            continue;
        }
        // A number in e-notation.
        if ch.is_ascii_digit()
            && let Some((len, mantissa, exponent)) = e_notation(&c[i..])
            && ends(i + len)
        {
            let _ = write!(out, "{mantissa}$\\times 10^{{{exponent}}}$");
            (i, changed) = (i + len, true);
            continue;
        }
        out.push(ch);
        i += 1;
    }
    if changed { Cow::Owned(out) } else { Cow::Borrowed(plain) }
}

/// A number `1.5e-3` at the start of `c`: its length, mantissa and exponent (`−3`).
fn e_notation(c: &[char]) -> Option<(usize, String, String)> {
    let digits = |from: usize| c[from.min(c.len())..].iter().take_while(|x| x.is_ascii_digit()).count();
    let mut k = digits(0);
    if c.get(k) == Some(&'.') {
        k += 1 + digits(k + 1);
    }
    if c.get(k) != Some(&'e') {
        return None;
    }
    let mantissa: String = c[..k].iter().collect();
    k += 1;
    let sign = match c.get(k) {
        Some('-') => {
            k += 1;
            "−"
        }
        Some('+') => {
            k += 1;
            ""
        }
        _ => "",
    };
    let n = digits(k);
    if n == 0 || n > 3 {
        return None;
    }
    let exponent: String = c[k..k + n].iter().collect();
    let exponent = exponent.trim_start_matches('0');
    let exponent = if exponent.is_empty() { "0" } else { exponent };
    Some((k + n, mantissa, format!("{sign}{exponent}")))
}

/// A material's properties set as mathematics: `$\varepsilon_r$ = 2.2 · $\mu_r$ = 1 ·
/// $\tan\delta$ = 0.001`, or for a dispersive one `$\varepsilon_\infty$ = 5 · 1 Debye pole · …`.
pub fn material(m: &emcraft_model::Material) -> String {
    let mut parts = Vec::new();
    if m.is_dispersive() {
        parts.push(format!("$\\varepsilon_{{\\infty}}$ = {}", m.eps_r));
        // The poles as the summary names them.
        let summary = m.summary();
        if let Some(poles) = summary.split(", ").nth(1) {
            parts.push(poles.to_owned());
        }
    } else {
        parts.push(format!("$\\varepsilon_r$ = {}", m.eps_r));
    }
    parts.push(format!("$\\mu_r$ = {}", m.mu_r));
    if !m.loss_tangent.is_zero() {
        parts.push(format!("$\\tan\\,\\delta$ = {}", m.loss_tangent));
    }
    if !m.magnetic_loss_tangent.is_zero() {
        parts.push(format!("$\\tan\\,\\delta_m$ = {}", m.magnetic_loss_tangent));
    }
    parts.join("  ·  ")
}

/// The TeX symbol of a command parameter the dialogs show beside its name (`eps_r` → ε_r).
pub fn parameter_symbol(name: &str) -> Option<&'static str> {
    Some(match name {
        "eps_r" => "$\\varepsilon_r$",
        "mu_r" => "$\\mu_r$",
        "loss_tangent" => "$\\tan\\,\\delta$",
        "magnetic_loss_tangent" => "$\\tan\\,\\delta_m$",
        "conductivity" | "sigma" => "$\\sigma$",
        "impedance" | "reference" => "$Z_0$",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_symbols_become_tex() {
        let cases = [
            ("εr 2.1, μr 1, tan δm 0.05", "$\\varepsilon_{r}$ 2.1, $\\mu_{r}$ 1, tan $\\delta_{m}$ 0.05"),
            ("ε∞ 5", "$\\varepsilon_{\\infty}$ 5"),
            ("εeff", "$\\varepsilon_{eff}$"),
            ("S11 and S21", "$S_{11}$ and $S_{21}$"),
            ("σ = 5.800e7 S/m", "σ = 5.800$\\times 10^{7}$ S/m"),
            ("1e-3", "1$\\times 10^{−3}$"),
            ("χ_k(ω)", "$\\chi_{k}$(ω)"),
            ("比誘電率 εr > 0", "比誘電率 $\\varepsilon_{r}$ > 0"),
        ];
        for (plain, tex) in cases {
            assert_eq!(texify(plain), tex, "{plain}");
        }
        // Left alone: words, identifiers, quoted names, S followed by more.
        for plain in ["Stop", "S111", "`per_wavelength`", "loss_tangent", "εrx", "ωt 90°", "WR-90", "v1.2e"] {
            assert!(matches!(texify(plain), Cow::Borrowed(_)), "{plain}: {}", texify(plain));
        }
    }

    #[test]
    fn hostile_text_is_read() {
        let long = "εr_".repeat(5000);
        for s in ["", "$", "$$", "_", "S1", "1e", "1e9999", "\\$", "$\\frac{$", long.as_str(), "ε∞∞"] {
            let _ = texify(s);
        }
    }
}
