//! The guide's kernels: H and E on a tensor grid with the CPML along z and dispersive edges,
//! one unit per E_y node of the source plane, one cube per projection. Edges and faces with a
//! zero coefficient are not updated (conductors).

use cubecl::prelude::*;

use super::{Cells, Xyz, at, point};
use crate::buffer::Real;

/// The CPML along z of the two components with a z derivative: their auxiliary variables ψ
/// and (b, a) per z position (ψ ← b ψ + a ∂).
#[derive(CubeLaunch, CubeType)]
pub(crate) struct ZCpml<F: Float> {
    psi_x: Box<[F]>,
    psi_y: Box<[F]>,
    ba: Box<[F]>,
}

/// The dispersive edges (`emcraft_fdtd::Poles`): each E edge's slot (`u32::MAX` for none) per
/// component, each slot's ε₀ε∞ and terms, each term's kind (0 Debye, 1 second order), its three
/// coefficients and its polarizations now and a step before.
#[derive(CubeLaunch, CubeType)]
pub(crate) struct Poles<F: Float> {
    slot_x: Box<[u32]>,
    slot_y: Box<[u32]>,
    slot_z: Box<[u32]>,
    eps: Box<[F]>,
    start: Box<[u32]>,
    kind: Box<[u32]>,
    k: Box<[F]>,
    p: Box<[F]>,
    prev: Box<[F]>,
    dt: F,
}

/// The new E on a dispersive edge of `slot` from E now and the bracketed curl term, advancing
/// its polarizations (Taflove & Hagness §9.4, as `emcraft_fdtd`'s `Dispersion::step`).
#[cube]
fn advance<F: Real>(poles: &mut Poles<F>, slot: u32, e: F, curl: F) -> F {
    let s = slot as usize;
    let begin = poles.start[s] as usize;
    let end = poles.start[s + 1] as usize;
    let mut kb_sum = F::new(0.0_f32);
    let mut rhs = poles.dt * curl;
    let mut t = begin;
    while t < end {
        let p = poles.p[t];
        if poles.kind[t] == 0 {
            kb_sum += poles.k[3 * t + 1];
            rhs -= (poles.k[3 * t] - F::new(1.0_f32)) * p;
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
        if poles.kind[t] == 0 {
            poles.p[t] = poles.k[3 * t] * poles.p[t] + poles.k[3 * t + 1] * (e_new + e);
        }
        t += 1;
    }
    e_new
}

/// H from n − ½ to n + ½ (`c` holds dt/μ, `d` the primal steps, the CPML's (b, a) per cell
/// centre along z).
#[cube(launch)]
pub(crate) fn step_h<F: Real>(mut h: Xyz<F>, e: Xyz<F>, c: Xyz<F>, mut cpml: ZCpml<F>, d: Xyz<F>, n: Cells) {
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
                h.x[id] -= cf * (dez_dy - dey_dz - psi);
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
                h.y[id] -= cf * (dex_dz + psi - dez_dx);
            }
        }
        if i < nx && j < ny {
            let id = at(nx, ny, i, j, k);
            let cf = c.z[id];
            if cf != zero {
                let dey_dx = (e.y[at(nx + 1, ny, i + 1, j, k)] - e.y[at(nx + 1, ny, i, j, k)]) / d.x[i];
                let dex_dy = (e.x[at(nx, ny + 1, i, j + 1, k)] - e.x[at(nx, ny + 1, i, j, k)]) / d.y[j];
                h.z[id] -= cf * (dey_dx - dex_dy);
            }
        }
    }
}

/// E from n to n + 1 on interior edges (`c` holds dt/ε, `d` the dual steps, the CPML's (b, a)
/// per z node); with `dispersive`, the edges with poles by the auxiliary equations.
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
    if k <= nz {
        if i < nx && j >= 1 && j < ny && k >= 1 && k < nz {
            let id = at(nx, ny + 1, i, j, k);
            let cf = c.x[id];
            if cf != zero {
                let dhz_dy = (h.z[at(nx, ny, i, j, k)] - h.z[at(nx, ny, i, j - 1, k)]) / d.y[j];
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
        if i >= 1 && i < nx && j < ny && k >= 1 && k < nz {
            let id = at(nx + 1, ny, i, j, k);
            let cf = c.y[id];
            if cf != zero {
                let dhx_dz = (h.x[at(nx + 1, ny, i, j, k)] - h.x[at(nx + 1, ny, i, j, k - 1)]) / d.z[k];
                let dhz_dx = (h.z[at(nx, ny, i, j, k)] - h.z[at(nx, ny, i - 1, j, k)]) / d.x[i];
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
        if i >= 1 && i < nx && j >= 1 && j < ny && k < nz {
            let id = at(nx + 1, ny + 1, i, j, k);
            let cf = c.z[id];
            if cf != zero {
                let dhy_dx = (h.y[at(nx, ny + 1, i, j, k)] - h.y[at(nx, ny + 1, i - 1, j, k)]) / d.x[i];
                let dhx_dy = (h.x[at(nx + 1, ny, i, j, k)] - h.x[at(nx + 1, ny, i, j - 1, k)]) / d.y[j];
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
