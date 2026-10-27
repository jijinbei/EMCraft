//! What the charts share: their axes' ranges and the dB scale of S-parameters.

use emcraft_render::Axis;
use emcraft_render::chart::Series;

/// The lowest a chart's dB axis reaches.
pub(crate) const DB_FLOOR: f64 = -80.0;

/// The least and the greatest of `values` (∞ and −∞ when there are none; NaN is passed over).
pub(crate) fn span(values: impl IntoIterator<Item = f64>) -> (f64, f64) {
    values.into_iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(v), b.max(v)))
}

/// An axis over the finite `values`, padded by a twentieth of their range (of their size, for
/// one value); 0 to 1 when there are none.
pub(crate) fn padded_axis(label: &str, values: impl Iterator<Item = f64>) -> Axis {
    let (lo, hi) = span(values.filter(|v| v.is_finite()));
    if !lo.is_finite() {
        return Axis::new(label, 0.0, 1.0);
    }
    let pad = if hi > lo { 0.05 * (hi - lo) } else { 0.05 * lo.abs().max(1.0) };
    Axis::new(label, lo - pad, hi + pad)
}

/// A dB axis for `series`: from the ten below their lowest point (between −80 and −10 dB) to
/// 2 dB.
pub(crate) fn db_axis(label: &str, series: &[Series]) -> Axis {
    let lowest = series.iter().flat_map(|s| s.points.iter().map(|p| p[1])).fold(0.0f64, f64::min);
    Axis::new(label, ((lowest / 10.0).floor() * 10.0).clamp(DB_FLOOR, -10.0), 2.0)
}
