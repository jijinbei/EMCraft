//! The field on its planes in the 3D view: the colour map cell by cell and the arrows, filled,
//! on both sides of the plane.

use emcraft_engine::Session;
use emcraft_engine::view::Plane;
use emcraft_render::plot::ScalarKind;

use super::style::RIM;
use super::{Built, FieldLayer, plain};

/// Most cells along the field plane's longer side.
const FIELD_CELLS: usize = 120;

/// The axis normal to a field plane (0 x, 1 y, 2 z).
pub(crate) fn normal_axis(plane: Plane) -> usize {
    match plane {
        Plane::E => 0,
        Plane::H => 1,
        Plane::Cross => 2,
    }
}

/// A point of the plane `at` (m from the model's low corner `lo` along its normal) from its
/// picture's coordinates (m): an H-plane's across x, along z; an E-plane's across y, along z; a
/// cross-section's x and y.
fn on_plane(plane: Plane, lo: [f64; 3], at: f64, u: f64, v: f64) -> [f64; 3] {
    match plane {
        Plane::H => [v, lo[1] + at, u],
        Plane::E => [lo[0] + at, v, u],
        Plane::Cross => [u, v, lo[2] + at],
    }
}

/// The largest finite magnitude among `values`.
pub(super) fn largest_magnitude(values: &[f64]) -> f64 {
    values.iter().filter(|v| v.is_finite()).fold(0.0, |a: f64, b| a.max(b.abs()))
}

/// The field's colour map on its plane, cell by cell (cells outside the mesh left out).
pub(super) fn field_plane(session: &Session, f: &FieldLayer, shared_peak: f64, legend: bool, out: &mut Built) {
    let (Some(s), Some((lo, _))) = (f.plot.scalar.as_ref(), out.bounds) else { return };
    let (nx, ny) = (s.nx, s.ny);
    if nx < 2 || ny < 2 || s.values.len() < nx * ny {
        return;
    }
    let l = session.display.length;
    let (h0, h1, v0, v1) = (l.to_si(f.plot.x.min), l.to_si(f.plot.x.max), l.to_si(f.plot.y.min), l.to_si(f.plot.y.max));
    let point = |u: f64, v: f64| on_plane(f.plane, lo, f.at, u, v);
    let own = s.range.filter(|r| *r > 0.0).unwrap_or_else(|| largest_magnitude(&s.values));
    let peak = if shared_peak > 0.0 { shared_peak } else { own };
    let color = |v: f64| match s.kind {
        ScalarKind::Signed => emcraft_render::colormap::diverging(v / peak.max(1e-300)),
        ScalarKind::Magnitude => emcraft_render::colormap::sequential(v / peak.max(1e-300)),
    };
    let step = nx.max(ny).div_ceil(FIELD_CELLS).max(1);
    let at = |i: usize, j: usize| s.values.get(j * nx + i).copied().unwrap_or(f64::NAN);
    let (mut j, du, dv) = (0, (h1 - h0) / (nx - 1) as f64, (v1 - v0) / (ny - 1) as f64);
    while j + 1 < ny {
        let j1 = (j + step).min(ny - 1);
        let mut i = 0;
        while i + 1 < nx {
            let i1 = (i + step).min(nx - 1);
            let corners = [at(i, j), at(i1, j), at(i1, j1), at(i, j1)];
            if corners.iter().all(|v| v.is_finite()) {
                let mean = corners.iter().sum::<f64>() / 4.0;
                let (u0, u1, w0, w1) = (h0 + du * i as f64, h0 + du * i1 as f64, v0 + dv * j as f64, v0 + dv * j1 as f64);
                out.scene.quad([point(u0, w0), point(u1, w0), point(u1, w1), point(u0, w1)], color(mean), 255);
            }
            i = i1;
        }
        j = j1;
    }
    if legend {
        out.legend.push((plain(&s.label), color(peak)));
    }
    field_arrows(session, f, lo, legend, out);
}

/// The plot's arrows (E's, H's) on the field's plane, filled (a shaft and a head), a hair off
/// the plane on both sides so they show from either.
fn field_arrows(session: &Session, f: &FieldLayer, lo: [f64; 3], legend: bool, out: &mut Built) {
    let l = session.display.length;
    let normal = normal_axis(f.plane);
    let (xa, xb) = f.plot.x.span();
    let lift = 1e-3 * l.to_si(xb - xa);
    // Plane coordinates (display units) to the model's (m), `side` lifts off the plane.
    let point = |p: [f64; 2], side: f64| {
        let mut q = on_plane(f.plane, lo, f.at, l.to_si(p[0]), l.to_si(p[1]));
        q[normal] += side;
        q
    };
    for set in &f.plot.arrows {
        let largest = set.arrows.iter().map(|(_, v)| v[0].hypot(v[1])).filter(|m| m.is_finite()).fold(0.0, f64::max);
        let scale = set.scale.filter(|s| *s > 0.0 && s.is_finite()).unwrap_or(largest);
        if !(scale > 0.0) {
            continue;
        }
        for (p, v) in &set.arrows {
            // Arrows above the scale (a source's spike) stop at the full length.
            let k = f.plot.arrow_length / scale.max(v[0].hypot(v[1]));
            let d = [v[0] * k, v[1] * k];
            let len = d[0].hypot(d[1]);
            if !(len.is_finite() && len > 0.08 * f.plot.arrow_length) {
                continue;
            }
            let (u, n) = ([d[0] / len, d[1] / len], [-d[1] / len, d[0] / len]);
            // In plane coordinates: `along` the arrow from its tail, `across` it.
            let at = |along: f64, across: f64| [p[0] - d[0] / 2.0 + along * u[0] + across * n[0], p[1] - d[1] / 2.0 + along * u[1] + across * n[1]];
            let (head, shaft) = (0.4 * len, (0.07 * f.plot.arrow_length).min(0.15 * len));
            // A white rim under each arrow (between it and the plane), as the pictures have, so
            // a dark arrow shows on the colour map's dark end too.
            let rim = 0.45 * shaft;
            for side in [lift, -lift] {
                let q = |along: f64, across: f64| point(at(along, across), 0.5 * side);
                let (h, w) = (head + 2.0 * rim, shaft / 2.0 + rim);
                out.scene.quad([q(-rim, -w), q(len - head, -w), q(len - head, w), q(-rim, w)], RIM, 255);
                out.scene.tri(q(len + rim - h, -0.45 * h), q(len + rim, 0.0), q(len + rim - h, 0.45 * h), RIM, 255);
            }
            for side in [lift, -lift] {
                let q = |along: f64, across: f64| point(at(along, across), side);
                out.scene.quad([q(0.0, -shaft / 2.0), q(len - head, -shaft / 2.0), q(len - head, shaft / 2.0), q(0.0, shaft / 2.0)], set.color, 255);
                out.scene.tri(q(len - head, -0.45 * head), q(len, 0.0), q(len - head, 0.45 * head), set.color, 255);
            }
        }
        if legend {
            out.legend.push((plain(&set.label), set.color));
        }
    }
}
