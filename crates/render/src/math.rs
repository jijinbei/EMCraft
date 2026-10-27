//! LaTeX mathematics in labels: text between `$` signs is read as TeX math — superscripts
//! `x^2`, `e^{j\omega t}`, subscripts `S_{21}`, fractions `\frac{a}{b}`, roots `\sqrt{x}`,
//! Greek letters and the usual symbols — and laid out by [`crate::text`]. Outside `$…$` text
//! is plain; `\$` is a dollar sign.
//!
//! A subset of TeX's math mode (D. E. Knuth, *The TeXbook*, Addison-Wesley 1984, ch. 16–18;
//! the command names are LaTeX's): enough for the labels of fields, S-parameters and
//! materials. Unknown commands show their name; braces left open close at the end. Nothing
//! here can fail: any string lays out.
//!
//! [`to_unicode`] writes the same as plain Unicode (`S_{21}` → `S₂₁`, `e^{j\omega t}` →
//! `e^(jωt)`) for places that draw text themselves, such as the desktop app's widgets.

/// A piece of laid-out text.
#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    /// Characters set side by side.
    Text(String),
    /// A base with a superscript, a subscript or both.
    Scripts {
        base: Vec<Node>,
        sup: Option<Vec<Node>>,
        sub: Option<Vec<Node>>,
    },
    Frac {
        num: Vec<Node>,
        den: Vec<Node>,
    },
    Sqrt(Vec<Node>),
    /// Space in ems.
    Space(f32),
}

/// Most nodes one label may make (input is hostile; labels are short).
const MAX_NODES: usize = 4096;
/// Deepest nesting of groups and scripts.
const MAX_DEPTH: usize = 16;

/// Parses a label: plain text and `$…$` math.
pub fn parse(text: &str) -> Vec<Node> {
    let mut out = Vec::new();
    let mut plain = String::new();
    let mut chars = text.chars().peekable();
    let mut budget = MAX_NODES;
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'$') => {
                chars.next();
                plain.push('$');
            }
            '$' => {
                let math: String = chars.by_ref().take_while(|c| *c != '$').collect();
                flush(&mut plain, &mut out);
                let tokens: Vec<char> = math.chars().collect();
                let mut pos = 0;
                out.extend(list(&tokens, &mut pos, 0, &mut budget, false));
            }
            c => plain.push(c),
        }
    }
    flush(&mut plain, &mut out);
    out
}

fn flush(text: &mut String, out: &mut Vec<Node>) {
    if !text.is_empty() {
        out.push(Node::Text(std::mem::take(text)));
    }
}

/// Math nodes from `t[pos..]` up to the end, or to a closing brace when `group`.
fn list(t: &[char], pos: &mut usize, depth: usize, budget: &mut usize, group: bool) -> Vec<Node> {
    let mut out: Vec<Node> = Vec::new();
    while *pos < t.len() && *budget > 0 {
        *budget -= 1;
        let c = t[*pos];
        *pos += 1;
        match c {
            '}' if group => return out,
            '}' => {}
            '^' | '_' => {
                let arg = argument(t, pos, depth, budget);
                // The base: the last node, unless it is a script already (then add to it).
                let base = match out.pop() {
                    Some(Node::Scripts { base, sup, sub }) if (c == '^' && sup.is_none()) || (c == '_' && sub.is_none()) => {
                        let (sup, sub) = if c == '^' { (Some(arg), sub) } else { (sup, Some(arg)) };
                        out.push(Node::Scripts { base, sup, sub });
                        continue;
                    }
                    Some(Node::Text(mut s)) if s.chars().count() > 1 => {
                        // Only the last character carries the script: `ab^2`.
                        let last = s.pop().map(String::from).unwrap_or_default();
                        out.push(Node::Text(s));
                        vec![Node::Text(last)]
                    }
                    Some(n) => vec![n],
                    None => Vec::new(),
                };
                let (sup, sub) = if c == '^' { (Some(arg), None) } else { (None, Some(arg)) };
                out.push(Node::Scripts { base, sup, sub });
            }
            '{' => {
                let inner = if depth < MAX_DEPTH { list(t, pos, depth + 1, budget, true) } else { Vec::new() };
                out.extend(inner);
            }
            '\\' => {
                let name = command_name(t, pos);
                command(&name, t, pos, depth, budget, &mut out);
            }
            ' ' => {}
            c => push_char(&mut out, c),
        }
    }
    out
}

