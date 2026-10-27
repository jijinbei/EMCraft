//! Meshing for EMCraft: structured tetrahedral meshes of a rectangular waveguide
//! ([`waveguide`]), the global numbering of edges and faces the vector elements need
//! ([`Topology`]), and point location on the structured grid ([`locate`]).
//!
//! Each grid cell is split into six tetrahedra around its main diagonal (the Kuhn, or
//! Freudenthal, triangulation of the cube: H. W. Kuhn, *Some combinatorial lemmas in topology*,
//! IBM J. Res. Dev. 4(5):518–524, 1960). Every cell face is then cut along the diagonal from its
//! lowest to its highest corner, the same in both cells that share it, so the mesh is conforming
//! without any search.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod adapt;
pub mod csg;
pub mod solid;
mod topology;

pub use topology::{LOCAL_EDGES, LOCAL_FACES, Topology};

use emcraft_model::geom::triple;
use emcraft_model::mesh::TETS_PER_CELL;
use emcraft_model::{Aabb, Boundary, BoundaryFace, Grid, Mesh, Model, Point3, Span, SymmetryKind, positive};

/// The most tetrahedra a mesh may have (a cap on hostile sizes).
pub const MAX_TETS: usize = 3_000_000;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum MeshError {
    #[error("invalid {what}: {why}")]
    Invalid { what: String, why: String },
    #[error("the mesh would have {tets} tetrahedra, more than the limit of {limit}: use a larger element size")]
    TooLarge { tets: usize, limit: usize },
    #[error(transparent)]
    Model(#[from] emcraft_model::ModelError),
}

pub type Result<T> = std::result::Result<T, MeshError>;

/// Divisions of a length by a target element size: at least `min`.
fn divisions(length: f64, size: f64, min: usize) -> usize {
    let n = (length / size * (1.0 - 1e-9)).ceil();
    if n.is_finite() && n >= 1.0 { (n as usize).max(min) } else { min }
}

/// Coordinates `0, l/n, …, l`.
fn line(start: f64, length: f64, n: usize) -> impl Iterator<Item = f64> {
    (0..=n).map(move |i| if i == n { start + length } else { start + length * i as f64 / n as f64 })
}

/// The six tetrahedra of a cell as corner offsets (dx, dy, dz) ∈ {0,1}³ encoded as dx + 2dy + 4dz:
/// one per axis order of the path from corner 0 to corner 7.
const KUHN: [[usize; 4]; TETS_PER_CELL] = {
    const AXES: [[usize; 3]; 6] = [[1, 2, 4], [1, 4, 2], [2, 1, 4], [2, 4, 1], [4, 1, 2], [4, 2, 1]];
    let mut out = [[0; 4]; 6];
    let mut i = 0;
    while i < 6 {
        let a = AXES[i];
        out[i] = [0, a[0], a[0] + a[1], 7];
        i += 1;
    }
    out
};

fn signed_volume(p: &[Point3; 4]) -> f64 {
    triple(p[1] - p[0], p[2] - p[0], p[3] - p[0]) / 6.0
}

/// How finely a waveguide is meshed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    /// The element size away from edges (m).
    pub size: f64,
    /// The element size at the re-entrant edges of steps and irises, as a fraction of `size`
    /// (1: no grading).
    pub edge: f64,
    /// How fast elements grow away from those edges: steps grow by this ratio.
    pub growth: f64,
    /// The element size across the narrow wall (y), when it differs from `size`. An H-plane
    /// device (steps and irises across x only) excited by TE10 has fields that do not vary
    /// along y, so one layer of elements represents them exactly there.
    pub size_y: Option<f64>,
    /// The element size at the edges of strips, as a fraction of `size`: a strip of no
    /// thickness is singular there more strongly than an iris (E ∝ r^{−1/2}).
    pub strip_edge: f64,
}

impl Options {
    /// Graded towards edges: a quarter of the size there, growing by half again per element.
    pub fn new(size: f64) -> Options {
        Options { size, edge: 0.25, growth: 1.5, size_y: None, strip_edge: 0.05 }
    }

    pub fn uniform(size: f64) -> Options {
        Options { size, edge: 1.0, growth: 1.5, size_y: None, strip_edge: 1.0 }
    }
}

