//! A board's region solved by the finite elements. `pcb.region` sets the box: a shielded solid
//! of air (its sides carry the ports, made with `model.port.create` on its faces) with the
//! region's lower-left corner at the origin, the board's plane the model's x–z (its y the
//! model's z) and its lowest layer `below` over the floor (the model's y up). `mesh.generate` then meshes it with the planar mesher (`emcraft_mesh::planar`): the
//! copper of every layer in one triangulation, extruded through the substrates (each a material,
//! `substrate εr …`) and the air, the copper conducting sheets inside.

use emcraft_mesh::planar::extrude::{Hole, Slab, Stack, extrude};
use emcraft_mesh::planar::{Outline, Request, offset, triangulate};
use emcraft_model::pcb::{Board, Region, Segment};
use emcraft_model::shape::{BoxSize, Shape, Solid};
use emcraft_model::{End, LumpedPort, Material, Ohms, Point3, Vec3};
use serde_json::{Value, json};

use crate::meshing::Sizes;
use crate::{EngineError, Params, Result, Session};

/// The air over and under a board when `pcb.region` leaves it out: this many thicknesses of the
/// thinnest outer dielectric (the outer traces' fields reach that far).
const AIR: f64 = 5.0;
/// The margin round the copper when `pcb.region` leaves the corners out: as many.
const MARGIN: f64 = 5.0;
/// How much each cell of the air grows over the one nearer the board.
const AIR_GROWTH: f64 = 1.4;
/// How much each cell across a substrate grows over the one nearer its copper.
const SUBSTRATE_GROWTH: f64 = 1.3;
/// The chords of a trace's round ends stay within this fraction of its width of the circle.
const SAG: f64 = 2e-3;

fn failed(why: impl std::fmt::Display) -> EngineError {
    EngineError::failed(why)
}

/// The scale of the fields round the outer traces: the thinnest dielectric next to an outer
/// layer (m), the board's thickness for a board of one layer.
fn outer_gap(b: &Board) -> f64 {
    let mut h = b.heights();
    h.sort_by(f64::total_cmp);
    match (h.first(), h.get(1), h.get(h.len().saturating_sub(2)), h.last()) {
        (Some(a), Some(b2), Some(c), Some(d)) if h.len() >= 2 => (b2 - a).min(d - c),
        _ => thickness(b),
    }
}

/// The board's thickness: from its lowest layer to its highest (m).
fn thickness(b: &Board) -> f64 {
    b.heights().into_iter().fold(0.0, f64::max)
}

/// The material name of the dielectric below layer `l`: its permittivity, and its loss tangent
/// when it has one.
fn substrate(l: &emcraft_model::pcb::Layer) -> String {
    match l.loss_below {
        0.0 => format!("substrate εr {}", l.eps_below),
        tan_d => format!("substrate εr {} tanδ {tan_d}", l.eps_below),
    }
}

/// Whether the dielectric below layer `l` is air (the region's own vacuum).
fn is_air(l: &emcraft_model::pcb::Layer) -> bool {
    l.eps_below == 1.0 && l.loss_below == 0.0
}

