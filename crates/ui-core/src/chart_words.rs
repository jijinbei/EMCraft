//! The words the engine's charts write, in English, and the window's language for them: a
//! chart's title, axes, legend and marks are each found among [`TEMPLATES`] (every one checked
//! against the translations by `t!`) and shown translated, the values a template holds
//! translated in turn (`frequency (GHz)` inside `{label} (dB)`). A text no template matches is
//! shown as it is.

use emcraft_render::chart::Chart;

/// The charts' texts, the most specific first: a template is tried before those with fewer
/// words.
pub const TEMPLATES: &[&str] = &[
    t!("System budget at {f} {unit}").en(),
    t!("System budget").en(),
    t!("noise figure (dB)").en(),
    t!("gain (dB)").en(),
    t!("stage").en(),
    t!("budget").en(),
    t!("gain").en(),
    t!("S-parameters").en(),
    t!("Q of each resonance").en(),
    t!("Energy gained by entry phase").en(),
    t!("entry phase (°)").en(),
    t!("energy gained (keV)").en(),
    t!("tracked").en(),
    t!("constant speed: qV₀T cos φ").en(),
    t!("Dispersion along {axis}").en(),
    t!("frequency ({unit})").en(),
    t!("$|S|$ (linear, 0 to 1)").en(),
    t!("phase (°)").en(),
    t!("solved in full").en(),
    t!("{entry} FEM (order {order})").en(),
    t!("{entry} time domain").en(),
    t!("round-trip time (ns)").en(),
    t!("TDR at port {port}, low pass: step response{impedance}").en(),
    t!("TDR at port {port}, band pass: where reflections come from").en(),
    t!("ρ (step)").en(),
    t!("reflection ρ, step response (unitless)").en(),
    t!("|h| (impulse)").en(),
    t!("|h|, impulse response (unitless)").en(),
    t!("light line").en(),
    t!("band {n} (FEM)").en(),
    t!("phase per period ψ (°)").en(),
    t!("evaluation").en(),
    t!("each solve").en(),
    t!("best so far").en(),
    t!("Sweep of {variable}").en(),
    t!("Eigenmodes as {variable} varies").en(),
    t!("{label} as {variable} varies").en(),
    // Before the general `{label} at …`, which would take it.
    t!("Far field at {f} {unit}, {held}").en(),
    t!("{label} at {f} {unit}").en(),
    t!("{label} (predicted)").en(),
    t!("mode {n}").en(),
    t!("FEM, {component}").en(),
    t!("Mie series").en(),
    t!("{label}: rings every {step} dB, outer {top}").en(),
    t!("directivity (dBi)").en(),
    t!("gain (dBi)").en(),
    t!("realized gain (dBi)").en(),
    t!("RCS (dBsm)").en(),
    t!("Hertzian dipole").en(),
    t!("{along} (degrees)").en(),
    t!("{label} (dB)").en(),
    t!("plane across {axis}").en(),
    t!("rotation about z ({group})").en(),
    t!("{mode} port {ports}").en(),
    t!("{mode} section {n}").en(),
    t!("{mode} sections {n}").en(),
    t!("{name} ({unit})").en(),
];

/// Most levels of templates inside templates translated.
const MAX_DEPTH: usize = 3;

/// `text` in the window's language: the first template it matches, translated, its values in
/// turn; else as it is.
pub fn translate(text: &str) -> String {
    translate_at(text, 0)
}

fn translate_at(text: &str, depth: usize) -> String {
    if text.is_empty() || depth > MAX_DEPTH {
        return text.to_owned();
    }
    for template in TEMPLATES {
        if let Some(values) = matches(template, text) {
            let values: Vec<(String, String)> = values.into_iter().map(|(k, v)| (k, translate_at(&v, depth + 1))).collect();
            let args: Vec<(&str, &dyn std::fmt::Display)> = values.iter().map(|(k, v)| (k.as_str(), v as &dyn std::fmt::Display)).collect();
            return crate::lang::fill(crate::lang::lookup(template), &args);
        }
    }
    text.to_owned()
}

