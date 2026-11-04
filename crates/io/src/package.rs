//! A package of named parts in one zip file: text (JSON) parts, deflated, and arrays of numbers,
//! little-endian and stored as they are (EMCraft's project files, docs/project-file.md).
//!
//! The same parts written in the same order give the same bytes: every part's time is fixed at
//! the zip epoch (1980-01-01). A package read is untrusted: its file size, every part's
//! expanded size and the sum of them are capped, an array's bytes must be a whole number of its
//! values and no more than the reader asks for; every problem is an [`Error`], never a panic.
//!
//! ```
//! use emcraft_io::package::{Reader, Writer};
//!
//! let dir = std::env::temp_dir().join(format!("emcraft-package-doc-{}", std::process::id()));
//! std::fs::create_dir_all(&dir).unwrap();
//! let path = dir.join("p.zip");
//! let mut w = Writer::create(&path).unwrap();
//! w.text("project.json", "{\"a\": 1}").unwrap();
//! w.values("mesh/nodes.f64", [0.5f64, -2.0, 1e-3]).unwrap();
//! w.values("mesh/tets.u32", [0u32, 1, 2, 3]).unwrap();
//! w.finish().unwrap();
//!
//! let mut r = Reader::open(&path).unwrap();
//! assert_eq!(r.text("project.json", 1 << 10).unwrap(), "{\"a\": 1}");
//! assert_eq!(r.values::<f64>("mesh/nodes.f64", 3).unwrap(), [0.5, -2.0, 1e-3]);
//! // More values than asked for, a part not there: errors, not panics.
//! assert!(r.values::<u32>("mesh/tets.u32", 3).is_err());
//! assert!(r.values::<f64>("nothing.f64", 1).is_err());
//! # std::fs::remove_dir_all(&dir).unwrap();
//! ```

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;

use crate::{Error, Result};

/// Largest package read (bytes on disk, and the sum of its parts expanded).
pub const MAX_PACKAGE: u64 = 4 << 30;

/// Most parts of a package.
pub const MAX_PARTS: usize = 1 << 16;

/// Longest part name.
const MAX_NAME: usize = 200;

/// Bytes written at a time.
const CHUNK: usize = 1 << 16;

fn invalid(msg: impl std::fmt::Display) -> Error {
    Error::Invalid(format!("package: {msg}"))
}

/// A number kept in an array part: its size and little-endian bytes.
pub trait Value: Copy {
    const SIZE: usize;
    fn put(self, out: &mut Vec<u8>);
    /// The value of its first [`Value::SIZE`] bytes (zero bytes where there are fewer).
    fn get(bytes: &[u8]) -> Self;
}

impl Value for f64 {
    const SIZE: usize = 8;
    fn put(self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.to_le_bytes());
    }
    fn get(bytes: &[u8]) -> f64 {
        let mut b = [0u8; 8];
        if let Some(s) = bytes.get(..8) {
            b.copy_from_slice(s);
        }
        f64::from_le_bytes(b)
    }
}

impl Value for u32 {
    const SIZE: usize = 4;
    fn put(self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.to_le_bytes());
    }
    fn get(bytes: &[u8]) -> u32 {
        let mut b = [0u8; 4];
        if let Some(s) = bytes.get(..4) {
            b.copy_from_slice(s);
        }
        u32::from_le_bytes(b)
    }
}

/// A name a part may have: 1 to 200 characters of `a-z 0-9 _ - . /`, no empty segment.
fn check_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name.len() <= MAX_NAME
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_-./".contains(&b))
        && name.split('/').all(|s| !s.is_empty() && s != "." && s != "..");
    if ok { Ok(()) } else { Err(invalid(format!("`{}` is not a part name", name.chars().take(MAX_NAME).collect::<String>()))) }
}

/// Writes a package to a file, part by part.
pub struct Writer {
    zip: zip::ZipWriter<BufWriter<File>>,
    parts: usize,
}

impl Writer {
    /// A package at `path` (created or emptied).
    pub fn create(path: &Path) -> Result<Writer> {
        let file = File::create(path).map_err(|e| invalid(format!("cannot write `{}`: {e}", path.display())))?;
        Ok(Writer { zip: zip::ZipWriter::new(BufWriter::new(file)), parts: 0 })
    }

