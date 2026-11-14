//! Binomial smoothing of the current and the charge (C. K. Birdsall, A. B. Langdon, *Plasma
//! Physics via Computer Simulation*, 1991, §8-6 and appendix C; against numerical Cherenkov
//! radiation, B. B. Godfrey, J.-L. Vay, J. Comput. Phys. 248:33–46, 2013): each pass along each
//! axis replaces a value by ½ itself and ¼ each neighbour, a node lattice's or a centre
//! lattice's, wrapped round a periodic axis and zero past a wall (k passes along an axis taken
//! at once, as the stencil C(2k, k + j)/4^k). The filter is a convolution
//! the grid's differences commute with (zero past the walls on both lattices), so the current
//! smoothed moves the charge smoothed alike and Gauss's law holds for it. Its response at
//! wavenumber k is cos²(kh/2) a pass along an axis: none at the grid's shortest wave.

use super::grid::{Grid, Lattice};

/// The most passes.
pub(crate) const MAX_PASSES: usize = 8;

/// `values` on `lattice` smoothed by `passes` passes along each axis: along each axis once by
/// their product, the binomial stencil C(2k, k + j)/4^k over j = −k…k, so that what a pass would
/// spill past a wall and the next read back is the same as on the lattice extended by zeros (the
/// convolution the differences commute with).
pub(crate) fn smooth(grid: &Grid, lattice: Lattice, values: &mut [f64], passes: usize) {
    if passes == 0 {
        return;
    }
    let k = passes.min(MAX_PASSES) as isize;
    // C(2k, k + j)/4^k, j = −k…k.
    let mut weights = vec![1.0f64];
    for _ in 0..2 * k {
        let mut next = vec![0.0; weights.len() + 1];
        for (i, w) in weights.iter().enumerate() {
            next[i] += 0.5 * w;
            next[i + 1] += 0.5 * w;
        }
        weights = next;
    }
    let mut spare = values.to_vec();
    for d in 0..3 {
        let axis = grid.axes[d];
        for p in lattice.points() {
            let i = p[d] as isize;
            let sum: f64 = (-k..=k)
                .zip(&weights)
                .map(|(j, w)| {
                    axis.wrap(i + j, lattice.node[d]).map_or(0.0, |m| {
                        let mut q = p;
                        q[d] = m;
                        w * values.get(lattice.at(q)).copied().unwrap_or(0.0)
                    })
                })
                .sum();
            if let Some(v) = spare.get_mut(lattice.at(p)) {
                *v = sum;
            }
        }
        values.copy_from_slice(&spare);
    }
}