/// Points dividing [u, v] (v included, u not): evenly at about `size`, or graded towards the
/// ends marked fine. The field at a re-entrant edge is singular (E ∝ r^{−1/3} at a thin
/// iris' edge: J. Meixner, *The behavior of electromagnetic fields at edges*, IEEE Trans.
/// Antennas Propag. 20(4):442–446, 1972), so a uniform mesh converges slowly there; a mesh
/// graded geometrically towards the edge restores the elements' rate (I. Babuška, R. B. Kellogg,
/// J. Pitkäranta, *Direct and inverse error estimates for finite elements with mesh refinements*,
/// Numer. Math. 33:447–471, 1979).
///
/// `left` and `right` are the ends' element sizes as fractions of the size (1: not graded).
fn graded(u: f64, v: f64, o: &Options, left: f64, right: f64) -> Vec<f64> {
    let len = v - u;
    let (fine_left, fine_right) = (left < 1.0, right < 1.0);
    if !(fine_left || fine_right) {
        let n = divisions(len, o.size, 1);
        return line(u, len, n).skip(1).collect();
    }
    // Geometric steps h0, h0·g, … from each fine end while below the size; the middle evenly.
    let ramp = |edge: f64| {
        let mut v = Vec::new();
        let mut h = edge.max(1e-3) * o.size;
        while h < o.size && v.len() < 200 {
            v.push(h);
            h *= o.growth;
        }
        v
    };
    let left = if fine_left { ramp(left) } else { Vec::new() };
    let right = if fine_right { ramp(right) } else { Vec::new() };
    let graded_len: f64 = left.iter().chain(&right).sum();
    let gap = len - graded_len;
    let mut steps: Vec<f64> = left.clone();
    if gap > 0.0 {
        let last = left.last().into_iter().chain(right.last()).fold(o.size, |m, h| m.min(*h));
        let n = (gap / o.size).ceil() as usize;
        if gap < last {
            // A gap too short for an element: the ramps stretch over it.
            steps.extend(right.iter().rev());
            let k = len / graded_len;
            steps.iter_mut().for_each(|h| *h *= k);
        } else {
            steps.extend(std::iter::repeat_n(gap / n as f64, n));
            steps.extend(right.iter().rev());
        }
    } else {
        // The ramps overlap: shrink them to fit.
        steps.extend(right.iter().rev());
        let k = len / graded_len;
        steps.iter_mut().for_each(|h| *h *= k);
    }
    let mut out = Vec::with_capacity(steps.len());
    let mut acc = u;
    for (k, h) in steps.iter().enumerate() {
        acc += h;
        out.push(if k + 1 == steps.len() { v } else { acc });
    }
    out
}

/// Meshes the model's waveguide with tetrahedra of about `size` (m) on a side, graded towards
/// the edges of steps and irises ([`Options::new`]).
pub fn waveguide(model: &Model, size: f64) -> Result<Mesh> {
    waveguide_with(model, &Options::new(size))
}

