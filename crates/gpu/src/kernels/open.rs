//! The open box's kernels: H and E on a uniform grid with a CPML on every derivative, the
//! materials' dispersive and lossy edges and magnetic losses' decay (each
//! component's two auxiliary variables absorb its derivatives along u and v, the axes across it
//! in cyclic order), the lumped ports and current elements, and the running transforms of the
//! tangential fields on the Huygens box (in f64 whatever the fields' precision).

use cubecl::prelude::*;

use super::{Cells, Poles, Xyz, advance, at, follow, point};
use crate::buffer::Real;

/// Each component's two auxiliary variables: for its derivative along u, then along v.
#[derive(CubeLaunch, CubeType)]
pub(crate) struct Psi<F: Float> {
    x0: Box<[F]>,
    x1: Box<[F]>,
    y0: Box<[F]>,
    y1: Box<[F]>,
    z0: Box<[F]>,
    z1: Box<[F]>,
}

/// The cell along each axis.
#[derive(CubeLaunch, CubeType)]
pub(crate) struct Steps<F: Float> {
    hx: F,
    hy: F,
    hz: F,
}

/// ψ ← b ψ + a ∂ with (b, a) the profile's at position `i` along its axis.
#[cube]
fn absorb<F: Real>(profile: &[F], i: usize, psi: F, derivative: F) -> F {
    profile[2 * i] * psi + profile[2 * i + 1] * derivative
}

/// H from n − ½ to n + ½: ∂H_a/∂t = −(1/μ) (∂E_v/∂u − ∂E_u/∂v), `cpml` holding each axis's
/// (b, a) at the half cells and `ch` dt/μ per face; with `magnetic`, H decays by `decay` first.
#[cube(launch)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn step_h<F: Real>(
    mut h: Xyz<F>,
    e: Xyz<F>,
    mut psi: Psi<F>,
    cpml: Xyz<F>,
    d: Steps<F>,
    ch: Xyz<F>,
    n: Cells,
    decay: Xyz<F>,
    #[comptime] magnetic: bool,
) {
    let p = point(&n);
    let (i, j, k, nx, ny, nz) = (p.i, p.j, p.k, p.nx, p.ny, p.nz);
    if k <= nz {
        if j < ny && k < nz {
            let id = at(nx + 1, ny, i, j, k);
            let dez_dy = (e.z[at(nx + 1, ny + 1, i, j + 1, k)] - e.z[at(nx + 1, ny + 1, i, j, k)]) / d.hy;
            let dey_dz = (e.y[at(nx + 1, ny, i, j, k + 1)] - e.y[at(nx + 1, ny, i, j, k)]) / d.hz;
            let q0 = absorb(&cpml.y, j, psi.x0[id], dez_dy);
            let q1 = absorb(&cpml.z, k, psi.x1[id], dey_dz);
            psi.x0[id] = q0;
            psi.x1[id] = q1;
            let mut old = h.x[id];
            if comptime![magnetic] {
                old *= decay.x[id];
            }
            h.x[id] = old - ch.x[id] * (dez_dy + q0 - dey_dz - q1);
        }
        if i < nx && k < nz {
            let id = at(nx, ny + 1, i, j, k);
            let dex_dz = (e.x[at(nx, ny + 1, i, j, k + 1)] - e.x[at(nx, ny + 1, i, j, k)]) / d.hz;
            let dez_dx = (e.z[at(nx + 1, ny + 1, i + 1, j, k)] - e.z[at(nx + 1, ny + 1, i, j, k)]) / d.hx;
            let q0 = absorb(&cpml.z, k, psi.y0[id], dex_dz);
            let q1 = absorb(&cpml.x, i, psi.y1[id], dez_dx);
            psi.y0[id] = q0;
            psi.y1[id] = q1;
            let mut old = h.y[id];
            if comptime![magnetic] {
                old *= decay.y[id];
            }
            h.y[id] = old - ch.y[id] * (dex_dz + q0 - dez_dx - q1);
        }
        if i < nx && j < ny {
            let id = at(nx, ny, i, j, k);
            let dey_dx = (e.y[at(nx + 1, ny, i + 1, j, k)] - e.y[at(nx + 1, ny, i, j, k)]) / d.hx;
            let dex_dy = (e.x[at(nx, ny + 1, i, j + 1, k)] - e.x[at(nx, ny + 1, i, j, k)]) / d.hy;
            let q0 = absorb(&cpml.x, i, psi.z0[id], dey_dx);
            let q1 = absorb(&cpml.y, j, psi.z1[id], dex_dy);
            psi.z0[id] = q0;
            psi.z1[id] = q1;
            let mut old = h.z[id];
            if comptime![magnetic] {
                old *= decay.z[id];
            }
            h.z[id] = old - ch.z[id] * (dey_dx + q0 - dex_dy - q1);
        }
    }
}

