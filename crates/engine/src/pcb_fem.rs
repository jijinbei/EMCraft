//! A board's region solved by the finite elements. `pcb.region` sets the box: a shielded solid
//! of air (its sides carry the ports, made with `model.port.create` on its faces) with the
//! region's lower-left corner at the origin, the board's plane the model's x–z (its y the
//! model's z) and its lowest layer `below` over the floor (the model's y up). `mesh.generate` then meshes it with the planar mesher (`emcraft_mesh::planar`): the
//! copper of every layer in one triangulation, extruded through the substrates (each a material,
//! `substrate εr …`) and the air, the copper conducting sheets inside.

use emcraft_mesh::planar::extrude::{Hole, Slab, Stack, extrude};
use emcraft_mesh::planar::{Outline, Request, offset, triangulate};
use emcraft_model::pcb::{Board, Region};
use emcraft_model::shape::{BoxSize, Shape, Solid};
use emcraft_model::{LumpedPort, Material, Ohms, Point3, Vec3};
use serde_json::{Value, json};

use crate::meshing::Sizes;
use crate::{EngineError, Params, Result, Session};

/// The air over and under a board when `pcb.region` leaves it out: this many board thicknesses.
const AIR: f64 = 5.0;
/// The margin round the copper when `pcb.region` leaves the corners out: this many board
/// thicknesses.
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

/// The board's thickness: from its lowest layer to its highest (m).
fn thickness(b: &Board) -> f64 {
    b.heights().into_iter().fold(0.0, f64::max)
}

/// The material name of a substrate of permittivity `er`.
fn substrate(er: f64) -> String {
    format!("substrate εr {er}")
}