/// Meshes the model's waveguide. The grid lines along x pass through every aperture's edges,
/// each section along z is divided on its own, so section boundaries and aperture walls are grid
/// planes; cells outside a section's aperture are left out. Next to the re-entrant edges of a
/// step or an iris the lines close in ([`Options`]). Faces at z = 0 and z = length are tagged as
/// ports 0 and 1, every other boundary face (the walls, and the faces of a step or an iris) as a
/// perfect conductor. A symmetry plane of the model ([`emcraft_model::Symmetry`]) is a grid
/// plane too: only the side below it is meshed, its faces tagged with its wall.
pub fn waveguide_with(model: &Model, o: &Options) -> Result<Mesh> {
    let size = positive("element size", o.size)?;
    if !(o.edge.is_finite() && o.edge > 0.0 && o.edge <= 1.0)
        || !(o.strip_edge.is_finite() && o.strip_edge > 0.0 && o.strip_edge <= 1.0)
        || !(o.growth.is_finite() && o.growth > 1.0)
    {
        return Err(MeshError::Invalid { what: "grading".into(), why: "edge and strip_edge must be in (0, 1] and growth > 1".into() });
    }
    let o = Options { size, ..*o };
    let w = model.waveguide.as_ref().ok_or_else(|| MeshError::Invalid { what: "model".into(), why: "has no waveguide".into() })?;
    // The cap before any line is made (grading only adds to this count).
    let estimate = [w.width / size, w.height / o.size_y.unwrap_or(size), w.length() / size].iter().map(|n| n.ceil().max(1.0)).product::<f64>()
        * TETS_PER_CELL as f64;
    if !(estimate <= MAX_TETS as f64) {
        return Err(MeshError::TooLarge { tets: estimate.min(usize::MAX as f64) as usize, limit: MAX_TETS });
    }
    let size_y = match o.size_y {
        Some(h) => positive("element size across y", h)?,
        None => size,
    };
    // Grid lines through every aperture edge on an axis, closing in on the re-entrant edges (a
    // narrower aperture's edge strictly inside its neighbour's); and the planes where the
    // aperture changes.
    let mut sharp_z = vec![false; w.sections.len() + 1];
    let ends = w.interfaces();
    // Layers' tops are planes; strips' edges are planes the lines close in on (the field is
    // singular at a strip's edge).
    let strip_x: Vec<f64> = w.strips.iter().flat_map(|s| [s.x0, s.x0 + s.width]).collect();
    let strip_y: Vec<f64> = w.strips.iter().map(|s| s.y).collect();
    let layer_y = w.layer_tops();
    let axis = |span: f64,
                aperture: &dyn Fn(usize) -> Option<Span>,
                step: f64,
                sharp_z: &mut Vec<bool>,
                planes: &[f64],
                edges: &[f64]|
     -> Result<Vec<f64>> {
        let tol = 1e-9 * span;
        let mut breaks = vec![0.0, span];
        breaks.extend(planes.iter().chain(edges).copied().filter(|v| *v > tol && *v < span - tol));
        let mut sharp: Vec<f64> = edges.iter().copied().filter(|v| *v > tol && *v < span - tol).collect();
        for i in 0..w.sections.len() {
            if let Some(a) = aperture(i) {
                breaks.push(a.start.clamp(0.0, span));
                breaks.push(a.end().clamp(0.0, span));
            }
            if i == 0 {
                continue;
            }
            if let (Some(p), Some(q)) = (aperture(i - 1), aperture(i)) {
                if p.same(q, tol) {
                    continue;
                }
                sharp_z[i] = true;
                let (narrow, wide) = if q.width < p.width { (q, p) } else { (p, q) };
                for e in [narrow.start, narrow.end()] {
                    if e > wide.start + tol && e < wide.end() - tol {
                        sharp.push(e);
                    }
                }
            }
        }
        breaks.sort_by(f64::total_cmp);
        breaks.dedup_by(|a, b| (*a - *b).abs() <= tol);
        if breaks.len() > 1000 {
            return Err(MeshError::Invalid { what: "waveguide".into(), why: "too many aperture edges".into() });
        }
        // A strip's edge, an aperture's re-entrant edge, or neither.
        let fraction = |v: f64| {
            if edges.iter().any(|e| (e - v).abs() <= tol) {
                o.strip_edge
            } else if sharp.iter().any(|e| (e - v).abs() <= tol) {
                o.edge
            } else {
                1.0
            }
        };
        let oo = Options { size: step, ..o };
        let mut lines = vec![0.0];
        for b in breaks.windows(2) {
            lines.extend(graded(b[0], b[1], &oo, fraction(b[0]), fraction(b[1])));
        }
        Ok(lines)
    };
    // Symmetry planes are grid planes; the lines beyond them are dropped (the half kept).
    if let Some(why) = model.symmetry_issue() {
        return Err(MeshError::Invalid { what: "symmetry".into(), why });
    }
    let cut = |d: usize| model.symmetry.iter().find(|p| p.axis.index() == d).map(|p| p.at);
    let cuts: Vec<(usize, f64, SymmetryKind)> = model.symmetry.iter().map(|p| (p.axis.index(), p.at, p.kind)).collect();
    let keep = |mut lines: Vec<f64>, at: Option<f64>, span: f64| {
        if let Some(at) = at {
            lines.retain(|v| *v <= at + 1e-9 * span);
            if let Some(last) = lines.last_mut() {
                *last = at;
            }
        }
        lines
    };
    let x_planes: Vec<f64> = cut(0).into_iter().collect();
    let y_planes: Vec<f64> = layer_y.iter().copied().chain(cut(1)).collect();
    let x = keep(axis(w.width, &|i| w.aperture(i), size, &mut sharp_z, &x_planes, &strip_x)?, cut(0), w.width);
    let y = keep(axis(w.height, &|i| w.aperture_y(i), size_y, &mut sharp_z, &y_planes, &strip_y)?, cut(1), w.height);
    let nx = (x.len() - 1).max(1);
    let ny = (y.len() - 1).max(1);
    let mut z = vec![0.0];
    let mut layers = Vec::new();
    let mut at = 0.0;
    let mut cells_z = 0usize;
    // Strips' ends inside a section split it: planes the lines close in on.
    let tolz = 1e-9 * w.length();
    let strip_z: Vec<f64> = w.strips.iter().flat_map(|s| [s.z0, s.z1]).collect();
    for (i, s) in w.sections.iter().enumerate() {
        let mut cuts: Vec<(f64, bool)> = vec![(ends[i], sharp_z[i]), (ends[i + 1], sharp_z[i + 1])];
        cuts.extend(strip_z.iter().filter(|z| **z > ends[i] + tolz && **z < ends[i + 1] - tolz).map(|z| (*z, true)));
        cuts.sort_by(|a, b| a.0.total_cmp(&b.0));
        cuts.dedup_by(|a, b| (a.0 - b.0).abs() <= tolz);
        let (m, _) = model.material(&s.material)?;
        let ap = w.aperture(i).unwrap_or(w.across_x());
        let apy = w.aperture_y(i).unwrap_or(w.across_y());
        for c in cuts.windows(2) {
            // A strip's end inside the guide is an edge; a section's aperture change, a step.
            let fraction = |(z, sharp): (f64, bool)| {
                if strip_z.iter().any(|e| (e - z).abs() <= tolz) && z > tolz && z < w.length() - tolz {
                    o.strip_edge
                } else if sharp {
                    o.edge
                } else {
                    1.0
                }
            };
            let zs = graded(c[0].0, c[1].0, &o, fraction(c[0]), fraction(c[1]));
            let n = zs.len();
            cells_z = cells_z.saturating_add(n);
            // The cap is checked before the lines are made.
            if nx.saturating_mul(ny).saturating_mul(cells_z).saturating_mul(TETS_PER_CELL) > MAX_TETS {
                break;
            }
            z.extend(zs);
            layers.extend(std::iter::repeat_n((m as u32, ap, apy), n));
        }
        at += s.length;
    }
    let tets = nx.saturating_mul(ny).saturating_mul(cells_z).saturating_mul(TETS_PER_CELL);
    if tets > MAX_TETS {
        return Err(MeshError::TooLarge { tets, limit: MAX_TETS });
    }
    let length = at;
    if let Some(last) = z.last_mut() {
        *last = length;
    }
    let grid = Grid { x, y, z, cells: Vec::new() };
    let ylayers =
        w.layer_tops().into_iter().zip(&w.layers).map(|(top, l)| Ok((top, model.material(&l.material)?.0 as u32))).collect::<Result<Vec<_>>>()?;
    let mut mesh = structured(grid, &layers, &ylayers, length, &cuts);
    add_strips(&mut mesh, &w.strips, w.height);
    Ok(mesh)
}