/// E from n to n + 1: ∂E_a/∂t = (1/ε) (∂H_v/∂u − ∂H_u/∂v) on edges with a coefficient and off
/// the low walls, `cpml` holding each axis's (b, a) at the nodes; with `dispersive`, the
/// slotted edges by the auxiliary equations.
#[cube(launch)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn step_e<F: Real>(
    mut e: Xyz<F>,
    h: Xyz<F>,
    c: Xyz<F>,
    mut psi: Psi<F>,
    cpml: Xyz<F>,
    d: Steps<F>,
    n: Cells,
    mut poles: Poles<F>,
    #[comptime] dispersive: bool,
) {
    let p = point(&n);
    let (i, j, k, nx, ny, nz) = (p.i, p.j, p.k, p.nx, p.ny, p.nz);
    let zero = F::new(0.0_f32);
    if k <= nz {
        if i < nx {
            let id = at(nx, ny + 1, i, j, k);
            let cf = c.x[id];
            if cf != zero && j > 0 && k > 0 {
                let dhz_dy = (h.z[at(nx, ny, i, j, k)] - h.z[at(nx, ny, i, j - 1, k)]) / d.hy;
                let dhy_dz = (h.y[at(nx, ny + 1, i, j, k)] - h.y[at(nx, ny + 1, i, j, k - 1)]) / d.hz;
                let q0 = absorb(&cpml.y, j, psi.x0[id], dhz_dy);
                let q1 = absorb(&cpml.z, k, psi.x1[id], dhy_dz);
                psi.x0[id] = q0;
                psi.x1[id] = q1;
                let curl = dhz_dy + q0 - dhy_dz - q1;
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
        if j < ny {
            let id = at(nx + 1, ny, i, j, k);
            let cf = c.y[id];
            if cf != zero && k > 0 && i > 0 {
                let dhx_dz = (h.x[at(nx + 1, ny, i, j, k)] - h.x[at(nx + 1, ny, i, j, k - 1)]) / d.hz;
                let dhz_dx = (h.z[at(nx, ny, i, j, k)] - h.z[at(nx, ny, i - 1, j, k)]) / d.hx;
                let q0 = absorb(&cpml.z, k, psi.y0[id], dhx_dz);
                let q1 = absorb(&cpml.x, i, psi.y1[id], dhz_dx);
                psi.y0[id] = q0;
                psi.y1[id] = q1;
                let curl = dhx_dz + q0 - dhz_dx - q1;
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
            if cf != zero && i > 0 && j > 0 {
                let dhy_dx = (h.y[at(nx, ny + 1, i, j, k)] - h.y[at(nx, ny + 1, i - 1, j, k)]) / d.hx;
                let dhx_dy = (h.x[at(nx + 1, ny, i, j, k)] - h.x[at(nx + 1, ny, i, j - 1, k)]) / d.hy;
                let q0 = absorb(&cpml.x, i, psi.z0[id], dhy_dx);
                let q1 = absorb(&cpml.y, j, psi.z1[id], dhx_dy);
                psi.z0[id] = q0;
                psi.z1[id] = q1;
                let curl = dhy_dx + q0 - dhx_dy - q1;
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

/// Component `axis` of `f` at `id`.
#[cube]
fn get<F: Real>(f: &Xyz<F>, axis: u32, id: usize) -> F {
    if axis == 0 {
        f.x[id]
    } else if axis == 1 {
        f.y[id]
    } else {
        f.z[id]
    }
}

#[cube]
fn set<F: Real>(f: &mut Xyz<F>, axis: u32, id: usize, value: F) {
    if axis == 0 {
        f.x[id] = value;
    } else if axis == 1 {
        f.y[id] = value;
    } else {
        f.z[id] = value;
    }
}

/// The lumped ports' edges (flattened, port p owning `start[p]..start[p + 1]`) and the current
/// elements (`emcraft_fdtd::PortPlan`, `CurrentPlan`).
#[derive(CubeLaunch, CubeType)]
pub(crate) struct Sources<F: Float> {
    /// Per edge: its component, index, (ca, cb, cv) and E before the step.
    edge_axis: Box<[u32]>,
    edge_id: Box<[u32]>,
    k: Box<[F]>,
    old: Box<[F]>,
    /// Per port: its first edge, (r_edge, sign, voltage per edge, cell along its axis).
    start: Box<[u32]>,
    port: Box<[F]>,
    /// Per current element: its component, index and coefficient.
    current_axis: Box<[u32]>,
    current_id: Box<[u32]>,
    current: Box<[F]>,
    ports: u32,
    currents: u32,
}

/// Keeps E on the ports' edges before the E update (one unit per edge).
#[cube(launch)]
pub(crate) fn remember<F: Real>(e: Xyz<F>, mut src: Sources<F>, edges: u32) {
    let t = ABSOLUTE_POS;
    if t < edges as usize {
        src.old[t] = get(&e, src.edge_axis[t], src.edge_id[t] as usize);
    }
}

/// After E's plain update: each port's edges redone semi-implicitly with the source at `s`,
/// its voltage and current to `record[2 (slot ports + p)]` and the next; then the current
/// elements (with `dispersive`, the terms of a slotted edge following its change). One unit, the
/// ports in order.
#[cube(launch)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn drive<F: Real>(
    mut e: Xyz<F>,
    c: Xyz<F>,
    src: Sources<F>,
    record: &mut [F],
    s: F,
    slot: u32,
    mut poles: Poles<F>,
    #[comptime] dispersive: bool,
) {
    if ABSOLUTE_POS == 0 {
        let ports = src.ports as usize;
        let half = F::new(0.5_f32);
        for p in 0..ports {
            let r_edge = src.port[4 * p];
            let sign = src.port[4 * p + 1];
            let vs = src.port[4 * p + 2] * s * sign;
            let step = src.port[4 * p + 3];
            let begin = src.start[p] as usize;
            let end = src.start[p + 1] as usize;
            let mut v = F::new(0.0_f32);
            let mut i = F::new(0.0_f32);
            for t in begin..end {
                let axis = src.edge_axis[t];
                let id = src.edge_id[t] as usize;
                let old = src.old[t];
                let curl = (get(&e, axis, id) - old) / get(&c, axis, id);
                let updated = src.k[3 * t] * old + src.k[3 * t + 1] * curl - src.k[3 * t + 2] * vs;
                set(&mut e, axis, id, updated);
                let e_avg = half * (old + updated);
                i += (vs + step * e_avg) / r_edge * sign;
                v += -step * e_avg * sign;
            }
            let base = 2 * ((slot as usize) * ports + p);
            record[base] = v;
            record[base + 1] = i / F::cast_from(end - begin);
        }
        for q in 0..src.currents as usize {
            let axis = src.current_axis[q];
            let id = src.current_id[q] as usize;
            let value = get(&e, axis, id) - src.current[q] * s;
            set(&mut e, axis, id, value);
            if comptime![dispersive] {
                follow(&mut poles, axis, id, -(src.current[q] * s));
            }
        }
    }
}

/// The Huygens box's samples: each one's axis, plane and cells across.
#[derive(CubeLaunch, CubeType)]
pub(crate) struct Samples {
    axis: Box<[u32]>,
    plane: Box<[u32]>,
    iu: Box<[u32]>,
    iv: Box<[u32]>,
    count: u32,
}

/// The running transforms: per frequency, per sample, (re, im) of x, y, z, for E and H; and
/// the batch's phasors Δt e^{−jωt}, per step, per frequency, (re, im) at E's time then H's
/// (made on the host, as the threads make them).
#[derive(CubeLaunch, CubeType)]
pub(crate) struct Transforms {
    phasors: Box<[f64]>,
    e: Box<[f64]>,
    h: Box<[f64]>,
    frequencies: u32,
}

/// The index of (i, j, k) on component `comp`'s E lattice (`h` false) or H lattice.
#[cube]
fn index(comp: u32, h: bool, nx: usize, ny: usize, i: usize, j: usize, k: usize) -> usize {
    let on = if h { 1usize } else { 0usize };
    let off = 1usize - on;
    let d0 = nx + if comp == 0 { on } else { off };
    let d1 = ny + if comp == 1 { on } else { off };
    at(d0, d1, i, j, k)
}

/// One along `axis`.
#[cube]
fn unit(axis: u32, along: u32) -> usize {
    if axis == along { 1usize } else { 0usize }
}

/// Adds the tangential E (at t = (step + 1) Δt) and H (at (step + ½) Δt) of the batch's step
/// `slot` at each
/// sample of the box to the transforms (one unit per sample): E_u averaged over v, v + 1 and
/// E_v over u, u + 1 at the face centre's node; H_u over u, u + 1 and a − ½, a + ½, H_v over v,
/// v + 1 and the same.
#[cube(launch)]
pub(crate) fn transform<F: Real>(e: Xyz<F>, h: Xyz<F>, samples: Samples, mut tr: Transforms, slot: u32, n: Cells) {
    let s = ABSOLUTE_POS;
    let count = samples.count as usize;
    if s < count {
        let nx = n.nx as usize;
        let ny = n.ny as usize;
        let a = samples.axis[s];
        let u = (a + 1) % 3;
        let v = (a + 2) % 3;
        let plane = samples.plane[s] as usize;
        let iu = samples.iu[s] as usize;
        let iv = samples.iv[s] as usize;
        // (i, j, k) of (plane along a, iu along u, iv along v).
        let i = if a == 0 {
            plane
        } else if u == 0 {
            iu
        } else {
            iv
        };
        let j = if a == 1 {
            plane
        } else if u == 1 {
            iu
        } else {
            iv
        };
        let k = if a == 2 {
            plane
        } else if u == 2 {
            iu
        } else {
            iv
        };
        let half = F::new(0.5_f32);
        let quarter = F::new(0.25_f32);
        let eu = half
            * (get(&e, u, index(u, false, nx, ny, i, j, k)) + get(&e, u, index(u, false, nx, ny, i + unit(v, 0), j + unit(v, 1), k + unit(v, 2))));
        let ev = half
            * (get(&e, v, index(v, false, nx, ny, i, j, k)) + get(&e, v, index(v, false, nx, ny, i + unit(u, 0), j + unit(u, 1), k + unit(u, 2))));
        // H a half cell below the plane, then the four positions straddling the face centre.
        let hi = i - unit(a, 0);
        let hj = j - unit(a, 1);
        let hk = k - unit(a, 2);
        let mut hu = F::new(0.0_f32);
        let mut hv = F::new(0.0_f32);
        for mask in 0..4u32 {
            let bu = mask & 1;
            let ba = (mask >> 1) & 1;
            let ou = if bu == 1 { 1usize } else { 0usize };
            let oa = if ba == 1 { 1usize } else { 0usize };
            let oi = ou * unit(u, 0) + oa * unit(a, 0);
            let oj = ou * unit(u, 1) + oa * unit(a, 1);
            let ok = ou * unit(u, 2) + oa * unit(a, 2);
            hu += get(&h, u, index(u, true, nx, ny, hi + oi, hj + oj, hk + ok));
            let wi = ou * unit(v, 0) + oa * unit(a, 0);
            let wj = ou * unit(v, 1) + oa * unit(a, 1);
            let wk = ou * unit(v, 2) + oa * unit(a, 2);
            hv += get(&h, v, index(v, true, nx, ny, hi + wi, hj + wj, hk + wk));
        }
        hu *= quarter;
        hv *= quarter;
        let eu = f64::cast_from(eu);
        let ev = f64::cast_from(ev);
        let hu = f64::cast_from(hu);
        let hv = f64::cast_from(hv);
        let freqs = tr.frequencies as usize;
        for fi in 0..freqs {
            let q = 4 * ((slot as usize) * freqs + fi);
            let ce = tr.phasors[q];
            let se = tr.phasors[q + 1];
            let chh = tr.phasors[q + 2];
            let sh = tr.phasors[q + 3];
            let base = 6 * (fi * count + s);
            let pu = base + 2 * (u as usize);
            let pv = base + 2 * (v as usize);
            tr.e[pu] += eu * ce;
            tr.e[pu + 1] += eu * se;
            tr.e[pv] += ev * ce;
            tr.e[pv + 1] += ev * se;
            tr.h[pu] += hu * chh;
            tr.h[pu + 1] += hu * sh;
            tr.h[pv] += hv * chh;
            tr.h[pv + 1] += hv * sh;
        }
    }
}
