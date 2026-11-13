//! The font that shows the window's language, as every window looks for it: the language's
//! own characters (its translation's kana and kanji, [`crate::lang::script_chars`]) are looked
//! for among the system's fonts (`fontdb`: the system's font folders, and on Linux the folders
//! fontconfig's files name) and a font file the person chose (Settings ▸ Language). A file chosen
//! comes first, then the families each system ships for the language ([`PREFERRED`], regular
//! weight upright first), then any face. A face is looked at for its character map only (the
//! file mapped, not read), [`MAX_CHECKED`] faces at most; the chosen face's file is read whole,
//! up to [`MAX_FONT`]. Nothing is embedded: where no font shows the language, the window stays
//! in English and says so ([`State::Missing`]).

use std::path::Path;

/// Where the window's language stands for a font.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// English, or a language the toolkit's own fonts show.
    NotNeeded,
    /// Shown with this family.
    Found(String),
    /// No font shows it: this many of its characters are missing in the best one looked at.
    Missing(usize),
}

impl State {
    /// The family in use, in a few words (for `ui.inspect`).
    pub fn text(&self) -> String {
        match self {
            State::Found(f) => f.clone(),
            State::Missing(n) => format!("missing ({n} characters)"),
            State::NotNeeded => "none needed".into(),
        }
    }
}

/// The font for `language`: how it stands, and the face found (the system's fonts searched,
/// with the font file `file` first).
pub fn choose(language: &str, file: Option<&Path>) -> (State, Option<Found>) {
    let needed = crate::lang::script_chars(language);
    if needed.is_empty() {
        return (State::NotNeeded, None);
    }
    match Library::system(file).find(&needed) {
        Ok(found) => (State::Found(found.family.clone()), Some(found)),
        Err(missing) => (State::Missing(missing), None),
    }
}

/// Faces whose character map is read in the search, at most.
pub const MAX_CHECKED: usize = 400;
/// The largest font file used (CJK collections run to about 30 MB).
pub const MAX_FONT: usize = 64 << 20;

/// The families each system ships for Japanese text, best first.
#[cfg(target_os = "windows")]
pub const PREFERRED: &[&str] = &["Yu Gothic UI", "Yu Gothic", "Meiryo UI", "Meiryo", "MS UI Gothic", "MS Gothic"];
#[cfg(target_os = "macos")]
pub const PREFERRED: &[&str] = &["Hiragino Sans", "Hiragino Kaku Gothic ProN", "Hiragino Kaku Gothic Pro"];
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub const PREFERRED: &[&str] = &["Noto Sans CJK JP", "Noto Sans JP", "IPAexGothic", "IPAGothic", "TakaoGothic", "VL Gothic"];

/// A face found and read.
#[derive(Clone, PartialEq)]
pub struct Found {
    /// Its family, as the font names it.
    pub family: String,
    /// The face's index in its file (0 unless a collection).
    pub index: u32,
    /// The whole file.
    pub bytes: Vec<u8>,
}

impl std::fmt::Debug for Found {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Found({:?}, face {}, {} bytes)", self.family, self.index, self.bytes.len())
    }
}

/// The fonts there are: the system's, and a file the user chose.
pub struct Library {
    db: fontdb::Database,
    /// The chosen file's faces.
    chosen: Vec<fontdb::ID>,
}

impl Library {
    /// The system's fonts and the font file `chosen` (a file that cannot be read is left out:
    /// [`Library::chosen_loaded`] says so).
    pub fn system(chosen: Option<&Path>) -> Library {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        Library::with(db, chosen)
    }

    /// The fonts of `folders` and `chosen` only (tests).
    #[cfg(test)]
    pub fn in_folders(folders: &[std::path::PathBuf], chosen: Option<&Path>) -> Library {
        let mut db = fontdb::Database::new();
        for f in folders {
            db.load_fonts_dir(f);
        }
        Library::with(db, chosen)
    }

    fn with(mut db: fontdb::Database, chosen: Option<&Path>) -> Library {
        let before: Vec<fontdb::ID> = db.faces().map(|f| f.id).collect();
        if let Some(path) = chosen {
            let _ = db.load_font_file(path);
        }
        let chosen = db.faces().map(|f| f.id).filter(|id| !before.contains(id)).collect();
        Library { db, chosen }
    }

