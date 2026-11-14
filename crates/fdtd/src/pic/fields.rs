//! The fields' leapfrog, plane by plane on the threads (rayon): Faraday's B from E and Ampère's
//! E from B and J (K. S. Yee 1966), through the absorber's layers (each difference across one
//! ∂ + ψ) and the media's edges (metal held at zero, a dielectric's εr). Each value is written
//! by one thread from values the half step does not change, in the same order whatever the
//! threads, so a run gives the same bits on any number of them.

use emcraft_model::units::{C0, EPS0};
use rayon::prelude::*;

use super::absorb::{self, Absorber};
use super::grid::{Grid, Lattice};
use super::media::Media;
use crate::yee::Cpml;

/// A difference `d` along `axis` at point `p` of a lattice through the layers' coefficients
/// `set`: d + ψ, ψ advanced in place.
#[inline]
fn stretch(set: Option<&[[Vec<Cpml>; 2]; 3]>, psi: Option<&mut f64>, (axis, node, i): (usize, bool, usize), d: f64) -> f64 {
    match (set, psi) {
        (Some(set), Some(psi)) => {
            let c = absorb::at(set, axis, node, i);
            if c == Cpml::NONE {
                return d;
            }
            *psi = c.psi(*psi, d);
            d + *psi
        }
        _ => d,
    }
}

/// `work(p, value, ψ)` at each point of `lattice`, on the threads by planes of z, over `values`
/// and, with an absorber, the point's two ψ.
fn by_planes(
    lattice: Lattice,
    values: &mut [f64],
    psi: Option<&mut [Vec<f64>; 2]>,
    work: impl Fn([usize; 3], &mut f64, [Option<&mut f64>; 2]) + Sync,
) {
    let plane = (lattice.dims[0] * lattice.dims[1]).max(1);
    let point = |k: usize, n: usize| [n % lattice.dims[0], n / lattice.dims[0], k];
    match psi {
        Some([p0, p1]) => {
            values.par_chunks_mut(plane).zip(p0.par_chunks_mut(plane)).zip(p1.par_chunks_mut(plane)).enumerate().for_each(|(k, ((v, s0), s1))| {
                for (n, ((v, s0), s1)) in v.iter_mut().zip(s0.iter_mut()).zip(s1.iter_mut()).enumerate() {
                    work(point(k, n), v, [Some(s0), Some(s1)]);
                }
            })
        }
        None => values.par_chunks_mut(plane).enumerate().for_each(|(k, v)| {
            for (n, v) in v.iter_mut().enumerate() {
                work(point(k, n), v, [None, None]);
            }
        }),
    }
}

/// B advanced by `dt` (s): ∂B/∂t = −∇ × E, on every face (those normal to a wall keep the zero
/// they start with, the tangential E about them being zero).
pub(crate) fn faraday(grid: &Grid, e: &[Vec<f64>; 3], b: &mut [Vec<f64>; 3], dt: f64, absorber: Option<&mut Absorber>) {
    let axes = grid.axes;
    let (set, mut psi) = match absorber {
        Some(ab) => (Some(&ab.b), Some(&mut ab.psi_b)),
        None => (None, None),
    };
    for (a, values) in b.iter_mut().enumerate() {
        let (q, r) = ((a + 1) % 3, (a + 2) % 3);
        let (lattice, eq, er) = (grid.b(a), grid.e(q), grid.e(r));
        by_planes(lattice, values, psi.as_mut().map(|p| &mut p[a]), |p, value, [s0, s1]| {
            // ∂_q E_r − ∂_r E_q, E's nodes p and p + 1 along q and r.
            let (Some(nq), Some(nr)) = (axes[q].wrap(p[q] as isize + 1, true), axes[r].wrap(p[r] as isize + 1, true)) else { return };
            let (mut pq, mut pr) = (p, p);
            (pq[q], pr[r]) = (nq, nr);
            let dq = (e[r][er.at(pq)] - e[r][er.at(p)]) / axes[q].h;
            let dr = (e[q][eq.at(pr)] - e[q][eq.at(p)]) / axes[r].h;
            let dq = stretch(set, s0, (q, lattice.node[q], p[q]), dq);
            let dr = stretch(set, s1, (r, lattice.node[r], p[r]), dr);
            *value -= dt * (dq - dr);
        });
    }
}

/// E advanced by `dt` (s): ∂E/∂t = (c²∇ × B − J/ε₀)/εr, off the walls it is tangential to and
/// the edges metal holds at zero.
pub(crate) fn ampere(
    grid: &Grid,
    e: &mut [Vec<f64>; 3],
    (b, j): (&[Vec<f64>; 3], &[Vec<f64>; 3]),
    dt: f64,
    (media, absorber): (Option<&Media>, Option<&mut Absorber>),
) {
    let axes = grid.axes;
    let (set, mut psi) = match absorber {
        Some(ab) => (Some(&ab.e), Some(&mut ab.psi_e)),
        None => (None, None),
    };
    for (a, values) in e.iter_mut().enumerate() {
        let (q, r) = ((a + 1) % 3, (a + 2) % 3);
        let (lattice, bq, br) = (grid.e(a), grid.b(q), grid.b(r));
        by_planes(lattice, values, psi.as_mut().map(|p| &mut p[a]), |p, value, [s0, s1]| {
            if grid.e_on_wall(a, p) {
                return;
            }
            // ∂_q B_r − ∂_r B_q, B's centres p − ½ and p + ½ along q and r.
            let (Some(mq), Some(mr)) = (axes[q].wrap(p[q] as isize - 1, false), axes[r].wrap(p[r] as isize - 1, false)) else { return };
            let (mut pq, mut pr) = (p, p);
            (pq[q], pr[r]) = (mq, mr);
            let dq = (b[r][br.at(p)] - b[r][br.at(pq)]) / axes[q].h;
            let dr = (b[q][bq.at(p)] - b[q][bq.at(pr)]) / axes[r].h;
            let dq = stretch(set, s0, (q, lattice.node[q], p[q]), dq);
            let dr = stretch(set, s1, (r, lattice.node[r], p[r]), dr);
            let id = lattice.at(p);
            if media.is_some_and(|m| m.edge(a, id)) {
                return;
            }
            let eps = media.map_or(1.0, |m| m.eps(a, id));
            *value += dt / eps * (C0 * C0 * (dq - dr) - j[a][id] / EPS0);
        });
    }
}