/// Tags the faces inside the mesh that lie on a strip as conductors (each once, with one of its
/// two tetrahedra): the solver eliminates their tangential field as on the walls.
fn add_strips(mesh: &mut Mesh, strips: &[emcraft_model::Strip], height: f64) {
    if strips.is_empty() {
        return;
    }
    let topo = Topology::new(mesh);
    let tol = 1e-9 * height;
    for (f, owners) in topo.faces.iter().zip(&topo.face_tets) {
        if owners[1] == u32::MAX {
            continue;
        }
        let p = f.map(|n| mesh.nodes[n as usize]);
        let Some(c) = Point3::centroid(p) else { continue };
        let on = strips.iter().any(|s| p.iter().all(|q| (q.y - s.y).abs() <= tol) && c.x > s.x0 && c.x < s.x0 + s.width && c.z > s.z0 && c.z < s.z1);
        if on {
            mesh.boundary.push(BoundaryFace { nodes: *f, tet: owners[0], tag: Boundary::Pec });
        }
    }
}

/// The tetrahedral mesh of a grid whose cell layers along z have the given materials and
/// apertures ((x start, width), (y start, height)): cells whose centre lies outside the aperture
/// are left out.
/// `ylayers` (tops ascending, material) override the material of the cells whose centre lies
/// below a top.
/// `cuts` (axis, position, wall) are symmetry planes the grid ends on: the faces there are tagged
/// with their wall.
fn structured(mut grid: Grid, layers: &[(u32, Span, Span)], ylayers: &[(f64, u32)], length: f64, cuts: &[(usize, f64, SymmetryKind)]) -> Mesh {
    let (nx, ny, nz) = (grid.x.len() - 1, grid.y.len() - 1, grid.z.len() - 1);
    let node = |i: usize, j: usize, k: usize| (i + (nx + 1) * (j + (ny + 1) * k)) as u32;
    let mut nodes = Vec::with_capacity((nx + 1) * (ny + 1) * (nz + 1));
    for &z in &grid.z {
        for &y in &grid.y {
            for &x in &grid.x {
                nodes.push(Point3::new(x, y, z));
            }
        }
    }
    let mut tets = Vec::with_capacity(nx * ny * nz * TETS_PER_CELL);
    let mut tet_material = Vec::with_capacity(tets.capacity());
    let mut cells = Vec::with_capacity(nx * ny * nz);
    for k in 0..nz {
        let all = Span::new(f64::NEG_INFINITY, f64::INFINITY);
        let (material, x_span, y_span) = layers.get(k).copied().unwrap_or((0, all, all));
        for j in 0..ny {
            let yc = 0.5 * (grid.y[j] + grid.y[j + 1]);
            let material = ylayers.iter().find(|(top, _)| yc < *top).map_or(material, |(_, m)| *m);
            for i in 0..nx {
                let xc = 0.5 * (grid.x[i] + grid.x[i + 1]);
                if !(x_span.contains(xc) && y_span.contains(yc)) {
                    cells.push(u32::MAX);
                    continue;
                }
                cells.push(tets.len() as u32);
                let corner = |c: usize| node(i + (c & 1), j + ((c >> 1) & 1), k + ((c >> 2) & 1));
                for pattern in KUHN {
                    let mut t = pattern.map(corner);
                    let p = t.map(|n| nodes[n as usize]);
                    if signed_volume(&p) < 0.0 {
                        t.swap(2, 3);
                    }
                    tets.push(t);
                    tet_material.push(material);
                }
            }
        }
    }
    grid.cells = cells;
    let mut mesh = Mesh { nodes, tets, tet_material, boundary: Vec::new(), grid: Some(grid), buckets: None, curved: Vec::new() };
    let topo = Topology::new(&mesh);
    let tol = 1e-9 * length.max(f64::MIN_POSITIVE);
    for (f, owners) in topo.faces.iter().zip(&topo.face_tets) {
        if owners[1] != u32::MAX {
            continue;
        }
        let zs = f.map(|n| mesh.nodes[n as usize].z);
        let tag = if zs.iter().all(|z| z.abs() <= tol) {
            Boundary::Port(0)
        } else if zs.iter().all(|z| (z - length).abs() <= tol) {
            Boundary::Port(1)
        } else if let Some(&(_, _, kind)) =
            cuts.iter().find(|(d, at, _)| f.iter().all(|n| (mesh.nodes[*n as usize][*d] - at).abs() <= 1e-9 * at.abs().max(f64::MIN_POSITIVE)))
        {
            Boundary::Symmetry(kind)
        } else {
            Boundary::Pec
        };
        mesh.boundary.push(BoundaryFace { nodes: *f, tet: owners[0], tag });
    }
    mesh
}