    fn start(&mut self, name: &str, method: zip::CompressionMethod) -> Result<()> {
        check_name(name)?;
        if self.parts >= MAX_PARTS {
            return Err(invalid(format!("more than {MAX_PARTS} parts")));
        }
        self.parts += 1;
        let opts = zip::write::SimpleFileOptions::default().compression_method(method).last_modified_time(zip::DateTime::DEFAULT).large_file(true);
        self.zip.start_file(name, opts).map_err(|e| invalid(format!("{name}: {e}")))
    }

    /// A text part (deflated).
    pub fn text(&mut self, name: &str, text: &str) -> Result<()> {
        self.start(name, zip::CompressionMethod::Deflated)?;
        self.zip.write_all(text.as_bytes()).map_err(|e| invalid(format!("{name}: {e}")))
    }

    /// An array part: `values` little-endian, stored as they are.
    pub fn values<T: Value>(&mut self, name: &str, values: impl IntoIterator<Item = T>) -> Result<()> {
        self.start(name, zip::CompressionMethod::Stored)?;
        let mut buf = Vec::with_capacity(CHUNK + T::SIZE);
        for v in values {
            v.put(&mut buf);
            if buf.len() >= CHUNK {
                self.zip.write_all(&buf).map_err(|e| invalid(format!("{name}: {e}")))?;
                buf.clear();
            }
        }
        self.zip.write_all(&buf).map_err(|e| invalid(format!("{name}: {e}")))
    }

    /// A part of raw bytes, stored as they are (a part copied from another package).
    pub fn raw(&mut self, name: &str, bytes: &[u8]) -> Result<()> {
        self.start(name, zip::CompressionMethod::Stored)?;
        self.zip.write_all(bytes).map_err(|e| invalid(format!("{name}: {e}")))
    }

    /// Closes the package (its directory written, the file flushed).
    pub fn finish(self) -> Result<()> {
        let out = self.zip.finish().map_err(|e| invalid(format!("closing it: {e}")))?;
        let file = out.into_inner().map_err(|e| invalid(format!("closing it: {e}")))?;
        file.sync_all().map_err(|e| invalid(format!("closing it: {e}")))
    }
}

/// Reads a package's parts, each at most once worth of the package's budget.
pub struct Reader {
    zip: zip::ZipArchive<BufReader<File>>,
    /// Bytes the parts read may still expand to.
    budget: u64,
}

impl Reader {
    /// The package at `path`: an error when it is not a zip, is larger than [`MAX_PACKAGE`] or
    /// has more than [`MAX_PARTS`] parts.
    pub fn open(path: &Path) -> Result<Reader> {
        let file = File::open(path).map_err(|e| invalid(format!("cannot read `{}`: {e}", path.display())))?;
        let size = file.metadata().map_err(|e| invalid(format!("cannot read `{}`: {e}", path.display())))?.len();
        if size > MAX_PACKAGE {
            return Err(invalid(format!("larger than {} GB", MAX_PACKAGE >> 30)));
        }
        let zip = zip::ZipArchive::new(BufReader::new(file)).map_err(|e| invalid(format!("not a package (zip): {e}")))?;
        if zip.len() > MAX_PARTS {
            return Err(invalid(format!("more than {MAX_PARTS} parts")));
        }
        Ok(Reader { zip, budget: MAX_PACKAGE })
    }

    /// Whether the package has a part `name`.
    pub fn has(&self, name: &str) -> bool {
        self.zip.index_for_name(name).is_some()
    }

    /// The parts' names, in the package's order.
    pub fn names(&self) -> Vec<String> {
        self.zip.file_names().filter_map(|n| n.ok().map(std::borrow::Cow::into_owned)).collect()
    }

    /// Part `name`'s bytes, at most `max` of them.
    pub fn bytes(&mut self, name: &str, max: u64) -> Result<Vec<u8>> {
        check_name(name)?;
        let max = max.min(self.budget);
        let mut part = self.zip.by_name(name).map_err(|e| invalid(format!("{name}: {e}")))?;
        // The declared size only sizes the first allocation (it may lie); the read is capped.
        let mut out = Vec::with_capacity(usize::try_from(part.size().min(max).min(64 << 20)).unwrap_or(0));
        (&mut part).take(max.saturating_add(1)).read_to_end(&mut out).map_err(|e| invalid(format!("{name}: {e}")))?;
        if out.len() as u64 > max {
            return Err(invalid(format!("{name} is larger than {max} bytes")));
        }
        self.budget -= out.len() as u64;
        Ok(out)
    }

