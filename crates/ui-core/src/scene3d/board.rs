//! A circuit board in 3D, the PCB window's main view: each layer's copper flat at its height,
//! coloured by its role (the selected route, its reference, the rest), the substrates as
//! see-through slabs between, the vias as posts and the region the finite elements solve as a
//! wire box. Boards are far thinner than they are wide, so the board's heights are stretched
//! (the legend says by how much) until the layers stand apart; the region's air is drawn as it
//! is.
//!
//! The copper is the planar mesher's (`emcraft_mesh::planar`) at a coarse size: each layer's
//! union of traces, pads and pours, as the finite elements see it; a board the mesher refuses
//! (two nets overlapping) is drawn trace by trace instead.

use emcraft_mesh::planar::{NO_NET, Outline, Request, offset, triangulate};
use emcraft_model::pcb::Board;
use emcraft_render::Rgb;

use super::{Built, Label, Pick};
use crate::lang::{args, trf};

/// A point of the board in the view's axes: the board's plane is the view's x–z (the view's y is
/// up), its y the view's z, so the iso and top views look down on it as a layout does.
fn at(x: f64, y: f64, height: f64) -> [f64; 3] {
    [x, height, y]
}

/// The board's height drawn at least this fraction of its width.
const STRETCHED: f64 = 0.06;
/// Each layer drawn at least this fraction of the board's width from the next.
const LAYER_GAP: f64 = 0.03;
/// How opaque planes are (copper of nets with no routes: grounds, supplies), so the layers under
/// them show through.
const PLANE_ALPHA: u8 = 150;
/// Sides of a via's post.
const VIA_SIDES: usize = 12;

/// How the board is laid in the view: its heights over the lowest layer (m) and how much they
/// are stretched.
pub struct Frame {
    pub stretch: f64,
    pub heights: Vec<f64>,
}

/// The board's frame: heights stretched until the board stands at least [`STRETCHED`] of its
/// width and its layers stand apart.
pub fn frame(board: &Board) -> Frame {
    let heights = board.heights();
    let thickness = heights.iter().copied().fold(0.0, f64::max);
    let width = board.bounds().map_or(0.0, |(lo, hi)| (hi[0] - lo[0]).max(hi[1] - lo[1]));
    // And each layer apart from the next by at least [`LAYER_GAP`] of the width.
    let mut sorted = heights.clone();
    sorted.sort_by(f64::total_cmp);
    let gap = sorted.windows(2).map(|w| w[1] - w[0]).fold(f64::INFINITY, f64::min);
    let by_board = if thickness > 0.0 { STRETCHED * width / thickness } else { 1.0 };
    let by_gap = if gap.is_finite() && gap > 0.0 { LAYER_GAP * width / gap } else { 1.0 };
    Frame { stretch: by_board.max(by_gap).max(1.0), heights }
}

impl Frame {
    /// The view's point over board point `p` at depth `z` (the board's z, m, down from its top
    /// layer).
    pub fn point(&self, p: [f64; 2], z: f64, board: &Board) -> [f64; 3] {
        let bottom = board.layers.iter().map(|l| l.z).fold(f64::NEG_INFINITY, f64::max);
        at(p[0], p[1], (bottom - z) * self.stretch)
    }
}

/// The board's nets in the order of their names: a [`Pick::Net`] indexes it.
pub fn nets(board: &Board) -> Vec<String> {
    let mut nets: Vec<String> = board
        .segments
        .iter()
        .map(|g| g.net.clone())
        .chain(board.copper.iter().map(|c| c.net.clone()))
        .chain(board.vias.iter().map(|v| v.net.clone()))
        .collect();
    nets.sort();
    nets.dedup();
    nets
}

/// The colours of the selected route, its reference and the other copper.
struct NetColours {
    route: Rgb,
    reference: Rgb,
    copper: Rgb,
}

