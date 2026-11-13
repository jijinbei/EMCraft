//! The solver's large buffers, allocated so that running out of memory is an error the session
//! reports, never an abort. `vec![x; n]` and `Vec::with_capacity(n)` abort the process when the
//! allocation fails (`handle_alloc_error`), which no `catch_unwind` can stop; here the size is
//! checked for overflow and the memory asked for with `try_reserve_exact`, and a refusal comes
//! back as [`OutOfMemory`], saying how much and for what.
//!
//! A buffer of [`LARGE`] bytes or more is first weighed against the memory the system has
//! available (Linux's `MemAvailable`): one larger than all of it is refused before it is asked
//! for, since Linux promises memory it may not have and kills the process when it is touched.
//!
//! Every buffer whose size grows with the problem (a front, an update, the factor's columns, a
//! block's product, the assembled matrix) is made here; small ones of a bounded size (a port's
//! matrix, an element's) need not be. `cargo xtask style` refuses `vec![…; a * b]` in the
//! factorization and the assembly outside tests, so a product-sized buffer cannot slip back
//! there.
//!
//! ```
//! use emcraft_solver::memory::{self, OutOfMemory};
//! let v: Vec<f64> = memory::zeros(4, "a test")?;
//! assert_eq!(v, [0.0; 4]);
//! let copy = memory::copied(&v[1..], "a test")?;
//! assert_eq!(copy.len(), 3);
//! // A size beyond what any machine has is refused, not aborted.
//! let huge = memory::zeros::<f64>(usize::MAX / 4, "a front");
//! assert!(matches!(huge, Err(OutOfMemory { what: "a front", .. })));
//! # Ok::<(), OutOfMemory>(())
//! ```

/// Memory the system would not give: what it was for and how many bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("not enough memory for {what}: {:.2} GB could not be allocated (coarsen the mesh or lower the order)", *bytes as f64 / 1e9)]
pub struct OutOfMemory {
    pub what: &'static str,
    pub bytes: u64,
}

/// The size (bytes) from which a buffer is weighed against the available memory first.
pub const LARGE: u64 = 256 << 20;

/// The bytes of `n` values of `T`, `u64::MAX` past what an address can hold.
fn bytes<T>(n: usize) -> u64 {
    n.checked_mul(std::mem::size_of::<T>()).map_or(u64::MAX, |b| b as u64)
}

/// An empty vector with room for `n` values of `T`, for `what`.
pub fn with_capacity<T>(n: usize, what: &'static str) -> Result<Vec<T>, OutOfMemory> {
    let size = bytes::<T>(n);
    within(size, what, if size >= LARGE { crate::multifrontal::available_memory() } else { None })?;
    let mut v = Vec::new();
    v.try_reserve_exact(n).map_err(|_| OutOfMemory { what, bytes: size })?;
    Ok(v)
}

/// Refuses `size` bytes for `what` past the `available` memory (bytes; `None`: not known).
fn within(size: u64, what: &'static str, available: Option<u64>) -> Result<(), OutOfMemory> {
    match available {
        Some(free) if size > free => Err(OutOfMemory { what, bytes: size }),
        _ => Ok(()),
    }
}

/// `n` copies of `value`, for `what`.
pub fn filled<T: Clone>(n: usize, value: T, what: &'static str) -> Result<Vec<T>, OutOfMemory> {
    let mut v = with_capacity(n, what)?;
    v.resize(n, value);
    Ok(v)
}

/// `n` zeros (`T`'s default), for `what`.
pub fn zeros<T: Clone + Default>(n: usize, what: &'static str) -> Result<Vec<T>, OutOfMemory> {
    filled(n, T::default(), what)
}

/// A copy of `s`, for `what`.
pub fn copied<T: Clone>(s: &[T], what: &'static str) -> Result<Vec<T>, OutOfMemory> {
    let mut v = with_capacity(s.len(), what)?;
    v.extend_from_slice(s);
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A size whose bytes overflow, and one no machine has, are refused saying what they were
    /// for; one that fits is made whole.
    #[test]
    fn sizes_past_the_memory_are_refused() {
        assert_eq!(zeros::<u64>(usize::MAX, "x"), Err(OutOfMemory { what: "x", bytes: u64::MAX }));
        let e = with_capacity::<[u8; 16]>(usize::MAX / 32, "a front").map(|v| v.capacity());
        assert!(matches!(e, Err(OutOfMemory { what: "a front", bytes }) if bytes > 1 << 60), "{e:?}");
        assert_eq!(filled(3, 7u8, "x").map(|v| v.len()), Ok(3));
    }

    /// A buffer larger than the memory the system has left is refused before it is asked for
    /// (Linux would promise it, then kill the process as it is filled); one within it, or with
    /// the memory not known, is asked for.
    #[test]
    fn a_buffer_past_the_available_memory_is_refused_first() {
        let gb = 1u64 << 30;
        assert_eq!(within(2 * gb, "a front", Some(gb)), Err(OutOfMemory { what: "a front", bytes: 2 * gb }));
        assert_eq!(within(gb, "a front", Some(2 * gb)), Ok(()));
        assert_eq!(within(u64::MAX, "a front", None), Ok(()));
    }
}
