//! `cargo xtask font-subset`: the fonts the apps embed, cut down to the characters they need,
//! from the full fonts kept beside them (`fonts/full/`).
//!
//! - Noto Sans JP (the window's Japanese fallback, `crates/ui-egui/fonts/`): ASCII, Latin-1,
//!   general punctuation, kana (with the half-width forms), CJK symbols and full-width forms,
//!   JIS X 0208's non-kanji rows 1–8 and first-level kanji (rows 16–47, 2965 kanji), read from
//!   the WHATWG EUC-JP table through `encoding_rs`, and every character of `locales/ja.json`.
//! - DejaVu Sans (the text in pictures, `crates/render/fonts/`): Latin (Basic to Extended-B),
//!   Greek, Cyrillic, punctuation, scripts, currency, letterlike symbols, number forms, arrows,
//!   mathematical operators, technical symbols, geometric shapes, and every character the
//!   crates below the UI write in their sources. Its `kern` table (which pictures kern by) is
//!   rebuilt for the subset's glyph ids.
//!
//! Glyphs are cut by `allsorts` (Apache-2.0), which keeps each kept glyph's outline and metrics
//! as they are; this command then checks that against the source, glyph by glyph and (for
//! DejaVu) kerning pair by kerning pair, before writing.

use std::path::Path;

use allsorts::binary::read::ReadScope;
use allsorts::font_data::FontData;
use allsorts::subset::{CmapTarget, SubsetProfile, subset};

/// What a subset does with the source's kerning.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kerning {
    /// Left out of the subset.
    Dropped,
    /// Its `kern` table rebuilt for the subset's glyph ids (pictures kern by it).
    Rebuilt,
}

pub fn run(root: &Path) -> Result<(), String> {
    let ui = root.join("crates/ui-egui/fonts");
    let mut jp = ranges(&[(0x20, 0x7E), (0xA0, 0xFF), (0x2000, 0x206F), (0x3000, 0x30FF), (0x31F0, 0x31FF), (0xFF00, 0xFFEF)]);
    jp.extend(jis_x_0208());
    let ja = std::fs::read_to_string(root.join("crates/ui-egui/locales/ja.json")).map_err(|e| format!("ja.json: {e}"))?;
    jp.extend(ja.chars().filter(|c| !c.is_control()));
    write(&ui.join("full/NotoSansJP-Regular.otf"), &ui.join("NotoSansJP-Regular-subset.otf"), jp, Kerning::Dropped)?;

    let render = root.join("crates/render/fonts");
    let mut dv = ranges(&[
        (0x20, 0x7E),
        (0xA0, 0x24F),
        (0x370, 0x3FF),
        (0x400, 0x4FF),
        (0x2000, 0x206F),
        (0x2070, 0x209F),
        (0x20A0, 0x20CF),
        (0x2100, 0x214F),
        (0x2150, 0x218F),
        (0x2190, 0x21FF),
        (0x2200, 0x22FF),
        (0x2300, 0x23FF),
        (0x25A0, 0x25FF),
    ]);
    dv.extend(source_chars(root)?);
    write(&render.join("full/DejaVuSans.ttf"), &render.join("DejaVuSans-subset.ttf"), dv, Kerning::Rebuilt)
}

fn ranges(r: &[(u32, u32)]) -> Vec<char> {
    r.iter().flat_map(|&(a, b)| (a..=b).filter_map(char::from_u32)).collect()
}

/// JIS X 0208's rows 1–8 (symbols, digits, kana, Greek, Cyrillic, box drawing) and 16–47
/// (first-level kanji), decoded through EUC-JP (row r, cell c: bytes 0xA0 + r, 0xA0 + c).
fn jis_x_0208() -> Vec<char> {
    let mut out = Vec::new();
    for row in (1u8..=8).chain(16..=47) {
        for cell in 1u8..=94 {
            let bytes = [0xA0 + row, 0xA0 + cell];
            let (s, _, unmapped) = encoding_rs::EUC_JP.decode(&bytes);
            if !unmapped {
                out.extend(s.chars());
            }
        }
    }
    out
}

