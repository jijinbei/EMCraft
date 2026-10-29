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

/// The dispersive and lossy edges (`emcraft_fdtd::Poles`): each E edge's slot (`u32::MAX` for
/// none) per component, each slot's ε₀ε∞, σΔt/2 and terms, each term's kind (0 Debye, 1 second
/// order, 2 a branch of surface current), its three coefficients and its states now and a step
/// before.
#[derive(CubeLaunch, CubeType)]
pub(crate) struct Poles<F: Float> {
    pub(crate) slot_x: Box<[u32]>,
    pub(crate) slot_y: Box<[u32]>,
    pub(crate) slot_z: Box<[u32]>,
    eps: Box<[F]>,
    loss: Box<[F]>,
    start: Box<[u32]>,
    kind: Box<[u32]>,
    k: Box<[F]>,
    p: Box<[F]>,
    prev: Box<[F]>,
    dt: F,
}

/// The new E on a slotted edge of `slot` from E now and the bracketed curl term, advancing its
/// terms (Taflove & Hagness §9.4, as `emcraft_fdtd`'s `Dispersion::step`).
#[cube]
pub(crate) fn advance<F: Real>(poles: &mut Poles<F>, slot: u32, e: F, curl: F) -> F {
    let s = slot as usize;
    let begin = poles.start[s] as usize;
    let end = poles.start[s + 1] as usize;
    let one = F::new(1.0_f32);
    let mut kb_sum = poles.loss[s];
    let mut rhs = poles.dt * curl;
    let mut t = begin;
    while t < end {
        let p = poles.p[t];
        let kind = poles.kind[t];
        if kind == 0 {
            kb_sum += poles.k[3 * t + 1];
            rhs -= (poles.k[3 * t] - one) * p;
        } else if kind == 2 {
            kb_sum += poles.k[3 * t + 1];
            rhs -= (poles.k[3 * t] + one) * p;
        } else {
            let next = poles.k[3 * t] * p - poles.k[3 * t + 1] * poles.prev[t] + poles.k[3 * t + 2] * e;
            rhs -= next - p;
            poles.prev[t] = p;
            poles.p[t] = next;
        }
        t += 1;
    }
    let eps = poles.eps[s];
    let e_new = ((eps - kb_sum) * e + rhs) / (eps + kb_sum);
    t = begin;
    while t < end {
        if poles.kind[t] != 1 {
            poles.p[t] = poles.k[3 * t] * poles.p[t] + poles.k[3 * t + 1] * (e_new + e);
        }
        t += 1;
    }
    e_new
}

/// The terms of component `axis`'s edge `id`, if it has a slot, following a change `delta` of
/// Eⁿ⁺¹ made after its step (an impressed current; `emcraft_fdtd`'s `Dispersion::inject`).
#[cube]
pub(crate) fn follow<F: Real>(poles: &mut Poles<F>, axis: u32, id: usize, delta: F) {
    let slot = if axis == 0 {
        poles.slot_x[id]
    } else if axis == 1 {
        poles.slot_y[id]
    } else {
        poles.slot_z[id]
    };
    if slot != u32::MAX {
        let s = slot as usize;
        let end = poles.start[s + 1] as usize;
        let mut t = poles.start[s] as usize;
        while t < end {
            if poles.kind[t] != 1 {
                poles.p[t] += poles.k[3 * t + 1] * delta;
            }
            t += 1;
        }
    }
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