/// `pcb.region`: the box solved by the finite elements, round the copper or a net's routes by a
/// margin, or between given corners; the model's geometry becomes that box (its ports cleared).
pub fn region(s: &mut Session, p: &Params) -> Result<Value> {
    let board = s.model.pcb.clone().ok_or_else(|| failed("load a PCB first: pcb.import, pcb.load or pcb.sample"))?;
    // The fields of the outer traces reach a few of their dielectrics' thicknesses out.
    let t = outer_gap(&board);
    let margin = p.length("margin").check(|m| *m >= 0.0, "must be ≥ 0").or(MARGIN * t)?;
    let around = match p.string("net").optional()? {
        Some(net) => {
            let ends: Vec<[f64; 2]> = board.segments.iter().filter(|g| g.net == net).flat_map(|g| [g.start, g.end]).collect();
            if ends.is_empty() {
                return Err(p.error("net", "name a routed net of the board"));
            }
            let lo = ends.iter().fold([f64::INFINITY; 2], |l, q| [l[0].min(q[0]), l[1].min(q[1])]);
            let hi = ends.iter().fold([f64::NEG_INFINITY; 2], |h, q| [h[0].max(q[0]), h[1].max(q[1])]);
            (lo, hi)
        }
        None => board.bounds().ok_or_else(|| failed("the board has no copper"))?,
    };
    let corner = |name: &str, fallback: [f64; 2]| -> Result<[f64; 2]> {
        match p.lengths(name).optional()? {
            Some(v) => <[f64; 2]>::try_from(v.as_slice()).map_err(|_| p.error(name, "give two lengths [x, y]")),
            None => Ok(fallback),
        }
    };
    let lo = corner("lo", [around.0[0] - margin, around.0[1] - margin])?;
    let hi = corner("hi", [around.1[0] + margin, around.1[1] + margin])?;
    let above = p.length("above").check(|h| *h >= 0.0, "must be ≥ 0").or(AIR * t)?;
    let below = p.length("below").check(|h| *h >= 0.0, "must be ≥ 0").or(AIR * t)?;
    let region = Region { lo, hi, above, below };
    region.check()?;
    install(s, board, region)?;
    let show = |v: [f64; 2]| json!([s.show_length(v[0]), s.show_length(v[1])]);
    Ok(json!({"lo": show(lo), "hi": show(hi), "above": s.show_length(above), "below": s.show_length(below)}))
}

/// The model's geometry the box of `board`'s `region`, its substrates materials.
fn install(s: &mut Session, mut board: Board, region: Region) -> Result<()> {
    let height = region.below + thickness(&board) + region.above;
    // The board's plane is the model's x–z, its height y (the 3D view's up).
    let size = BoxSize::new(Vec3::new(region.hi[0] - region.lo[0], height, region.hi[1] - region.lo[1]))?;
    s.model.set_solid(Solid { shape: Shape::Box { size }, material: "vacuum".into() })?;
    for l in board.layers.iter().filter(|l| !is_air(l)) {
        let name = substrate(l);
        if s.model.material(&name).is_err() {
            s.model.add_material(Material::new(&name, l.eps_below, 1.0)?.with_loss_tangent(l.loss_below)?)?;
        }
    }
    s.model.ports.clear();
    board.region = Some(region);
    s.model.pcb = Some(board);
    Ok(())
}

/// Whether the model is a board's region (meshed by [`generate`]).
pub(crate) fn has_region(s: &Session) -> bool {
    s.model.pcb.as_ref().is_some_and(|b| b.region.is_some())
}

