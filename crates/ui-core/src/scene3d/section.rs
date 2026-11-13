//! What the 3D view's section view shows besides the cut itself, whatever window draws it:
//! dimensions on the cut — the cut outline's overall extents, a guide's section and iris
//! apertures — and the ports marked where they meet the cut with their name and size
//! ([`notes`]), the presets for a guide's H-plane (y = b/2) and E-plane (x = a/2) cuts, and the
//! camera's goal for looking straight at the cut ([`look`]). Everything is placed in the model
//! (m) on the cut plane, for each window to project with its camera every frame.

use emcraft_render::view3d::{Scene, StandardView};

use super::Cut;
use crate::navigation::{Goal, view_from};

/// The least span (m) a section's outline or dimension is drawn for: a flat model still draws.
const MIN_SPAN: f64 = 1e-12;

/// A cut's outline reaches this fraction of the model's widest side beyond it.
const OUTLINE_PAD: f64 = 0.25;

/// A dimension line sits this fraction of the section's larger side off its edge.
const DIMENSION_GAP: f64 = 0.08;

/// Most dimensions drawn on one cut (a long iris filter's apertures beyond it are left out).
pub const MAX_DIMS: usize = 24;

/// A preset cut of a guide: its H-plane (across y, through the middle of the height) or its
/// E-plane (across x, through the middle of the width).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    HPlane,
    EPlane,
}

impl Preset {
    /// The preset's cut through the middle of `bounds` (m), the low side kept.
    pub fn cut(self, bounds: ([f64; 3], [f64; 3])) -> Cut {
        Cut::middle(if self == Preset::HPlane { 1 } else { 0 }, bounds)
    }

    /// Whether `cut` is this preset's.
    pub fn is(self, cut: Cut, bounds: ([f64; 3], [f64; 3])) -> bool {
        let c = self.cut(bounds);
        cut.axis == c.axis && (cut.at - c.at).abs() <= 1e-9 * (bounds.1[c.axis] - bounds.0[c.axis]).abs().max(1e-12)
    }
}

/// The two axes in the plane of a cut across `k`, in the order x, y, z.
fn across(k: usize) -> (usize, usize) {
    match k {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    }
}

/// The box (m) of the cut outlines and any triangles on the plane,
/// else the model's box flattened onto the plane.
pub fn outline(scene: &Scene, cut: Cut, bounds: ([f64; 3], [f64; 3])) -> ([f64; 3], [f64; 3]) {
    let k = cut.axis.min(2);
    let size = (0..3).map(|d| (bounds.1[d] - bounds.0[d]).abs()).fold(0.0, f64::max).max(MIN_SPAN);
    let eps = 1e-7 * size;
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    let on_plane = |p: &[f64; 3]| (p[k] - cut.at).abs() <= eps;
    let points = scene
        .tris
        .iter()
        .filter(|t| t.p.iter().all(on_plane))
        .flat_map(|t| t.p)
        .chain(scene.lines.iter().filter(|l| on_plane(&l.a) && on_plane(&l.b)).flat_map(|l| [l.a, l.b]));
    for q in points {
        for d in 0..3 {
            lo[d] = lo[d].min(q[d]);
            hi[d] = hi[d].max(q[d]);
        }
    }
    if !(lo[0] <= hi[0]) {
        (lo, hi) = bounds;
    }
    lo[k] = cut.at;
    hi[k] = cut.at;
    (lo, hi)
}

/// The model's box flattened onto the cut's plane.
pub fn flat(cut: Cut, (mut lo, mut hi): ([f64; 3], [f64; 3])) -> ([f64; 3], [f64; 3]) {
    let k = cut.axis.min(2);
    (lo[k], hi[k]) = (cut.at, cut.at);
    (lo, hi)
}

/// The camera's goal for looking straight at the cut: its box framed from the side that was
/// cut away, so the kept side lies behind the cut faces.
pub fn look(cut: Cut, outline: ([f64; 3], [f64; 3])) -> Option<Goal> {
    let sign = cut.keep.sign();
    let side: StandardView = view_from(cut.axis.min(2), sign)?;
    // Framed with room round the cut for its dimensions and the ports' labels, and below it for
    // the view's bars.
    let k = cut.axis.min(2);
    let (mut lo, mut hi) = outline;
    let pad = OUTLINE_PAD * (0..3).filter(|d| *d != k).map(|d| hi[d] - lo[d]).fold(0.0, f64::max);
    for d in (0..3).filter(|d| *d != k) {
        lo[d] -= pad;
        hi[d] += pad;
    }
    Some(Goal::Face(lo, hi, side))
}

