//! The app's preferences, kept between launches: the recent projects, the theme, the language,
//! where dialogs go and the display units of new projects. A JSON file in the user's configuration directory
//! (`$EMCRAFT_CONFIG_DIR`, else `$XDG_CONFIG_HOME/emcraft`, else `~/.config/emcraft`); the
//! headless harness keeps none.

use std::path::{Path, PathBuf};

use emcraft_model::units::{Frequency, Length};
use serde::{Deserialize, Serialize};

/// Most recent projects remembered.
pub const MAX_RECENT: usize = 12;

/// Largest preferences file read.
const MAX_BYTES: u64 = 1 << 20;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    /// Project files opened or saved, the newest first.
    pub recent: Vec<String>,
    /// The theme's name (`light` when empty).
    pub theme: String,
    /// Older preferences' theme switch, read as the theme `dark` and never written.
    #[serde(skip_serializing)]
    pub dark: bool,
    /// The window's language (`en` when empty or unknown).
    pub language: String,
    /// Dialogs dock at the right, beside what they edit (new preferences; a file keeps its own).
    pub dock_dialogs: bool,
    /// The 3D view drawn by the window's GPU (fast, painter's order) instead of the CPU
    /// renderer (depth-buffered, supersampled) at rest.
    pub gpu3d: bool,
    /// The 3D view's mouse mapping, by id (`crate::navigation::NAV_PRESETS`; the default
    /// when unknown).
    pub navigation: String,
    /// What an orbit turns about, by id (`crate::navigation::OrbitAround`).
    pub orbit_around: String,
    /// The 3D view's orientation control, by id (`crate::navigation::Navigator`).
    pub navigator: String,
    /// The display units new projects start with.
    pub length: Length,
    pub frequency: Frequency,
}

/// New preferences: dialogs docked, so they do not cover what they edit; the rest empty, the
/// light theme, English, the CPU's 3D view, millimetres and gigahertz.
impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            recent: Vec::new(),
            theme: String::new(),
            dark: false,
            language: String::new(),
            dock_dialogs: true,
            gpu3d: false,
            navigation: crate::navigation::FUSION.into(),
            orbit_around: String::new(),
            navigator: String::new(),
            length: Length::default(),
            frequency: Frequency::default(),
        }
    }
}

impl Prefs {
    /// Puts `path` first in the recent list.
    pub fn add_recent(&mut self, path: &str) {
        self.recent.retain(|p| p != path);
        self.recent.insert(0, path.to_owned());
        self.recent.truncate(MAX_RECENT);
    }

    /// The preferences in `path`; the defaults when it is missing, too large or unreadable.
    pub fn load(path: &Path) -> Prefs {
        let fits = std::fs::metadata(path).is_ok_and(|m| m.len() <= MAX_BYTES);
        let mut p: Prefs = fits.then(|| std::fs::read(path).ok()).flatten().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        p.recent.truncate(MAX_RECENT);
        if p.theme.is_empty() && std::mem::take(&mut p.dark) {
            p.theme = "dark".into();
        }
        p.theme = p.theme.chars().take(emcraft_engine::theme::MAX_NAME).collect();
        p.language = crate::lang::clip_id(&p.language);
        for id in [&mut p.navigation, &mut p.orbit_around, &mut p.navigator] {
            *id = id.chars().take(32).collect();
        }
        p
    }

    /// The language's id: the one kept, English when it is empty or unknown.
    pub fn language(&self) -> &str {
        crate::lang::find(&self.language).map_or(crate::lang::ENGLISH, |l| l.id)
    }

    /// The 3D view's mouse mapping.
    pub fn nav(&self) -> &'static crate::navigation::NavPreset {
        crate::navigation::preset(&self.navigation)
    }

    /// What an orbit turns about.
    pub fn orbit_around(&self) -> crate::navigation::OrbitAround {
        crate::navigation::OrbitAround::parse(&self.orbit_around).unwrap_or_default()
    }

    /// The 3D view's orientation control.
    pub fn navigator(&self) -> crate::navigation::Navigator {
        crate::navigation::Navigator::parse(&self.navigator).unwrap_or_default()
    }

    /// The theme's name.
    pub fn theme(&self) -> &str {
        if self.theme.is_empty() { "light" } else { &self.theme }
    }

    /// Writes the preferences to `path` (its directory made when missing), through a
    /// temporary file renamed over the old one.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        let temp = path.with_extension("json.tmp");
        std::fs::write(&temp, text)?;
        std::fs::rename(&temp, path)
    }
}

