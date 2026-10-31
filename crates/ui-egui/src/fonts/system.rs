//! An installed Japanese font for the window: the folders where the system keeps fonts walked
//! once, each font file's faces read for their family names, weight and slant (a few small
//! reads per file: the table directory, `name` and `OS/2`, never the outlines), and the best face
//! taken — a family this system ships for Japanese text first ([`PREFERRED`], regular weight
//! upright first), else any face whose character map covers [`SAMPLE`]. Collections (`.ttc`)
//! give each face's index.
//!
//! The work is bounded: [`MAX_DEPTH`] folders deep, [`MAX_ENTRIES`] folder entries,
//! [`MAX_FILES`] font files and [`MAX_FACES`] faces per collection looked at; tables and the
//! chosen file read only up to their caps. A file that cannot be read or parsed is passed over.
//! The order is fixed (folders as listed, their entries sorted), so a machine always gets the
//! same face.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Japanese text a face must show to be used.
pub const SAMPLE: &str = "設定日本語";

/// Folder levels walked below each font folder.
pub const MAX_DEPTH: usize = 8;
/// Folder entries looked at, in all.
pub const MAX_ENTRIES: usize = 50_000;
/// Font files opened, in all.
pub const MAX_FILES: usize = 5_000;
/// Faces read from one collection.
pub const MAX_FACES: u32 = 64;
/// Tables of a face's directory read.
const MAX_TABLES: u16 = 512;
/// The largest `name` table read.
const MAX_NAME: u32 = 1 << 20;
/// The largest `cmap` table read.
const MAX_CMAP: u32 = 16 << 20;
/// The largest font file used (CJK collections run to about 30 MB).
pub const MAX_FONT: u64 = 64 << 20;
/// Faces whose character map is read in the search for any face covering [`SAMPLE`].
const MAX_COVERAGE: usize = 200;
/// Faces of preferred families tried in turn when one fails to load.
const MAX_TRIES: usize = 8;

/// The families each system ships for Japanese text, best first.
#[cfg(target_os = "windows")]
pub const PREFERRED: &[&str] = &["Yu Gothic UI", "Yu Gothic", "Meiryo UI", "Meiryo", "MS UI Gothic", "MS Gothic"];
#[cfg(target_os = "macos")]
pub const PREFERRED: &[&str] = &["Hiragino Sans", "Hiragino Kaku Gothic ProN", "Hiragino Kaku Gothic Pro"];
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub const PREFERRED: &[&str] = &["Noto Sans CJK JP", "Noto Sans JP", "IPAexGothic", "IPAGothic", "TakaoGothic", "VL Gothic"];

/// A font found and read.
#[derive(Clone, PartialEq)]
pub struct Found {
    /// The family the face was chosen by (as its `name` table spells it).
    pub family: String,
    pub path: PathBuf,
    /// The face's index in its file (0 unless a collection).
    pub index: u32,
    /// The whole file.
    pub bytes: Vec<u8>,
}

impl std::fmt::Debug for Found {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Found({:?}, {}, face {}, {} bytes)", self.family, self.path.display(), self.index, self.bytes.len())
    }
}

/// The best Japanese face in the system's font folders.
pub fn find() -> Option<Found> {
    find_in(&folders(), PREFERRED)
}

/// The best Japanese face in `folders`: a face of `families` (earlier first; then regular
/// weight, then upright), else any face covering [`SAMPLE`].
pub fn find_in(folders: &[PathBuf], families: &[&str]) -> Option<Found> {
    let faces: Vec<Face> = font_files(folders).iter().flat_map(|p| read_faces(p).unwrap_or_default()).collect();
    let wanted: Vec<String> = families.iter().map(|f| f.to_lowercase()).collect();
    // Each face of a wanted family with the family's place in `families`.
    let mut ranked: Vec<(usize, &Face)> =
        faces.iter().filter_map(|f| Some((wanted.iter().position(|w| f.families.iter().any(|n| n.to_lowercase() == *w))?, f))).collect();
    ranked.sort_by_key(|(rank, f)| (*rank, f.plainness()));
    let preferred = ranked.iter().take(MAX_TRIES).find_map(|(rank, face)| families.get(*rank).and_then(|family| load(face, family)));
    if preferred.is_some() {
        return preferred;
    }
    let mut rest: Vec<&Face> = faces.iter().filter(|f| f.cmap.is_some()).collect();
    rest.sort_by_key(|f| f.plainness());
    rest.into_iter().take(MAX_COVERAGE).filter(|f| covers_sample(f)).find_map(|f| load(f, f.families.first().map_or("", String::as_str)))
}

