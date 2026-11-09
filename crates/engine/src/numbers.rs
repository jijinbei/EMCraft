//! Small numeric helpers the commands share.

/// Two frequencies are the same within this fraction of them: a solved frequency read back from a
/// reply or a file in display units (round-off near 1e-15) matches its own.
pub(crate) const SAME_FREQUENCY: f64 = 1e-9;

/// Whether `a` and `b` (Hz) are the same frequency ([`SAME_FREQUENCY`]).
pub(crate) fn same_frequency(a: f64, b: f64) -> bool {
    (a - b).abs() <= SAME_FREQUENCY * a.abs().max(b.abs())
}

/// Two lengths are the same within this fraction of the length they are measured on (a guide's
/// width, a period): far above the round-off of metres from display units.
pub(crate) const GEOMETRY_TOLERANCE: f64 = 1e-9;

/// Two marks on a chart (cut-offs, resonances) within this fraction of their frequency are one.
pub(crate) const SAME_MARK: f64 = 1e-6;

/// `n` values evenly spaced from `start` to `stop`, both included (`start` alone when `n` is 1).
pub(crate) fn evenly_spaced(start: f64, stop: f64, n: usize) -> Vec<f64> {
    (0..n).map(|k| if n == 1 { start } else { start + (stop - start) * k as f64 / (n - 1) as f64 }).collect()
}

/// The index of the value in `values` nearest `target` (the first of equals); `None` when there
/// are none.
pub(crate) fn nearest(values: &[f64], target: f64) -> Option<usize> {
    (0..values.len()).min_by(|&a, &b| (values[a] - target).abs().total_cmp(&(values[b] - target).abs()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evenly_spaced_values_include_both_ends() {
        assert_eq!(evenly_spaced(1.0, 3.0, 3), vec![1.0, 2.0, 3.0]);
        assert_eq!(evenly_spaced(5.0, 9.0, 1), vec![5.0]);
        assert!(evenly_spaced(0.0, 1.0, 0).is_empty());
        assert_eq!(nearest(&[1.0, 4.0, 2.0], 2.4), Some(2));
        assert_eq!(nearest(&[], 2.4), None);
    }
}
