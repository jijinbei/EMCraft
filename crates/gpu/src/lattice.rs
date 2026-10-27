//! A Yee grid on a device: its cells, the lengths of its six components' lattices (E_a a half
//! cell along a and nodes across, H_a nodes along a and half cells across), and the launch
//! shapes of kernels run over it — one unit per point, in cubes the device holds.

use cubecl::prelude::*;
use emcraft_fdtd::{FdtdError, Result};

use crate::device::cube_units;
use crate::kernels::CellsLaunch;

/// The most units per cube of a kernel run over points.
const UNITS: u32 = 256;

/// A grid of `n` cells as the kernels see it.
pub(crate) struct Lattice {
    n: [u32; 3],
    /// The lengths of E's three components' lattices, and of H's.
    pub(crate) e: [usize; 3],
    pub(crate) h: [usize; 3],
    /// Units per cube.
    units: u32,
}

impl Lattice {
    /// The grid of `n` cells on `client`'s device; refused when an index would not fit the
    /// kernels' 32 bits.
    pub(crate) fn new(client: &Client, n: [usize; 3]) -> Result<Lattice> {
        let [nx, ny, nz] = n;
        let e = [nx * (ny + 1) * (nz + 1), (nx + 1) * ny * (nz + 1), (nx + 1) * (ny + 1) * nz];
        let h = [(nx + 1) * ny * nz, nx * (ny + 1) * nz, nx * ny * (nz + 1)];
        if e.iter().chain(&h).any(|&l| l >= u32::MAX as usize / 2) {
            return Err(FdtdError::TooLarge { cells: nx * ny * nz, limit: emcraft_fdtd::MAX_CELLS });
        }
        Ok(Lattice { n: n.map(|v| v as u32), e, h, units: cube_units(client, UNITS) })
    }

    /// The cells as a kernel argument.
    pub(crate) fn cells(&self) -> CellsLaunch {
        let [nx, ny, nz] = self.n;
        CellsLaunch::new(nx, ny, nz)
    }

    /// E_y nodes on a z plane, (nx + 1) × ny.
    pub(crate) fn plane(&self) -> u32 {
        let [nx, ny, _] = self.n;
        (nx + 1) * ny
    }

    /// The launch of one unit per node of the grid, (nx + 1) × (ny + 1) × (nz + 1).
    pub(crate) fn over_nodes(&self) -> (CubeCount, CubeDim) {
        let [nx, ny, nz] = self.n;
        self.over((nx + 1) * (ny + 1) * (nz + 1))
    }

    /// The launch of one unit per item of `count`.
    pub(crate) fn over(&self, count: u32) -> (CubeCount, CubeDim) {
        line(count, self.units)
    }
}

/// The launch of one unit per item of `count` in cubes of `units`.
pub(crate) fn line(count: u32, units: u32) -> (CubeCount, CubeDim) {
    (CubeCount::Static(count.div_ceil(units.max(1)).max(1), 1, 1), CubeDim::new_1d(units.max(1)))
}
