//! A theme made from another by a tint: every colour of its base moved in OKLCH
//! ([`emcraft_render::oklch`]) by its role, so a one-hue theme is five numbers rather than a
//! hundred colours copied by hand, and keeps its base's contrasts by construction.
//!
//! - **Neutral** colours (panels, text, borders, the 3D view's walls, the pictures' ink) take the
//!   tint's hue, their lightness scaled by `lightness`, their chroma `chroma` at middle
//!   lightness and less towards black and white (`chroma · 4L(1 − L)`).
//! - **Accents** (the accent, the ribbon's tabs, the kinds of analysis) take the tint's hue too,
//!   their lightness scaled the same, their chroma at most `accent`.
//! - **Colours told apart** (errors, warnings and success, the axes, ports, sources, materials,
//!   the curves of a chart) keep their hue and lightness, their chroma scaled by `soften`.
//!
//! Alpha is kept. A tint reads as `{"hue": 73, "chroma": 0.05, "lightness": 0.97, "accent": 0.08,
//! "soften": 0.75}`, every number optional (no hue change, greys, lightness kept, grey
//! accents, colours told apart kept).

use emcraft_render::oklch::Oklch;
use serde_json::{Map, Value};

use super::Colour;

/// The largest chroma a tint may ask for (sRGB holds about 0.37 at most).
const MAX_CHROMA: f64 = 0.37;

/// A tint's numbers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tint {
    pub hue: f64,
    pub chroma: f64,
    pub lightness: f64,
    pub accent: f64,
    pub soften: f64,
}

/// A colour's role under a tint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    Neutral,
    Accent,
    Distinct,
}

/// The colours told apart, by part and name.
const DISTINCT: &[(&str, &str)] = &[
    ("ui", "error"),
    ("ui", "warning"),
    ("ui", "success"),
    ("ui", "form_error"),
    ("ui", "form_error_fill"),
    ("ui", "axis_x"),
    ("ui", "axis_y"),
    ("ui", "axis_z"),
    ("scene", "port"),
    ("scene", "port_soft"),
    ("scene", "port_input"),
    ("scene", "port_output"),
    ("scene", "pml"),
    ("scene", "source"),
    ("scene", "periodic"),
    ("scene", "highlight"),
    ("scene", "symmetry_electric"),
    ("scene", "symmetry_magnetic"),
    ("scene", "strip"),
    ("scene", "intersect"),
    ("picture", "h_arrows"),
];

/// The role of colour `key` of `part` (a chart's curves by their place: the last is grey).
fn role(part: &str, key: &str, index: Option<usize>) -> Role {
    let accent = ["accent", "accent_soft", "selected_ink", "info"].contains(&key) || key.starts_with("tab_") || key.starts_with("kind_");
    match (part, key, index) {
        ("picture", "series", Some(i)) if i + 1 < emcraft_render::palette::SERIES => Role::Distinct,
        ("scene", k, _) if k.starts_with("material_") => Role::Distinct,
        _ if DISTINCT.contains(&(part, key)) => Role::Distinct,
        ("ui", _, _) if accent => Role::Accent,
        _ => Role::Neutral,
    }
}

/// A number of the tint: `key` of `o`, `default` without it, refused outside `range`.
fn number(o: &Map<String, Value>, key: &str, default: f64, range: std::ops::RangeInclusive<f64>) -> Result<f64, String> {
    match o.get(key) {
        None => Ok(default),
        Some(v) => {
            v.as_f64().filter(|x| range.contains(x)).ok_or_else(|| format!("`tint.{key}` must be a number from {} to {}", range.start(), range.end()))
        }
    }
}

