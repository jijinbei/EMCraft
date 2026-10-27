//! The kernels: one unit per lattice point for H and E (each unit updating the components that
//! have that point), one per E_y node of the source plane, and one cube per projection.
//!
//! Lattices (n cells along each axis): E_x (nx, ny+1, nz+1), E_y (nx+1, ny, nz+1),
//! E_z (nx+1, ny+1, nz); H_x (nx+1, ny, nz), H_y (nx, ny+1, nz), H_z (nx, ny, nz+1); indices
//! i + d₀ (j + d₁ k). Edges and faces with a zero coefficient are not updated (conductors).

use cubecl::prelude::*;

use crate::{Buf, Real};

/// The index of (i, j, k) on a lattice of d0 × d1 × ….
#[cube]
fn at(d0: usize, d1: usize, i: usize, j: usize, k: usize) -> usize {
    i + d0 * (j + d1 * k)
}

/// H from n − ½ to n + ½, with the CPML's ψ for the z derivatives; `cpml` holds (b, a) per
/// cell centre along z.
#[cube(launch)]
pub(crate) fn step_h<F: Real>(
    hx: &mut [F],
    hy: &mut [F],
    hz: &mut [F],
    ex: &[F],
    ey: &[F],
    ez: &[F],
    chx: &[F],
    chy: &[F],
    chz: &[F],
    psi_hx: &mut [F],
    psi_hy: &mut [F],
    dx: &[F],
    dy: &[F],
    dz: &[F],
    cpml: &[F],
    nx: u32,
    ny: u32,
    nz: u32,
) {
    let nx = nx as usize;
    let ny = ny as usize;
    let nz = nz as usize;
    let pos = ABSOLUTE_POS;
    let i = pos % (nx + 1);
    let j = (pos / (nx + 1)) % (ny + 1);
    let k = pos / ((nx + 1) * (ny + 1));
    let zero = F::new(0.0_f32);
    if k <= nz {
        if j < ny && k < nz {
            let id = at(nx + 1, ny, i, j, k);
            let c = chx[id];
            if c != zero {
                let dez_dy = (ez[at(nx + 1, ny + 1, i, j + 1, k)] - ez[at(nx + 1, ny + 1, i, j, k)]) / dy[j];
                let dey_dz = (ey[at(nx + 1, ny, i, j, k + 1)] - ey[at(nx + 1, ny, i, j, k)]) / dz[k];
                let psi = cpml[2 * k] * psi_hx[id] + cpml[2 * k + 1] * dey_dz;
                psi_hx[id] = psi;
                hx[id] -= c * (dez_dy - dey_dz - psi);
            }
        }
        if i < nx && k < nz {
            let id = at(nx, ny + 1, i, j, k);
            let c = chy[id];
            if c != zero {
                let dex_dz = (ex[at(nx, ny + 1, i, j, k + 1)] - ex[at(nx, ny + 1, i, j, k)]) / dz[k];
                let dez_dx = (ez[at(nx + 1, ny + 1, i + 1, j, k)] - ez[at(nx + 1, ny + 1, i, j, k)]) / dx[i];
                let psi = cpml[2 * k] * psi_hy[id] + cpml[2 * k + 1] * dex_dz;
                psi_hy[id] = psi;
                hy[id] -= c * (dex_dz + psi - dez_dx);
            }
        }
        if i < nx && j < ny {
            let id = at(nx, ny, i, j, k);
            let c = chz[id];
            if c != zero {
                let dey_dx = (ey[at(nx + 1, ny, i + 1, j, k)] - ey[at(nx + 1, ny, i, j, k)]) / dx[i];
                let dex_dy = (ex[at(nx, ny + 1, i, j + 1, k)] - ex[at(nx, ny + 1, i, j, k)]) / dy[j];
                hz[id] -= c * (dey_dx - dex_dy);
            }
        }
    }
}

