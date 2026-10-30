//! Themes as files: the format a theme is written in (docs/themes.md), read and checked, and
//! the [`Palette`] pictures are drawn in (`view.*`'s `theme`: `light`, `dark` or a theme file).
//! The built-in themes are files of the same format (`crates/engine/themes/light.json` and
//! `dark.json`, compiled in), naming every colour there is: no theme colour lives in code.
//!
//! A theme file is a JSON object:
//!
//! ```json
//! {"name": "Night shift", "base": "dark",
//!  "ui": {"accent": "#4a9cf0", "app_bar_hover": "#ffffff1c"},
//!  "scene": {"port": "#ff9a3c"},
//!  "picture": {"ground": "#20242c", "series": ["#5a96f0", "#f06e5a"]}}
//! ```
//!
//! Every colour is `#rrggbb` or `#rrggbbaa` (straight, not premultiplied, alpha). A user's
//! theme starts from its `base` (light or dark) and overrides the colours it names; a built-in
//! one has no base and names every colour. The `picture` part is resolved here onto the base's
//! palette and checked (its names, opaque colours, the ink readable on the ground); the `ui` and
//! `scene` parts are kept as colours by name, whose names the desktop app checks when it loads
//! the theme. Everything read is hostile: the file's size, the name's length and the number of
//! colours are capped, and text echoed in a problem is clipped.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use emcraft_render::Rgb;
use emcraft_render::palette::{self, Palette};
use serde_json::{Map, Value, json};

use crate::text::{clip, unknown};
use crate::{EngineError, Params, Result, Session};

/// The largest theme file read (bytes).
pub const MAX_BYTES: u64 = 64 << 10;
/// The longest theme name (characters).
pub const MAX_NAME: usize = 40;
/// The longest description (characters).
pub const MAX_DESCRIPTION: usize = 200;
/// The most colours a theme's `ui` or `scene` part names.
pub const MAX_COLOURS: usize = 256;
/// The longest colour name (characters).
pub const MAX_KEY: usize = 40;
/// The most problems reported for one file.
const MAX_PROBLEMS: usize = 20;

/// The keys of a theme file.
const KEYS: [&str; 6] = ["name", "description", "base", "ui", "scene", "picture"];

emcraft_macros::named_enum! {
    /// A built-in theme: what a user's theme starts from.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
    pub enum Base {
        #[default]
        Light = "light",
        Dark = "dark",
    }
}

/// The built-in themes' files.
const LIGHT_FILE: &str = include_str!("../themes/light.json");
const DARK_FILE: &str = include_str!("../themes/dark.json");

impl Base {
    /// The built-in theme's file.
    pub const fn text(self) -> &'static str {
        match self {
            Base::Light => LIGHT_FILE,
            Base::Dark => DARK_FILE,
        }
    }

    /// The built-in theme read (once) and checked.
    pub fn checked(self) -> &'static Checked {
        static LIGHT: LazyLock<Checked> = LazyLock::new(|| check_as(LIGHT_FILE, Some(Base::Light)));
        static DARK: LazyLock<Checked> = LazyLock::new(|| check_as(DARK_FILE, Some(Base::Dark)));
        match self {
            Base::Light => &LIGHT,
            Base::Dark => &DARK,
        }
    }

    /// The built-in theme; refused with its problems should its file be broken.
    pub fn theme(self) -> Result<&'static ThemeFile> {
        let c = self.checked();
        if c.problems.is_empty() {
            Ok(&c.theme)
        } else {
            Err(EngineError::failed(format!("the built-in theme `{self}` is broken: {}", c.problems.join("; "))))
        }
    }

    /// The built-in theme's picture palette ([`Palette::fallback`] should its file be broken).
    pub fn palette(self) -> Palette {
        self.theme().map_or_else(|_| Palette::fallback(), |t| t.picture)
    }
}