/// Barycentric coordinates of `p` in a tetrahedron.
pub fn barycentric(c: &[Point3; 4], p: Point3) -> [f64; 4] {
    let v = signed_volume(c);
    let mut l = [0.0; 4];
    for (i, li) in l.iter_mut().enumerate() {
        let mut q = *c;
        q[i] = p;
        *li = signed_volume(&q) / v;
    }
    l
}

/// The index of the largest `x[k] ≤ v` among the grid lines (clamped to the cells).
fn cell_index(lines: &[f64], v: f64) -> Option<usize> {
    let n = lines.len().checked_sub(1)?;
    if n == 0 || !v.is_finite() {
        return None;
    }
    let k = lines.partition_point(|x| *x <= v);
    Some(k.saturating_sub(1).min(n - 1))
}

/// The tetrahedron containing `p` and its barycentric coordinates, or `None` outside the grid
/// (points within 10⁻⁹ of the domain size of its boundary count as inside).
pub fn locate(mesh: &Mesh, p: Point3) -> Option<(usize, [f64; 4])> {
    if mesh.grid.is_none() {
        return locate_in_buckets(mesh, p);
    }
    let g = mesh.grid.as_ref()?;
    let (nx, ny) = (g.x.len().checked_sub(1)?, g.y.len().checked_sub(1)?);
    let span = |l: &[f64]| (l.first().copied().unwrap_or(0.0), l.last().copied().unwrap_or(0.0));
    for (l, v) in [(&g.x, p.x), (&g.y, p.y), (&g.z, p.z)] {
        let (lo, hi) = span(l);
        let tol = 1e-9 * (hi - lo).abs();
        if !(v >= lo - tol && v <= hi + tol) {
            return None;
        }
    }
    // The cell holding p, and its neighbours when p lies on a grid plane (the cell on one side
    // may be left out, at a step or an iris).
    let near = |lines: &[f64], v: f64| -> Option<Vec<usize>> {
        let c = cell_index(lines, v)?;
        let n = lines.len() - 1;
        let tol = 1e-9 * (lines[n] - lines[0]).abs();
        let mut out = vec![c];
        if c > 0 && (v - lines[c]).abs() <= tol {
            out.push(c - 1);
        }
        if c + 1 < n && (v - lines[c + 1]).abs() <= tol {
            out.push(c + 1);
        }
        Some(out)
    };
    let (is, js, ks) = (near(&g.x, p.x)?, near(&g.y, p.y)?, near(&g.z, p.z)?);
    let mut best: Option<(usize, [f64; 4], f64)> = None;
    for &k in &ks {
        for &j in &js {
            for &i in &is {
                let first = *g.cells.get(i + nx * (j + ny * k))?;
                if first == u32::MAX {
                    continue;
                }
                for t in first as usize..first as usize + TETS_PER_CELL {
                    let c = mesh.corners(t)?;
                    let l = barycentric(&c, p);
                    let worst = l.iter().copied().fold(f64::INFINITY, f64::min);
                    if best.as_ref().is_none_or(|b| worst > b.2) {
                        best = Some((t, l, worst));
                    }
                }
            }
        }
    }
    // Inside a cell that was left out: not in the mesh.
    best.filter(|b| b.2 >= -1e-9).map(|(t, l, _)| (t, l))
}