    /// Whether the chosen file gave a face.
    pub fn chosen_loaded(&self) -> bool {
        !self.chosen.is_empty()
    }

    /// The face showing every one of `needed`, or how many of them the best face looked at
    /// lacks (all of them when there is none).
    pub fn find(&self, needed: &[char]) -> Result<Found, usize> {
        let wanted: Vec<String> = PREFERRED.iter().map(|f| f.to_lowercase()).collect();
        let mut faces: Vec<(usize, u16, bool, fontdb::ID)> = self
            .db
            .faces()
            .map(|f| {
                let rank = if self.chosen.contains(&f.id) {
                    0
                } else {
                    1 + f.families.iter().filter_map(|(name, _)| wanted.iter().position(|w| *w == name.to_lowercase())).min().unwrap_or(wanted.len())
                };
                (rank, f.weight.0.abs_diff(400), f.style != fontdb::Style::Normal, f.id)
            })
            .collect();
        faces.sort_by_key(|&(rank, weight, slanted, _)| (rank, weight, slanted));
        let mut fewest = needed.len();
        for &(_, _, _, id) in faces.iter().take(MAX_CHECKED) {
            let missing = self.db.with_face_data(id, |data, index| lacks(data, index, needed)).unwrap_or(needed.len());
            if missing == 0
                && let Some(found) = self.read(id)
            {
                return Ok(found);
            }
            fewest = fewest.min(missing);
        }
        Err(fewest)
    }

    fn read(&self, id: fontdb::ID) -> Option<Found> {
        let family = self.db.face(id)?.families.first().map_or_default(|(n, _)| n.clone());
        self.db.with_face_data(id, |data, index| (data.len() <= MAX_FONT).then(|| Found { family, index, bytes: data.to_vec() })).flatten()
    }
}

/// How many of `needed` face `index` of `data` has no glyph for (all when it cannot be read).
fn lacks(data: &[u8], index: u32, needed: &[char]) -> usize {
    match ttf_parser::Face::parse(data, index) {
        Ok(face) => needed.iter().filter(|c| !face.glyph_index(**c).is_some_and(|g| g.0 != 0)).count(),
        Err(_) => needed.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let base = std::env::var_os("EMCRAFT_SCRATCH").map_or_else(std::env::temp_dir, PathBuf::from);
        let dir = base.join(format!("emcraft-fonts-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    /// A folder holding a face shows what it has; one holding a broken file and no other says
    /// how many characters are missing; a chosen file comes first, one that does not read
    /// gives none. (The pictures' Latin face stands in for a language's: the search is the
    /// same.)
    #[test]
    fn finds_a_face_showing_the_characters_or_counts_them() {
        let latin: Vec<char> = "Settings".chars().collect();
        let good = scratch("good");
        let _ = std::fs::write(good.join("face.ttf"), emcraft_render::text::face_bytes());
        let _ = std::fs::write(good.join("broken.ttf"), b"not a font");
        assert!(Library::in_folders(std::slice::from_ref(&good), None).find(&latin).is_ok());
        // A face that lacks two of them counts them.
        let some: Vec<char> = "S日本".chars().collect();
        assert_eq!(Library::in_folders(std::slice::from_ref(&good), None).find(&some).err(), Some(2));
        let bad = scratch("bad");
        let _ = std::fs::write(bad.join("broken.ttf"), b"not a font");
        assert_eq!(Library::in_folders(std::slice::from_ref(&bad), None).find(&latin).err(), Some(latin.len()));
        let chosen = Library::in_folders(std::slice::from_ref(&bad), Some(&good.join("face.ttf")));
        assert!(chosen.chosen_loaded() && chosen.find(&latin).is_ok());
        assert!(!Library::in_folders(&[], Some(Path::new("/no/such/font.otf"))).chosen_loaded());
        // English needs no font.
        assert_eq!(choose("en", None), (State::NotNeeded, None));
        for dir in [good, bad] {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}
