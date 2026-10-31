//! Grid lines kept off other bodies' planes.
//!
//! A box is tessellated as a grid on each face, a cylinder as rings along its axis. When another
//! body's flat face lies in the plane of one of those grid lines (a plate's face at z = 20 mm in
//! a guide whose walls are divided every 4 mm), the two surfaces cross along a line a hair from
//! the grid line — the features are moved by a millionth of the size for general position — and
//! the Boolean leaves slivers of wall nanometres wide whose points boundary recovery finds on
//! another boundary edge ("the surface is not a valid complex"). Such a grid line is moved off
//! the plane by a third of its cell first: every vertex on it moves along the line's axis, so it
//! stays on its faces (a box's faces across the other axes, a cylinder's side) and the body is
//! the same; only its triangles change. Grid lines farther than [`NEAR`] of the size from every
//! other plane keep their places, so meshes without such a coincidence stay as they were, bit
//! for bit: ten times the features' shift, the scale of the slivers (a plane a few millionths
//! of the size off a grid line — 28.57 mm against 200/7 in the open-waveguide oracle — meshes
//! as it is).

use emcraft_model::Point3;

/// A grid line within this fraction of the element size from another body's plane is moved.
pub(super) const NEAR: f64 = 1e-5; // unit-factors: relative to the element size

/// The planes across each axis (x, y, z) other bodies' flat faces lie in.
pub(super) type Planes = [Vec<f64>; 3];

/// The tessellation's interior grid lines across `axes` (its vertices' distinct coordinates
/// along an axis other than the lowest and highest) moved off `planes` when within `tol`: by a
/// third of the narrower neighbouring cell, up unless that lands near another plane. Returns
/// whether any moved.
pub(super) fn keep_off(positions: &mut [Point3], axes: &[usize], planes: &Planes, tol: f64) -> bool {
    let mut moved = false;
    for &d in axes {
        let Some(list) = planes.get(d) else { continue };
        if list.is_empty() {
            continue;
        }
        let mut values: Vec<f64> = positions.iter().map(|p| p[d]).collect();
        values.sort_by(f64::total_cmp);
        values.dedup();
        let near = |x: f64| list.iter().map(|p| (p - x).abs()).fold(f64::INFINITY, f64::min);
        let mut shifts: Vec<(f64, f64)> = Vec::new();
        for k in 1..values.len().saturating_sub(1) {
            let v = values[k];
            if near(v) > tol {
                continue;
            }
            let cell = (v - values[k - 1]).min(values[k + 1] - v);
            // A third of the cell up, else down, else whichever of a few other places lies
            // farthest from every plane.
            let candidates = [1.0 / 3.0, -1.0 / 3.0, 0.2, -0.2, 0.45, -0.45].map(|f| v + f * cell).map(|x| (near(x), x));
            let best = candidates.iter().find(|c| c.0 > 0.25 * cell).or_else(|| candidates.iter().max_by(|a, b| a.0.total_cmp(&b.0))).copied();
            if let Some((gap, x)) = best
                && gap > tol
            {
                shifts.push((v, x));
            }
        }
        for p in positions.iter_mut() {
            if let Ok(i) = shifts.binary_search_by(|s| s.0.total_cmp(&p[d])) {
                p[d] = shifts[i].1;
                moved = true;
            }
        }
    }
    moved
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_lines_on_another_plane_move_off_it_and_others_stay() {
        // A row of points along x at 0, 1, 2, 3, 4; planes at 2 + 1e-9 and 4 (an end: kept).
        let mut p: Vec<Point3> = (0..5).map(|i| Point3::new(i as f64, 7.0, 0.0)).collect();
        let before = p.clone();
        assert!(keep_off(&mut p, &[0], &[vec![2.0 + 1e-9, 4.0], vec![], vec![]], 1e-3));
        assert!((p[2].x - (2.0 + 1.0 / 3.0)).abs() < 1e-12, "{:?}", p[2]);
        for i in [0, 1, 3, 4] {
            assert_eq!(p[i], before[i]);
        }
        // Nothing near: nothing moves, bit for bit.
        let mut q = before.clone();
        assert!(!keep_off(&mut q, &[0, 1, 2], &[vec![2.5], vec![7.0], vec![]], 1e-3));
        assert_eq!(q, before);
    }
}