/// `mesh.generate` on a board's region: `size` in the plane far from copper, `strip_edge` (a
/// fraction of it) at the copper's edges, by default a sixth of the narrowest trace or a quarter
/// of the thinnest substrate (the fields are singular at a strip's edges). The board's copper in
/// one planar triangulation ([`Copper::of`]), extruded through its stack ([`stack`]) with its
/// vias' barrels taken out ([`barrels`]), laid in the model's axes.
pub(crate) fn generate(s: &mut Session, p: &Params, sizes: &Sizes) -> Result<Value> {
    let board = s.model.pcb.clone().ok_or_else(|| failed("no board"))?;
    let region = board.region.ok_or_else(|| failed("no region: set one with pcb.region"))?;
    let size = sizes.default.get();
    let edge = match p.number("strip_edge").check(|f| *f > 0.0 && *f <= 1.0, "between 0 and 1").optional()? {
        Some(f) => f * size,
        None => edge_size(&board, &region, size),
    };
    let copper = Copper::of(&board, &region, &s.model.lumped)?;
    let w = [region.hi[0] - region.lo[0], region.hi[1] - region.lo[1]];
    // An open boundary's layer: its inner box a line of the mesh in the plane (and a level
    // across the air, below), so no prism straddles where the layer begins.
    let layer = s.model.open.as_ref().map(|o| o.thickness.get());
    let lines: Vec<[[f64; 2]; 2]> = match layer {
        Some(d) if !(2.0 * d < w[0] && 2.0 * d < w[1] && d < region.above && d < region.below) => {
            return Err(failed("the open boundary's layer must be thinner than the air above and below the board and than half the region's sides"));
        }
        Some(d) => {
            let c = [[d, d], [w[0] - d, d], [w[0] - d, w[1] - d], [d, w[1] - d]];
            (0..4).map(|k| [c[k], c[(k + 1) % 4]]).collect()
        }
        None => Vec::new(),
    };
    let planar = triangulate(&Request {
        lo: [0.0, 0.0],
        hi: w,
        layers: &copper.layers,
        nets: &copper.nets,
        outlines: &copper.fine,
        coarse: &copper.coarse,
        lines: &lines,
        size,
        edge_size: edge,
    })?;
    let material = |l: &emcraft_model::pcb::Layer| -> Result<u32> { if is_air(l) { Ok(0) } else { Ok(s.model.material(&substrate(l))?.0 as u32) } };
    let (slabs, sheets) = stack(&board, &region, edge, size, layer, material)?;
    let holes = barrels(&board, &copper, &planar, &sheets, s.display.length)?;
    let (mut mesh, sheet_nets) = extrude(&planar, &Stack { z0: 0.0, slabs, sheets, holes })?;
    // Into the model's axes: the board's (x, y, height) is (x, z, y); the swap mirrors, so
    // each tetrahedron's last two nodes swap to keep its volume positive.
    for p in &mut mesh.nodes {
        *p = Point3::new(p.x, p.z, p.y);
    }
    for t in &mut mesh.tets {
        t.swap(2, 3);
    }
    // Binned for locating points (fields, probes).
    mesh.buckets = Some(emcraft_mesh::solid::buckets(&mesh));
    let reply = json!({
        "nodes": mesh.nodes.len(),
        "tetrahedra": mesh.tets.len(),
        "triangles": planar.triangles.len(),
        "copper_faces": sheet_nets.len(),
        "edge_size": s.show_length(edge),
    });
    s.model.set_mesh(mesh);
    Ok(reply)
}

/// The board's layers from the lowest up (indices into its layers).
fn upwards(board: &Board) -> Vec<usize> {
    let heights = board.heights();
    let mut up: Vec<usize> = (0..board.layers.len()).collect();
    up.sort_by(|&a, &b| heights[a].total_cmp(&heights[b]));
    up
}

/// The copper's edge size by default: a sixth of the narrowest trace in the region, a quarter of
/// the thinnest substrate, a quarter of `size`, whichever is least (but no less than `size`/200).
fn edge_size(board: &Board, region: &Region, size: f64) -> f64 {
    let heights = board.heights();
    let thinnest = upwards(board).windows(2).map(|w| heights[w[1]] - heights[w[0]]).fold(f64::INFINITY, f64::min);
    let inside = |g: &&Segment| {
        let (lo, hi) = ([g.start[0].min(g.end[0]), g.start[1].min(g.end[1])], [g.start[0].max(g.end[0]), g.start[1].max(g.end[1])]);
        lo[0] <= region.hi[0] && hi[0] >= region.lo[0] && lo[1] <= region.hi[1] && hi[1] >= region.lo[1]
    };
    let narrowest = board.segments.iter().filter(inside).map(|g| g.width).fold(f64::INFINITY, f64::min);
    (0.25 * size).min(narrowest / 6.0).min(0.25 * thinnest).max(size / 200.0)
}

/// A board's copper as the planar mesher takes it, the region's lower-left corner at the origin:
/// the traces (cut square where a sheet port feeds them) and pads, fine at their edges; the pours,
/// fine near those; and each via's barrel on a layer of its own after the board's, as a "net" of
/// its own (so the triangles it covers are known).
struct Copper {
    layers: Vec<String>,
    nets: Vec<String>,
    fine: Vec<Outline>,
    coarse: Vec<Outline>,
    /// The barrels' layer, and the first barrel's net (barrel i is `first_barrel + i`).
    barrels: usize,
    first_barrel: u32,
}