/// The tint `v` gives, or the problem with it.
///
/// ```
/// use emcraft_engine::theme::tint::read;
/// let t = read(&serde_json::json!({"hue": 73, "chroma": 0.05})).unwrap_or_else(|e| panic!("{e}"));
/// assert_eq!((t.hue, t.lightness, t.soften), (73.0, 1.0, 1.0));
/// assert!(read(&serde_json::json!({"chroma": 2})).is_err());
/// assert!(read(&serde_json::json!({"shade": 1})).is_err());
/// ```
pub fn read(v: &Value) -> Result<Tint, String> {
    const KEYS: [&str; 5] = ["hue", "chroma", "lightness", "accent", "soften"];
    let o = v.as_object().ok_or("`tint` must be an object: {\"hue\": …, \"chroma\": …, \"lightness\": …, \"accent\": …, \"soften\": …}")?;
    if let Some(k) = o.keys().find(|k| !KEYS.contains(&k.as_str())) {
        return Err(format!("`tint` has no `{}`; it has {}", crate::text::clip(k, 40), KEYS.join(", ")));
    }
    Ok(Tint {
        hue: number(o, "hue", 0.0, 0.0..=360.0)?,
        chroma: number(o, "chroma", 0.0, 0.0..=MAX_CHROMA)?,
        lightness: number(o, "lightness", 1.0, 0.5..=1.5)?,
        accent: number(o, "accent", 0.0, 0.0..=MAX_CHROMA)?,
        soften: number(o, "soften", 1.0, 0.0..=1.0)?,
    })
}

impl Tint {
    /// `c`, colour `key` of `part` (its place in a list, if in one), under the tint.
    fn colour(&self, part: &str, key: &str, index: Option<usize>, c: Colour) -> Colour {
        let o = c.rgb().oklch();
        let l = (o.l * self.lightness).clamp(0.0, 1.0);
        let moved = match role(part, key, index) {
            Role::Neutral => Oklch { l, c: self.chroma * 4.0 * l * (1.0 - l), h: self.hue },
            Role::Accent => Oklch { l, c: o.c.min(self.accent), h: self.hue },
            Role::Distinct => Oklch { c: o.c * self.soften, ..o },
        };
        let rgb = moved.rgb();
        Colour { r: rgb.0, g: rgb.1, b: rgb.2, a: c.a }
    }

    /// The colours of `part` (an object of colours by name, lists of them too) under the tint;
    /// what is not a colour is left as it is.
    pub fn part(&self, part: &str, v: &Value) -> Value {
        let one = |key: &str, index: Option<usize>, v: &Value| match v.as_str().and_then(Colour::parse) {
            Some(c) => Value::String(self.colour(part, key, index, c).hex()),
            None => v.clone(),
        };
        match v.as_object() {
            Some(o) => Value::Object(
                o.iter()
                    .map(|(k, v)| {
                        let moved = match v.as_array() {
                            Some(list) => Value::Array(list.iter().enumerate().map(|(i, c)| one(k, Some(i), c)).collect()),
                            None => one(k, None, v),
                        };
                        (k.clone(), moved)
                    })
                    .collect(),
            ),
            None => v.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Under a tint a neutral colour takes its hue at the lightness asked, an accent's chroma is
    /// capped, a colour told apart keeps its hue and lightness; alpha stays.
    #[test]
    fn each_role_moves_its_own_way() {
        let t = Tint { hue: 73.0, chroma: 0.05, lightness: 0.97, accent: 0.08, soften: 0.5 };
        let at = |part: &str, key: &str, hex: &str| Colour::parse(hex).map(|c| t.colour(part, key, None, c));
        let panel = at("ui", "panel", "#fafbfc").map(|c| c.rgb().oklch());
        assert!(panel.is_some_and(|o| (o.h - 73.0).abs() < 3.0 && (o.l - 0.988 * 0.97).abs() < 0.01), "{panel:?}");
        let accent = at("ui", "accent", "#2678da").map(|c| c.rgb().oklch());
        assert!(accent.is_some_and(|o| o.c <= 0.081 && (o.h - 73.0).abs() < 3.0), "{accent:?}");
        let error = (Colour::parse("#d63636").map(|c| c.rgb().oklch()), at("ui", "error", "#d63636").map(|c| c.rgb().oklch()));
        assert!(
            matches!(error, (Some(a), Some(b)) if (a.h - b.h).abs() < 3.0 && (a.l - b.l).abs() < 0.01 && (b.c - a.c / 2.0).abs() < 0.01),
            "{error:?}"
        );
        assert_eq!(at("ui", "app_bar_hover", "#ffffff1c").map(|c| c.a), Some(0x1c));
    }
}