/// The tetrahedron of an unstructured mesh containing `p` (within 10⁻⁹ in barycentric terms).
fn locate_in_buckets(mesh: &Mesh, p: Point3) -> Option<(usize, [f64; 4])> {
    let b = mesh.buckets.as_ref()?;
    if !p.is_finite() {
        return None;
    }
    let mut ijk = [0usize; 3];
    for d in 0..3 {
        let f = (p[d] - b.lo[d]) / b.step[d];
        if !(f >= -1e-9 && f <= b.dims[d] as f64 + 1e-9) {
            return None;
        }
        ijk[d] = (f.floor().max(0.0) as usize).min(b.dims[d] - 1);
    }
    let cell = ijk[0] + b.dims[0] * (ijk[1] + b.dims[1] * ijk[2]);
    let (s, e) = (*b.start.get(cell)? as usize, *b.start.get(cell + 1)? as usize);
    let mut best: Option<(usize, [f64; 4], f64)> = None;
    for &t in b.items.get(s..e)? {
        let c = mesh.corners(t as usize)?;
        let l = barycentric(&c, p);
        let worst = l.iter().copied().fold(f64::INFINITY, f64::min);
        if best.as_ref().is_none_or(|x| worst > x.2) {
            best = Some((t as usize, l, worst));
        }
    }
    best.filter(|x| x.2 >= -1e-9).map(|(t, l, _)| (t, l))
}

