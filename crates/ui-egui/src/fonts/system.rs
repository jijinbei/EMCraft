//! The fonts this system has (`fontdb`: the system's font folders, and on Linux the folders
//! fontconfig's files name), and the face that shows a language: every character of its
//! translation that egui's own fonts lack. A font file the user chose comes first, then the
//! families each system ships for the language ([`PREFERRED`], regular weight upright first),
//! then any face. A face is looked at for its character map only (the file mapped, not read),
//! [`MAX_CHECKED`] faces at most; the chosen face's file is read whole, up to [`MAX_FONT`].

use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;

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
    pub fn in_folders(folders: &[PathBuf], chosen: Option<&Path>) -> Library {
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

    /// The faces there are.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.db.len()
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
        let family = self.db.face(id)?.families.first().map(|(n, _)| n.clone()).unwrap_or_default();
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

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("emcraft-fonts-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A folder holding the test font shows Japanese; one holding a broken file and no other
    /// says how many characters are missing; a chosen file comes first.
    #[test]
    fn finds_a_face_showing_the_characters_or_counts_them() {
        let needed: Vec<char> = "設定日本語".chars().collect();
        let good = scratch("good");
        std::fs::write(good.join("jp.otf"), super::super::NOTO_SANS_JP_OTF).unwrap();
        std::fs::write(good.join("broken.ttf"), b"not a font").unwrap();
        let found = Library::in_folders(std::slice::from_ref(&good), None).find(&needed).unwrap();
        assert!(found.family.contains("Noto Sans JP"), "{found:?}");
        let bad = scratch("bad");
        std::fs::write(bad.join("broken.ttf"), b"not a font").unwrap();
        assert_eq!(Library::in_folders(std::slice::from_ref(&bad), None).find(&needed), Err(5));
        let chosen = Library::in_folders(std::slice::from_ref(&bad), Some(&good.join("jp.otf")));
        assert!(chosen.chosen_loaded());
        assert!(chosen.find(&needed).is_ok());
        assert!(!Library::in_folders(&[], Some(Path::new("/no/such/font.otf"))).chosen_loaded());
        for dir in [good, bad] {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    /// The system's own search, with what it found (`cargo test -- --ignored --nocapture`).
    #[test]
    #[ignore = "reads the machine's fonts"]
    fn system_search() {
        let started = std::time::Instant::now();
        let lib = Library::system(None);
        let found = lib.find(&"設定日本語".chars().collect::<Vec<_>>());
        println!("{} faces, {:?} in {:?}", lib.len(), found, started.elapsed());
    }
}
