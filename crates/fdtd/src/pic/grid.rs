//! The particle-in-cell grid: uniform cells, each axis periodic or closed by conducting walls,
//! and the Yee lattices on it — E on the cells' edges, B on their faces, charge on the nodes.

use crate::{FdtdError, MAX_CELLS, Result, invalid};

/// How an axis ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wall {
    /// What leaves through one end comes back through the other.
    Periodic,
    /// Perfectly conducting walls at both ends: E tangential to them is zero, particles reaching
    /// them are absorbed.
    Conductor,
}

/// One axis of the grid: `cells` of `h` (m), ended by `wall`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Axis {
    pub cells: usize,
    pub h: f64,
    pub wall: Wall,
}

impl Axis {
    /// Its length (m).
    pub fn length(self) -> f64 {
        self.cells as f64 * self.h
    }

    /// The points of a lattice along it: at the nodes or at the cells' centres. A periodic axis
    /// has as many of either as cells (its last node is its first); between walls the nodes
    /// include both walls.
    pub(crate) fn points(self, node: bool) -> usize {
        if node && self.wall == Wall::Conductor { self.cells + 1 } else { self.cells }
    }

    /// Point `i` of a lattice along it: wrapped round a periodic axis, `None` past a wall.
    #[inline]
    pub(crate) fn wrap(self, i: isize, node: bool) -> Option<usize> {
        let n = self.points(node) as isize;
        match self.wall {
            Wall::Periodic => Some(i.rem_euclid(n) as usize),
            Wall::Conductor => (0..n).contains(&i).then_some(i as usize),
        }
    }

    /// Point `i` of a lattice along it, the nearest one past a wall.
    #[inline]
    pub(crate) fn clamp(self, i: isize, node: bool) -> usize {
        let n = self.points(node) as isize;
        match self.wall {
            Wall::Periodic => i.rem_euclid(n) as usize,
            Wall::Conductor => i.clamp(0, n - 1) as usize,
        }
    }

    /// Whether node `i` lies on a conducting wall.
    #[inline]
    pub(crate) fn on_wall(self, i: usize) -> bool {
        self.wall == Wall::Conductor && (i == 0 || i == self.cells)
    }
}

/// A lattice of the grid: which axes it lies on the nodes of (else on the cells' centres), and
/// its points along each, row-major in x.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Lattice {
    pub(crate) node: [bool; 3],
    pub(crate) dims: [usize; 3],
}

impl Lattice {
    pub(crate) fn len(self) -> usize {
        self.dims.iter().product()
    }

    /// The index of point `p`.
    #[inline]
    pub(crate) fn at(self, p: [usize; 3]) -> usize {
        p[0] + self.dims[0] * (p[1] + self.dims[1] * p[2])
    }

    /// Its points, in the order of their indices.
    pub(crate) fn points(self) -> impl Iterator<Item = [usize; 3]> {
        let [nx, ny, nz] = self.dims;
        (0..nz).flat_map(move |k| (0..ny).flat_map(move |j| (0..nx).map(move |i| [i, j, k])))
    }
}

/// The grid: three axes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    pub axes: [Axis; 3],
}

impl Grid {
    /// The grid of `axes`, checked: at least a cell of a finite size > 0 along each, at most
    /// [`MAX_CELLS`] in all.
    ///
    /// ```
    /// use emcraft_fdtd::pic::{Axis, Grid, Wall};
    /// let x = Axis { cells: 8, h: 1e-3, wall: Wall::Periodic };
    /// let z = Axis { cells: 20, h: 5e-4, wall: Wall::Conductor };
    /// let g = Grid::new([x, x, z])?;
    /// assert_eq!(g.cells(), 8 * 8 * 20);
    /// assert!(Grid::new([x, x, Axis { cells: 0, ..z }]).is_err());
    /// # Ok::<(), emcraft_fdtd::FdtdError>(())
    /// ```
    pub fn new(axes: [Axis; 3]) -> Result<Grid> {
        for (d, a) in axes.iter().enumerate() {
            if a.cells == 0 || !(a.h.is_finite() && a.h > 0.0) {
                return Err(invalid("grid", format!("axis {d} needs at least one cell of a finite size > 0")));
            }
        }
        let cells = axes.iter().try_fold(1usize, |n, a| n.checked_mul(a.cells)).unwrap_or(usize::MAX);
        if cells > MAX_CELLS {
            return Err(FdtdError::TooLarge { cells, limit: MAX_CELLS });
        }
        Ok(Grid { axes })
    }