/// The user's configuration directory, when the environment names one: `$EMCRAFT_CONFIG_DIR`,
/// else on Windows `%APPDATA%\emcraft`, else `$XDG_CONFIG_HOME/emcraft`, else `~/.config/emcraft`.
pub fn config_dir() -> Option<PathBuf> {
    let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    let platform = if cfg!(windows) { var("APPDATA").map(|d| d.join("emcraft")) } else { None };
    var("EMCRAFT_CONFIG_DIR")
        .or(platform)
        .or_else(|| var("XDG_CONFIG_HOME").map(|d| d.join("emcraft")))
        .or_else(|| var("HOME").map(|h| h.join(".config").join("emcraft")))
}

/// Where the preferences live, when the environment names a home.
pub fn default_path() -> Option<PathBuf> {
    Some(config_dir()?.join("prefs.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferences_round_trip_and_survive_bad_files() {
        let dir = std::env::temp_dir().join(format!("emcraft-prefs-{}", std::process::id()));
        let path = dir.join("sub").join("prefs.json");
        let mut p = Prefs { theme: "Night shift".into(), length: Length::Metre, ..Prefs::default() };
        for k in 0..20 {
            p.add_recent(&format!("/tmp/{k}.emcraft"));
        }
        p.add_recent("/tmp/3.emcraft");
        assert_eq!(p.recent.len(), MAX_RECENT);
        assert_eq!(p.recent[0], "/tmp/3.emcraft");
        assert_eq!(p.recent.iter().filter(|r| *r == "/tmp/3.emcraft").count(), 1);
        p.save(&path).unwrap();
        assert_eq!(Prefs::load(&path), p);
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(Prefs::load(&path), Prefs::default());
        std::fs::write(&path, r#"{"dark": true}"#).unwrap();
        // Older preferences named the dark theme by a switch.
        assert_eq!(Prefs::load(&path).theme(), "dark");
        std::fs::write(&path, r#"{"dark": false}"#).unwrap();
        assert_eq!(Prefs::load(&path).theme(), "light");
        std::fs::write(&path, format!(r#"{{"theme": "{}"}}"#, "x".repeat(5000))).unwrap();
        assert_eq!(Prefs::load(&path).theme.len(), emcraft_engine::theme::MAX_NAME);
        // The language: an unknown or overlong one reads as English.
        std::fs::write(&path, r#"{"language": "ja"}"#).unwrap();
        assert_eq!(Prefs::load(&path).language(), "ja");
        std::fs::write(&path, format!(r#"{{"language": "{}"}}"#, "x".repeat(5000))).unwrap();
        let odd = Prefs::load(&path);
        assert_eq!((odd.language.len(), odd.language()), (crate::lang::MAX_ID, "en"));
        assert_eq!(Prefs::default().language(), "en");
        // Dialogs dock by default; preferences that chose floating keep it.
        assert!(Prefs::default().dock_dialogs);
        std::fs::write(&path, r#"{"dock_dialogs": false, "theme": "dark"}"#).unwrap();
        assert!(!Prefs::load(&path).dock_dialogs);
        std::fs::write(&path, r#"{"theme": "dark"}"#).unwrap();
        assert!(Prefs::load(&path).dock_dialogs);
        assert_eq!(Prefs::load(&dir.join("missing.json")), Prefs::default());
        // The navigation: SolidCraft's default mapping; unknown ones read as the defaults.
        assert_eq!(Prefs::default().nav().id, "fusion");
        std::fs::write(&path, r#"{"navigation": "blender", "orbit_around": "cursor", "navigator": "axes"}"#).unwrap();
        let nav = Prefs::load(&path);
        assert_eq!(
            (nav.nav().id, nav.orbit_around(), nav.navigator()),
            ("blender", crate::navigation::OrbitAround::Cursor, crate::navigation::Navigator::Axes)
        );
        std::fs::write(&path, format!(r#"{{"navigation": "{}", "orbit_around": "sideways"}}"#, "x".repeat(5000))).unwrap();
        let odd = Prefs::load(&path);
        assert_eq!((odd.nav().id, odd.navigation.len(), odd.orbit_around()), ("fusion", 32, crate::navigation::OrbitAround::ViewCentre));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
