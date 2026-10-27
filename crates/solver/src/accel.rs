//! Large fronts of the factorization on a device (`emcraft-gpu`'s, through CubeCL): the device
//! holds a front, the host finds each block's pivots on the block's columns read back from it,
//! and the device applies the block's interchanges and its update of every later column, the
//! bulk of the work.
//!
//! An accelerator is set for the calling thread with [`with`]; the factorization's sequential
//! top (where the large fronts are) takes the fronts it [`Accelerator::takes`], and the
//! subtrees on other threads stay on the CPU. A device's sums run in its own fixed order: the
//! factor is the same bit for bit on the same device, and agrees with the CPU's to rounding.

use std::cell::RefCell;
use std::ops::Range;
use std::sync::Arc;

use num_complex::Complex64 as C;

/// A device that factors fronts.
pub trait Accelerator {
    /// What it is, for messages.
    fn name(&self) -> String;

    /// Whether it takes a front of `m` rows with `k` pivots (large enough to pay for the
    /// transfers, small enough for its memory).
    fn takes(&self, m: usize, k: usize) -> bool;

    /// Holds the front `f` (column-major m × m, full in its first columns and lower after).
    fn front(&self, f: &[C], m: usize) -> Result<Box<dyn DeviceFront>, String>;
}

/// A front held by a device.
pub trait DeviceFront {
    /// Columns `cols` (all m rows of each, column-major) into `out`.
    fn read(&mut self, cols: Range<usize>, out: &mut [C]) -> Result<(), String>;

    /// Interchanges rows p and q of columns `cols`, for each (p, q) of `swaps` in order.
    fn swap_rows(&mut self, cols: Range<usize>, swaps: &[(usize, usize)]) -> Result<(), String>;

    /// The update of a block of nb pivots ending at column `be` of a front with `k` pivots:
    /// F[i][j] −= Σₜ W[i − be][t] L[j − be][t] for every column j ≥ be and row i ≥ be with
    /// j < k or i ≥ j, summed over t in order; W and L are (m − be) × nb, column-major.
    fn update(&mut self, be: usize, k: usize, w: &[C], l: &[C]) -> Result<(), String>;
}

thread_local! {
    static CURRENT: RefCell<Option<Arc<dyn Accelerator>>> = const { RefCell::new(None) };
}

/// Runs `f` with `accel` as the calling thread's accelerator (none for `None`), the previous one
/// restored after, even if `f` unwinds.
pub fn with<T>(accel: Option<Arc<dyn Accelerator>>, f: impl FnOnce() -> T) -> T {
    struct Restore(Option<Arc<dyn Accelerator>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let previous = self.0.take();
            CURRENT.with(|c| *c.borrow_mut() = previous);
        }
    }
    let previous = CURRENT.with(|c| c.replace(accel));
    let _restore = Restore(previous);
    f()
}

/// The calling thread's accelerator.
pub(crate) fn current() -> Option<Arc<dyn Accelerator>> {
    CURRENT.with(|c| c.borrow().clone())
}
