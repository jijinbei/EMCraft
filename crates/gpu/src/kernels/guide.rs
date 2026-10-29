//! The guide's kernels: H and E on a tensor grid with the CPML along z, dispersive and lossy
//! edges, the walls' edges with finite conductivity and magnetic losses; one unit per E_y node
//! of the source plane, one cube per projection. Edges and faces with a zero coefficient are not
//! updated (perfect conductors).

use cubecl::prelude::*;

use super::{Cells, Poles, Xyz, advance, at, point};
use crate::buffer::Real;

/// The CPML along z of the two components with a z derivative: their auxiliary variables ψ
/// and (b, a) per z position (ψ ← b ψ + a ∂).
#[derive(CubeLaunch, CubeType)]
pub(crate) struct ZCpml<F: Float> {
    psi_x: Box<[F]>,
    psi_y: Box<[F]>,
    ba: Box<[F]>,
}

/// H from n − ½ to n + ½ (`c` holds dt/μ, `d` the primal steps, the CPML's (b, a) per cell
/// centre along z); with `magnetic`, H decays by `decay` first (magnetic losses).
#[cube(launch)]
pub(crate) fn step_h<F: Real>(
    mut h: Xyz<F>,
    e: Xyz<F>,
    c: Xyz<F>,
    mut cpml: ZCpml<F>,
    d: Xyz<F>,
    n: Cells,
    decay: Xyz<F>,
    #[comptime] magnetic: bool,
) {
    let p = point(&n);
    let (i, j, k, nx, ny, nz) = (p.i, p.j, p.k, p.nx, p.ny, p.nz);
    let zero = F::new(0.0_f32);
    if k <= nz {
        if j < ny && k < nz {
            let id = at(nx + 1, ny, i, j, k);
            let cf = c.x[id];
            if cf != zero {
                let dez_dy = (e.z[at(nx + 1, ny + 1, i, j + 1, k)] - e.z[at(nx + 1, ny + 1, i, j, k)]) / d.y[j];
                let dey_dz = (e.y[at(nx + 1, ny, i, j, k + 1)] - e.y[at(nx + 1, ny, i, j, k)]) / d.z[k];
                let psi = cpml.ba[2 * k] * cpml.psi_x[id] + cpml.ba[2 * k + 1] * dey_dz;
                cpml.psi_x[id] = psi;
                let mut old = h.x[id];
                if comptime![magnetic] {
                    old *= decay.x[id];
                }
                h.x[id] = old - cf * (dez_dy - dey_dz - psi);
            }
        }
        if i < nx && k < nz {
            let id = at(nx, ny + 1, i, j, k);
            let cf = c.y[id];
            if cf != zero {
                let dex_dz = (e.x[at(nx, ny + 1, i, j, k + 1)] - e.x[at(nx, ny + 1, i, j, k)]) / d.z[k];
                let dez_dx = (e.z[at(nx + 1, ny + 1, i + 1, j, k)] - e.z[at(nx + 1, ny + 1, i, j, k)]) / d.x[i];
                let psi = cpml.ba[2 * k] * cpml.psi_y[id] + cpml.ba[2 * k + 1] * dex_dz;
                cpml.psi_y[id] = psi;
                let mut old = h.y[id];
                if comptime![magnetic] {
                    old *= decay.y[id];
                }
                h.y[id] = old - cf * (dex_dz + psi - dez_dx);
            }
        }
        if i < nx && j < ny {
            let id = at(nx, ny, i, j, k);
            let cf = c.z[id];
            if cf != zero {
                let dey_dx = (e.y[at(nx + 1, ny, i + 1, j, k)] - e.y[at(nx + 1, ny, i, j, k)]) / d.x[i];
                let dex_dy = (e.x[at(nx, ny + 1, i, j + 1, k)] - e.x[at(nx, ny + 1, i, j, k)]) / d.y[j];
                let mut old = h.z[id];
                if comptime![magnetic] {
                    old *= decay.z[id];
                }
                h.z[id] = old - cf * (dey_dx - dex_dy);
            }
        }
    }
}

/// The index below `i` and above it on a lattice of `n` points along an axis, clamped, and
/// whether each lies on the lattice (H beyond the grid's outer walls is zero).
#[derive(CubeType)]
struct Around<F: Float> {
    below: usize,
    above: usize,
    w_below: F,
    w_above: F,
}

#[cube]
fn around<F: Real>(i: usize, n: usize) -> Around<F> {
    let zero = F::new(0.0_f32);
    let one = F::new(1.0_f32);
    Around::<F> {
        below: if i > 0 { i - 1 } else { 0usize },
        above: if i < n { i } else { n - 1 },
        w_below: if i > 0 { one } else { zero },
        w_above: if i < n { one } else { zero },
    }
}

