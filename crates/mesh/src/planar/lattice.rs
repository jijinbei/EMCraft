//! The lattice the planar mesh's points sit on: whole multiples of [`STEP`] from the region's
//! lower corner. On it every coordinate is a whole number, exact in a double, so the same input
//! gives the same points bit for bit, and the predicates decide on exact values.

/// The lattice's step: 1 nm, far below any copper feature and any element.
pub const STEP: f64 = 1e-9;

/// The longest side a region may have: 1 m, 10⁹ steps (whole numbers stay exact through the
/// predicates' products).
pub const MAX_SIDE: f64 = 1.0;

/// Lattice coordinates about an origin (metres in, whole numbers of steps out).
#[derive(Clone, Copy, Debug)]
pub(super) struct Lattice {
    origin: [f64; 2],
}

impl Lattice {
    pub fn new(origin: [f64; 2]) -> Self {
        Lattice { origin }
    }

    /// The lattice point nearest `p` (metres).
    pub fn on(&self, p: [f64; 2]) -> [f64; 2] {
        [((p[0] - self.origin[0]) / STEP).round(), ((p[1] - self.origin[1]) / STEP).round()]
    }

    /// Lattice point `q` in metres.
    pub fn back(&self, q: [f64; 2]) -> [f64; 2] {
        [self.origin[0] + q[0] * STEP, self.origin[1] + q[1] * STEP]
    }
}