/// Every character other than ASCII in the sources of the crates below the UI (what pictures
/// can write, and more).
fn source_chars(root: &Path) -> Result<Vec<char>, String> {
    let mut out = Vec::new();
    let crates = std::fs::read_dir(root.join("crates")).map_err(|e| format!("crates: {e}"))?;
    let mut dirs: Vec<_> = crates.flatten().map(|e| e.path()).filter(|p| !p.ends_with("ui-egui")).collect();
    dirs.sort();
    for d in dirs {
        let mut stack = vec![d.join("src")];
        while let Some(dir) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            for p in rd.flatten().map(|e| e.path()) {
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|e| e == "rs") {
                    let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
                    out.extend(text.chars().filter(|c| !c.is_ascii() && !c.is_control()));
                }
            }
        }
    }
    Ok(out)
}

/// Subsets `source` to `chars` (those it has) and writes `out`, after checking the subset.
fn write(source: &Path, out: &Path, mut chars: Vec<char>, kern: Kerning) -> Result<(), String> {
    let src = std::fs::read(source).map_err(|e| format!("{}: {e}", source.display()))?;
    chars.sort();
    chars.dedup();
    let bytes = cut(&src, &chars, kern).map_err(|e| format!("{}: {e}", source.display()))?;
    std::fs::write(out, &bytes).map_err(|e| format!("{}: {e}", out.display()))?;
    println!("{} ({} bytes) -> {} ({} bytes)", source.display(), src.len(), out.display(), bytes.len());
    Ok(())
}

/// The subset of `src` holding `chars` (those it has), checked against it.
fn cut(src: &[u8], chars: &[char], kern: Kerning) -> Result<Vec<u8>, String> {
    let face = ttf_parser::Face::parse(src, 0).map_err(|e| format!("not a font: {e}"))?;
    let kept: Vec<char> = chars.iter().copied().filter(|c| face.glyph_index(*c).is_some_and(|g| g.0 != 0)).collect();
    let mut glyphs: Vec<u16> = std::iter::once(0).chain(kept.iter().filter_map(|c| face.glyph_index(*c)).map(|g| g.0)).collect();
    glyphs.sort();
    glyphs.dedup();
    let data = ReadScope::new(src).read::<FontData>().map_err(|e| format!("allsorts: {e}"))?;
    let provider = data.table_provider(0).map_err(|e| format!("allsorts: {e}"))?;
    let mut bytes = subset(&provider, &glyphs, &SubsetProfile::Minimal, CmapTarget::Unicode).map_err(|e| format!("subset: {e}"))?;
    if kern == Kerning::Rebuilt {
        let cut = ttf_parser::Face::parse(&bytes, 0).map_err(|e| format!("the subset does not parse: {e}"))?;
        let table = kern_table(&face, &cut, &kept)?;
        bytes = with_table(&bytes, *b"kern", &table)?;
    }
    check(&face, &bytes, &kept, kern)?;
    Ok(bytes)
}