/// E from n to n + 1 on every edge with a coefficient (the walls' edges too, with finite
/// conductivity; `c` holds dt/ε, `d` the dual steps, the CPML's (b, a) per z node); with
/// `dispersive`, the slotted edges by the auxiliary equations.
#[cube(launch)]
pub(crate) fn step_e<F: Real>(
    mut e: Xyz<F>,
    h: Xyz<F>,
    c: Xyz<F>,
    mut cpml: ZCpml<F>,
    d: Xyz<F>,
    n: Cells,
    mut poles: Poles<F>,
    #[comptime] dispersive: bool,
) {
    let p = point(&n);
    let (i, j, k, nx, ny, nz) = (p.i, p.j, p.k, p.nx, p.ny, p.nz);
    let zero = F::new(0.0_f32);
    let x = around::<F>(i, nx);
    let y = around::<F>(j, ny);
    if k <= nz {
        if i < nx && k >= 1 && k < nz {
            let id = at(nx, ny + 1, i, j, k);
            let cf = c.x[id];
            if cf != zero {
                let dhz_dy = (y.w_above * h.z[at(nx, ny, i, y.above, k)] - y.w_below * h.z[at(nx, ny, i, y.below, k)]) / d.y[j];
                let dhy_dz = (h.y[at(nx, ny + 1, i, j, k)] - h.y[at(nx, ny + 1, i, j, k - 1)]) / d.z[k];
                let psi = cpml.ba[2 * k] * cpml.psi_x[id] + cpml.ba[2 * k + 1] * dhy_dz;
                cpml.psi_x[id] = psi;
                let curl = dhz_dy - dhy_dz - psi;
                let mut next = e.x[id] + cf * curl;
                if comptime![dispersive] {
                    let slot = poles.slot_x[id];
                    if slot != u32::MAX {
                        next = advance(&mut poles, slot, e.x[id], curl);
                    }
                }
                e.x[id] = next;
            }
        }
        if j < ny && k >= 1 && k < nz {
            let id = at(nx + 1, ny, i, j, k);
            let cf = c.y[id];
            if cf != zero {
                let dhx_dz = (h.x[at(nx + 1, ny, i, j, k)] - h.x[at(nx + 1, ny, i, j, k - 1)]) / d.z[k];
                let dhz_dx = (x.w_above * h.z[at(nx, ny, x.above, j, k)] - x.w_below * h.z[at(nx, ny, x.below, j, k)]) / d.x[i];
                let psi = cpml.ba[2 * k] * cpml.psi_y[id] + cpml.ba[2 * k + 1] * dhx_dz;
                cpml.psi_y[id] = psi;
                let curl = dhx_dz + psi - dhz_dx;
                let mut next = e.y[id] + cf * curl;
                if comptime![dispersive] {
                    let slot = poles.slot_y[id];
                    if slot != u32::MAX {
                        next = advance(&mut poles, slot, e.y[id], curl);
                    }
                }
                e.y[id] = next;
            }
        }
        if k < nz {
            let id = at(nx + 1, ny + 1, i, j, k);
            let cf = c.z[id];
            if cf != zero {
                let dhy_dx = (x.w_above * h.y[at(nx, ny + 1, x.above, j, k)] - x.w_below * h.y[at(nx, ny + 1, x.below, j, k)]) / d.x[i];
                let dhx_dy = (y.w_above * h.x[at(nx + 1, ny, i, y.above, k)] - y.w_below * h.x[at(nx + 1, ny, i, y.below, k)]) / d.y[j];
                let curl = dhy_dx - dhx_dy;
                let mut next = e.z[id] + cf * curl;
                if comptime![dispersive] {
                    let slot = poles.slot_z[id];
                    if slot != u32::MAX {
                        next = advance(&mut poles, slot, e.z[id], curl);
                    }
                }
                e.z[id] = next;
            }
        }
    }
}

/// E_y += s × profile on plane `k`'s updated edges (`plane` E_y nodes a plane).
#[cube(launch)]
pub(crate) fn inject<F: Real>(ey: &mut [F], cey: &[F], profile: &[F], s: F, plane: u32, k: u32) {
    let plane = plane as usize;
    let m = ABSOLUTE_POS;
    if m < plane {
        let id = m + plane * (k as usize);
        if cey[id] != F::new(0.0_f32) {
            ey[id] += s * profile[m];
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
