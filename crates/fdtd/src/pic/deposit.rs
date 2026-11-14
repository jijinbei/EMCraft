//! A particle's charge and current on the grid, first-order shapes.
//!
//! **Charge.** q on the 8 nodes around the particle, weighted by S(x/h − i) = max(0, 1 − |x/h − i|)
//! along each axis, over the cell's volume.
//!
//! **Current: Esirkepov's scheme** (T. Zh. Esirkepov, *Exact charge conservation scheme for
//! particle-in-cell simulation with an arbitrary form-factor*, Comput. Phys. Commun.
//! 135:144–153, 2001, eqs. 23–31). A particle moving from x₀ to x₁ within a step (less than a cell
//! along each axis, which the Courant limit ensures) changes its node weights from S⁰ to S¹. The
//! change is split into three parts,
//!
//! W_x = ΔS_x (S⁰_y S⁰_z + ½ ΔS_y S⁰_z + ½ S⁰_y ΔS_z + ⅓ ΔS_y ΔS_z), and W_y, W_z alike,
//!
//! which sum to S¹_x S¹_y S¹_z − S⁰_x S⁰_y S⁰_z, and the current on the edges along x is their
//! running sum, J_x(i + ½) = J_x(i − ½) − q W_x(i)/(Δt h_y h_z): its divergence is exactly minus
//! the change of the charge, so the continuity equation — and with it Gauss's law — holds at
//! every node. A particle's weights before and after lie on the 4 nodes from the one below its
//! old cell to two above it, along each axis.

use super::grid::Grid;

/// The first-order shape: the weight on a node `s` cells away.
#[inline]
fn shape(s: f64) -> f64 {
    (1.0 - s.abs()).max(0.0)
}

/// The nodes around a move along one axis: the first (the node below the old cell), and the
/// weights before and after on it and the three above.
struct Window {
    first: isize,
    before: [f64; 4],
    after: [f64; 4],
}

impl Window {
    fn of(x0: f64, x1: f64, h: f64) -> Window {
        let (s0, s1) = (x0 / h, x1 / h);
        let first = s0.floor() as isize - 1;
        let at = |s: f64| std::array::from_fn(|m| shape(s - (first + m as isize) as f64));
        Window { first, before: at(s0), after: at(s1) }
    }

    fn change(&self) -> [f64; 4] {
        std::array::from_fn(|m| self.after[m] - self.before[m])
    }
}

/// Adds the current of `charge` (C) moving from `x0` to `x1` (m, unwrapped across periodic
/// faces) in `dt` (s) to `j` (A/m², on E's lattices).
pub(crate) fn current(grid: &Grid, j: &mut [Vec<f64>; 3], x0: [f64; 3], x1: [f64; 3], charge: f64, dt: f64) {
    let w: [Window; 3] = std::array::from_fn(|d| Window::of(x0[d], x1[d], grid.axes[d].h));
    let ds = w.each_ref().map(Window::change);
    for a in 0..3 {
        let (b, c) = ((a + 1) % 3, (a + 2) % 3);
        let (axis, lattice) = (grid.axes, grid.e(a));
        let factor = -charge / (dt * axis[b].h * axis[c].h);
        for mb in 0..4 {
            for mc in 0..4 {
                let (s0b, s0c, dsb, dsc) = (w[b].before[mb], w[c].before[mc], ds[b][mb], ds[c][mc]);
                let across = s0b * s0c + 0.5 * dsb * s0c + 0.5 * s0b * dsc + dsb * dsc / 3.0;
                if across == 0.0 {
                    continue;
                }
                let (Some(pb), Some(pc)) = (axis[b].wrap(w[b].first + mb as isize, true), axis[c].wrap(w[c].first + mc as isize, true)) else {
                    continue;
                };
                let mut sum = 0.0;
                // The edge above node m along a; the last one's sum is zero (Σ ΔS = 0).
                for ma in 0..3 {
                    sum += factor * ds[a][ma] * across;
                    let Some(pa) = axis[a].wrap(w[a].first + ma as isize, false) else { continue };
                    let mut p = [0usize; 3];
                    (p[a], p[b], p[c]) = (pa, pb, pc);
                    if let Some(v) = j[a].get_mut(lattice.at(p)) {
                        *v += sum;
                    }
                }
            }
        }
    }
}

/// Adds the charge density (C/m³) of `charge` (C) at `x` (m) to `rho` on the nodes.
pub(crate) fn charge(grid: &Grid, rho: &mut [f64], x: [f64; 3], charge: f64) {
    let nodes = grid.nodes();
    let q = charge / grid.cell_volume();
    let mut ids = [[None; 2]; 3];
    let mut ws = [[0.0; 2]; 3];
    for d in 0..3 {
        let s = x[d] / grid.axes[d].h;
        let i0 = s.floor();
        ids[d] = [grid.axes[d].wrap(i0 as isize, true), grid.axes[d].wrap(i0 as isize + 1, true)];
        ws[d] = [1.0 - (s - i0), s - i0];
    }
    for k in 0..2 {
        for j in 0..2 {
            for i in 0..2 {
                let (Some(pi), Some(pj), Some(pk)) = (ids[0][i], ids[1][j], ids[2][k]) else { continue };
                if let Some(v) = rho.get_mut(nodes.at([pi, pj, pk])) {
                    *v += q * ws[0][i] * ws[1][j] * ws[2][k];
                }
            }
        }
    }
}
