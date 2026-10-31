//! Labels set as mathematics: TeX-like text (`$\varepsilon_r$ = 2.2`, `$\tan\delta_m$`,
//! `$S_{21}$`) laid out as an egui [`LayoutJob`] with real subscripts and superscripts — smaller
//! and lowered or raised — and Greek letters, so the window's labels read like the typeset
//! formulas of the documentation instead of `εr` on the line.
//!
//! The TeX is read by [`emcraft_render::math::parse`] (the subset the pictures' labels use:
//! scripts, fractions, roots, Greek and the usual symbols; *The TeXbook*, ch. 16–18); this
//! module only sets the nodes in egui's text. Text the window writes in plain Unicode (`εr 2.1,
//! μr 1`, `S11`, `tan δm`, `f_c`, `5.8e7`) is turned into that TeX by [`texify`], so a label
//! keeps its plain text for handles and tests and shows the rich form.
//!
//! ```
//! use emcraft_ui_egui::math;
//! // Plain text in, TeX out: the symbols the window writes become scripts.
//! assert_eq!(math::texify("vacuum: εr 1, μr 1"), "vacuum: $\\varepsilon_{r}$ 1, $\\mu_{r}$ 1");
//! assert_eq!(math::texify("|S21| at f_c"), "|$S_{21}$| at $f_{c}$");
//! // Identifiers, prices and unknown words stay as they are.
//! assert_eq!(math::texify("`loss_tangent` costs $5"), "`loss_tangent` costs \\$5");
//! // Any string lays out: hostile input makes a job, never a panic.
//! let job = math::job("$x^{{{{", 13.0, egui::Color32::WHITE);
//! assert!(!job.sections.is_empty());
//! ```

use std::borrow::Cow;

use egui::text::{LayoutJob, TextFormat};
use egui::{Align, Color32, FontId, Response};
use emcraft_render::math::Node;

/// A script's size against its base's (TeX's script style is 70 %).
const SCRIPT: f32 = 0.72;
/// The smallest size a nested script takes, against the label's.
const SMALLEST: f32 = 0.5;
/// Longest text [`texify`] reads (labels are short; a hostile one is cut).
const MAX_TEXT: usize = 4096;

/// Where a run of text sits: on the line, lowered or raised.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Place {
    Line,
    Sub,
    Sup,
}

/// `tex` (plain text with `$…$` math) set at `size` points in `color`.
pub fn job(tex: &str, size: f32, color: Color32) -> LayoutJob {
    let mut job = LayoutJob::default();
    set(&mut job, &emcraft_render::math::parse(tex), size, size, Place::Line, color);
    if job.sections.is_empty() {
        job.append("", 0.0, format(size, size, Place::Line, color));
    }
    job
}

/// Plain text (as the window writes it) set as mathematics: [`job`] of [`texify`].
pub fn rich(plain: &str, size: f32, color: Color32) -> LayoutJob {
    job(&texify(plain), size, color)
}

/// A label of plain text set as mathematics, its plain text the accessible name.
pub fn label(ui: &mut egui::Ui, plain: &str, size: f32, color: Color32) -> Response {
    let r = ui.label(rich(plain, size, color));
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, plain));
    r
}

/// Plain text set as mathematics and painted with `align` at `pos`, clipped to `clip`; the
/// rectangle it took.
pub fn paint(painter: &egui::Painter, pos: egui::Pos2, align: egui::Align2, plain: &str, size: f32, color: Color32) -> egui::Rect {
    let galley = painter.layout_job(rich(plain, size, color));
    let rect = align.anchor_size(pos, galley.size());
    painter.galley(rect.min, galley, color);
    rect
}

/// `plain` on hover, set as mathematics when it holds any (else egui's plain tooltip).
pub fn tip(resp: Response, plain: &str) -> Response {
    match texify(plain) {
        Cow::Borrowed(_) => resp.on_hover_text(plain),
        Cow::Owned(tex) => resp.on_hover_ui(|ui| {
            let size = ui.style().text_styles.get(&egui::TextStyle::Body).map_or(13.0, |f| f.size);
            ui.label(job(&tex, size, ui.visuals().text_color()));
        }),
    }
}