impl Copper {
    fn of(board: &Board, region: &Region, lumped: &[LumpedPort]) -> Result<Copper> {
        // Nets in the order of their names.
        let mut nets: Vec<String> = board
            .segments
            .iter()
            .map(|g| &g.net)
            .chain(board.copper.iter().map(|c| &c.net))
            .chain(board.vias.iter().map(|v| &v.net))
            .cloned()
            .collect();
        nets.sort();
        nets.dedup();
        let net = |n: &str| nets.binary_search_by(|x| x.as_str().cmp(n)).map(|i| i as u32).map_err(|_| failed("a net without a name"));
        let layer = |n: &str| layer_index(board, n);
        let at = |q: [f64; 2]| [q[0] - region.lo[0], q[1] - region.lo[1]];
        // A trace fed by a sheet port at an end stops there: no round end beyond the feed.
        let fed = |q: [f64; 2]| {
            let q = at(q);
            lumped.iter().any(|l| l.across.is_some() && (l.to.x - q[0]).hypot(l.to.z - q[1]) <= 1e-9)
        };
        let mut fine = Vec::with_capacity(board.segments.len() + 2 * board.vias.len());
        for g in &board.segments {
            let ring = offset::trace_ends(at(g.start), at(g.end), g.width, SAG * g.width, [!fed(g.start), !fed(g.end)]);
            fine.push(Outline { layer: layer(&g.layer)?, net: net(&g.net)?, ring });
        }
        let coarse = board
            .copper
            .iter()
            .map(|c| Ok(Outline { layer: layer(&c.layer)?, net: net(&c.net)?, ring: c.polygon.iter().map(|q| at(*q)).collect() }))
            .collect::<Result<_>>()?;
        let barrels = board.layers.len();
        let first_barrel = nets.len() as u32;
        let mut barrel_nets = Vec::with_capacity(board.vias.len());
        for (i, v) in board.vias.iter().enumerate() {
            for l in &v.layers {
                fine.push(Outline { layer: layer(l)?, net: net(&v.net)?, ring: offset::disc(at(v.at), v.diameter, SAG * v.diameter) });
            }
            barrel_nets.push(format!("via {} ({})", i + 1, v.net));
            fine.push(Outline { layer: barrels, net: first_barrel + i as u32, ring: offset::disc(at(v.at), v.barrel(), SAG * v.barrel()) });
        }
        nets.extend(barrel_nets);
        let layers = board.layers.iter().map(|l| l.name.clone()).chain(std::iter::once("vias".to_owned())).collect();
        Ok(Copper { layers, nets, fine, coarse, barrels, first_barrel })
    }
}

/// The index of the board's layer `name`.
fn layer_index(board: &Board, name: &str) -> Result<usize> {
    board.layers.iter().position(|l| l.name == name).ok_or_else(|| failed(format!("no layer {name}")))
}

/// The stack from the floor up — the open boundary's layer (`layer`, its thickness), the air,
/// the substrates, the air, the layer — and each copper layer's interface in it. A substrate is
/// two halves, each fine at a layer with traces (the fields' edge singularities) and growing to
/// the middle, at a plane alone from a few edge sizes; the air grows away from the board.
fn stack(
    board: &Board,
    region: &Region,
    edge: f64,
    size: f64,
    layer: Option<f64>,
    material: impl Fn(&emcraft_model::pcb::Layer) -> Result<u32>,
) -> Result<(Vec<Slab>, Vec<(usize, usize)>)> {
    const VACUUM: u32 = 0;
    // A slab `h` thick whose cells grow by `growth` from `first` at its bottom (`up`) or top.
    let graded = |h: f64, material: u32, first: f64, growth: f64, up: bool| {
        let mut n = 1;
        while n < 64 && first * (growth.powi(n as i32) - 1.0) / (growth - 1.0) < h {
            n += 1;
        }
        Slab { thickness: h, material, cells: n, ratio: if up { growth } else { 1.0 / growth } }
    };
    let air = |h: f64, up: bool| graded(h, VACUUM, 2.0 * edge, AIR_GROWTH, up);
    // The layer's air in slabs of their own, about square with the size far out.
    let layer_slab = |d: f64| Slab { thickness: d, material: VACUUM, cells: ((d / size).ceil() as usize).clamp(2, 16), ratio: 1.0 };
    let depth = layer.unwrap_or(0.0);
    let heights = board.heights();
    let up = upwards(board);
    let mut slabs: Vec<Slab> = layer.map(layer_slab).into_iter().collect();
    if region.below > 0.0 {
        slabs.push(air(region.below - depth, false));
    }
    let mut sheets = up.first().map(|&l| (l, slabs.len())).into_iter().collect::<Vec<_>>();
    let routed = |l: usize| board.segments.iter().any(|g| g.layer == board.layers[l].name);
    for w in up.windows(2) {
        let (lower, upper) = (w[0], w[1]);
        let h = heights[upper] - heights[lower];
        // A layer's dielectric lies below it, down to the next layer.
        let m = material(&board.layers[upper])?;
        let first = |l: usize| if routed(l) { edge } else { (4.0 * edge).min(h / 4.0) };
        slabs.push(graded(h / 2.0, m, first(lower), SUBSTRATE_GROWTH, true));
        slabs.push(graded(h / 2.0, m, first(upper), SUBSTRATE_GROWTH, false));
        sheets.push((upper, slabs.len()));
    }
    if region.above > 0.0 {
        slabs.push(air(region.above - depth, true));
    }
    slabs.extend(layer.map(layer_slab));
    Ok((slabs, sheets))
}

