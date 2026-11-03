//! A board's region solved by the finite elements. `pcb.region` sets the box: a shielded solid
//! of air (its sides carry the ports, made with `model.port.create` on its faces) with the
//! board's lower-left corner of the region at the origin and its lowest layer `below` over the
//! floor. `mesh.generate` then meshes it with the planar mesher (`emcraft_mesh::planar`): the
//! copper of every layer in one triangulation, extruded through the substrates (each a material,
//! `substrate εr …`) and the air, the copper conducting sheets inside.

use emcraft_mesh::planar::extrude::{Slab, Stack, extrude};
use emcraft_mesh::planar::{Outline, Request, offset, triangulate};
use emcraft_model::pcb::{Board, Region};
use emcraft_model::shape::{BoxSize, Shape, Solid};
use emcraft_model::{Material, Vec3};
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
    let size = BoxSize::new(Vec3::new(region.hi[0] - region.lo[0], region.hi[1] - region.lo[1], height))?;
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
    for g in &board.segments {
        outlines.push(Outline { layer: layer(&g.layer)?, net: net(&g.net)?, ring: offset::trace(at(g.start), at(g.end), g.width, SAG * g.width) });
    }
    for c in &board.copper {
        outlines.push(Outline { layer: layer(&c.layer)?, net: net(&c.net)?, ring: c.polygon.iter().map(|q| at(*q)).collect() });
    }
    for v in &board.vias {
        for l in &v.layers {
            outlines.push(Outline { layer: layer(l)?, net: net(&v.net)?, ring: offset::disc(at(v.at), v.diameter, SAG * v.diameter) });
        }
    }
    let names: Vec<String> = board.layers.iter().map(|l| l.name.clone()).collect();
    let w = [region.hi[0] - region.lo[0], region.hi[1] - region.lo[1]];
    let planar = triangulate(&Request { lo: [0.0, 0.0], hi: w, layers: &names, nets: &nets, outlines: &outlines, size, edge_size: edge })?;
    // The stack from the floor: air, the substrates, air; cells across a substrate about twice
    // the edge size, the air's growing away from the board.
    let vacuum = 0;
    let material = |er: f64| -> Result<u32> { if er == 1.0 { Ok(vacuum) } else { Ok(s.model.material(&substrate(er))?.0 as u32) } };
    let air = |h: f64, towards_board_first: bool| {
        let first = 2.0 * edge;
        let mut n = 1;
        while n < 64 && first * (AIR_GROWTH.powi(n as i32) - 1.0) / (AIR_GROWTH - 1.0) < h {
            n += 1;
        }
        let ratio = if towards_board_first { AIR_GROWTH } else { 1.0 / AIR_GROWTH };
        Slab { thickness: h, material: vacuum, cells: n, ratio }
    };
    let mut slabs = Vec::new();
    if region.below > 0.0 {
        slabs.push(air(region.below, false));
    }
    let mut sheets = vec![(up[0], slabs.len())];
    for (k, w2) in up.windows(2).enumerate() {
        // A layer's dielectric lies below it, down to the next layer.
        let er = board.layers[w2[1]].eps_below;
        let h = gaps[k];
        slabs.push(Slab { thickness: h, material: material(er)?, cells: ((h / (2.0 * edge)).ceil() as usize).clamp(1, 16), ratio: 1.0 });
        sheets.push((w2[1], slabs.len()));
    }
    if region.above > 0.0 {
        slabs.push(air(region.above, true));
    }
    let (mesh, sheet_nets) = extrude(&planar, &Stack { z0: 0.0, slabs, sheets })?;
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
