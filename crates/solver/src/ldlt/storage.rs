//! Where the factor's columns are kept: in memory up to a budget, beyond it in a scratch file on
//! disk (out-of-core).

use std::borrow::Cow;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use num_complex::Complex64 as C;

use crate::multifrontal::FactorError;

/// Where a supernode's factored columns are kept.
#[derive(Clone, Debug)]
pub(super) enum Columns {
    Memory(Vec<C>),
    /// In the scratch file: `len` complex numbers from byte `offset`.
    Disk {
        offset: u64,
        len: usize,
    },
}

/// The factor's columns beyond the memory budget, written to a file on disk (out-of-core):
/// the file is unlinked as soon as it is made, so it goes when the factorization does.
#[derive(Debug)]
pub(super) struct Scratch {
    file: Mutex<std::fs::File>,
    /// Bytes written.
    pub end: u64,
}

/// The most bytes of factor kept in memory before the rest goes to the scratch file: the
/// environment's `EMCRAFT_FACTOR_MEMORY_GB`, else no cap of its own — the factorization's
/// [`crate::multifrontal::Plan`] keeps in memory what the system's available memory leaves
/// beside its fronts.
pub(crate) fn memory_budget() -> u64 {
    std::env::var("EMCRAFT_FACTOR_MEMORY_GB")
        .ok()
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|g| g.is_finite() && *g >= 0.0)
        .map_or(u64::MAX, |gb| (gb * 1e9) as u64)
}

/// The directories for scratch files, in the order tried: `EMCRAFT_SCRATCH`, then
/// `~/.cache/emcraft` (on disk; the system's temporary directory is often in memory), then the
/// temporary directory. A directory that cannot be used (a drive not mounted, no permission)
/// passes to the next.
fn scratch_dirs() -> Vec<std::path::PathBuf> {
    let mut dirs: Vec<std::path::PathBuf> = std::env::var_os("EMCRAFT_SCRATCH").into_iter().map(Into::into).collect();
    if let Some(h) = std::env::var_os("HOME") {
        dirs.push(std::path::Path::new(&h).join(".cache").join("emcraft"));
    }
    dirs.push(std::env::temp_dir());
    dirs
}

fn poisoned<T>(_: T) -> FactorError {
    FactorError::Scratch("the file's lock was poisoned".into())
}

impl Scratch {
    fn new() -> Result<Scratch, FactorError> {
        Scratch::in_first(&scratch_dirs())
    }

    /// A scratch file in the first of `dirs` that takes one; why each refused when none does.
    fn in_first(dirs: &[std::path::PathBuf]) -> Result<Scratch, FactorError> {
        static COUNT: AtomicU64 = AtomicU64::new(0);
        let mut refused = Vec::new();
        for dir in dirs {
            let n = COUNT.fetch_add(1, Ordering::Relaxed);
            let path = dir.join(format!("factor-{}-{n}.bin", std::process::id()));
            let opened = std::fs::create_dir_all(dir).and_then(|_| std::fs::OpenOptions::new().read(true).write(true).create_new(true).open(&path));
            match opened {
                Ok(file) => {
                    // Unlinked at once where the system allows it (the open handle keeps the data).
                    let _ = std::fs::remove_file(&path);
                    return Ok(Scratch { file: Mutex::new(file), end: 0 });
                }
                Err(e) => refused.push(format!("{}: {e}", dir.display())),
            }
        }
        Err(FactorError::Scratch(format!("no directory takes the factor's scratch file ({})", refused.join("; "))))
    }

    fn write(&mut self, v: &[C]) -> Result<Columns, FactorError> {
        use std::io::{Seek, SeekFrom, Write};
        let mut bytes = Vec::with_capacity(v.len() * 16);
        for z in v {
            bytes.extend_from_slice(&z.re.to_le_bytes());
            bytes.extend_from_slice(&z.im.to_le_bytes());
        }
        let mut f = self.file.lock().map_err(poisoned)?;
        f.seek(SeekFrom::Start(self.end)).and_then(|_| f.write_all(&bytes)).map_err(|e| FactorError::Scratch(e.to_string()))?;
        let offset = self.end;
        self.end += bytes.len() as u64;
        Ok(Columns::Disk { offset, len: v.len() })
    }