/// The values of `template`'s placeholders that make it `text`, each found as the shortest
/// run before the next literal piece; none when it is not `text`.
fn matches(template: &str, text: &str) -> Option<Vec<(String, String)>> {
    // The template's pieces: literal, name, literal, name, …, literal.
    let mut literals = Vec::new();
    let mut names = Vec::new();
    let mut rest = template;
    loop {
        match rest.find('{').and_then(|i| rest[i..].find('}').map(|j| (i, i + j))) {
            Some((i, j)) if rest[i + 1..j].chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && j > i + 1 => {
                literals.push(&rest[..i]);
                names.push(rest[i + 1..j].to_owned());
                rest = &rest[j + 1..];
            }
            _ => {
                literals.push(rest);
                break;
            }
        }
    }
    let first = literals.first().copied().unwrap_or("");
    let mut at = text.strip_prefix(first)?;
    if names.is_empty() {
        return at.is_empty().then(Vec::new);
    }
    let mut out = Vec::with_capacity(names.len());
    for (k, name) in names.into_iter().enumerate() {
        let next = literals.get(k + 1).copied().unwrap_or("");
        let last = k + 2 == literals.len();
        let end = if last {
            if next.is_empty() { at.len() } else { at.len().checked_sub(next.len()).filter(|e| at.get(*e..) == Some(next))? }
        } else if next.is_empty() {
            return None;
        } else {
            at.find(next)?
        };
        let value = &at[..end];
        if value.is_empty() && !last {
            return None;
        }
        out.push((name, value.to_owned()));
        at = &at[end + next.len()..];
    }
    at.is_empty().then_some(out)
}

/// `chart`'s texts in the window's language.
pub fn translate_chart(chart: &mut Chart) {
    chart.title = translate(&chart.title);
    chart.x.label = translate(&chart.x.label);
    chart.y.label = translate(&chart.y.label);
    for s in &mut chart.series {
        s.label = translate(&s.label);
    }
    for m in &mut chart.marks {
        m.1 = translate(&m.1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A text whose end is not a template's ends there in the middle of a character (`→`): no
    /// match, and no panic slicing it (a pulse chart's title once took the window down).
    #[test]
    fn a_text_ending_inside_a_character_matches_nothing_and_does_not_panic() {
        assert_eq!(matches("{a} ns)", "Pulse: ×1.58, 8.00 → 4.60 ns"), None);
        for text in ["→", "a → b", "×→×", "Pulse: ×1.58 peak power, 8.00 → 4.60 ns (linear chirp)"] {
            for template in TEMPLATES {
                let _ = matches(template, text);
            }
            let _ = translate_at(text, 0);
        }
    }

    #[test]
    fn templates_match_and_fill() {
        assert_eq!(matches("frequency ({unit})", "frequency (GHz)"), Some(vec![("unit".into(), "GHz".into())]));
        assert_eq!(
            matches("{entry} FEM (order {order})", "$|S_{21}|$ FEM (order 2)"),
            Some(vec![("entry".into(), "$|S_{21}|$".into()), ("order".into(), "2".into())])
        );
        assert_eq!(matches("S-parameters", "S-parameters"), Some(vec![]));
        assert_eq!(matches("S-parameters", "S-parameters!"), None);
        assert_eq!(matches("{label} (dB)", "gain"), None);
        crate::lang::set("ja");
        assert_eq!(translate("frequency (GHz)"), "周波数 (GHz)");
        assert_eq!(translate("$|S_{21}|$ FEM (order 2)"), "$|S_{21}|$ FEM（2 次）");
        assert_eq!(translate("frequency (GHz) (dB)"), "周波数 (GHz) (dB)");
        assert_eq!(translate("nothing like it"), "nothing like it");
        crate::lang::set("en");
        assert_eq!(translate("frequency (GHz)"), "frequency (GHz)");
    }
}