/// A colour as a theme file writes it: `#rrggbb` or `#rrggbbaa`, the alpha straight.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Colour {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Colour {
    /// `#rrggbb` (opaque) or `#rrggbbaa`, hex digits of either case.
    ///
    /// ```
    /// use emcraft_engine::theme::Colour;
    /// assert_eq!(Colour::parse("#1E5ac8"), Some(Colour { r: 30, g: 90, b: 200, a: 255 }));
    /// assert_eq!(Colour::parse("#ffffff1c").map(|c| c.a), Some(28));
    /// assert_eq!(Colour::parse("1e5ac8"), None);
    /// assert_eq!(Colour::parse("#1e5ac"), None);
    /// ```
    pub fn parse(s: &str) -> Option<Colour> {
        let hex = s.strip_prefix('#')?;
        if !(hex.len() == 6 || hex.len() == 8) || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let byte = |i: usize| hex.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok());
        Some(Colour { r: byte(0)?, g: byte(2)?, b: byte(4)?, a: if hex.len() == 8 { byte(6)? } else { 255 } })
    }

    pub fn rgb(self) -> Rgb {
        Rgb(self.r, self.g, self.b)
    }

    /// `#rrggbb`, or `#rrggbbaa` when not opaque.
    pub fn hex(self) -> String {
        let rgb = self.rgb().hex();
        if self.a == 255 { rgb } else { format!("{rgb}{:02x}", self.a) }
    }
}

/// A theme as its file gives it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ThemeFile {
    pub name: String,
    pub description: String,
    /// The built-in theme it starts from (a built-in theme's own).
    pub base: Base,
    /// The desktop app's colours (its tokens) by name.
    pub ui: BTreeMap<String, Colour>,
    /// The 3D view's colours by name.
    pub scene: BTreeMap<String, Colour>,
    /// The base's palette with the file's `picture` colours over it.
    pub picture: Palette,
}

/// A theme file's text checked: the theme as far as it reads, and every problem found (none:
/// the theme is good).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Checked {
    pub theme: ThemeFile,
    pub problems: Vec<String>,
}

impl Checked {
    fn problem(&mut self, p: String) {
        if self.problems.len() < MAX_PROBLEMS {
            self.problems.push(p);
        }
    }
}

/// A key or a text from the file, clipped for a problem's sentence.
fn shown(s: &str) -> String {
    clip(s, MAX_KEY)
}

/// A colour named `key` in `part`, or the problem with it.
fn colour(part: &str, key: &str, v: &Value) -> std::result::Result<Colour, String> {
    v.as_str().and_then(Colour::parse).ok_or_else(|| {
        let got = v.as_str().map_or_else(|| clip(&v.to_string(), 20), |s| format!("\"{}\"", clip(s, 20)));
        format!("`{part}.{}` must be a colour `#rrggbb` or `#rrggbbaa`, got {got}", shown(key))
    })
}

/// An opaque colour named `key` in `part` (pictures and the 3D view have no see-through
/// colours), or the problem with it.
fn opaque(part: &str, key: &str, v: &Value) -> std::result::Result<Rgb, String> {
    let c = colour(part, key, v)?;
    if c.a != 255 {
        return Err(format!("`{part}.{}` must be opaque (`#rrggbb`)", shown(key)));
    }
    Ok(c.rgb())
}

