//! What fills the box's cells (docs/adr/0012-particles.md, stage C): vacuum, a lossless
//! dielectric or a perfect conductor, each cell as its centre lies, the bodies as staircases of
//! cells (A. Taflove, S. C. Hagness, *Computational Electrodynamics*, 3rd ed., 2005, §3.6 and
//! §3.11).
//!
//! **Conductors.** E on an edge touching a metal cell is held at zero (the tangential field on
//! the staircase's faces, and none inside); a particle that ends a step in a metal cell is
//! absorbed there; Gauss's law is checked only at nodes touching no metal.
//!
//! **Dielectrics.** An edge takes the mean relative permittivity of the four cells around it
//! (their areas across it are equal on the uniform grid), and Ampère's law reads
//! ε₀εr ∂E/∂t = ∇ × B/μ₀ − J there; the charge then keeps ∇·(ε₀εr E) = ρ.

use emcraft_particles::V3;

use super::grid::{Grid, Lattice};

/// What fills a cell.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Medium {
    /// A perfect conductor.
    Metal,
    /// A lossless dielectric of this relative permittivity (1: vacuum).
    Dielectric(f64),
}

/// The cells' media and what they make of the E edges.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Media {
    metal: Vec<bool>,
    lattice: Lattice,
    edges: [Vec<bool>; 3],
    eps: [Vec<f64>; 3],
}

/// The cells around edge `p` of E_a: cell p along a, the two either side of its node across
/// (past a wall, none).
fn around_edge(grid: &Grid, a: usize, p: [usize; 3]) -> impl Iterator<Item = [usize; 3]> + '_ {
    let (b, c) = ((a + 1) % 3, (a + 2) % 3);
    let around = move |d: usize| [p[d] as isize - 1, p[d] as isize].map(|i| grid.axes[d].wrap(i, false));
    around(b).into_iter().flatten().flat_map(move |ib| {
        around(c).into_iter().flatten().map(move |ic| {
            let mut q = p;
            (q[b], q[c]) = (ib, ic);
            q
        })
    })
}

impl Media {
    /// The cells of `grid` filled as `medium` says at their centres (m); a permittivity below 1
    /// or not a number is vacuum's.
    pub(crate) fn new(grid: &Grid, medium: impl Fn(V3) -> Medium) -> Media {
        let lattice = grid.lattice([false; 3]);
        let cells: Vec<Medium> = lattice.points().map(|p| medium(grid.position(lattice, p))).collect();
        let metal: Vec<bool> = cells.iter().map(|m| *m == Medium::Metal).collect();
        let eps_of = |m: &Medium| match *m {
            Medium::Dielectric(e) if e.is_finite() && e >= 1.0 => e,
            _ => 1.0,
        };
        let edges = [0, 1, 2]
            .map(|a| grid.e(a).points().map(|p| around_edge(grid, a, p).any(|q| metal.get(lattice.at(q)).copied().unwrap_or(false))).collect());
        let eps = [0, 1, 2].map(|a| {
            grid.e(a)
                .points()
                .map(|p| {
                    let (sum, n) = around_edge(grid, a, p).fold((0.0, 0usize), |(s, n), q| (s + cells.get(lattice.at(q)).map_or(1.0, eps_of), n + 1));
                    if n > 0 { sum / n as f64 } else { 1.0 }
                })
                .collect()
        });
        Media { metal, lattice, edges, eps }
    }

    /// Whether any cell is metal or a dielectric.
    pub(crate) fn any(&self) -> bool {
        self.metal.contains(&true) || self.eps.iter().flatten().any(|e| *e != 1.0)
    }

    /// Whether edge `id` of E_a is held at zero.
    #[inline]
    pub(crate) fn edge(&self, a: usize, id: usize) -> bool {
        self.edges.get(a).and_then(|e| e.get(id)).copied().unwrap_or(false)
    }

    /// The relative permittivity on edge `id` of E_a.
    #[inline]
    pub(crate) fn eps(&self, a: usize, id: usize) -> f64 {
        self.eps.get(a).and_then(|e| e.get(id)).copied().unwrap_or(1.0)
    }

    /// Whether the cell holding `x` (m) is metal.
    pub(crate) fn at(&self, grid: &Grid, x: V3) -> bool {
        let mut p = [0usize; 3];
        for d in 0..3 {
            let a = grid.axes[d];
            p[d] = a.clamp((x[d] / a.h).floor() as isize, false);
        }
        self.metal.get(self.lattice.at(p)).copied().unwrap_or(false)
    }

    /// Whether any of the 8 cells around node `p` is metal.
    pub(crate) fn near_node(&self, grid: &Grid, p: [usize; 3]) -> bool {
        let around = |d: usize| [p[d] as isize - 1, p[d] as isize].map(|i| grid.axes[d].wrap(i, false));
        around(0).into_iter().flatten().any(|i| {
            around(1)
                .into_iter()
                .flatten()
                .any(|j| around(2).into_iter().flatten().any(|k| self.metal.get(self.lattice.at([i, j, k])).copied().unwrap_or(false)))
        })
    }
}