/// The folders this system keeps fonts in, in the order they are searched.
pub fn folders() -> Vec<PathBuf> {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    let mut out: Vec<PathBuf> = Vec::new();
    if cfg!(target_os = "windows") {
        out.extend(env("WINDIR").or_else(|| env("SystemRoot")).map(|w| w.join("Fonts")));
        out.extend(env("LOCALAPPDATA").map(|l| l.join("Microsoft").join("Windows").join("Fonts")));
    } else if cfg!(target_os = "macos") {
        out.push("/System/Library/Fonts".into());
        out.push("/Library/Fonts".into());
        out.extend(env("HOME").map(|h| h.join("Library").join("Fonts")));
        out.push("/System/Library/AssetsV2".into());
    } else {
        let home = env("HOME");
        out.extend(env("XDG_DATA_HOME").or_else(|| home.as_ref().map(|h| h.join(".local").join("share"))).map(|d| d.join("fonts")));
        out.extend(home.map(|h| h.join(".fonts")));
        let data = std::env::var("XDG_DATA_DIRS").ok().filter(|d| !d.is_empty()).unwrap_or_else(|| "/usr/local/share:/usr/share".into());
        out.extend(data.split(':').filter(|d| !d.is_empty()).map(|d| Path::new(d).join("fonts")));
        out.push("/usr/share/fonts".into());
        out.push("/usr/local/share/fonts".into());
        out.push("/run/current-system/sw/share/X11/fonts".into());
    }
    // Each folder once, where it first comes.
    let mut unique: Vec<PathBuf> = Vec::with_capacity(out.len());
    for p in out {
        if !unique.contains(&p) {
            unique.push(p);
        }
    }
    unique
}

/// What a face says of itself.
#[derive(Clone, Debug)]
struct Face {
    path: PathBuf,
    index: u32,
    /// Its family names (`name` ids 1 and 16, every language).
    families: Vec<String>,
    /// `OS/2` usWeightClass (400 when missing).
    weight: u16,
    italic: bool,
    /// Where its `cmap` lies in the file.
    cmap: Option<Span>,
}

impl Face {
    /// How far it is from a plain face, least first: its weight's distance from regular, then
    /// slanted after upright.
    fn plainness(&self) -> (u16, bool) {
        (self.weight.abs_diff(400), self.italic)
    }
}

/// Where a table lies in a font file: its offset and length (bytes).
#[derive(Clone, Copy, Debug)]
struct Span {
    at: u32,
    len: u32,
}

impl Span {
    /// Its bytes in `file`, at most `most` of them.
    fn read(self, file: &mut File, most: u32) -> Option<Vec<u8>> {
        read_at(file, u64::from(self.at), self.len.min(most) as usize)
    }
}

/// The font files under `folders` (`.ttf`, `.otf`, `.ttc`, `.otc`), within the caps.
fn font_files(folders: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut entries = 0;
    for root in folders {
        let mut stack = vec![(root.clone(), 0usize)];
        while let Some((dir, depth)) = stack.pop() {
            let Ok(read) = std::fs::read_dir(&dir) else { continue };
            let mut names: Vec<PathBuf> = Vec::new();
            for e in read.flatten() {
                entries += 1;
                if entries > MAX_ENTRIES {
                    return out;
                }
                names.push(e.path());
            }
            names.sort();
            let mut subdirs = Vec::new();
            for p in names {
                if p.is_dir() {
                    if depth < MAX_DEPTH {
                        subdirs.push((p, depth + 1));
                    }
                } else if is_font_file(&p) {
                    if out.len() >= MAX_FILES {
                        return out;
                    }
                    out.push(p);
                }
            }
            // Popped in name order.
            stack.extend(subdirs.into_iter().rev());
        }
    }
    out
}

