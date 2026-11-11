//! The field on its planes in the 3D view: the colour map and the arrows, filled, on both sides
//! of the plane, with the colour bar and the arrows' scale for the legend. The view's own plane
//! is coloured by the GPU (`gpu3d::field`); planes kept beside it cell by cell here, by the same
//! scale ([`emcraft_render::plot::Scalar::scale`]).

use emcraft_engine::Session;
use emcraft_engine::view::Plane;

use super::style::Colours;
use super::{Built, FieldLayer};

/// Most cells along the field plane's longer side.
const FIELD_CELLS: usize = 120;

/// The axis normal to a field plane (0 x, 1 y, 2 z).
pub fn normal_axis(plane: Plane) -> usize {
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

/// A field plane's corners (m): at the picture's low corner, at its far end along the
/// picture's horizontal axis and along its vertical one; `x` and `y` the picture's spans (display
/// units), `lo` the model's low corner.
pub fn plane_frame(session: &Session, plane: Plane, at: f64, x: (f64, f64), y: (f64, f64), lo: [f64; 3]) -> [[f64; 3]; 3] {
    let l = session.display().length;
    let (h0, h1, v0, v1) = (l.to_si(x.0), l.to_si(x.1), l.to_si(y.0), l.to_si(y.1));
    [on_plane(plane, lo, at, h0, v0), on_plane(plane, lo, at, h1, v0), on_plane(plane, lo, at, h0, v1)]
}

/// The field's colour map on its plane, cell by cell (cells outside the mesh left out); one
/// quad over the plane when the GPU colours it ([`FieldLayer::gpu`]).
pub(super) fn field_plane(
    colours: &Colours,
    session: &Session,
    f: &FieldLayer,
    shared_peak: f64,
    legend: bool,
    bounds: Option<([f64; 3], [f64; 3])>,
) -> Built {
    let mut out = Built::default();
    let (Some(s), Some((lo, _))) = (f.plot.scalar.as_ref(), bounds) else { return out };
    let (nx, ny) = (s.nx, s.ny);
    if nx < 2 || ny < 2 || s.values.len() < nx * ny {
        return out;
    }
    let l = session.display().length;
    let (h0, h1, v0, v1) = (l.to_si(f.plot.x.min), l.to_si(f.plot.x.max), l.to_si(f.plot.y.min), l.to_si(f.plot.y.max));
    let point = |u: f64, v: f64| on_plane(f.plane, lo, f.at, u, v);
    let scale = s.scale(shared_peak);
    if legend {
        out.colour_bar = Some(super::ColourBar { label: s.label.clone(), scale });
    }
    if f.gpu {
        let [c, u, v] = plane_frame(session, f.plane, f.at, f.plot.x.span(), f.plot.y.span(), lo);
        let far = [u[0] + v[0] - c[0], u[1] + v[1] - c[1], u[2] + v[2] - c[2]];
        out.scene.quad([c, u, far, v], scale.colour(0.0), 255);
        return out.merge(field_arrows(colours, session, f, lo, legend));
    }
    let step = nx.max(ny).div_ceil(FIELD_CELLS).max(1);
    let at = |i: usize, j: usize| s.values.get(j * nx + i).copied().unwrap_or(f64::NAN);
    let (mut j, du, dv) = (0, (h1 - h0) / (nx - 1) as f64, (v1 - v0) / (ny - 1) as f64);
    while j + 1 < ny {
        let j1 = (j + step).min(ny - 1);
        let mut i = 0;
        while i + 1 < nx {
            let i1 = (i + step).min(nx - 1);
            // The corners inside the mesh: a cell with half of them or more is drawn (as the
            // GPU's plane ends half way to a sample outside).
            let inside: Vec<f64> = [at(i, j), at(i1, j), at(i1, j1), at(i, j1)].into_iter().filter(|v| v.is_finite()).collect();
            if inside.len() >= 2 {
                let mean = inside.iter().sum::<f64>() / inside.len() as f64;
                let (u0, u1, w0, w1) = (h0 + du * i as f64, h0 + du * i1 as f64, v0 + dv * j as f64, v0 + dv * j1 as f64);
                out.scene.quad([point(u0, w0), point(u1, w0), point(u1, w1), point(u0, w1)], scale.colour(mean), 255);
            }
            i = i1;
        }
        j = j1;
    }
    out.merge(field_arrows(colours, session, f, lo, legend))
}

/// The legend's line for the arrows of E (or H, `magnetic`) scaled to `longest` (V/m, A/m): an
/// arrow at full length is that much.
pub fn arrow_legend(magnetic: bool, longest: f64) -> String {
    let (field, unit) = if magnetic { ("H", "A/m") } else { ("E", "V/m") };
    crate::lang::trf(
        t!("{field} in the plane · longest {value} {unit}"),
        crate::lang::args!(field = format!("$\\mathbf{{{field}}}$"), value = emcraft_render::axes::format(longest), unit = unit),
    )
}

/// The plot's arrows (E's, H's), generated and shaded in WGSL, a hair off
/// the plane on both sides so they show from either.
fn field_arrows(colours: &Colours, session: &Session, f: &FieldLayer, lo: [f64; 3], legend: bool) -> Built {
    let mut out = Built::default();
    let l = session.display().length;
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
        let color = match set.pen {
            emcraft_render::Pen::EArrows => colours.scene.port_input,
            emcraft_render::Pen::HArrows => colours.scene.port_output,
            _ => colours.palette.pen(set.pen),
        };
        if legend && !f.gpu {
            out.arrow_legend.push((arrow_legend(set.pen == emcraft_render::Pen::HArrows, scale), color));
        }
        if f.gpu {
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
            for side in [lift, -lift] {
                out.arrows.push(super::Arrow {
                    start: point([p[0] - d[0] / 2.0, p[1] - d[1] / 2.0], side),
                    end: point([p[0] + d[0] / 2.0, p[1] + d[1] / 2.0], side),
                    color,
                    section_clip: true,
                    motion: None,
                });
            }
        }
    }
    out
}