    pub fn cells(&self) -> usize {
        self.axes.iter().map(|a| a.cells).product()
    }

    /// A cell's volume (m³).
    pub fn cell_volume(&self) -> f64 {
        self.axes.iter().map(|a| a.h).product()
    }

    /// The box's lengths (m).
    pub fn lengths(&self) -> [f64; 3] {
        self.axes.map(Axis::length)
    }

    /// The lattice on the nodes of `node`'s axes.
    pub(crate) fn lattice(&self, node: [bool; 3]) -> Lattice {
        Lattice { node, dims: [0, 1, 2].map(|d| self.axes[d].points(node[d])) }
    }

    /// E_a's lattice: the edges along a, at the cells' centres along a and the nodes across.
    pub(crate) fn e(&self, a: usize) -> Lattice {
        self.lattice([0, 1, 2].map(|d| d != a))
    }

    /// B_a's lattice: the faces normal to a, at the nodes along a and the centres across.
    pub(crate) fn b(&self, a: usize) -> Lattice {
        self.lattice([0, 1, 2].map(|d| d == a))
    }

    /// The nodes, where the charge lies.
    pub(crate) fn nodes(&self) -> Lattice {
        self.lattice([true; 3])
    }

    /// The position (m) of point `p` of `lattice`.
    pub(crate) fn position(&self, lattice: Lattice, p: [usize; 3]) -> [f64; 3] {
        [0, 1, 2].map(|d| (p[d] as f64 + if lattice.node[d] { 0.0 } else { 0.5 }) * self.axes[d].h)
    }

    /// Whether point `p` of E_a's lattice lies on a conducting wall it is tangential to.
    pub(crate) fn e_on_wall(&self, a: usize, p: [usize; 3]) -> bool {
        (0..3).any(|d| d != a && self.axes[d].on_wall(p[d]))
    }

    /// The largest stable time step (s) times `courant`: c Δt √(Σ 1/h²) = courant.
    pub fn time_step(&self, courant: f64) -> f64 {
        crate::yee::time_step(courant, self.axes.map(|a| a.h))
    }

    /// The value of `values` on `lattice` at `x` (m), interpolated linearly along each axis
    /// (the particles' first-order shape); past a wall, the nearest point's.
    pub(crate) fn interpolate(&self, lattice: Lattice, values: &[f64], x: [f64; 3]) -> f64 {
        let mut ids = [[0usize; 2]; 3];
        let mut ws = [[0.0f64; 2]; 3];
        for d in 0..3 {
            let a = self.axes[d];
            let s = x[d] / a.h - if lattice.node[d] { 0.0 } else { 0.5 };
            let i0 = s.floor();
            let f = s - i0;
            let i0 = i0 as isize;
            ids[d] = [a.clamp(i0, lattice.node[d]), a.clamp(i0 + 1, lattice.node[d])];
            ws[d] = [1.0 - f, f];
        }
        let mut sum = 0.0;
        for k in 0..2 {
            for j in 0..2 {
                for i in 0..2 {
                    let id = lattice.at([ids[0][i], ids[1][j], ids[2][k]]);
                    sum += ws[0][i] * ws[1][j] * ws[2][k] * values.get(id).copied().unwrap_or(0.0);
                }
            }
        }
        sum
    }
}