/// Builds the board's scene: `route` the selected net, `reference` the route's reference net.
pub(super) fn build(c: &super::style::Colours, board: &Board, route: &str, reference: &str) -> Built {
    let mut out = Built::default();
    let nets_in = NetColours { route: c.route, reference: c.reference, copper: c.scene.strip };
    let Some((lo, hi)) = board.bounds() else { return out };
    let Frame { stretch, heights } = frame(board);
    let thickness = heights.iter().copied().fold(0.0, f64::max);
    let z = |layer: usize| heights.get(layer).copied().unwrap_or(0.0) * stretch;
    let nets = nets(board);
    let colour = |net: &str| {
        if net == route {
            nets_in.route
        } else if net == reference {
            nets_in.reference
        } else {
            nets_in.copper
        }
    };
    // The substrates: a slab between each layer and the next one down.
    let mut up: Vec<usize> = (0..board.layers.len()).collect();
    up.sort_by(|&a, &b| heights[a].total_cmp(&heights[b]));
    for w in up.windows(2) {
        let er = board.layers[w[1]].eps_below;
        let shade = if er > 1.0 { c.scene.materials[0] } else { c.scene.vacuum };
        // Kept just inside its layers, so the copper on them is not lost in its faces.
        let inset = 0.02 * (z(w[1]) - z(w[0]));
        out.scene.cuboid(at(lo[0], lo[1], z(w[0]) + inset), at(hi[0], hi[1], z(w[1]) - inset), shade, 38);
    }
    out = out.merge(copper(board, &nets, &z, &colour));
    // The route's centre lines over its copper: a trace a few pixels wide would be all outline.
    // Lifted a little off the copper, so the depth test keeps them whole.
    let lift = 0.01 * thickness * stretch;
    for g in board.segments.iter().filter(|g| g.net == route) {
        if let Some(l) = board.layers.iter().position(|l| l.name == g.layer) {
            out.scene.line(at(g.start[0], g.start[1], z(l) + lift), at(g.end[0], g.end[1], z(l) + lift), nets_in.route, 255);
        }
    }
    // Vias: posts between their layers.
    for (i, v) in board.vias.iter().enumerate() {
        let ends: Vec<f64> = v.layers.iter().filter_map(|n| board.layers.iter().position(|l| &l.name == n)).map(&z).collect();
        let [a, b] = ends[..] else { continue };
        let r = 0.5 * v.diameter;
        let ring: Vec<[f64; 2]> = (0..VIA_SIDES)
            .map(|k| std::f64::consts::TAU * k as f64 / VIA_SIDES as f64)
            .map(|phi| [v.at[0] + r * phi.cos(), v.at[1] + r * phi.sin()])
            .collect();
        out.scene.tagged(Pick::Via(i).tag(), |scene| {
            for k in 0..VIA_SIDES {
                let (p, q) = (ring[k], ring[(k + 1) % VIA_SIDES]);
                let (bottom, top) = (a.min(b), a.max(b));
                scene.quad([at(p[0], p[1], bottom), at(q[0], q[1], bottom), at(q[0], q[1], top), at(p[0], p[1], top)], colour(&v.net), 255);
            }
        });
    }
    // The region the finite elements solve.
    if let Some(r) = &board.region {
        // The board stretched, the air about it as it is.
        let (bottom, top) = (-r.below, thickness * stretch + r.above);
        let corners = |h: f64| [at(r.lo[0], r.lo[1], h), at(r.hi[0], r.lo[1], h), at(r.hi[0], r.hi[1], h), at(r.lo[0], r.hi[1], h)];
        let (b, t2) = (corners(bottom), corners(top));
        for k in 0..4 {
            for (p, q) in [(b[k], b[(k + 1) % 4]), (t2[k], t2[(k + 1) % 4]), (b[k], t2[k])] {
                out.scene.line(p, q, c.wire, 200);
            }
        }
    }
    // The layers' names at the board's left edge.
    for (i, l) in board.layers.iter().enumerate() {
        out.labels.push(Label { at: at(lo[0], hi[1], z(i)), text: l.name.clone(), color: c.wire, facing: None, anchor: None });
    }
    // No route chosen yet: no swatch without a name.
    if !route.is_empty() {
        out.legend.push((route.to_owned(), nets_in.route));
    }
    if !reference.is_empty() {
        out.legend.push((reference.to_owned(), nets_in.reference));
    }
    if stretch > 1.0 {
        out.legend.push((trf(t!("heights × {k}"), args!(k = format!("{stretch:.0}"))), c.wire));
    }
    let (bottom, top) = board.region.map_or((0.0, thickness * stretch), |r| (-r.below, thickness * stretch + r.above));
    let (rlo, rhi) = board.region.map_or((lo, hi), |r| (r.lo, r.hi));
    out.bounds = Some((at(rlo[0].min(lo[0]), rlo[1].min(lo[1]), bottom), at(rhi[0].max(hi[0]), rhi[1].max(hi[1]), top)));
    out
}