fn push_char(out: &mut Vec<Node>, c: char) {
    if let Some(Node::Text(s)) = out.last_mut() {
        s.push(c);
    } else {
        out.push(Node::Text(c.to_string()));
    }
}

fn push_str(out: &mut Vec<Node>, s: &str) {
    s.chars().for_each(|c| push_char(out, c));
}

/// A script's or a command's argument: a braced group, a command, or one character.
fn argument(t: &[char], pos: &mut usize, depth: usize, budget: &mut usize) -> Vec<Node> {
    while t.get(*pos) == Some(&' ') {
        *pos += 1;
    }
    if depth >= MAX_DEPTH {
        return Vec::new();
    }
    match t.get(*pos) {
        Some('{') => {
            *pos += 1;
            list(t, pos, depth + 1, budget, true)
        }
        Some('\\') => {
            // One command, with its own arguments.
            *pos += 1;
            let name = command_name(t, pos);
            let mut out = Vec::new();
            command(&name, t, pos, depth + 1, budget, &mut out);
            out
        }
        Some(c) => {
            let c = *c;
            *pos += 1;
            vec![Node::Text(c.to_string())]
        }
        None => Vec::new(),
    }
}

/// The name after a backslash: letters, or one other character.
fn command_name(t: &[char], pos: &mut usize) -> String {
    if t.get(*pos).is_some_and(|c| c.is_ascii_alphabetic()) {
        let n: String = t[*pos..].iter().take_while(|c| c.is_ascii_alphabetic()).collect();
        *pos += n.len();
        n
    } else {
        let n = t.get(*pos).map(|c| c.to_string()).unwrap_or_default();
        *pos += 1;
        n
    }
}

fn command(name: &str, t: &[char], pos: &mut usize, depth: usize, budget: &mut usize, out: &mut Vec<Node>) {
    match name {
        "frac" | "dfrac" | "tfrac" => {
            let num = argument(t, pos, depth + 1, budget);
            let den = argument(t, pos, depth + 1, budget);
            out.push(Node::Frac { num, den });
        }
        "sqrt" => {
            let body = argument(t, pos, depth + 1, budget);
            out.push(Node::Sqrt(body));
        }
        "mathrm" | "text" | "mathit" | "mathbf" | "operatorname" | "boldsymbol" | "vec" | "hat" | "bar" | "tilde" => {
            out.extend(argument(t, pos, depth + 1, budget));
        }
        "left" | "right" | "big" | "Big" | "bigg" | "Bigg" | "displaystyle" | "limits" => {}
        "," => out.push(Node::Space(0.17)),
        ":" | ">" => out.push(Node::Space(0.22)),
        ";" => out.push(Node::Space(0.28)),
        "!" => out.push(Node::Space(-0.17)),
        " " => out.push(Node::Space(0.25)),
        "quad" => out.push(Node::Space(1.0)),
        "qquad" => out.push(Node::Space(2.0)),
        "{" | "}" | "%" | "&" | "#" | "_" | "$" | "|" => push_str(out, name),
        "\\" => push_char(out, ' '),
        _ => match symbol(name) {
            Some(s) => push_str(out, s),
            None => push_str(out, name),
        },
    }
}