/// E from n to n + 1 on interior edges, with the CPML's ψ; `cpml` holds (b, a) per z node.
#[cube(launch)]
pub(crate) fn step_e<F: Real>(
    ex: &mut [F],
    ey: &mut [F],
    ez: &mut [F],
    hx: &[F],
    hy: &[F],
    hz: &[F],
    cex: &[F],
    cey: &[F],
    cez: &[F],
    psi_ex: &mut [F],
    psi_ey: &mut [F],
    dxd: &[F],
    dyd: &[F],
    dzd: &[F],
    cpml: &[F],
    nx: u32,
    ny: u32,
    nz: u32,
) {
    let nx = nx as usize;
    let ny = ny as usize;
    let nz = nz as usize;
    let pos = ABSOLUTE_POS;
    let i = pos % (nx + 1);
    let j = (pos / (nx + 1)) % (ny + 1);
    let k = pos / ((nx + 1) * (ny + 1));
    let zero = F::new(0.0_f32);
    if k <= nz {
        if i < nx && j >= 1 && j < ny && k >= 1 && k < nz {
            let id = at(nx, ny + 1, i, j, k);
            let c = cex[id];
            if c != zero {
                let dhz_dy = (hz[at(nx, ny, i, j, k)] - hz[at(nx, ny, i, j - 1, k)]) / dyd[j];
                let dhy_dz = (hy[at(nx, ny + 1, i, j, k)] - hy[at(nx, ny + 1, i, j, k - 1)]) / dzd[k];
                let psi = cpml[2 * k] * psi_ex[id] + cpml[2 * k + 1] * dhy_dz;
                psi_ex[id] = psi;
                ex[id] += c * (dhz_dy - dhy_dz - psi);
            }
        }
        if i >= 1 && i < nx && j < ny && k >= 1 && k < nz {
            let id = at(nx + 1, ny, i, j, k);
            let c = cey[id];
            if c != zero {
                let dhx_dz = (hx[at(nx + 1, ny, i, j, k)] - hx[at(nx + 1, ny, i, j, k - 1)]) / dzd[k];
                let dhz_dx = (hz[at(nx, ny, i, j, k)] - hz[at(nx, ny, i - 1, j, k)]) / dxd[i];
                let psi = cpml[2 * k] * psi_ey[id] + cpml[2 * k + 1] * dhx_dz;
                psi_ey[id] = psi;
                ey[id] += c * (dhx_dz + psi - dhz_dx);
            }
        }
        if i >= 1 && i < nx && j >= 1 && j < ny && k < nz {
            let id = at(nx + 1, ny + 1, i, j, k);
            let c = cez[id];
            if c != zero {
                let dhy_dx = (hy[at(nx, ny + 1, i, j, k)] - hy[at(nx, ny + 1, i - 1, j, k)]) / dxd[i];
                let dhx_dy = (hx[at(nx + 1, ny, i, j, k)] - hx[at(nx + 1, ny, i, j - 1, k)]) / dyd[j];
                ez[id] += c * (dhy_dx - dhx_dy);
            }
        }
    }
}

/// E_y += s × profile on the source plane's updated edges.
#[cube(launch)]
pub(crate) fn inject<F: Real>(ey: &mut [F], cey: &[F], source: &[F], s: F, plane: u32, k: u32) {
    let plane = plane as usize;
    let m = ABSOLUTE_POS;
    if m < plane {
        let id = m + plane * (k as usize);
        if cey[id] != F::new(0.0_f32) {
            ey[id] += s * source[m];
        }
    }
}

/// Cube 0 projects E_y on plane `k1` with the first half of `weights`, cube 1 on `k2` with the
/// second; each unit sums every `width`-th node from its own, then pairs fold in a fixed tree.
/// The two sums go to `record[2 slot]` and `record[2 slot + 1]`.
#[cube(launch)]
pub(crate) fn project<F: Real>(ey: &[F], weights: &[F], record: &mut [F], plane: u32, k1: u32, k2: u32, slot: u32, #[comptime] width: usize) {
    let plane = plane as usize;
    let cube = CUBE_POS;
    let k = if cube == 0 { k1 as usize } else { k2 as usize };
    let u = UNIT_POS as usize;
    // The cube's width as a run-time value (the comptime one sizes the shared memory).
    let units = CUBE_DIM as usize;
    let mut sums = Shared::<[F]>::new_slice(width);
    let mut acc = F::new(0.0_f32);
    let mut m = u;
    while m < plane {
        acc += ey[m + plane * k] * weights[m + plane * cube];
        m += units;
    }
    sums[u] = acc;
    sync_cube();
    let mut half = units / 2;
    while half > 0 {
        if u < half {
            let other = sums[u + half];
            sums[u] += other;
        }
        sync_cube();
        half /= 2;
    }
    if u == 0 {
        record[2 * (slot as usize) + cube] = sums[0];
    }
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