/// A name of a colour: a short word of letters, digits and `_`.
fn good_key(k: &str) -> bool {
    !k.is_empty() && k.chars().count() <= MAX_KEY && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The `ui` or `scene` part: colours by name (`opaque_only` for the scene).
fn colour_map(part: &str, v: &Value, opaque_only: bool, out: &mut Checked) -> BTreeMap<String, Colour> {
    let mut map = BTreeMap::new();
    let Some(o) = v.as_object() else {
        out.problem(format!("`{part}` must be an object of colours by name"));
        return map;
    };
    if o.len() > MAX_COLOURS {
        out.problem(format!("`{part}` names {} colours; at most {MAX_COLOURS}", o.len()));
        return map;
    }
    for (k, v) in o {
        if !good_key(k) {
            out.problem(format!("`{part}` has a bad name `{}`: letters, digits and `_`, at most {MAX_KEY}", shown(k)));
            continue;
        }
        let c = if opaque_only { opaque(part, k, v).map(|c| Colour { r: c.0, g: c.1, b: c.2, a: 255 }) } else { colour(part, k, v) };
        match c {
            Ok(c) => {
                map.insert(k.clone(), c);
            }
            Err(e) => out.problem(e),
        }
    }
    map
}

/// The `picture` part over `base`'s palette, or for a built-in theme (`whole`) every colour of
/// a palette.
fn picture(v: &Value, base: Base, whole: bool, out: &mut Checked) -> Palette {
    let mut p = if whole {
        Palette::fallback()
    } else {
        match base.theme() {
            Ok(t) => t.picture,
            Err(e) => {
                out.problem(e.to_string());
                Palette::fallback()
            }
        }
    };
    let Some(o) = v.as_object() else {
        out.problem("`picture` must be an object of colours by name".into());
        return p;
    };
    if whole {
        let missing: Vec<&str> = palette::COLOURS.iter().chain(&["series"]).copied().filter(|k| !o.contains_key(*k)).collect();
        if !missing.is_empty() {
            out.problem(format!("`picture` must name every colour; missing {}", missing.join(", ")));
        }
        if o.get("series").and_then(Value::as_array).is_some_and(|l| l.len() != palette::SERIES) {
            out.problem(format!("`picture.series` must list all {} colours", palette::SERIES));
        }
    }
    for (k, v) in o {
        if k == "series" {
            match v.as_array() {
                Some(list) if list.len() <= palette::SERIES => {
                    for (i, (slot, c)) in p.series.iter_mut().zip(list).enumerate() {
                        match opaque("picture", &format!("series[{i}]"), c) {
                            Ok(c) => *slot = c,
                            Err(e) => out.problem(e),
                        }
                    }
                }
                _ => out.problem(format!("`picture.series` must be a list of at most {} colours", palette::SERIES)),
            }
            continue;
        }
        let Some(slot) = p.colour_mut(k) else {
            out.problem(unknown(format!("`picture` has no colour `{}`", shown(k)), "it has ", palette::COLOURS.iter().chain(&["series"])));
            continue;
        };
        match opaque("picture", k, v) {
            Ok(c) => *slot = c,
            Err(e) => out.problem(e),
        }
    }
    if let Some(problem) = p.contrast_problem() {
        out.problem(format!("`picture`: {problem}"));
    }
    p
}

/// Checks a user's theme file's text: the theme as far as it reads, and its problems.
pub fn check(text: &str) -> Checked {
    check_as(text, None)
}

/// Checks a theme file's text as the built-in theme `builtin`, or as a user's theme.
fn check_as(text: &str, builtin: Option<Base>) -> Checked {
    let mut out = Checked::default();
    if text.len() as u64 > MAX_BYTES {
        out.problem(format!("the file is {} bytes; at most {MAX_BYTES} are read", text.len()));
        return out;
    }
    let v: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            out.problem(format!("not JSON: {e}"));
            return out;
        }
    };
    let Some(o) = v.as_object() else {
        out.problem("a theme is a JSON object: {\"name\": …, \"base\": …, \"ui\": {…}, \"scene\": {…}, \"picture\": {…}}".into());
        return out;
    };
    for k in o.keys().filter(|k| !KEYS.contains(&k.as_str())) {
        out.problem(unknown(format!("unknown key `{}`", shown(k)), "a theme has ", KEYS));
    }
    match o.get("name").and_then(Value::as_str).map(str::trim) {
        None => out.problem(format!("`name` is required: a text of at most {MAX_NAME} characters")),
        Some(n) if n.is_empty() || n.chars().count() > MAX_NAME || n.chars().any(char::is_control) => {
            out.problem(format!("`name` must be 1 to {MAX_NAME} characters, no control characters"));
        }
        Some(n) => match builtin {
            Some(b) if b.id() != n => out.problem(format!("the built-in theme's `name` must be `{b}`")),
            None if Base::parse(&n.to_lowercase()).is_some() => out.problem(format!("`name` `{n}` is a built-in theme's")),
            _ => out.theme.name = n.to_owned(),
        },
    }
    match o.get("description") {
        None => {}
        Some(Value::String(d)) if d.chars().count() <= MAX_DESCRIPTION => out.theme.description = d.clone(),
        Some(_) => out.problem(format!("`description` must be a text of at most {MAX_DESCRIPTION} characters")),
    }
    match (o.get("base"), builtin) {
        (None, b) => out.theme.base = b.unwrap_or_default(),
        (Some(_), Some(b)) => {
            out.theme.base = b;
            out.problem("a built-in theme has no `base`: it is one".into());
        }
        (Some(b), None) => match b.as_str().and_then(Base::parse) {
            Some(b) => out.theme.base = b,
            None => out.problem(format!("`base` must be {}", <Base as emcraft_macros::Named>::listed())),
        },
    }
    let base = out.theme.base;
    let empty = Value::Object(Map::new());
    out.theme.ui = colour_map("ui", o.get("ui").unwrap_or(&empty), false, &mut out);
    out.theme.scene = colour_map("scene", o.get("scene").unwrap_or(&empty), true, &mut out);
    out.theme.picture = picture(o.get("picture").unwrap_or(&empty), base, builtin.is_some(), &mut out);
    out
}

