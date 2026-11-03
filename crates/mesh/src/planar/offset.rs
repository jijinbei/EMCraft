//! Copper outlines from a board's items: a trace of some width with round ends, a disc (a via's
//! pad, a dot of a trace), and any outline clipped to the region.

use std::f64::consts::PI;

/// The most chords on half a circle.
const MAX_CHORDS: usize = 64;

/// The chords on half a circle of radius `r` whose sagitta stays within `sag`: n with
/// r (1 − cos(π / 2n)) ≤ sag, at least 2.
fn chords(r: f64, sag: f64) -> usize {
    let half = (1.0 - (sag / r).min(1.0)).acos();
    if !(half > 0.0) {
        return MAX_CHORDS;
    }
    ((PI / (2.0 * half)).ceil() as usize).clamp(2, MAX_CHORDS)
}

/// The outline of a trace from `a` to `b`, `width` wide, its ends round: counterclockwise, the
/// arcs as chords within `sag` of the circle (metres). A trace of no length is a disc.
pub fn trace(a: [f64; 2], b: [f64; 2], width: f64, sag: f64) -> Vec<[f64; 2]> {
    trace_ends(a, b, width, sag, [true, true])
}

/// [`trace`] with each end round or cut square across at the end point (`round` for `a`, `b`):
/// a trace fed by a port at its end stops there.
pub fn trace_ends(a: [f64; 2], b: [f64; 2], width: f64, sag: f64, round: [bool; 2]) -> Vec<[f64; 2]> {
    let r = 0.5 * width;
    let d = [b[0] - a[0], b[1] - a[1]];
    let len = d[0].hypot(d[1]);
    if !(len > 0.0) {
        return disc(a, width, sag);
    }
    let n = chords(r, sag);
    // From the right of the direction round b's end, then round a's (a cut end: its two
    // corners alone).
    let right = (-d[0]).atan2(d[1]);
    let mut ring = Vec::with_capacity(2 * n + 2);
    for (c, start, rounded) in [(b, right, round[1]), (a, right + PI, round[0])] {
        let steps = if rounded { n } else { 1 };
        for k in 0..=steps {
            let phi = start + PI * k as f64 / steps as f64;
            ring.push([c[0] + r * phi.cos(), c[1] + r * phi.sin()]);
        }
    }
    ring
}

/// A disc of diameter `d` about `c`, counterclockwise, within `sag` of the circle (metres).
pub fn disc(c: [f64; 2], d: f64, sag: f64) -> Vec<[f64; 2]> {
    let r = 0.5 * d;
    let n = 2 * chords(r, sag);
    (0..n).map(|k| 2.0 * PI * k as f64 / n as f64).map(|phi| [c[0] + r * phi.cos(), c[1] + r * phi.sin()]).collect()
}

/// `ring` clipped to the box `lo`..`hi` (Sutherland–Hodgman: I. E. Sutherland, G. W. Hodgman,
/// *Reentrant polygon clipping*, Comm. ACM 17(1), 1974). A ring that leaves the box and comes
/// back keeps the stretch of the box's side between, which encloses nothing.
pub fn clipped(ring: &[[f64; 2]], lo: [f64; 2], hi: [f64; 2]) -> Vec<[f64; 2]> {
    let mut out = ring.to_vec();
    // Each side as (axis, bound, keep the side above it).
    for (d, at, above) in [(0, lo[0], true), (0, hi[0], false), (1, lo[1], true), (1, hi[1], false)] {
        let input = std::mem::take(&mut out);
        let inside = |p: &[f64; 2]| if above { p[d] >= at } else { p[d] <= at };
        let cross = |p: &[f64; 2], q: &[f64; 2]| {
            let t = (at - p[d]) / (q[d] - p[d]);
            let mut x = [p[0] + t * (q[0] - p[0]), p[1] + t * (q[1] - p[1])];
            x[d] = at;
            x
        };
        for (i, q) in input.iter().enumerate() {
            let p = &input[(i + input.len() - 1) % input.len()];
            match (inside(p), inside(q)) {
                (true, true) => out.push(*q),
                (true, false) => out.push(cross(p, q)),
                (false, true) => {
                    out.push(cross(p, q));
                    out.push(*q);
                }
                (false, false) => {}
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(ring: &[[f64; 2]]) -> f64 {
        (0..ring.len()).map(|i| ring[i][0] * ring[(i + 1) % ring.len()][1] - ring[(i + 1) % ring.len()][0] * ring[i][1]).sum::<f64>() / 2.0
    }

    #[test]
    fn a_trace_is_a_rectangle_with_half_discs_counterclockwise() {
        let (w, l) = (0.2e-3, 3e-3);
        let ring = trace([1e-3, 1e-3], [1e-3 + l * 0.6, 1e-3 + l * 0.8], w, 1e-7);
        let exact = w * l + PI * w * w / 4.0;
        // Chords inside the circle: a little less, by under the sagitta times the perimeter.
        assert!(area(&ring) < exact && area(&ring) > exact - 1e-7 * PI * w, "{} {exact}", area(&ring));
    }

    #[test]
    fn a_trace_cut_square_at_an_end_stops_there() {
        let (w, l) = (0.2e-3, 3e-3);
        let ring = trace_ends([0.0, 0.0], [l, 0.0], w, 1e-7, [false, true]);
        let exact = w * l + PI * w * w / 8.0;
        assert!(area(&ring) < exact && area(&ring) > exact - 1e-7 * PI * w, "{} {exact}", area(&ring));
        assert!(ring.iter().all(|p| p[0] >= -1e-15));
    }

    #[test]
    fn clipping_keeps_the_part_inside() {
        let square = [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]];
        let c = clipped(&square, [0.0, -2.0], [2.0, 0.5]);
        assert!((area(&c) - 1.5).abs() < 1e-15, "{c:?}");
        assert!(clipped(&square, [2.0, 2.0], [3.0, 3.0]).is_empty());
    }
}