/// A dimension: from `a` to `b` (m), its line moved by `off` (m) off them, labelled `text`.
#[derive(Clone, Debug, PartialEq)]
pub struct Dim {
    pub a: [f64; 3],
    pub b: [f64; 3],
    pub off: [f64; 3],
    pub text: String,
}

/// A port where it meets the cut: the segment (m), its label and its role in the S-parameters
/// (the driven port is the input, as the browser's pills say).
#[derive(Clone, Debug, PartialEq)]
pub struct PortMark {
    pub a: [f64; 3],
    pub b: [f64; 3],
    pub text: String,
    pub input: bool,
}

/// What the cut is annotated with: its dimensions and the ports it meets.
/// `driven` is the input port's index (`Session::driven_port`).
pub fn notes(
    m: &emcraft_model::Model,
    l: emcraft_model::units::Length,
    driven: Option<usize>,
    cut: Cut,
    outline: ([f64; 3], [f64; 3]),
) -> (Vec<Dim>, Vec<PortMark>) {
    let k = cut.axis.min(2);
    let c = cut.at;
    let (u, v) = across(k);
    let (lo, hi) = outline;
    let gap = DIMENSION_GAP * (hi[u] - lo[u]).abs().max((hi[v] - lo[v]).abs()).max(MIN_SPAN);
    let len = |x: f64| format!("{} {}", crate::dialogs::short_number(l.from_si(x)), l.symbol());
    let at = |pu: f64, pv: f64| {
        let mut p = [0.0; 3];
        (p[k], p[u], p[v]) = (c, pu, pv);
        p
    };
    let along = |d: usize, s: f64| {
        let mut p = [0.0; 3];
        p[d] = s;
        p
    };
    let mut dims = Vec::new();
    // The outline's overall extents: across u under it, across v beside it.
    if hi[u] - lo[u] > 0.0 {
        dims.push(Dim { a: at(lo[u], lo[v]), b: at(hi[u], lo[v]), off: along(v, -gap), text: len(hi[u] - lo[u]) });
    }
    if hi[v] - lo[v] > 0.0 {
        dims.push(Dim { a: at(lo[u], lo[v]), b: at(lo[u], hi[v]), off: along(u, -gap), text: len(hi[v] - lo[v]) });
    }
    // A guide's apertures where they narrow (an iris, a step), in the sections the cut crosses.
    if let Some(w) = m.waveguide() {
        let mut z = 0.0;
        for (i, s) in w.sections.iter().enumerate() {
            let (z0, z1) = (z, z + s.length.get());
            z = z1;
            let (Some(ax), Some(ay)) = (w.aperture(i), w.aperture_y(i)) else { continue };
            let inside = |span: emcraft_model::Span, x: f64| x >= span.start - 1e-12 && x <= span.start + span.width + 1e-12;
            let narrow = |span: emcraft_model::Span, full: emcraft_model::Span| (span.width - full.width).abs() > 1e-9 * full.width.max(1e-12);
            let zm = 0.5 * (z0 + z1);
            match k {
                // H-plane: the x aperture of each section the cut's y passes through.
                1 if inside(ay, c) && narrow(ax, w.across_x()) => {
                    dims.push(Dim { a: [ax.start, c, zm], b: [ax.start + ax.width, c, zm], off: [0.0; 3], text: len(ax.width) });
                }
                // E-plane: the y aperture.
                0 if inside(ax, c) && narrow(ay, w.across_y()) => {
                    dims.push(Dim { a: [c, ay.start, zm], b: [c, ay.start + ay.width, zm], off: [0.0; 3], text: len(ay.width) });
                }
                // Across the guide: the opening of the section cut.
                2 if c >= z0 && c <= z1 && (narrow(ax, w.across_x()) || narrow(ay, w.across_y())) => {
                    let (ym, xm) = (ay.start + 0.5 * ay.width, ax.start + 0.5 * ax.width);
                    dims.push(Dim { a: [ax.start, ym, c], b: [ax.start + ax.width, ym, c], off: [0.0; 3], text: len(ax.width) });
                    dims.push(Dim { a: [xm, ay.start, c], b: [xm, ay.start + ay.width, c], off: [0.0; 3], text: len(ay.width) });
                }
                _ => {}
            }
        }
    }
    dims.truncate(MAX_DIMS);
    let ports = m.bounds().map_or_default(|b| port_marks(m, b, k, c, driven, &len));
    (dims, ports)
}