/// Each via's barrel as a hole through the stack between its layers' interfaces (`sheets`),
/// refused where it would pass through another net's copper on a layer between its ends (the
/// copper needs a clearance there: the barrel would join the nets).
fn barrels(
    board: &Board,
    copper: &Copper,
    planar: &emcraft_mesh::planar::Planar,
    sheets: &[(usize, usize)],
    length: emcraft_model::units::Length,
) -> Result<Vec<Hole>> {
    let heights = board.heights();
    let interface_of = |name: &str| layer_index(board, name).ok().and_then(|l| sheets.iter().find(|s| s.0 == l)).map(|s| s.1);
    let covered = planar.nets.get(copper.barrels).map(Vec::as_slice).unwrap_or(&[]);
    let mut holes = Vec::with_capacity(board.vias.len());
    for (i, v) in board.vias.iter().enumerate() {
        let (Some(from), Some(to)) = (interface_of(&v.layers[0]), interface_of(&v.layers[1])) else { continue };
        let own = copper.first_barrel + i as u32;
        let triangles: Vec<u32> = (0..covered.len() as u32).filter(|&t| covered[t as usize] == own).collect();
        let (a, b) = (heights[layer_index(board, &v.layers[0])?], heights[layer_index(board, &v.layers[1])?]);
        for (l, name) in board.layers.iter().enumerate().filter(|(l, _)| heights[*l] > a.min(b) && heights[*l] < a.max(b)) {
            let on = planar.nets.get(l).map(Vec::as_slice).unwrap_or(&[]);
            let other = triangles
                .iter()
                .filter_map(|&t| on.get(t as usize))
                .find(|&&n| n != emcraft_mesh::planar::NO_NET && copper.nets.get(n as usize) != Some(&v.net));
            if let Some(&other) = other {
                let other = copper.nets.get(other as usize).cloned().unwrap_or_default();
                return Err(failed(format!(
                    "the via of {} at ({:.3}, {:.3}) passes through {other}'s copper on {} with no clearance: give the pour a hole round it",
                    v.net,
                    length.from_si(v.at[0]),
                    length.from_si(v.at[1]),
                    name.name
                )));
            }
        }
        if !triangles.is_empty() {
            holes.push(Hole { triangles, from, to });
        }
    }
    Ok(holes)
}