/// The character of a symbol command.
pub fn symbol(name: &str) -> Option<&'static str> {
    Some(match name {
        "alpha" => "α",
        "beta" => "β",
        "gamma" => "γ",
        "delta" => "δ",
        "epsilon" => "ϵ",
        "varepsilon" => "ε",
        "zeta" => "ζ",
        "eta" => "η",
        "theta" => "θ",
        "vartheta" => "ϑ",
        "iota" => "ι",
        "kappa" => "κ",
        "lambda" => "λ",
        "mu" => "μ",
        "nu" => "ν",
        "xi" => "ξ",
        "pi" => "π",
        "rho" => "ρ",
        "sigma" => "σ",
        "tau" => "τ",
        "upsilon" => "υ",
        "phi" => "ϕ",
        "varphi" => "φ",
        "chi" => "χ",
        "psi" => "ψ",
        "omega" => "ω",
        "Gamma" => "Γ",
        "Delta" => "Δ",
        "Theta" => "Θ",
        "Lambda" => "Λ",
        "Xi" => "Ξ",
        "Pi" => "Π",
        "Sigma" => "Σ",
        "Phi" => "Φ",
        "Psi" => "Ψ",
        "Omega" => "Ω",
        "cdot" => "·",
        "times" => "×",
        "div" => "÷",
        "pm" => "±",
        "mp" => "∓",
        "le" | "leq" => "≤",
        "ge" | "geq" => "≥",
        "ne" | "neq" => "≠",
        "approx" => "≈",
        "sim" => "∼",
        "propto" => "∝",
        "equiv" => "≡",
        "infty" => "∞",
        "partial" => "∂",
        "nabla" => "∇",
        "circ" | "degree" => "°",
        "angle" => "∠",
        "to" | "rightarrow" => "→",
        "leftarrow" => "←",
        "leftrightarrow" => "↔",
        "Rightarrow" => "⇒",
        "int" => "∫",
        "oint" => "∮",
        "sum" => "∑",
        "prod" => "∏",
        "ell" => "ℓ",
        "hbar" => "ħ",
        "Re" => "Re",
        "Im" => "Im",
        "ln" => "ln",
        "log" => "log",
        "exp" => "exp",
        "sin" => "sin",
        "cos" => "cos",
        "tan" => "tan",
        "max" => "max",
        "min" => "min",
        "dagger" => "†",
        "ast" => "∗",
        "star" => "⋆",
        "perp" => "⊥",
        "parallel" => "∥",
        "langle" => "⟨",
        "rangle" => "⟩",
        "lvert" | "rvert" | "vert" => "|",
        "lVert" | "rVert" | "Vert" => "‖",
        "cdots" => "⋯",
        "ldots" | "dots" => "…",
        "hat" => "^",
        "prime" => "′",
        _ => return None,
    })
}

/// A label in plain Unicode: scripts as Unicode super- and subscripts where every character
/// has one (`S_{21}` → `S₂₁`), a lone letter after its base (`E_y` → `Ey`), else marked
/// (`e^(jωt)`, `x_(ab)`); fractions as `a/b`; roots as
/// `√(…)`.
pub fn to_unicode(text: &str) -> String {
    let mut out = String::new();
    write_unicode(&parse(text), &mut out);
    out
}

fn write_unicode(nodes: &[Node], out: &mut String) {
    for n in nodes {
        match n {
            Node::Text(s) => out.push_str(s),
            Node::Space(w) if *w > 0.0 => out.push(' '),
            Node::Space(_) => {}
            Node::Frac { num, den } => {
                wrapped(num, out);
                out.push('/');
                wrapped(den, out);
            }
            Node::Sqrt(body) => {
                out.push('√');
                wrapped(body, out);
            }
            Node::Scripts { base, sup, sub } => {
                write_unicode(base, out);
                if let Some(s) = sub {
                    script(s, out, subscript, '_');
                }
                if let Some(s) = sup {
                    script(s, out, superscript, '^');
                }
            }
        }
    }
}

/// `nodes`, in parentheses unless one plain word.
fn wrapped(nodes: &[Node], out: &mut String) {
    let mut s = String::new();
    write_unicode(nodes, &mut s);
    if s.chars().count() <= 1 || (matches!(nodes, [Node::Text(_)]) && s.chars().all(char::is_alphanumeric)) {
        out.push_str(&s);
    } else {
        out.push('(');
        out.push_str(&s);
        out.push(')');
    }
}

fn script(nodes: &[Node], out: &mut String, map: fn(char) -> Option<char>, mark: char) {
    let mut s = String::new();
    write_unicode(nodes, &mut s);
    match s.chars().map(map).collect::<Option<String>>() {
        Some(t) if !t.is_empty() => out.push_str(&t),
        // One letter or digit without a subscript form follows its base (E_y → Ey).
        _ if mark == '_' && s.chars().count() == 1 && s.chars().all(char::is_alphanumeric) => out.push_str(&s),
        _ => {
            out.push(mark);
            if s.chars().count() == 1 {
                out.push_str(&s);
            } else {
                out.push('(');
                out.push_str(&s);
                out.push(')');
            }
        }
    }
}

