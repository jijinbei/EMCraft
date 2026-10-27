//! The app's preferences, kept between launches: the recent projects, the theme, where dialogs
//! go and the display units of new projects. A JSON file in the user's configuration directory
//! (`$EMCRAFT_CONFIG_DIR`, else `$XDG_CONFIG_HOME/emcraft`, else `~/.config/emcraft`); the
//! headless harness keeps none.

use std::path::{Path, PathBuf};

use emcraft_model::units::{Frequency, Length};
use serde::{Deserialize, Serialize};

/// Most recent projects remembered.
pub const MAX_RECENT: usize = 12;

/// Largest preferences file read.
const MAX_BYTES: u64 = 1 << 20;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    /// Project files opened or saved, the newest first.
    pub recent: Vec<String>,
    pub dark: bool,
    pub dock_dialogs: bool,
    /// The 3D view drawn by the window's GPU (fast, painter's order) instead of the CPU
    /// renderer (depth-buffered, supersampled) at rest.
    pub gpu3d: bool,
    /// The display units new projects start with.
    pub length: Length,
    pub frequency: Frequency,
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
        p
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

/// Where the preferences live, when the environment names a home.
pub fn default_path() -> Option<PathBuf> {
    let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    let dir = var("EMCRAFT_CONFIG_DIR")
        .or_else(|| var("XDG_CONFIG_HOME").map(|d| d.join("emcraft")))
        .or_else(|| var("HOME").map(|h| h.join(".config").join("emcraft")))?;
    Some(dir.join("prefs.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferences_round_trip_and_survive_bad_files() {
        let dir = std::env::temp_dir().join(format!("emcraft-prefs-{}", std::process::id()));
        let path = dir.join("sub").join("prefs.json");
        let mut p = Prefs { dark: true, length: Length::Metre, ..Prefs::default() };
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
        assert!(Prefs::load(&path).dark);
        assert_eq!(Prefs::load(&dir.join("missing.json")), Prefs::default());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