/// A user's theme file's text read as a theme; refused with its problems.
pub fn parse(text: &str) -> Result<ThemeFile> {
    let c = check(text);
    if c.problems.is_empty() { Ok(c.theme) } else { Err(EngineError::Params(c.problems.join("; "))) }
}

/// The text of the theme file at `path` (at most [`MAX_BYTES`]).
pub fn read_text(path: &str) -> Result<String> {
    let bytes = crate::geometry::read_capped(path, MAX_BYTES)?;
    String::from_utf8(bytes).map_err(|_| EngineError::failed(format!("`{}` is not UTF-8 text", clip(path, 200))))
}

/// The user's theme file at `path`.
pub fn read(path: &str) -> Result<ThemeFile> {
    parse(&read_text(path)?).map_err(|e| EngineError::Params(format!("theme `{}`: {e}", clip(path, 200))))
}

/// A picture palette by name: `light`, `dark` (the built-in themes) or the path of a theme
/// file.
pub fn palette_named(theme: &str) -> Result<Palette> {
    match Base::parse(theme) {
        Some(b) => Ok(b.palette()),
        None => Ok(read(theme)?.picture),
    }
}

/// The palette a picture command's `theme` asks for (light without one).
pub(crate) fn palette_of(p: &Params) -> Result<Palette> {
    match p.string("theme").optional()? {
        None => Ok(Base::Light.palette()),
        Some(t) => palette_named(t),
    }
}

/// A palette as JSON: its colours by name, `series` a list.
pub fn palette_json(p: &Palette) -> Value {
    let mut o: Map<String, Value> = palette::COLOURS.iter().filter_map(|k| Some(((*k).to_owned(), json!(p.colour(k)?.hex())))).collect();
    o.insert("series".into(), json!(p.series.iter().map(|c| c.hex()).collect::<Vec<_>>()));
    Value::Object(o)
}