/// `pcb.port`: a lumped port at a trace's end, a sheet the trace's width wide from its reference
/// to it — the reference the nearest layer below with another net's copper under that end, else
/// the nearest above, else the box's floor — its source in series with `impedance` (50 Ω by
/// default). The trace is cut square at
/// the port when meshed. Without waveguide ports,
/// solve.frequency reads S of the lumped ports.
pub fn port(s: &mut Session, p: &Params) -> Result<Value> {
    let board = s.model.pcb.clone().ok_or_else(|| failed("load a PCB first"))?;
    let region = board.region.ok_or_else(|| failed("set the region first: pcb.region"))?;
    let name = p.string("name").required()?.to_owned();
    let id = p.string("segment").required()?;
    let g = board.segments.iter().find(|g| g.id == id).ok_or_else(|| p.error("segment", "name a segment of the board"))?;
    let at = g.at(p.pick("end").or(End::Start)?);
    let heights = board.heights();
    let layer = board.layers.iter().position(|l| l.name == g.layer).ok_or_else(|| failed("the segment's layer is missing"))?;
    let top = heights[layer];
    // The nearest layer below with copper of another net there (a pour, a trace), else the floor.
    let covered = |l: usize| {
        let name = &board.layers[l].name;
        board.copper.iter().any(|c| &c.layer == name && c.net != g.net && inside(&c.polygon, at))
            || board
                .segments
                .iter()
                .any(|x| &x.layer == name && x.net != g.net && emcraft_model::pcb::distance(near_point(x.start, x.end, at), at) <= 0.5 * x.width)
    };
    // The nearest such layer below, else above (a bottom-side trace over the plane above it),
    // else the floor.
    let below = (0..board.layers.len()).filter(|&l| heights[l] < top && covered(l)).max_by(|&a, &b| heights[a].total_cmp(&heights[b]));
    let above = (0..board.layers.len()).filter(|&l| heights[l] > top && covered(l)).min_by(|&a, &b| heights[a].total_cmp(&heights[b]));
    let reference = below.or(above);
    let bottom = reference.map_or(-region.below, |l| heights[l]);
    let (x, y) = (at[0] - region.lo[0], at[1] - region.lo[1]);
    let d = [g.end[0] - g.start[0], g.end[1] - g.start[1]];
    let len = d[0].hypot(d[1]);
    let across = Vec3::new(-d[1] / len, 0.0, d[0] / len) * (0.5 * g.width);
    let impedance: Ohms = p.checked("impedance").or(Ohms::new(50.0)?)?;
    s.model.add_lumped_port(LumpedPort {
        name: name.clone(),
        from: Point3::new(x, region.below + bottom, y),
        to: Point3::new(x, region.below + top, y),
        impedance,
        voltage: [1.0, 0.0],
        radius: 0.0,
        across: Some(across),
    })?;
    let reference = reference.map_or_else(|| "floor".to_owned(), |l| board.layers[l].name.clone());
    Ok(json!({"name": name, "reference": reference, "lumped_ports": s.model.lumped.len()}))
}

/// `pcb.element`: a lumped element on a layer of the board's region, from one board point to
/// another (a chip resistor bridging two traces), its parts in Ω, nH and pF.
pub fn element(s: &mut Session, p: &Params) -> Result<Value> {
    let board = s.model.pcb.clone().ok_or_else(|| failed("load a PCB first"))?;
    let region = board.region.ok_or_else(|| failed("set the region first: pcb.region"))?;
    let layer = p.string("layer").required()?;
    let l = board.layers.iter().position(|x| x.name == layer).ok_or_else(|| p.error("layer", "name a layer of the board"))?;
    let height = region.below + board.heights()[l];
    let unit = s.display.length;
    let point = |name: &str| -> Result<Point3> {
        let list = p.list(name, 2).required()?;
        let xy: Vec<f64> = list.iter().map(|v| p.number_of(name, v).map(|x| unit.to_si(x))).collect::<Result<_>>()?;
        match xy[..] {
            [x, y] => Ok(Point3::new(x - region.lo[0], height, y - region.lo[1])),
            _ => Err(p.error(name, "is a board point [x, y]")),
        }
    };
    let (from, to) = (point("from")?, point("to")?);
    let d = to - from;
    let across = match p.positive_length("width").optional()? {
        Some(w) if d.norm() > 0.0 => Some(Vec3::new(-d.z, 0.0, d.x) * (0.5 * w / d.norm())),
        _ => None,
    };
    let part = |name: &str, si: f64| -> Result<f64> {
        let v = p.number(name).or(0.0)?;
        if !(v.is_finite() && v >= 0.0) {
            return Err(p.error(name, "must be a finite number ≥ 0"));
        }
        Ok(v * si)
    };
    s.model.add_element(emcraft_model::LumpedElement {
        name: p.string("name").required()?.to_owned(),
        from,
        to,
        resistance: part("resistance", 1.0)?,
        inductance: part("inductance", 1e-9)?,
        capacitance: part("capacitance", 1e-12)?,
        arrangement: p.pick("arrangement").or(emcraft_model::Arrangement::Series)?,
        across,
    })?;
    Ok(json!({"elements": s.model.elements.len()}))
}