/// `pcb.region`: the box solved by the finite elements, round the copper or a net's routes by a
/// margin, or between given corners; the model's geometry becomes that box (its ports cleared).
pub fn region(s: &mut Session, p: &Params) -> Result<Value> {
    let board = s.model.pcb.clone().ok_or_else(|| failed("load a PCB first: pcb.import, pcb.load or pcb.sample"))?;
    let t = thickness(&board);
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
    for l in &board.layers {
        let name = substrate(l.eps_below);
        if l.eps_below != 1.0 && s.model.material(&name).is_err() {
            s.model.add_material(Material::new(&name, l.eps_below, 1.0)?)?;
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
/// of the thinnest substrate (the fields are singular at a strip's edges).
pub(crate) fn generate(s: &mut Session, p: &Params, sizes: &Sizes) -> Result<Value> {
    let board = s.model.pcb.clone().ok_or_else(|| failed("no board"))?;
    let region = board.region.ok_or_else(|| failed("no region: set one with pcb.region"))?;
    let size = sizes.default.get();
    let heights = board.heights();
    // The layers from the lowest up, and the substrates between them.
    let mut up: Vec<usize> = (0..board.layers.len()).collect();
    up.sort_by(|&a, &b| heights[a].total_cmp(&heights[b]));
    let gaps: Vec<f64> = up.windows(2).map(|w| heights[w[1]] - heights[w[0]]).collect();
    let thinnest = gaps.iter().copied().fold(f64::INFINITY, f64::min);
    let narrowest = board.segments.iter().map(|g| g.width).fold(f64::INFINITY, f64::min);
    let edge = match p.number("strip_edge").check(|f| *f > 0.0 && *f <= 1.0, "between 0 and 1").optional()? {
        Some(f) => f * size,
        None => (0.25 * size).min(narrowest / 6.0).min(0.25 * thinnest).max(size / 200.0),
    };
    // The copper, the region's lower-left corner at the origin; nets in the order of their names.
    let mut nets: Vec<String> = board
        .segments
        .iter()
        .map(|g| g.net.clone())
        .chain(board.copper.iter().map(|c| c.net.clone()))
        .chain(board.vias.iter().map(|v| v.net.clone()))
        .collect();
    nets.sort();
    nets.dedup();
    let net = |n: &str| nets.binary_search_by(|x| x.as_str().cmp(n)).map(|i| i as u32).map_err(|_| failed("a net without a name"));
    let layer = |n: &str| board.layers.iter().position(|l| l.name == n).ok_or_else(|| failed(format!("no layer {n}")));
    let at = |q: [f64; 2]| [q[0] - region.lo[0], q[1] - region.lo[1]];
    let mut outlines = Vec::new();
    // A trace fed by a sheet port at an end stops there: no round end beyond the feed.
    let fed = |q: [f64; 2]| {
        let q = at(q);
        s.model.lumped.iter().any(|l| l.across.is_some() && (l.to.x - q[0]).hypot(l.to.z - q[1]) <= 1e-9)
    };
    for g in &board.segments {
        let ring = offset::trace_ends(at(g.start), at(g.end), g.width, SAG * g.width, [!fed(g.start), !fed(g.end)]);
        outlines.push(Outline { layer: layer(&g.layer)?, net: net(&g.net)?, ring });
    }
    for c in &board.copper {
        outlines.push(Outline { layer: layer(&c.layer)?, net: net(&c.net)?, ring: c.polygon.iter().map(|q| at(*q)).collect() });
    }
    // Vias: their pads on their layers, their barrels on a layer of their own after the
    // board's (each its own "net", so the triangles it covers are known).
    let barrels = board.layers.len();
    let mut nets = nets.clone();
    let first_via = nets.len() as u32;
    for (i, v) in board.vias.iter().enumerate() {
        for l in &v.layers {
            outlines.push(Outline { layer: layer(l)?, net: net(&v.net)?, ring: offset::disc(at(v.at), v.diameter, SAG * v.diameter) });
        }
        nets.push(format!("via {} ({})", i + 1, v.net));
        outlines.push(Outline { layer: barrels, net: first_via + i as u32, ring: offset::disc(at(v.at), v.barrel(), SAG * v.barrel()) });
    }
    let mut names: Vec<String> = board.layers.iter().map(|l| l.name.clone()).collect();
    names.push("vias".into());
    let w = [region.hi[0] - region.lo[0], region.hi[1] - region.lo[1]];
    // An open boundary's layer: its inner box a line of the mesh in the plane (and a level
    // across the air, below), so no prism straddles where the layer begins.
    let layer_depth = s.model.open.as_ref().map(|o| o.thickness.get());
    let mut lines = Vec::new();
    if let Some(d) = layer_depth {
        if !(2.0 * d < w[0] && 2.0 * d < w[1] && d < region.above && d < region.below) {
            return Err(failed("the open boundary's layer must be thinner than the air above and below the board and than half the region's sides"));
        }
        let c = [[d, d], [w[0] - d, d], [w[0] - d, w[1] - d], [d, w[1] - d]];
        lines.extend((0..4).map(|k| [c[k], c[(k + 1) % 4]]));
    }
    let planar =
        triangulate(&Request { lo: [0.0, 0.0], hi: w, layers: &names, nets: &nets, outlines: &outlines, lines: &lines, size, edge_size: edge })?;
    // The stack from the floor: air, the substrates, air; cells across a substrate about twice
    // the edge size, the air's growing away from the board.
    let vacuum = 0;
    let material = |er: f64| -> Result<u32> { if er == 1.0 { Ok(vacuum) } else { Ok(s.model.material(&substrate(er))?.0 as u32) } };
    // A slab `h` thick whose cells grow by `growth` from `first` at its bottom (`up`) or top.
    let graded = |h: f64, material: u32, first: f64, growth: f64, up: bool| {
        let mut n = 1;
        while n < 64 && first * (growth.powi(n as i32) - 1.0) / (growth - 1.0) < h {
            n += 1;
        }
        Slab { thickness: h, material, cells: n, ratio: if up { growth } else { 1.0 / growth } }
    };
    let air = |h: f64, up: bool| graded(h, vacuum, 2.0 * edge, AIR_GROWTH, up);
    // The layer's air in slabs of their own, about square with the size far out.
    let layer_slab = |d: f64| Slab { thickness: d, material: vacuum, cells: ((d / size).ceil() as usize).clamp(2, 16), ratio: 1.0 };
    let mut slabs = Vec::new();
    if let Some(d) = layer_depth {
        slabs.push(layer_slab(d));
    }
    if region.below > 0.0 {
        slabs.push(air(region.below - layer_depth.unwrap_or(0.0), false));
    }
    let mut sheets = vec![(up[0], slabs.len())];
    for (k, w2) in up.windows(2).enumerate() {
        // A layer's dielectric lies below it, down to the next layer.
        let er = board.layers[w2[1]].eps_below;
        let h = gaps[k];
        // Two halves, each fine at its copper (the fields' edge singularities) and growing
        // to the middle.
        let m = material(er)?;
        slabs.push(graded(h / 2.0, m, edge, SUBSTRATE_GROWTH, true));
        slabs.push(graded(h / 2.0, m, edge, SUBSTRATE_GROWTH, false));
        sheets.push((w2[1], slabs.len()));
    }
    if region.above > 0.0 {
        slabs.push(air(region.above - layer_depth.unwrap_or(0.0), true));
    }
    if let Some(d) = layer_depth {
        slabs.push(layer_slab(d));
    }
    // Each via's barrel: its triangles' prisms out between its layers' interfaces.
    let interface_of = |name: &str| layer(name).ok().and_then(|l| sheets.iter().find(|s| s.0 == l)).map(|s| s.1);
    let mut holes = Vec::new();
    for (i, v) in board.vias.iter().enumerate() {
        let (Some(from), Some(to)) = (interface_of(&v.layers[0]), interface_of(&v.layers[1])) else { continue };
        let covered = planar.nets.get(barrels).into_iter().flatten().enumerate();
        let triangles: Vec<u32> = covered.filter(|(_, n)| **n == first_via + i as u32).map(|(t, _)| t as u32).collect();
        if !triangles.is_empty() {
            holes.push(Hole { triangles, from, to });
        }
    }
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

/// `pcb.port`: a lumped port at a trace's end, a sheet the trace's width wide from its reference
/// up to it — the reference the nearest layer below with copper under that end, else the box's
/// floor — its source in series with `impedance` (50 Ω by default). The trace is cut square at
/// the port when meshed. Without waveguide ports,
/// solve.frequency reads S of the lumped ports.
pub fn port(s: &mut Session, p: &Params) -> Result<Value> {
    let board = s.model.pcb.clone().ok_or_else(|| failed("load a PCB first"))?;
    let region = board.region.ok_or_else(|| failed("set the region first: pcb.region"))?;
    let name = p.string("name").required()?.to_owned();
    let id = p.string("segment").required()?;
    let g = board.segments.iter().find(|g| g.id == id).ok_or_else(|| p.error("segment", "name a segment of the board"))?;
    let end = p.string("end").check(|e| *e == "start" || *e == "end", "`start` or `end`").or("start")?;
    let at = if end == "start" { g.start } else { g.end };
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
    let reference = (0..board.layers.len()).filter(|&l| heights[l] < top && covered(l)).max_by(|&a, &b| heights[a].total_cmp(&heights[b]));
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
/// assert_eq!(route_ends(board, "DATA"), vec![("signal:0".to_string(), "start"), ("signal:1".to_string(), "end")]);
/// assert!(route_ends(board, "nothing").is_empty());
/// ```
pub fn route_ends(board: &Board, net: &str) -> Vec<(String, &'static str)> {
    let route: Vec<_> = board.segments.iter().filter(|g| g.net == net).collect();
    let meets = |q: [f64; 2]| route.iter().flat_map(|g| [g.start, g.end]).filter(|r| emcraft_model::pcb::distance(*r, q) <= 1e-9).count();
    route
        .iter()
        .flat_map(|g| [(g, g.start, "start"), (g, g.end, "end")])
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