/// The source's horizontal format-0 kerning between `kept` characters, in the subset's glyph
/// ids, as a `kern` table (version 0; subtables of at most 10 000 pairs, split by first glyph).
fn kern_table(src: &ttf_parser::Face, cut: &ttf_parser::Face, kept: &[char]) -> Result<Vec<u8>, String> {
    let Some(table) = src.tables().kern else { return Err("the source has no kern table".into()) };
    let mut map = std::collections::BTreeMap::new();
    for c in kept {
        if let (Some(a), Some(b)) = (src.glyph_index(*c), cut.glyph_index(*c)) {
            map.insert(a.0, b.0);
        }
    }
    let mut found = std::collections::BTreeMap::new();
    for st in table.subtables {
        let ttf_parser::kern::Format::Format0(f0) = st.format else { return Err("a kern subtable not of format 0".into()) };
        if !st.horizontal || st.variable || st.has_cross_stream {
            return Err("a kern subtable not plain horizontal".into());
        }
        for p in f0.pairs {
            if let (Some(l), Some(r)) = (map.get(&p.left().0), map.get(&p.right().0)) {
                // The first subtable's value wins, as readers look them up.
                found.entry((*l, *r)).or_insert(p.value);
            }
        }
    }
    let pairs: Vec<KernPair> = found.into_iter().map(|((left, right), value)| KernPair { left, right, value }).collect();
    let chunks = subtables(&pairs);
    let mut out = Vec::new();
    let put = |out: &mut Vec<u8>, v: u16| out.extend(v.to_be_bytes());
    put(&mut out, 0);
    put(&mut out, u16::try_from(chunks.len()).map_err(|_| "too many kern subtables")?);
    for chunk in chunks {
        let n = u16::try_from(chunk.len()).map_err(|_| "kern subtable too long")?;
        let len = u16::try_from(14 + 6 * chunk.len()).map_err(|_| "kern subtable too long")?;
        let power = 1u16 << (15 - n.leading_zeros()).min(15);
        let search = power * 6;
        put(&mut out, 0);
        put(&mut out, len);
        put(&mut out, 1); // horizontal, format 0
        put(&mut out, n);
        put(&mut out, search);
        put(&mut out, 15 - power.leading_zeros() as u16);
        put(&mut out, n * 6 - search);
        for p in chunk {
            put(&mut out, p.left);
            put(&mut out, p.right);
            out.extend(p.value.to_be_bytes());
        }
    }
    Ok(out)
}

/// Kerning between two glyphs (the subset's ids, font units).
#[derive(Clone, Copy)]
struct KernPair {
    left: u16,
    right: u16,
    value: i16,
}

/// Most pairs a `kern` subtable holds here.
const MAX_SUBTABLE: usize = 10_000;

/// `pairs` (sorted) cut into subtables of at most [`MAX_SUBTABLE`], split where the first glyph
/// changes so each glyph's pairs stay in one subtable (unless one glyph has more than fit).
fn subtables(pairs: &[KernPair]) -> Vec<&[KernPair]> {
    let mut chunks = Vec::new();
    let mut rest = pairs;
    while !rest.is_empty() {
        let most = rest.len().min(MAX_SUBTABLE);
        let at = (1..=most).rev().find(|&at| at == rest.len() || rest[at - 1].left != rest[at].left).unwrap_or(most);
        let (a, b) = rest.split_at(at);
        chunks.push(a);
        rest = b;
    }
    chunks
}

fn be16(b: &[u8], at: usize) -> Result<u16, String> {
    b.get(at..at + 2).map(|s| u16::from_be_bytes([s[0], s[1]])).ok_or_else(|| "truncated font".to_owned())
}

fn be32(b: &[u8], at: usize) -> Result<u32, String> {
    b.get(at..at + 4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]])).ok_or_else(|| "truncated font".to_owned())
}

fn checksum(data: &[u8]) -> u32 {
    data.chunks(4).fold(0u32, |sum, c| {
        let mut w = [0u8; 4];
        w[..c.len()].copy_from_slice(c);
        sum.wrapping_add(u32::from_be_bytes(w))
    })
}