/// Each layer's copper at its height, tagged by net: the planar mesher's union, or the traces
/// one by one when it refuses the board.
/// A board's region made the model, before it is meshed: its substrates and copper where the
/// mesh will put them (the region's lower-left corner the origin, the board's plane the model's
/// x–z, the lowest layer `below` over the floor), unstretched.
pub(super) fn in_model(c: &super::style::Colours, board: &Board) -> Built {
    let mut out = Built::default();
    let Some(region) = board.region else { return out };
    let mut b = board.clone();
    let shift = |p: &mut [f64; 2]| {
        p[0] -= region.lo[0];
        p[1] -= region.lo[1];
    };
    for g in &mut b.segments {
        shift(&mut g.start);
        shift(&mut g.end);
    }
    for c in &mut b.copper {
        c.polygon.iter_mut().for_each(shift);
    }
    for v in &mut b.vias {
        shift(&mut v.at);
    }
    let heights = b.heights();
    let z = |l: usize| region.below + heights.get(l).copied().unwrap_or(0.0);
    let (w, d) = (region.hi[0] - region.lo[0], region.hi[1] - region.lo[1]);
    let mut up: Vec<usize> = (0..b.layers.len()).collect();
    up.sort_by(|&p, &q| heights[p].total_cmp(&heights[q]));
    for pair in up.windows(2) {
        if b.layers[pair[1]].eps_below > 1.0 {
            let inset = 0.02 * (z(pair[1]) - z(pair[0]));
            out.scene.cuboid(at(0.0, 0.0, z(pair[0]) + inset), at(w, d, z(pair[1]) - inset), c.scene.materials[0], 60);
        }
    }
    let nets = nets(&b);
    let strip = c.scene.strip;
    out = out.merge(copper_in(&b, &nets, &z, &|_| strip, ([0.0, 0.0], [w, d])));
    out
}

fn copper(board: &Board, nets: &[String], z: &dyn Fn(usize) -> f64, colour: &dyn Fn(&str) -> Rgb) -> Built {
    board.bounds().map_or_else(Built::default, |(lo, hi)| copper_in(board, nets, z, colour, (lo, hi)))
}

/// [`copper`] cut to the box `lo`..`hi` (m, the board's plane).
fn copper_in(board: &Board, nets: &[String], z: &dyn Fn(usize) -> f64, colour: &dyn Fn(&str) -> Rgb, (lo, hi): ([f64; 2], [f64; 2])) -> Built {
    let mut out = Built::default();
    let net = |n: &str| nets.binary_search_by(|x| x.as_str().cmp(n)).unwrap_or(0) as u32;
    let layer = |n: &str| board.layers.iter().position(|l| l.name == n);
    let mut outlines = Vec::new();
    for g in &board.segments {
        if let Some(l) = layer(&g.layer) {
            outlines.push(Outline { layer: l, net: net(&g.net), ring: offset::trace(g.start, g.end, g.width, 0.02 * g.width) });
        }
    }
    for c in &board.copper {
        if let Some(l) = layer(&c.layer) {
            outlines.push(Outline { layer: l, net: net(&c.net), ring: c.polygon.clone() });
        }
    }
    for v in &board.vias {
        for l in v.layers.iter().filter_map(|n| layer(n)) {
            outlines.push(Outline { layer: l, net: net(&v.net), ring: offset::disc(v.at, v.diameter, 0.02 * v.diameter) });
        }
    }
    // Planes see-through, routes solid.
    let alpha = |name: &str| if board.segments.iter().any(|g| g.net == name) { 255 } else { PLANE_ALPHA };
    let margin = 1e-3 * (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1e-6);
    let (lo, hi) = ([lo[0] - margin, lo[1] - margin], [hi[0] + margin, hi[1] + margin]);
    let size = (hi[0] - lo[0]).max(hi[1] - lo[1]);
    let names: Vec<String> = board.layers.iter().map(|l| l.name.clone()).collect();
    let request = Request { lo, hi, layers: &names, nets, outlines: &outlines, coarse: &[], lines: &[], size, edge_size: size };
    match triangulate(&request) {
        Ok(planar) => {
            for (l, covered) in planar.nets.iter().enumerate() {
                let h = z(l);
                for (t, &n) in planar.triangles.iter().zip(covered) {
                    let Some(name) = nets.get(n as usize).filter(|_| n != NO_NET) else { continue };
                    let [a, b, c] = t.map(|v| planar.points.get(v as usize).copied().unwrap_or([0.0; 2]));
                    out.scene.tagged(Pick::Net(n as usize).tag(), |scene| {
                        scene.tri(at(a[0], a[1], h), at(b[0], b[1], h), at(c[0], c[1], h), colour(name), alpha(name))
                    });
                }
            }
        }
        Err(_) => {
            for o in &outlines {
                let name = nets.get(o.net as usize).map_or("", String::as_str);
                let h = z(o.layer);
                let Some(&first) = o.ring.first() else { continue };
                out.scene.tagged(Pick::Net(o.net as usize).tag(), |scene| {
                    for w in o.ring.windows(2).skip(1) {
                        scene.tri(at(first[0], first[1], h), at(w[0][0], w[0][1], h), at(w[1][0], w[1][1], h), colour(name), alpha(name));
                    }
                });
            }
        }
    }
    out
}