fn superscript(c: char) -> Option<char> {
    Some(match c {
        '0' => '⁰',
        '1' => '¹',
        '2' => '²',
        '3' => '³',
        '4' => '⁴',
        '5' => '⁵',
        '6' => '⁶',
        '7' => '⁷',
        '8' => '⁸',
        '9' => '⁹',
        '+' => '⁺',
        '-' | '−' => '⁻',
        '=' => '⁼',
        '(' => '⁽',
        ')' => '⁾',
        'n' => 'ⁿ',
        'i' => 'ⁱ',
        _ => return None,
    })
}

fn subscript(c: char) -> Option<char> {
    Some(match c {
        '0' => '₀',
        '1' => '₁',
        '2' => '₂',
        '3' => '₃',
        '4' => '₄',
        '5' => '₅',
        '6' => '₆',
        '7' => '₇',
        '8' => '₈',
        '9' => '₉',
        '+' => '₊',
        '-' | '−' => '₋',
        '=' => '₌',
        '(' => '₍',
        ')' => '₎',
        'a' => 'ₐ',
        'e' => 'ₑ',
        'o' => 'ₒ',
        'x' => 'ₓ',
        'h' => 'ₕ',
        'k' => 'ₖ',
        'l' => 'ₗ',
        'm' => 'ₘ',
        'n' => 'ₙ',
        'p' => 'ₚ',
        's' => 'ₛ',
        't' => 'ₜ',
        'r' => 'ᵣ',
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> Node {
        Node::Text(s.into())
    }

    #[test]
    fn math_parses_scripts_fractions_roots_and_symbols() {
        assert_eq!(parse("plain $x$"), vec![t("plain "), t("x")]);
        assert_eq!(parse("$e^{j\\omega t}$"), vec![Node::Scripts { base: vec![t("e")], sup: Some(vec![t("jωt")]), sub: None }]);
        assert_eq!(parse("$S_{21}$"), vec![Node::Scripts { base: vec![t("S")], sup: None, sub: Some(vec![t("21")]) }]);
        assert_eq!(parse("$x_1^2$"), vec![Node::Scripts { base: vec![t("x")], sup: Some(vec![t("2")]), sub: Some(vec![t("1")]) }]);
        assert_eq!(parse("$ab^2$"), vec![t("a"), Node::Scripts { base: vec![t("b")], sup: Some(vec![t("2")]), sub: None }]);
        assert_eq!(parse("$\\frac{1}{2}$"), vec![Node::Frac { num: vec![t("1")], den: vec![t("2")] }]);
        assert_eq!(parse("$\\sqrt{\\varepsilon_r}$")[0], Node::Sqrt(vec![Node::Scripts { base: vec![t("ε")], sup: None, sub: Some(vec![t("r")]) }]));
        assert_eq!(parse("$\\frac12$"), vec![Node::Frac { num: vec![t("1")], den: vec![t("2")] }]);
        assert_eq!(
            parse("$k^\\frac{1}{2}$"),
            vec![Node::Scripts { base: vec![t("k")], sup: Some(vec![Node::Frac { num: vec![t("1")], den: vec![t("2")] }]), sub: None }]
        );
        assert_eq!(parse("cost \\$5"), vec![t("cost $5")]);
        assert_eq!(parse("$\\unknown$"), vec![t("unknown")]);
    }

    #[test]
    fn hostile_math_never_fails() {
        for s in ["$", "$$", "${{{{", "$}}}$", "$^_^_$", "$\\$", "$\\frac$", "$\\sqrt", "$x^$", "$\\frac{\\frac{\\frac{a}{b}}{c}}{d}$"] {
            let _ = parse(s);
            let _ = to_unicode(s);
        }
        let deep = format!("${}x{}$", "{".repeat(10_000), "}".repeat(10_000));
        let _ = parse(&deep);
        let long = format!("${}$", "x^".repeat(100_000));
        let _ = parse(&long);
    }

    #[test]
    fn unicode_forms() {
        assert_eq!(to_unicode("$S_{21}$"), "S₂₁");
        assert_eq!(to_unicode("$x^2$ m"), "x² m");
        assert_eq!(to_unicode("$E_y$ (V/m)"), "Ey (V/m)");
        assert_eq!(to_unicode("$e^{j\\omega t}$"), "e^(jωt)");
        assert_eq!(to_unicode("$\\varepsilon_r$"), "εᵣ");
        assert_eq!(to_unicode("$\\frac{1}{2}$"), "1/2");
        assert_eq!(to_unicode("$\\sqrt{\\varepsilon_r \\mu_r}$"), "√(εᵣμᵣ)");
    }
}