/// The point of segment `a`–`b` nearest `p`.
fn near_point(a: [f64; 2], b: [f64; 2], p: [f64; 2]) -> [f64; 2] {
    let d = [b[0] - a[0], b[1] - a[1]];
    let len2 = d[0] * d[0] + d[1] * d[1];
    let t = if len2 > 0.0 { (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / len2).clamp(0.0, 1.0) } else { 0.0 };
    [a[0] + t * d[0], a[1] + t * d[1]]
}

/// Whether `p` lies inside `polygon` (even–odd, as the board's copper is read).
fn inside(polygon: &[[f64; 2]], p: [f64; 2]) -> bool {
    let mut odd = false;
    for (i, a) in polygon.iter().enumerate() {
        let b = polygon[(i + 1) % polygon.len()];
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]) {
            odd = !odd;
        }
    }
    odd
}

/// The two free ends of `net`'s route, where a port goes: (segment id, `start` or `end`) where
/// only one of its segments ends (to a nanometre), in the order of the segments.
///
/// ```
/// use emcraft_engine::Session;
/// use emcraft_engine::pcb_fem::route_ends;
/// let mut s = Session::new();
/// s.execute("pcb.sample", &serde_json::json!({})).unwrap();
/// let board = s.model().pcb.as_ref().unwrap();
/// // The sample's route is two segments end to end: its ends are the first's start and the
/// // second's end.
/// use emcraft_model::End;
/// assert_eq!(route_ends(board, "DATA"), vec![("signal:0".to_string(), End::Start), ("signal:1".to_string(), End::End)]);
/// assert!(route_ends(board, "nothing").is_empty());
/// ```
pub fn route_ends(board: &Board, net: &str) -> Vec<(String, End)> {
    let route: Vec<_> = board.segments.iter().filter(|g| g.net == net).collect();
    let meets = |q: [f64; 2]| route.iter().flat_map(|g| [g.start, g.end]).filter(|r| emcraft_model::pcb::distance(*r, q) <= 1e-9).count();
    route
        .iter()
        .flat_map(|g| [End::Start, End::End].map(|e| (g, g.at(e), e)))
        .filter(|(_, q, _)| meets(*q) == 1)
        .map(|(g, _, e)| (g.id.clone(), e))
        .collect()
}

/// An element size for `net`'s route swept up to `stop` (Hz): a tenth of the shortest
/// wavelength in the board's substrates, at most an eighth of the route's length (m).
///
/// ```
/// use emcraft_engine::Session;
/// use emcraft_engine::pcb_fem::route_size;
/// let mut s = Session::new();
/// s.execute("pcb.sample", &serde_json::json!({})).unwrap();
/// let board = s.model().pcb.as_ref().unwrap();
/// // The 40 mm route: an eighth of it at 1 GHz (λ in εr 4.2 is 146 mm), λ/10 = 0.73 mm at
/// // 20 GHz.
/// assert!((route_size(board, "DATA", 1e9) - 0.005).abs() < 1e-12);
/// assert!((route_size(board, "DATA", 20e9) - 0.000731).abs() < 1e-6);
/// ```
pub fn route_size(board: &Board, net: &str, stop: f64) -> f64 {
    let er = board.layers.iter().map(|l| l.eps_below).fold(1.0, f64::max);
    let length: f64 = board.segments.iter().filter(|g| g.net == net).map(|g| emcraft_model::pcb::distance(g.start, g.end)).sum();
    (emcraft_model::units::C0 / (stop * er.sqrt()) / 10.0).min(length / 8.0)
}