    /// Text part `name` (UTF-8), at most `max` bytes.
    pub fn text(&mut self, name: &str, max: u64) -> Result<String> {
        String::from_utf8(self.bytes(name, max)?).map_err(|_| invalid(format!("{name} is not UTF-8 text")))
    }

    /// Array part `name`: at most `max` values, its bytes a whole number of them.
    pub fn values<T: Value>(&mut self, name: &str, max: usize) -> Result<Vec<T>> {
        let bytes = self.bytes(name, (max as u64).saturating_mul(T::SIZE as u64))?;
        if !bytes.len().is_multiple_of(T::SIZE) {
            return Err(invalid(format!("{name}: {} bytes are not a whole number of {}-byte values", bytes.len(), T::SIZE)));
        }
        Ok(bytes.chunks_exact(T::SIZE).map(T::get).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use proptest::test_runner::{Config, RngSeed};

    fn scratch(name: &str) -> std::path::PathBuf {
        let base = std::env::var_os("EMCRAFT_SCRATCH").map_or_else(std::env::temp_dir, std::path::PathBuf::from);
        let dir = base.join(format!("emcraft-package-{name}-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        dir.join("p.zip")
    }

    fn write(path: &Path, values: &[f64]) {
        let mut w = Writer::create(path).unwrap();
        w.text("project.json", "{}").unwrap();
        w.values("a.f64", values.iter().copied()).unwrap();
        w.values("b.u32", [7u32, u32::MAX]).unwrap();
        w.finish().unwrap();
    }

    #[test]
    fn parts_read_back_bit_for_bit_and_the_bytes_are_the_same_twice() {
        let (p, q) = (scratch("twice-a"), scratch("twice-b"));
        let values = [0.0, -0.0, f64::MIN_POSITIVE, 1e308, -1.5, f64::NAN];
        write(&p, &values);
        write(&q, &values);
        assert_eq!(std::fs::read(&p).unwrap(), std::fs::read(&q).unwrap());
        let mut r = Reader::open(&p).unwrap();
        let back: Vec<f64> = r.values("a.f64", 6).unwrap();
        assert!(back.iter().zip(&values).all(|(a, b)| a.to_bits() == b.to_bits()));
        assert_eq!(r.values::<u32>("b.u32", 2).unwrap(), [7, u32::MAX]);
        assert!(r.has("a.f64") && !r.has("c.f64"));
        // Read as values of another size: not a whole number of them.
        let mut r = Reader::open(&p).unwrap();
        assert!(r.values::<u32>("project.json", 100).is_err());
    }

    #[test]
    fn names_outside_the_alphabet_are_refused() {
        let mut w = Writer::create(&scratch("names")).unwrap();
        for bad in ["", "../x", "a//b", "A.f64", "a b", "/a", "a/./b"] {
            assert!(w.text(bad, "").is_err(), "{bad}");
        }
    }

    #[test]
    fn a_file_that_is_no_zip_is_refused() {
        let p = scratch("nozip");
        std::fs::write(&p, b"{\"format\": \"emcraft-project\"}").unwrap();
        assert!(Reader::open(&p).is_err());
    }

    proptest! {
        #![proptest_config(Config { cases: 64, rng_seed: RngSeed::Fixed(0x454d_4372), failure_persistence: None, ..Config::default() })]

        /// A package cut short or with bytes changed opens or reads with an error, never a
        /// panic.
        #[test]
        fn damaged_packages_never_panic(cut in 0usize..400, flips in prop::collection::vec((0usize..400, any::<u8>()), 0..6)) {
            let p = scratch("damaged");
            write(&p, &[1.0, 2.0, 3.0]);
            let mut bytes = std::fs::read(&p).unwrap();
            for (at, b) in flips {
                if let Some(x) = bytes.get_mut(at) {
                    *x ^= b;
                }
            }
            bytes.truncate(cut.max(1).min(bytes.len()));
            std::fs::write(&p, &bytes).unwrap();
            if let Ok(mut r) = Reader::open(&p) {
                let _ = r.text("project.json", 1 << 10);
                let _ = r.values::<f64>("a.f64", 3);
                let _ = r.values::<u32>("b.u32", 2);
            }
        }
    }
}