fn format(size: f32, base: f32, place: Place, color: Color32) -> TextFormat {
    let mut f = TextFormat { font_id: FontId::proportional(size), color, ..TextFormat::default() };
    match place {
        Place::Line => {}
        // The top of the line: a superscript's top meets its base's.
        Place::Sup => f.valign = Align::TOP,
        // The bottom of the line with a short line box, so the glyphs drop below the base's
        // baseline by about a tenth of its size.
        Place::Sub => {
            f.valign = Align::BOTTOM;
            f.line_height = Some((size * 1.15 - base * 0.2).max(size * 0.5));
        }
    }
    f
}

/// Appends `nodes` at `size` (the label's is `base`) in `place`.
fn set(job: &mut LayoutJob, nodes: &[Node], size: f32, base: f32, place: Place, color: Color32) {
    for n in nodes {
        match n {
            Node::Text(s) => job.append(s, 0.0, format(size, base, place, color)),
            Node::Space(w) if *w > 0.1 => job.append(" ", 0.0, format(size, base, place, color)),
            Node::Space(_) => {}
            Node::Scripts { base: b, sup, sub } => {
                set(job, b, size, base, place, color);
                let small = (size * SCRIPT).max(base * SMALLEST);
                if let Some(s) = sub {
                    set(job, s, small, base, Place::Sub, color);
                }
                if let Some(s) = sup {
                    set(job, s, small, base, Place::Sup, color);
                }
            }
            Node::Frac { num, den } => {
                grouped(job, num, size, base, place, color);
                job.append("/", 0.0, format(size, base, place, color));
                grouped(job, den, size, base, place, color);
            }
            Node::Sqrt(body) => {
                job.append("√", 0.0, format(size, base, place, color));
                grouped(job, body, size, base, place, color);
            }
        }
    }
}

/// `nodes`, in parentheses unless they are one character.
fn grouped(job: &mut LayoutJob, nodes: &[Node], size: f32, base: f32, place: Place, color: Color32) {
    let single = matches!(nodes, [Node::Text(s)] if s.chars().count() == 1);
    if !single {
        job.append("(", 0.0, format(size, base, place, color));
    }
    set(job, nodes, size, base, place, color);
    if !single {
        job.append(")", 0.0, format(size, base, place, color));
    }
}

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
                out.push_str(&format!("${name}_{{\\infty}}$"));
                (i, changed) = (i + 2, true);
                continue;
            }
            let sub = GREEK_SUBS.iter().find(|s| s.chars().enumerate().all(|(k, x)| c.get(i + 1 + k) == Some(&x)) && ends(i + 1 + s.len()));
            if let Some(sub) = sub {
                out.push_str(&format!("${name}_{{{sub}}}$"));
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
                out.push_str(&format!("${base}_{{{sub}}}$"));
                (i, changed) = (i + 2 + n, true);
                continue;
            }
        }
        // An S-parameter: S and two digits.
        if ch == 'S' && c.get(i + 1).is_some_and(char::is_ascii_digit) && c.get(i + 2).is_some_and(char::is_ascii_digit) && ends(i + 3) {
            out.push_str(&format!("$S_{{{}{}}}$", c[i + 1], c[i + 2]));
            (i, changed) = (i + 3, true);
            continue;
        }
        // A number in e-notation.
        if ch.is_ascii_digit()
            && let Some((len, mantissa, exponent)) = e_notation(&c[i..])
            && ends(i + len)
        {
            out.push_str(&format!("{mantissa}$\\times 10^{{{exponent}}}$"));
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
    fn scripts_are_smaller_and_lowered_or_raised() {
        let j = job("$\\varepsilon_r$ = 2 $x^2$", 14.0, Color32::WHITE);
        let texts: Vec<(&str, f32, Align)> =
            j.sections.iter().map(|s| (&j.text[s.byte_range.start.0..s.byte_range.end.0], s.format.font_id.size, s.format.valign)).collect();
        assert_eq!(texts[0], ("ε", 14.0, Align::BOTTOM));
        assert_eq!(texts[1].0, "r");
        assert!(texts[1].1 < 14.0 && j.sections[1].format.line_height.is_some(), "a subscript: smaller, its line box short");
        let sup = texts.iter().find(|t| t.0 == "2" && t.2 == Align::TOP);
        assert!(sup.is_some_and(|s| s.1 < 14.0), "{texts:?}");
    }

    #[test]
    fn hostile_text_lays_out() {
        let long = "εr_".repeat(5000);
        for s in ["", "$", "$$", "_", "S1", "1e", "1e9999", "\\$", "$\\frac{$", long.as_str(), "ε∞∞"] {
            let _ = texify(s);
            let _ = rich(s, 12.0, Color32::WHITE);
        }
    }
}