/// The ports' faces where the plane across `k` at `c` meets them: a segment each.
fn port_marks(
    m: &emcraft_model::Model,
    b: emcraft_model::Aabb,
    k: usize,
    c: f64,
    driven: Option<usize>,
    len: &dyn Fn(f64) -> String,
) -> Vec<PortMark> {
    let (lo, hi) = (b.min.to_array(), b.max.to_array());
    let mut out = Vec::new();
    for (j, port) in m.ports.iter().enumerate().take(64) {
        // The face as a box flat across its normal: its own rectangle, or the model's end.
        let (n, flo, fhi) = match port.face {
            Some(f) => {
                let q = f.corners();
                let mut flo = [f64::INFINITY; 3];
                let mut fhi = [f64::NEG_INFINITY; 3];
                for p in q {
                    for (d, x) in p.to_array().into_iter().enumerate() {
                        flo[d] = flo[d].min(x);
                        fhi[d] = fhi[d].max(x);
                    }
                }
                (f.axis.index(), flo, fhi)
            }
            None => {
                let z = if port.end == emcraft_model::End::Start { lo[2] } else { hi[2] };
                (2, [lo[0], lo[1], z], [hi[0], hi[1], z])
            }
        };
        if n == k || c < flo[k] - 1e-12 || c > fhi[k] + 1e-12 {
            continue;
        }
        let other = 3 - n - k;
        let mut a = [0.0; 3];
        a[n] = flo[n];
        a[k] = c;
        let mut e = a;
        a[other] = flo[other];
        e[other] = fhi[other];
        let input = driven == Some(j);
        let size = len(fhi[other] - flo[other]);
        let text = if input {
            crate::lang::trf(t!("{name} · in · {size}"), crate::lang::args!(name = port.name, size = size))
        } else {
            crate::lang::trf(t!("{name} · out · {size}"), crate::lang::args!(name = port.name, size = size))
        };
        out.push(PortMark { a, b: e, text, input });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// An iris filter cut in its H-plane: the overall length and width, each iris's aperture,
    /// and the two ports at its ends, sized as the guide is wide.
    #[test]
    fn a_cut_of_a_guide_carries_its_dimensions_and_ports() {
        let mut s = emcraft_engine::Session::new();
        s.execute("file.sample", &json!({"name": "iris-bandpass"})).unwrap();
        let m = s.model();
        let b = m.bounds().unwrap();
        let bounds = (b.min.to_array(), b.max.to_array());
        let cut = Preset::HPlane.cut(bounds);
        assert!(Preset::HPlane.is(cut, bounds) && !Preset::EPlane.is(cut, bounds));
        let (dims, ports) = notes(m, s.display().length, s.driven_port(), cut, outline(&Scene::default(), cut, bounds));
        let w = m.waveguide().unwrap();
        assert!(dims.len() > 2, "{dims:?}");
        assert!(dims[0].text.starts_with(&crate::dialogs::short_number(w.width.get() * 1e3)), "{dims:?}");
        assert_eq!(ports.len(), 2);
        // Port 1 is driven: the input; port 2 an output.
        assert!(ports[0].input && !ports[1].input && ports[0].text.starts_with("1 · in · "), "{ports:?}");
        assert!(ports.iter().all(|p| (p.b[0] - p.a[0] - w.width.get()).abs() < 1e-12), "{ports:?}");
        // Across x through the middle (the E-plane), the irises do not narrow the height: only the
        // overall extents.
        let (dims, ports) =
            notes(m, s.display().length, s.driven_port(), Preset::EPlane.cut(bounds), outline(&Scene::default(), Preset::EPlane.cut(bounds), bounds));
        assert_eq!(dims.len(), 2);
        assert_eq!(ports.len(), 2);
        // Facing the cut: from +y, the low side kept.
        assert!(look(cut, bounds).is_some());
    }
}