/// `theme.check`: reads a theme file (or a built-in theme, by name) and reports what it gives and
/// what is wrong with it.
pub fn check_command(_: &mut Session, p: &Params) -> Result<Value> {
    let path = p.string("path").required()?;
    let read;
    let c = match Base::parse(path) {
        Some(b) => b.checked(),
        None => {
            read = check(&read_text(path)?);
            &read
        }
    };
    let colours = |m: &BTreeMap<String, Colour>| m.iter().map(|(k, c)| (k.clone(), json!(c.hex()))).collect::<Map<String, Value>>();
    Ok(json!({
        "ok": c.problems.is_empty(),
        "problems": c.problems,
        "name": c.theme.name,
        "description": c.theme.description,
        "base": c.theme.base.id(),
        "picture": palette_json(&c.theme.picture),
        "ui": colours(&c.theme.ui),
        "scene": colours(&c.theme.scene),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r##"{
        "name": "Night shift", "base": "dark", "description": "for late solves",
        "ui": {"accent": "#4A9CF0", "app_bar_hover": "#ffffff1c"},
        "scene": {"port": "#ff9a3c"},
        "picture": {"ground": "#20242c", "series": ["#5a96f0", "#f06e5a"], "h_arrows": "#008040"}
    }"##;

    /// Both built-in themes read without a problem, name every picture colour and keep their
    /// inks readable: the fallback is never needed.
    #[test]
    fn the_built_in_themes_are_whole_and_readable() {
        for b in Base::ALL {
            let c = b.checked();
            assert_eq!(c.problems, Vec::<String>::new(), "{b}");
            assert_eq!((c.theme.name.as_str(), c.theme.base), (b.id(), b));
            assert_ne!(c.theme.picture, Palette::fallback());
            assert_eq!(c.theme.picture.contrast_problem(), None);
            // Every series colour shows on the ground as a line (2: the light theme's orange).
            for s in c.theme.picture.series {
                assert!(s.contrast_ratio(c.theme.picture.ground) >= 2.0, "{b}: {s:?}");
            }
            assert!(!c.theme.ui.is_empty() && !c.theme.scene.is_empty());
        }
        assert_ne!(Base::Light.palette(), Base::Dark.palette());
        assert_eq!(Base::Light.palette().ground, Rgb::WHITE);
        // A built-in theme's rules: its own name, no base, every picture colour.
        let bad = check_as(r##"{"name": "light", "base": "dark", "picture": {"ink": "#000000"}}"##, Some(Base::Light)).problems.join("; ");
        assert!(bad.contains("has no `base`") && bad.contains("must name every colour; missing ground"), "{bad}");
        assert!(check_as(r#"{"name": "other"}"#, Some(Base::Dark)).problems[0].contains("must be `dark`"));
    }

    #[test]
    fn a_good_file_resolves_onto_its_base() {
        let t = parse(GOOD).unwrap();
        assert_eq!((t.name.as_str(), t.base, t.description.as_str()), ("Night shift", Base::Dark, "for late solves"));
        assert_eq!(t.ui["accent"], Colour { r: 0x4a, g: 0x9c, b: 0xf0, a: 255 });
        assert_eq!(t.ui["app_bar_hover"].a, 0x1c);
        assert_eq!(t.scene["port"].rgb(), Rgb(0xff, 0x9a, 0x3c));
        let mut want = Base::Dark.palette();
        want.ground = Rgb(0x20, 0x24, 0x2c);
        want.series[0] = Rgb(0x5a, 0x96, 0xf0);
        want.series[1] = Rgb(0xf0, 0x6e, 0x5a);
        want.h_arrows = Rgb(0, 0x80, 0x40);
        assert_eq!(t.picture, want);
        // The bare minimum: a name; light by default.
        let t = parse(r#"{"name": "plain"}"#).unwrap();
        assert_eq!((t.base, t.picture), (Base::Light, Base::Light.palette()));
        assert_eq!(palette_json(&Base::Light.palette())["series"][0], "#1e5ac8");
        assert_eq!(palette_json(&Base::Light.palette())["ground"], "#ffffff");
    }

    fn problems(text: &str) -> Vec<String> {
        let c = check(text);
        assert!(parse(text).is_err(), "{text}");
        c.problems
    }

    #[test]
    fn each_kind_of_bad_file_is_refused_with_its_reason() {
        let one = |text: &str, want: &str| {
            let p = problems(text);
            assert!(p.iter().any(|m| m.contains(want)), "{text}: {p:?}");
        };
        one("{ not json", "not JSON");
        one("[1, 2]", "a theme is a JSON object");
        one(r#"{"base": "light"}"#, "`name` is required");
        one(r#"{"name": ""}"#, "`name` must be 1 to 40");
        one(&format!(r#"{{"name": "{}"}}"#, "x".repeat(41)), "`name` must be 1 to 40");
        one(r#"{"name": "a\u0007b"}"#, "no control characters");
        one(r#"{"name": "Dark"}"#, "is a built-in theme's");
        one(r#"{"name": "t", "base": "sepia"}"#, "`base` must be light or dark");
        one(r#"{"name": "t", "colours": {}}"#, "unknown key `colours` (a theme has name, description, base, ui, scene, picture)");
        one(r#"{"name": "t", "ui": {"accent": "blue"}}"#, "`ui.accent` must be a colour `#rrggbb` or `#rrggbbaa`, got \"blue\"");
        one(r##"{"name": "t", "ui": {"accent": "#12345"}}"##, "`ui.accent` must be a colour");
        one(r#"{"name": "t", "ui": {"accent": 7}}"#, "got 7");
        one(r##"{"name": "t", "ui": {"bad key!": "#123456"}}"##, "bad name `bad key!`");
        one(r#"{"name": "t", "ui": []}"#, "`ui` must be an object");
        one(r##"{"name": "t", "scene": {"port": "#12345680"}}"##, "`scene.port` must be opaque");
        one(r##"{"name": "t", "picture": {"paper": "#ffffff"}}"##, "`picture` has no colour `paper` (it has ground, ink, ink_dim");
        one(r##"{"name": "t", "picture": {"grid": "#ffffff80"}}"##, "`picture.grid` must be opaque");
        one(r##"{"name": "t", "picture": {"series": ["#ffffff", "x"]}}"##, "`picture.series[1]` must be a colour");
        one(r##"{"name": "t", "picture": {"series": "#ffffff"}}"##, "`picture.series` must be a list of at most 6");
        one(r##"{"name": "t", "picture": {"ink": "#eeeeee"}}"##, "`picture`: `ink` on `ground` has a contrast of");
        one(r##"{"name": "t", "base": "dark", "picture": {"ink_dim": "#3a3f4a"}}"##, "`ink_dim` on `ground`");
        one(r#"{"name": "t", "description": 5}"#, "`description` must be a text");
    }

    #[test]
    fn hostile_sizes_are_refused_before_they_cost() {
        let huge = format!(r#"{{"name": "t", "ui": {{"a": "{}"}}}}"#, " ".repeat(MAX_BYTES as usize));
        assert!(problems(&huge)[0].contains("at most 65536 are read"));
        let many: Vec<String> = (0..=MAX_COLOURS).map(|i| format!(r##""c{i}": "#000000""##)).collect();
        assert!(problems(&format!(r#"{{"name": "t", "ui": {{{}}}}}"#, many.join(",")))[0].contains("at most 256"));
        let series = [r##""#000000""##; 7].join(",");
        assert!(problems(&format!(r#"{{"name": "t", "picture": {{"series": [{series}]}}}}"#))[0].contains("at most 6"));
        // A file of nothing but bad colours reports a bounded number of problems, clipped.
        let bad: Vec<String> = (0..200).map(|i| format!(r#""k{i}": "{}""#, "z".repeat(200))).collect();
        let p = problems(&format!(r#"{{"name": "t", "ui": {{{}}}}}"#, bad.join(",")));
        assert_eq!(p.len(), MAX_PROBLEMS);
        assert!(p.iter().all(|m| m.len() < 200), "{p:?}");
        let long_key = format!(r##"{{"name": "t", "picture": {{"{}": "#000000"}}}}"##, "k".repeat(5000));
        assert!(problems(&long_key).iter().all(|m| m.len() < 400));
    }

    #[test]
    fn the_picture_palette_by_name_or_file() {
        assert_eq!(palette_named("light").unwrap(), Base::Light.palette());
        assert_eq!(palette_named("dark").unwrap(), Base::Dark.palette());
        let dir = crate::tests::scratch("themes");
        let _ = std::fs::create_dir_all(&dir);
        let path = format!("{dir}/night.json");
        std::fs::write(&path, GOOD).unwrap();
        assert_eq!(palette_named(&path).unwrap().ground, Rgb(0x20, 0x24, 0x2c));
        assert!(palette_named(&format!("{dir}/missing.json")).is_err());
        std::fs::write(&path, r##"{"name": "t", "picture": {"ink": "#ffffff"}}"##).unwrap();
        let e = palette_named(&path).unwrap_err().to_string();
        assert!(e.contains("`ink` on `ground`"), "{e}");
    }
}