/// `font` with the table `tag` added (or replaced): the directory rebuilt in tag order, the
/// tables 4-byte aligned, checksums and `head`'s checksum adjustment recomputed.
fn with_table(font: &[u8], tag: [u8; 4], data: &[u8]) -> Result<Vec<u8>, String> {
    let n = usize::from(be16(font, 4)?);
    let mut tables: Vec<([u8; 4], Vec<u8>)> = Vec::new();
    for i in 0..n {
        let r = 12 + i * 16;
        let t: [u8; 4] = font.get(r..r + 4).and_then(|s| s.try_into().ok()).ok_or("truncated font")?;
        let (at, len) = (be32(font, r + 8)? as usize, be32(font, r + 12)? as usize);
        let body = font.get(at..at + len).ok_or("a table outside the font")?;
        if t != tag {
            tables.push((t, body.to_vec()));
        }
    }
    tables.push((tag, data.to_vec()));
    tables.sort_by_key(|t| t.0);
    let n = tables.len();
    let power = 1usize << (usize::BITS - 1 - n.leading_zeros());
    let mut out = font.get(..4).ok_or("truncated font")?.to_vec();
    out.extend(u16::try_from(n).map_err(|_| "too many tables")?.to_be_bytes());
    out.extend(((power * 16) as u16).to_be_bytes());
    out.extend((power.trailing_zeros() as u16).to_be_bytes());
    out.extend(((n * 16 - power * 16) as u16).to_be_bytes());
    let mut at = 12 + 16 * n;
    let mut head_at = None;
    for (t, body) in &mut tables {
        if t == b"head" {
            // Zeroed while the checksums are taken.
            body.get_mut(8..12).ok_or("head too short")?.fill(0);
            head_at = Some(at);
        }
        out.extend(*t);
        out.extend(checksum(body).to_be_bytes());
        out.extend((at as u32).to_be_bytes());
        out.extend((body.len() as u32).to_be_bytes());
        at += body.len().div_ceil(4) * 4;
    }
    for (_, body) in &tables {
        out.extend(body);
        out.resize(out.len().div_ceil(4) * 4, 0);
    }
    let head_at = head_at.ok_or("no head table")?;
    let adjust = 0xB1B0_AFBAu32.wrapping_sub(checksum(&out));
    out[head_at + 8..head_at + 12].copy_from_slice(&adjust.to_be_bytes());
    Ok(out)
}

/// Records a glyph's outline.
#[derive(Default)]
struct Outline(String);

impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0 += &format!("M{x} {y}");
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0 += &format!("L{x} {y}");
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.0 += &format!("Q{x1} {y1} {x} {y}");
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.0 += &format!("C{x1} {y1} {x2} {y2} {x} {y}");
    }
    fn close(&mut self) {
        self.0 += "Z";
    }
}

fn outline(face: &ttf_parser::Face, g: ttf_parser::GlyphId) -> String {
    let mut p = Outline::default();
    let bounds = face.outline_glyph(g, &mut p);
    format!("{bounds:?}{}", p.0)
}

/// The subset maps every kept character to a glyph of the same outline, advance and side
/// bearing as the source's, with the same vertical metrics and (with `kern`) the same kerning
/// between every two kept characters.
fn check(src: &ttf_parser::Face, bytes: &[u8], kept: &[char], kern: Kerning) -> Result<(), String> {
    let cut = ttf_parser::Face::parse(bytes, 0).map_err(|e| format!("the subset does not parse: {e}"))?;
    let metrics =
        |f: &ttf_parser::Face| (f.units_per_em(), f.ascender(), f.descender(), f.line_gap(), f.typographic_ascender(), f.typographic_descender());
    if metrics(src) != metrics(&cut) {
        return Err("the subset's metrics differ".into());
    }
    let mut ids = Vec::new();
    for c in kept {
        let (Some(a), Some(b)) = (src.glyph_index(*c), cut.glyph_index(*c)) else { return Err(format!("{c:?} missing from the subset")) };
        if outline(src, a) != outline(&cut, b)
            || src.glyph_hor_advance(a) != cut.glyph_hor_advance(b)
            || src.glyph_hor_side_bearing(a) != cut.glyph_hor_side_bearing(b)
        {
            return Err(format!("{c:?} differs in the subset"));
        }
        ids.push((a, b));
    }
    if kern == Kerning::Rebuilt {
        let pairs = |f: &ttf_parser::Face, l, r| {
            f.tables().kern.and_then(|k| k.subtables.into_iter().filter(|s| s.horizontal).find_map(|s| s.glyphs_kerning(l, r)))
        };
        for (la, lb) in &ids {
            for (ra, rb) in &ids {
                if pairs(src, *la, *ra) != pairs(&cut, *lb, *rb) {
                    return Err(format!("kerning of glyphs {la:?} {ra:?} differs in the subset"));
                }
            }
        }
    }
    Ok(())
}
