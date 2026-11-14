//! Conducting bodies inside the box, as staircases of cells (docs/adr/0012-particles.md, stage
//! C): a cell is metal when its centre lies in a conductor. E on an edge touching a metal cell
//! is held at zero (the tangential field on the staircase's faces, and none inside), a particle
//! that ends a step in a metal cell is absorbed there, and Gauss's law is checked only at nodes
//! touching no metal (A. Taflove, S. C. Hagness, *Computational Electrodynamics*, 3rd ed., 2005,
//! §3.6, a perfect conductor's cells).

use emcraft_particles::V3;

use super::grid::{Grid, Lattice};

/// The metal cells and the E edges they hold at zero.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Metal {
    cells: Vec<bool>,
    lattice: Lattice,
    edges: [Vec<bool>; 3],
}

impl Metal {
    /// The cells of `grid` whose centres `metal` says are in a conductor.
    pub(crate) fn new(grid: &Grid, metal: impl Fn(V3) -> bool) -> Metal {
        let lattice = grid.lattice([false; 3]);
        let cells: Vec<bool> = lattice.points().map(|p| metal(grid.position(lattice, p))).collect();
        let edges = [0, 1, 2].map(|a| {
            let e = grid.e(a);
            e.points().map(|p| Metal::touches(grid, lattice, &cells, a, p)).collect()
        });
        Metal { cells, lattice, edges }
    }

    /// Whether any cell around edge `p` of E_a is metal: cell p along a, the two either side of
    /// the edge's node across (past a wall, none).
    fn touches(grid: &Grid, lattice: Lattice, cells: &[bool], a: usize, p: [usize; 3]) -> bool {
        let (b, c) = ((a + 1) % 3, (a + 2) % 3);
        let around = |d: usize| [p[d] as isize - 1, p[d] as isize].map(|i| grid.axes[d].wrap(i, false));
        around(b).into_iter().flatten().any(|ib| {
            around(c).into_iter().flatten().any(|ic| {
                let mut q = p;
                (q[b], q[c]) = (ib, ic);
                cells.get(lattice.at(q)).copied().unwrap_or(false)
            })
        })
    }

    /// Whether any cell is metal.
    pub(crate) fn any(&self) -> bool {
        self.cells.contains(&true)
    }

    /// Whether edge `id` of E_a is held at zero.
    #[inline]
    pub(crate) fn edge(&self, a: usize, id: usize) -> bool {
        self.edges.get(a).and_then(|e| e.get(id)).copied().unwrap_or(false)
    }

    /// Whether the cell holding `x` (m) is metal.
    pub(crate) fn at(&self, grid: &Grid, x: V3) -> bool {
        let mut p = [0usize; 3];
        for d in 0..3 {
            let a = grid.axes[d];
            p[d] = a.clamp((x[d] / a.h).floor() as isize, false);
        }
        self.cells.get(self.lattice.at(p)).copied().unwrap_or(false)
    }

    /// Whether any of the 8 cells around node `p` is metal.
    pub(crate) fn near_node(&self, grid: &Grid, p: [usize; 3]) -> bool {
        let around = |d: usize| [p[d] as isize - 1, p[d] as isize].map(|i| grid.axes[d].wrap(i, false));
        around(0).into_iter().flatten().any(|i| {
            around(1)
                .into_iter()
                .flatten()
                .any(|j| around(2).into_iter().flatten().any(|k| self.cells.get(self.lattice.at([i, j, k])).copied().unwrap_or(false)))
        })
    }
}