fn is_font_file(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| ["ttf", "otf", "ttc", "otc"].iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// `len` bytes of `file` from `offset`.
fn read_at(file: &mut File, offset: u64, len: usize) -> Option<Vec<u8>> {
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut buf = vec![0; len];
    file.read_exact(&mut buf).ok()?;
    Some(buf)
}

fn be16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*b.get(at)?, *b.get(at + 1)?]))
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes([*b.get(at)?, *b.get(at + 1)?, *b.get(at + 2)?, *b.get(at + 3)?]))
}

/// The faces of the font file at `path`.
fn read_faces(path: &Path) -> Option<Vec<Face>> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let head = read_at(&mut file, 0, 12)?;
    let offsets: Vec<u32> = if head.get(..4) == Some(b"ttcf") {
        let n = be32(&head, 8)?.min(MAX_FACES);
        let table = read_at(&mut file, 12, n as usize * 4)?;
        (0..n as usize).filter_map(|i| be32(&table, i * 4)).collect()
    } else {
        vec![0]
    };
    let faces = offsets.iter().enumerate().filter_map(|(i, off)| read_face(&mut file, len, path, i as u32, *off)).collect();
    Some(faces)
}

/// The face whose table directory starts at `offset`.
fn read_face(file: &mut File, len: u64, path: &Path, index: u32, offset: u32) -> Option<Face> {
    let head = read_at(file, u64::from(offset), 12)?;
    let tables = be16(&head, 4)?.min(MAX_TABLES);
    let records = read_at(file, u64::from(offset) + 12, usize::from(tables) * 16)?;
    // A table of at most `most` bytes, lying inside the file.
    let find = |tag: &[u8; 4], most: u32| -> Option<Span> {
        records.chunks_exact(16).find_map(|r| {
            let (at, size) = (be32(r, 8)?, be32(r, 12)?);
            (r.get(..4)? == tag && size <= most && u64::from(at) + u64::from(size) <= len).then_some(Span { at, len: size })
        })
    };
    let name = find(b"name", MAX_NAME)?.read(file, MAX_NAME)?;
    let os2 = find(b"OS/2", 1 << 16);
    let cmap = find(b"cmap", MAX_CMAP);
    let mut families: Vec<String> = Vec::new();
    for n in ttf_parser::name::Table::parse(&name)?.names {
        let family = n.name_id == ttf_parser::name_id::FAMILY || n.name_id == ttf_parser::name_id::TYPOGRAPHIC_FAMILY;
        if let Some(s) = n.to_string().filter(|_| family).map(|s| s.trim().to_owned()).filter(|s| !s.is_empty() && !families.contains(s)) {
            families.push(s);
        }
    }
    let os2 = os2.and_then(|s| s.read(file, 64));
    let weight = os2.as_deref().and_then(|b| be16(b, 4)).filter(|w| (1..=1000).contains(w)).unwrap_or(400);
    let italic = os2.as_deref().and_then(|b| be16(b, 62)).is_some_and(|s| s & 1 == 1);
    Some(Face { path: path.to_owned(), index, families, weight, italic, cmap })
}

/// Whether the face's character map holds every character of [`SAMPLE`].
fn covers_sample(face: &Face) -> bool {
    let Some(span) = face.cmap else { return false };
    let Some(bytes) = File::open(&face.path).ok().and_then(|mut f| span.read(&mut f, MAX_CMAP)) else { return false };
    let Some(cmap) = ttf_parser::cmap::Table::parse(&bytes) else { return false };
    SAMPLE.chars().all(|c| cmap.subtables.into_iter().any(|t| t.is_unicode() && t.glyph_index(u32::from(c)).is_some_and(|g| g.0 != 0)))
}

