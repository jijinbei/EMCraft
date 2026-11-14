//! Open faces: a convolutional perfectly matched layer inside a conducting wall
//! (J. A. Roden, S. D. Gedney, *Convolution PML (CPML): an efficient FDTD implementation of the
//! CFS–PML for arbitrary media*, Microw. Opt. Technol. Lett. 27(5):334–339, 2000; the grading
//! the time-domain solver's guides use, `crate::yee::Grading`). Within `cells` of a wall each
//! difference ∂_d across the layer becomes ∂_d + ψ, ψⁿ⁺¹ = b ψⁿ + a ∂_d, the coefficients graded
//! by the depth into the layer: what reaches it is absorbed before the wall behind it reflects
//! it, so the box is open on that axis. Particles go on through the layer to the wall and are
//! absorbed there, their current deposited all the way (the charge stays conserved; Gauss's law
//! is not kept inside the layer, whose curl is not the grid's).

use emcraft_model::units::C0;

use super::grid::{Grid, Lattice, Wall};
use crate::yee::{Cpml, Grading};
use crate::{Result, invalid};

/// The most cells a layer may be.
pub const MAX_LAYER: usize = 64;

/// The layers and their auxiliary variables.
pub(crate) struct Absorber {
    /// The layer's cells at each end of each axis, low and high (0: none).
    cells: [[usize; 2]; 3],
    /// Per axis, the coefficients at its nodes and at its cells' centres, for E's whole step
    /// and B's half steps.
    e: [[Vec<Cpml>; 2]; 3],
    b: [[Vec<Cpml>; 2]; 3],
    /// ψ of E_a's two differences (along a + 1 and a + 2) and of B_a's.
    pub(crate) psi_e: [[Vec<f64>; 2]; 3],
    pub(crate) psi_b: [[Vec<f64>; 2]; 3],
}

impl Absorber {
    /// No layers yet on `grid`, its variables allocated.
    pub(crate) fn new(grid: &Grid) -> Result<Absorber> {
        let field = |l: Lattice| -> Result<Vec<f64>> { super::zeros(l.len()) };
        let pair = |l: Lattice| -> Result<[Vec<f64>; 2]> { Ok([field(l)?, field(l)?]) };
        let none = |n: usize| vec![Cpml::NONE; n];
        let [ex, ey, ez] = [0, 1, 2].map(|a| pair(grid.e(a)));
        let [bx, by, bz] = [0, 1, 2].map(|a| pair(grid.b(a)));
        let coefficients = |d: usize| [none(grid.axes[d].points(true)), none(grid.axes[d].points(false))];
        Ok(Absorber {
            cells: [[0; 2]; 3],
            e: [0, 1, 2].map(coefficients),
            b: [0, 1, 2].map(coefficients),
            psi_e: [ex?, ey?, ez?],
            psi_b: [bx?, by?, bz?],
        })
    }

    /// A layer of `cells` inside the conducting wall of `axis` at its `high` end or its low one,
    /// for steps of `dt` (s).
    pub(crate) fn layer(&mut self, grid: &Grid, axis: usize, high: bool, cells: usize, dt: f64) -> Result<()> {
        let a = grid.axes.get(axis).copied().ok_or_else(|| invalid("absorber", format!("axis {axis}")))?;
        let other = self.cells[axis][usize::from(!high)];
        if a.wall != Wall::Conductor || cells == 0 || cells > MAX_LAYER || cells + other >= a.cells {
            return Err(invalid("absorber", format!("a layer of 1 to {MAX_LAYER} cells inside a conducting wall, the axis longer than its layers")));
        }
        self.cells[axis][usize::from(high)] = cells;
        let [lo, hi] = self.cells[axis].map(|n| n as f64 * a.h);
        let length = a.length();
        // The complex frequency shift at the box's own lowest frequencies.
        let fc = C0 / length;
        let profile = |grading: &Grading, node: bool| -> Vec<Cpml> {
            (0..a.points(node))
                .map(|i| {
                    let x = (i as f64 + if node { 0.0 } else { 0.5 }) * a.h;
                    let low = if lo > 0.0 { (lo - x) / lo } else { 0.0 };
                    let up = if hi > 0.0 { (x - (length - hi)) / hi } else { 0.0 };
                    grading.at(low.max(up).max(0.0))
                })
                .collect()
        };
        let (whole, half) = (Grading::new(a.h, dt, fc), Grading::new(a.h, 0.5 * dt, fc));
        self.e[axis] = [profile(&whole, true), profile(&whole, false)];
        self.b[axis] = [profile(&half, true), profile(&half, false)];
        Ok(())
    }

    /// Whether node `p` lies in a layer.
    pub(crate) fn inside(&self, grid: &Grid, p: [usize; 3]) -> bool {
        (0..3).any(|d| {
            let [lo, hi] = self.cells[d];
            (lo > 0 && p[d] <= lo) || (hi > 0 && p[d] + hi >= grid.axes[d].cells)
        })
    }

    /// The coefficients of a difference along `d` at point `i` of an E lattice (`b`: of a B
    /// lattice), node or centre along `d`.
    #[inline]
    pub(crate) fn at(&self, field_b: bool, d: usize, node: bool, i: usize) -> Cpml {
        let set = if field_b { &self.b } else { &self.e };
        set[d][usize::from(!node)].get(i).copied().unwrap_or(Cpml::NONE)
    }
}