/// The tetrahedra whose bounding boxes meet `region`: from the buckets of an unstructured mesh,
/// every tetrahedron otherwise (a box test still drops the far ones).
pub fn tets_near(mesh: &Mesh, region: Aabb) -> Vec<usize> {
    let Aabb { min: lo, max: hi } = region;
    let meets = |t: usize| mesh.corners(t).is_some_and(|c| Aabb::around(c).unwrap_or_default().overlaps(region));
    let Some(bk) = mesh.buckets.as_ref() else { return (0..mesh.tets.len()).filter(|&t| meets(t)).collect() };
    let cell = |p: Point3, d: usize| ((((p[d] - bk.lo[d]) / bk.step[d]).floor().max(0.0)) as usize).min(bk.dims[d].saturating_sub(1));
    let mut out = Vec::new();
    for k in cell(lo, 2)..=cell(hi, 2) {
        for j in cell(lo, 1)..=cell(hi, 1) {
            for i in cell(lo, 0)..=cell(hi, 0) {
                let b = i + bk.dims[0] * (j + bk.dims[1] * k);
                let (s, e) = (bk.start.get(b).copied().unwrap_or(0) as usize, bk.start.get(b + 1).copied().unwrap_or(0) as usize);
                out.extend(bk.items.get(s..e).unwrap_or(&[]).iter().map(|&t| t as usize));
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out.retain(|&t| meets(t));
    out
}

/// Volume of every tetrahedron, signed.
pub fn volumes(mesh: &Mesh) -> Vec<f64> {
    (0..mesh.tets.len()).map(|t| mesh.corners(t).map_or(0.0, |c| signed_volume(&c))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use emcraft_model::{Material, Section, Waveguide};

    fn model() -> Model {
        let mut m = Model::new();
        m.add_material(Material::new("ptfe", 2.0, 1.0).unwrap()).unwrap();
        let s = |l: f64, m: &str| Section::new(l, m);
        m.set_waveguide(Waveguide {
            width: 0.02286,
            height: 0.01016,
            sections: vec![s(0.015, "ptfe"), s(0.02, "vacuum"), s(0.015, "ptfe")],
            layers: Vec::new(),
            strips: Vec::new(),
        })
        .unwrap();
        m
    }

    fn tri_area(mesh: &Mesh, f: &[u32; 3]) -> f64 {
        let p = f.map(|n| mesh.nodes[n as usize]);
        0.5 * (p[1] - p[0]).cross(p[2] - p[0]).norm()
    }

    #[test]
    fn volume_is_the_guide_and_every_tet_is_positive() {
        let m = model();
        let mesh = waveguide(&m, 0.003).unwrap();
        let v = volumes(&mesh);
        assert!(v.iter().all(|&x| x > 0.0));
        let total: f64 = v.iter().sum();
        let exact = 0.02286 * 0.01016 * 0.05;
        assert!((total - exact).abs() < 1e-12 * exact, "{total} {exact}");
        // Section boundaries are grid planes: materials change exactly at z = 15 and 35 mm.
        let g = mesh.grid.as_ref().unwrap();
        assert!(g.z.iter().any(|z| (z - 0.015).abs() < 1e-15) && g.z.iter().any(|z| (z - 0.035).abs() < 1e-15));
        for (t, &mat) in mesh.tet_material.iter().enumerate() {
            let c = mesh.corners(t).unwrap();
            let zc = c.iter().map(|p| p[2]).sum::<f64>() / 4.0;
            let expect = if (0.015..0.035).contains(&zc) { 0 } else { 1 };
            assert_eq!(mat, expect);
        }
    }

    #[test]
    fn boundary_tags_and_areas() {
        let m = model();
        let mesh = waveguide(&m, 0.004).unwrap();
        let (a, b, l) = (0.02286, 0.01016, 0.05);
        let area = |tag: Boundary| mesh.boundary.iter().filter(|f| f.tag == tag).map(|f| tri_area(&mesh, &f.nodes)).sum::<f64>();
        assert!((area(Boundary::Port(0)) - a * b).abs() < 1e-14);
        assert!((area(Boundary::Port(1)) - a * b).abs() < 1e-14);
        assert!((area(Boundary::Pec) - 2.0 * (a + b) * l).abs() < 1e-13);
        // Every boundary face belongs to its tet.
        for f in &mesh.boundary {
            let t = mesh.tets[f.tet as usize];
            assert!(f.nodes.iter().all(|n| t.contains(n)));
        }
    }

    #[test]
    fn symmetry_planes_keep_the_side_below_them() {
        use emcraft_model::{Axis, Symmetry, SymmetryKind};
        let (a, b, l) = (0.02286, 0.01016, 0.05);
        let mut m = model();
        let full = waveguide(&m, 0.004).unwrap();
        m.set_symmetry(vec![Symmetry { axis: Axis::X, at: a / 2.0, kind: SymmetryKind::Magnetic }]).unwrap();
        let half = waveguide(&m, 0.004).unwrap();
        let area = |mesh: &Mesh, tag: Boundary| mesh.boundary.iter().filter(|f| f.tag == tag).map(|f| tri_area(mesh, &f.nodes)).sum::<f64>();
        assert!((volumes(&half).iter().sum::<f64>() - a * b * l / 2.0).abs() < 1e-12 * a * b * l);
        assert!((area(&half, Boundary::Symmetry(SymmetryKind::Magnetic)) - b * l).abs() < 1e-13);
        assert!((area(&half, Boundary::Port(0)) - a * b / 2.0).abs() < 1e-14);
        assert!((area(&half, Boundary::Pec) - (a + b) * l).abs() < 1e-13);
        assert!(half.nodes.iter().all(|p| p.x <= a / 2.0));
        assert!(half.tets.len() * 2 <= full.tets.len() + full.tets.len() / 4, "{} of {}", half.tets.len(), full.tets.len());
        // A quarter: an electric wall across y as well.
        m.set_symmetry(vec![
            Symmetry { axis: Axis::X, at: a / 2.0, kind: SymmetryKind::Magnetic },
            Symmetry { axis: Axis::Y, at: b / 2.0, kind: SymmetryKind::Electric },
        ])
        .unwrap();
        let quarter = waveguide(&m, 0.004).unwrap();
        assert!((volumes(&quarter).iter().sum::<f64>() - a * b * l / 4.0).abs() < 1e-12 * a * b * l);
        assert!((area(&quarter, Boundary::Symmetry(SymmetryKind::Electric)) - a / 2.0 * l).abs() < 1e-13);
        assert!((area(&quarter, Boundary::Symmetry(SymmetryKind::Magnetic)) - b / 2.0 * l).abs() < 1e-13);
        // A guide no longer symmetric is refused.
        let mut w = m.waveguide.clone().unwrap();
        w.sections[1].width = Some(0.01);
        w.sections[1].offset = Some(0.0);
        m.set_waveguide(w).unwrap();
        assert!(waveguide(&m, 0.004).is_err());
    }

    #[test]
    fn locate_finds_the_containing_tet() {
        let m = model();
        let mesh = waveguide(&m, 0.004).unwrap();
        for p in [[0.011, 0.005, 0.025], [0.0, 0.0, 0.0], [0.02286, 0.01016, 0.05], [0.001, 0.009, 0.0349]].map(Point3::from) {
            let (t, l) = locate(&mesh, p).unwrap();
            assert!(l.iter().all(|&x| x >= -1e-12), "{l:?}");
            assert!((l.iter().sum::<f64>() - 1.0).abs() < 1e-12);
            let c = mesh.corners(t).unwrap();
            for d in 0..3 {
                let q: f64 = (0..4).map(|i| l[i] * c[i][d]).sum();
                assert!((q - p[d]).abs() < 1e-14);
            }
        }
        assert!(locate(&mesh, Point3::new(0.03, 0.0, 0.0)).is_none());
        assert!(locate(&mesh, Point3::new(f64::NAN, 0.0, 0.0)).is_none());
    }

    #[test]
    fn an_iris_leaves_its_cells_out_and_walls_its_faces() {
        // A 22.86 mm guide with a 2 mm thick, 10 mm wide centred iris.
        let mut m = Model::new();
        let iris = Section { length: 0.002, material: "vacuum".into(), width: Some(0.01), offset: None, ..Default::default() };
        m.set_waveguide(Waveguide {
            width: 0.02286,
            height: 0.01016,
            sections: vec![Section::new(0.01, "vacuum"), iris, Section::new(0.01, "vacuum")],
            layers: Vec::new(),
            strips: Vec::new(),
        })
        .unwrap();
        let mesh = waveguide(&m, 0.003).unwrap();
        let (a, b) = (0.02286, 0.01016);
        let total: f64 = volumes(&mesh).iter().sum();
        let exact = a * b * 0.02 + 0.01 * b * 0.002;
        assert!((total - exact).abs() < 1e-12 * exact, "{total} {exact}");
        // The aperture's edges are grid lines.
        let g = mesh.grid.as_ref().unwrap();
        for x in [0.00643, 0.01643] {
            assert!(g.x.iter().any(|v| (v - x).abs() < 1e-12), "{x}");
        }
        // Conductor area: the side walls and the iris' two faces and its aperture walls.
        let area = |tag: Boundary| mesh.boundary.iter().filter(|f| f.tag == tag).map(|f| tri_area(&mesh, &f.nodes)).sum::<f64>();
        let pec = 2.0 * (a + b) * 0.02 + 2.0 * (0.01 + b) * 0.002 + 2.0 * (a - 0.01) * b;
        assert!((area(Boundary::Pec) - pec).abs() < 1e-12, "{} {pec}", area(Boundary::Pec));
        assert!((area(Boundary::Port(0)) - a * b).abs() < 1e-14);
        // Points in the iris' metal are outside; on its faces they are inside.
        assert!(locate(&mesh, Point3::new(0.002, 0.005, 0.011)).is_none());
        assert!(locate(&mesh, Point3::new(0.002, 0.005, 0.01)).is_some());
        assert!(locate(&mesh, Point3::new(0.011, 0.005, 0.011)).is_some());
    }

    #[test]
    fn lines_close_in_on_the_edges_of_an_iris_only() {
        let mut m = Model::new();
        let iris = Section { length: 0.002, material: "vacuum".into(), width: Some(0.01), offset: None, ..Default::default() };
        m.set_waveguide(Waveguide {
            width: 0.02286,
            height: 0.01016,
            sections: vec![Section::new(0.01, "vacuum"), iris, Section::new(0.01, "vacuum")],
            layers: Vec::new(),
            strips: Vec::new(),
        })
        .unwrap();
        let o = Options::new(0.004);
        let g = waveguide_with(&m, &o).unwrap().grid.unwrap();
        let spacing_at = |lines: &[f64], v: f64| {
            lines.windows(2).filter(|w| (w[0] - v).abs() < 1e-12 || (w[1] - v).abs() < 1e-12).map(|w| w[1] - w[0]).fold(f64::INFINITY, f64::min)
        };
        // At the iris' edges (x = 6.43, 16.43 mm; z = 10, 12 mm) about a quarter of the size.
        for x in [0.00643, 0.01643] {
            assert!(spacing_at(&g.x, x) <= 0.26 * o.size, "x {x}: {}", spacing_at(&g.x, x));
        }
        for z in [0.01, 0.012] {
            assert!(spacing_at(&g.z, z) <= 0.26 * o.size, "z {z}: {:?}", g.z);
        }
        // The guide's own walls and ends are not refined; spacing never exceeds the size.
        assert!(spacing_at(&g.x, 0.0) > 0.5 * o.size && spacing_at(&g.z, 0.0) > 0.5 * o.size);
        assert!(g.x.windows(2).chain(g.z.windows(2)).all(|w| w[1] > w[0] && w[1] - w[0] <= o.size * 1.2));
        // Without steps, grading changes nothing.
        let uniform_guide = model();
        assert_eq!(waveguide_with(&uniform_guide, &o).unwrap(), waveguide_with(&uniform_guide, &Options::uniform(0.004)).unwrap());
        assert!(waveguide_with(&m, &Options { size: 0.004, edge: 0.0, growth: 1.5, size_y: None, strip_edge: 0.05 }).is_err());
        assert!(waveguide_with(&m, &Options { size: 0.004, edge: 0.5, growth: 1.0, size_y: None, strip_edge: 0.05 }).is_err());
    }

    #[test]
    fn hostile_sizes_are_refused() {
        let m = model();
        assert!(waveguide(&m, 0.0).is_err());
        assert!(waveguide(&m, f64::NAN).is_err());
        assert!(waveguide(&m, -1.0).is_err());
        assert!(matches!(waveguide(&m, 1e-6), Err(MeshError::TooLarge { .. })));
        assert!(waveguide(&Model::new(), 0.001).is_err());
    }
}