/// The face's whole file, when it parses and shows [`SAMPLE`].
fn load(face: &Face, family: &str) -> Option<Found> {
    let file = File::open(&face.path).ok()?;
    if file.metadata().ok()?.len() > MAX_FONT {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_FONT).read_to_end(&mut bytes).ok()?;
    let parsed = ttf_parser::Face::parse(&bytes, face.index).ok()?;
    if !SAMPLE.chars().all(|c| parsed.glyph_index(c).is_some_and(|g| g.0 != 0)) {
        return None;
    }
    Some(Found { family: family.to_owned(), path: face.path.clone(), index: face.index, bytes })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder of its own under the scratch directory.
    fn scratch(name: &str) -> PathBuf {
        let base = std::env::var_os("EMCRAFT_SCRATCH").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let dir = base.join(format!("emcraft-fonts-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn finds_a_preferred_family_and_passes_over_broken_files() {
        let dir = scratch("preferred");
        let sub = dir.join("a").join("b");
        std::fs::create_dir_all(&sub).unwrap();
        // Hostile files first in the walk: empty, truncated, a collection claiming 2^32 faces.
        std::fs::write(dir.join("0-empty.ttf"), b"").unwrap();
        std::fs::write(dir.join("1-short.otf"), &super::super::NOTO_SANS_JP_OTF[..100]).unwrap();
        std::fs::write(dir.join("2-huge.ttc"), [b"ttcf".as_slice(), &[0, 1, 0, 0, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 12]].concat()).unwrap();
        std::fs::write(dir.join("3-not-a-font.txt"), b"hello").unwrap();
        std::fs::write(sub.join("NotoSansJP.otf"), super::super::NOTO_SANS_JP_OTF).unwrap();
        let found = find_in(std::slice::from_ref(&dir), &["No Such Family", "Noto Sans JP"]).unwrap();
        assert_eq!((found.family.as_str(), found.index), ("Noto Sans JP", 0));
        assert_eq!(found.path, sub.join("NotoSansJP.otf"));
        assert_eq!(found.bytes, super::super::NOTO_SANS_JP_OTF);
        // Not a preferred family: found by its character map.
        let any = find_in(std::slice::from_ref(&dir), &["Yu Gothic UI"]).unwrap();
        assert_eq!(any.family, "Noto Sans JP");
        // Nothing to find.
        assert_eq!(find_in(&[dir.join("0-empty.ttf"), dir.join("missing")], &["Noto Sans JP"]), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_collection_gives_its_face_index() {
        let dir = scratch("collection");
        // A collection of two faces: something else, then the subset.
        let font = super::super::NOTO_SANS_JP_OTF;
        let header_len = 12 + 2 * 4;
        let mut ttc = b"ttcf".to_vec();
        ttc.extend([0, 1, 0, 0]);
        ttc.extend(2u32.to_be_bytes());
        ttc.extend(0xffff_fff0u32.to_be_bytes());
        ttc.extend((header_len as u32).to_be_bytes());
        // The face's tables are at offsets from the start of the file: shift them.
        let tables = usize::from(be16(font, 4).unwrap());
        let mut face = font.to_vec();
        for i in 0..tables {
            let at = 12 + i * 16 + 8;
            let off = be32(font, at).unwrap() + header_len as u32;
            face[at..at + 4].copy_from_slice(&off.to_be_bytes());
        }
        ttc.extend(face);
        std::fs::write(dir.join("pair.ttc"), &ttc).unwrap();
        let found = find_in(std::slice::from_ref(&dir), &["Noto Sans JP"]).unwrap();
        assert_eq!((found.index, found.bytes.len()), (1, ttc.len()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// How long the search takes on this machine (`cargo test -p emcraft-ui-egui -- --ignored
    /// --nocapture system_search`).
    #[test]
    #[ignore = "depends on the machine's fonts"]
    fn system_search() {
        let t = std::time::Instant::now();
        let files = font_files(&folders());
        let walked = t.elapsed();
        let found = find();
        eprintln!("{} font files walked in {walked:.2?}; found {found:?} in {:.2?}", files.len(), t.elapsed());
    }
}
