//! The factorization's large fronts on a device (`emcraft_solver::accel`): the front uploaded
//! once, each block's columns read back for the host's pivoting, its interchanges and its
//! update of the later columns run by kernels, the Schur complement read back at the end.
//! Complex f64 throughout: the factorization's pivoting and accuracy need it, however slowly a
//! consumer GPU runs f64.

use std::ops::Range;
use std::sync::atomic::{AtomicUsize, Ordering};

use cubecl::prelude::*;
use emcraft_solver::Complex64 as C;
use emcraft_solver::accel::{Accelerator, DeviceFront};

use crate::Backend;
use crate::buffer::{Buf, Ints};
use crate::device::{cube_units, guarded};
use crate::kernels::front::{self as kernels, BLOCK, REACH, UNITS};

/// Fronts from this many rows go to the device (smaller ones do not pay for the transfers).
pub const MIN_ROWS: usize = 1024;

/// The largest front the device holds: 16 m² bytes (4.1 GB at 16 000 rows).
pub const MAX_ROWS: usize = 16_000;

/// An [`Accelerator`] on a CubeCL device.
pub struct Fronts {
    backend: Backend,
    client: Client,
    min_rows: usize,
    /// Units per side of an update cube (its square fits the device's cubes).
    side_units: u32,
    /// Fronts held so far.
    taken: AtomicUsize,
}

impl Fronts {
    /// The device of `backend`, taking fronts from `min_rows` rows ([`MIN_ROWS`] by default).
    pub fn new(backend: Backend, min_rows: Option<usize>) -> emcraft_fdtd::Result<Fronts> {
        let device = backend.device()?;
        let client = guarded(|| Ok(device.client()))?;
        let most = cube_units(&client, UNITS * UNITS);
        let side_units = (1..=UNITS).rev().find(|u| u.is_power_of_two() && u * u <= most).unwrap_or(1);
        Ok(Fronts { backend, client, min_rows: min_rows.unwrap_or(MIN_ROWS).max(1), side_units, taken: AtomicUsize::new(0) })
    }
}

impl Fronts {
    /// The fewest rows of a front it takes.
    pub fn min_rows(&self) -> usize {
        self.min_rows
    }

    /// The fronts this device has held so far.
    pub fn taken(&self) -> usize {
        self.taken.load(Ordering::Relaxed)
    }
}

/// Complex numbers as their bytes, (re, im) interleaved (no copy).
fn bytes(xs: &[C]) -> &[u8] {
    bytemuck::cast_slice(xs)
}

/// A device failure as the solver's error text.
fn text(e: emcraft_fdtd::FdtdError) -> String {
    e.to_string()
}

impl Accelerator for Fronts {
    fn name(&self) -> String {
        format!("{} (complex double)", self.backend)
    }

    fn takes(&self, m: usize, k: usize) -> bool {
        m >= self.min_rows && m <= MAX_ROWS && k > 0
    }

    fn front(&self, f: &[C], m: usize) -> Result<Box<dyn DeviceFront>, String> {
        if f.len() != m * m || m > MAX_ROWS {
            return Err(format!("a front of {} entries is not {m} × {m}", f.len()));
        }
        self.taken.fetch_add(1, Ordering::Relaxed);
        let client = self.client.clone();
        let front = guarded(|| Ok(Buf::<f64>::from_bytes(&client, bytes(f), 2 * f.len()))).map_err(text)?;
        Ok(Box::new(HeldFront { client, front, m, side_units: self.side_units }))
    }
}

/// A front on the device.
struct HeldFront {
    client: Client,
    front: Buf<f64>,
    m: usize,
    side_units: u32,
}

impl DeviceFront for HeldFront {
    fn read(&mut self, cols: Range<usize>, out: &mut [C]) -> Result<(), String> {
        let m = self.m;
        if cols.end > m || cols.start > cols.end || out.len() != (cols.end - cols.start) * m {
            return Err("columns outside the front".into());
        }
        let raw = guarded(|| self.front.read_bytes(&self.client, 2 * cols.start * m..2 * cols.end * m)).map_err(text)?;
        let dst: &mut [u8] = bytemuck::cast_slice_mut(out);
        if raw.len() != dst.len() {
            return Err("the device returned the wrong number of values".into());
        }
        dst.copy_from_slice(&raw);
        Ok(())
    }

    fn swap_rows(&mut self, cols: Range<usize>, swaps: &[(usize, usize)]) -> Result<(), String> {
        let m = self.m;
        if swaps.is_empty() || cols.is_empty() {
            return Ok(());
        }
        if cols.end > m || swaps.iter().any(|&(p, q)| p >= m || q >= m) {
            return Err("an interchange outside the front".into());
        }
        let pairs: Vec<u32> = swaps.iter().flat_map(|&(p, q)| [p as u32, q as u32]).collect();
        let columns = (cols.end - cols.start) as u32;
        guarded(|| {
            let list = Ints::new(&self.client, &pairs);
            let (count, dim) = crate::lattice::line(columns, cube_units(&self.client, 256));
            kernels::swap_rows::launch(
                &self.client,
                count,
                dim,
                self.front.arg(),
                list.arg(),
                swaps.len() as u32,
                m as u32,
                cols.start as u32,
                columns,
            );
            Ok(())
        })
        .map_err(text)
    }

    fn update(&mut self, be: usize, k: usize, w: &[C], l: &[C]) -> Result<(), String> {
        let m = self.m;
        let below = m.saturating_sub(be);
        if below == 0 {
            return Ok(());
        }
        let nb = w.len() / below;
        if nb == 0 || nb > BLOCK || w.len() != below * nb || l.len() != w.len() || k > m {
            return Err(format!("a block of {nb} pivots does not fit the device's tiles"));
        }
        let u = self.side_units;
        let tiles = (below as u32).div_ceil(u * REACH as u32);
        guarded(|| {
            let (wb, lb) = (Buf::<f64>::from_bytes(&self.client, bytes(w), 2 * w.len()), Buf::<f64>::from_bytes(&self.client, bytes(l), 2 * l.len()));
            kernels::update::launch(
                &self.client,
                CubeCount::Static(tiles, tiles, 1),
                CubeDim::new_2d(u, u),
                self.front.arg(),
                wb.arg(),
                lb.arg(),
                m as u32,
                be as u32,
                k as u32,
                nb as u32,
                u as usize,
            );
            Ok(())
        })
        .map_err(text)
    }
}
