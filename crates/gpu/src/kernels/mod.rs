//! The kernels, written in Rust with CubeCL: the guide's (`guide`) and the open box's (`open`),
//! on the shared lattice of the Yee cell.
//!
//! Lattices (n cells along each axis): E_x (nx, ny+1, nz+1), E_y (nx+1, ny, nz+1),
//! E_z (nx+1, ny+1, nz); H_x (nx+1, ny, nz), H_y (nx, ny+1, nz), H_z (nx, ny, nz+1); indices
//! i + d₀ (j + d₁ k). Each kernel runs one unit per point of the node lattice
//! (nx+1) × (ny+1) × (nz+1), updating the components that have that point.

use cubecl::prelude::*;

use crate::buffer::{Buf, Real};

pub(crate) mod front;
pub(crate) mod guide;
pub(crate) mod open;

/// One array per axis: a field's components, their coefficients, the grid's steps.
#[derive(CubeLaunch, CubeType)]
pub(crate) struct Xyz<F: Float> {
    pub(crate) x: Box<[F]>,
    pub(crate) y: Box<[F]>,
    pub(crate) z: Box<[F]>,
}

/// The grid's cells along each axis.
#[derive(CubeLaunch, CubeType)]
pub(crate) struct Cells {
    pub(crate) nx: u32,
    pub(crate) ny: u32,
    pub(crate) nz: u32,
}

/// A lattice point: the unit's (i, j, k) on the node lattice, and the cells along each axis.
#[derive(CubeType)]
pub(crate) struct Point {
    pub(crate) i: usize,
    pub(crate) j: usize,
    pub(crate) k: usize,
    pub(crate) nx: usize,
    pub(crate) ny: usize,
    pub(crate) nz: usize,
}

#[cube]
pub(crate) fn point(n: &Cells) -> Point {
    let nx = n.nx as usize;
    let ny = n.ny as usize;
    let nz = n.nz as usize;
    let pos = ABSOLUTE_POS;
    Point { i: pos % (nx + 1), j: (pos / (nx + 1)) % (ny + 1), k: pos / ((nx + 1) * (ny + 1)), nx, ny, nz }
}

/// The index of (i, j, k) on a lattice of d0 × d1 × ….
#[cube]
pub(crate) fn at(d0: usize, d1: usize, i: usize, j: usize, k: usize) -> usize {
    i + d0 * (j + d1 * k)
}

/// out[0] = 2 × 1.5, to see whether a device runs `F` at all.
#[cube(launch)]
fn twice<F: Real>(out: &mut [F], x: F) {
    if ABSOLUTE_POS == 0 {
        out[0] = x * F::new(2.0_f32);
    }
}

/// Whether the device runs kernels in `F`.
pub(crate) fn probe<F: Real>(client: &Client) -> bool {
    let out = Buf::<F>::zeros(client, 1);
    twice::launch::<F>(client, CubeCount::Static(1, 1, 1), CubeDim::new_1d(1), out.arg(), F::of(1.5));
    out.read(client).is_ok_and(|v| v.first() == Some(&3.0))
}