    fn read(&self, offset: u64, len: usize) -> Result<Vec<C>, FactorError> {
        use std::io::{Read, Seek, SeekFrom};
        let mut bytes = vec![0u8; len * 16];
        let mut f = self.file.lock().map_err(poisoned)?;
        f.seek(SeekFrom::Start(offset)).and_then(|_| f.read_exact(&mut bytes)).map_err(|e| FactorError::Scratch(e.to_string()))?;
        Ok(bytes
            .as_chunks::<16>()
            .0
            .iter()
            .map(|c| {
                let mut re = [0u8; 8];
                let mut im = [0u8; 8];
                re.copy_from_slice(&c[..8]);
                im.copy_from_slice(&c[8..]);
                C::new(f64::from_le_bytes(re), f64::from_le_bytes(im))
            })
            .collect())
    }
}

impl Columns {
    /// The columns, from memory or read back from `scratch`.
    pub fn get<'a>(&'a self, scratch: Option<&Scratch>) -> Result<Cow<'a, [C]>, FactorError> {
        match self {
            Columns::Memory(v) => Ok(Cow::Borrowed(v)),
            Columns::Disk { offset, len } => {
                let sc = scratch.ok_or_else(|| FactorError::Scratch("columns on disk without a file".into()))?;
                Ok(Cow::Owned(sc.read(*offset, *len)?))
            }
        }
    }
}

/// Keeps factored columns, shared by the threads of a factorization: in memory while they fit
/// the budget, then in the scratch file (made when first needed).
pub(super) struct Store {
    budget: u64,
    in_memory: AtomicU64,
    scratch: Mutex<Option<Scratch>>,
}

impl Store {
    pub fn new(budget: u64) -> Store {
        Store { budget, in_memory: AtomicU64::new(0), scratch: Mutex::new(None) }
    }

    /// Keeps `l`.
    pub fn keep(&self, l: Vec<C>) -> Result<Columns, FactorError> {
        let bytes = (l.len() * 16) as u64;
        let fits =
            self.in_memory.try_update(Ordering::SeqCst, Ordering::SeqCst, |used| (used + bytes <= self.budget).then_some(used + bytes)).is_ok();
        if fits {
            return Ok(Columns::Memory(l));
        }
        let mut guard = self.scratch.lock().map_err(|_| FactorError::Dimension("the scratch file's lock was poisoned".into()))?;
        if guard.is_none() {
            *guard = Some(Scratch::new()?);
        }
        match guard.as_mut() {
            Some(sc) => sc.write(&l),
            None => Ok(Columns::Memory(l)),
        }
    }

    /// The scratch file, when any columns went to it.
    pub fn into_scratch(self) -> Result<Option<Scratch>, FactorError> {
        self.scratch.into_inner().map_err(|_| FactorError::Dimension("the scratch file's lock was poisoned".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch directory that cannot be made (a drive not mounted) passes to the next; with
    /// none usable the error names each.
    #[test]
    fn an_unusable_scratch_directory_passes_to_the_next() {
        let base = std::env::temp_dir().join(format!("emcraft-scratch-test-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        // A plain file where a directory should be: no directory can be made under it.
        let blocker = base.join("not-a-directory");
        std::fs::write(&blocker, b"x").unwrap();
        let bad = blocker.join("scratch");
        let mut s = Scratch::in_first(&[bad.clone(), base.join("good")]).unwrap();
        let v = [C::new(1.5, -2.0), C::new(0.0, 3.25)];
        let Columns::Disk { offset, len } = s.write(&v).unwrap() else { panic!("not on disk") };
        assert_eq!(s.read(offset, len).unwrap(), v);
        let e = Scratch::in_first(std::slice::from_ref(&bad)).err().unwrap().to_string();
        assert!(e.contains(&bad.display().to_string()), "{e}");
        let _ = std::fs::remove_dir_all(&base);
    }
}
