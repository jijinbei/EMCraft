//! Structured tetrahedral meshes of a rectangular waveguide ([`waveguide`]).
//!
//! The grid lines along x and y pass through every aperture's edges, each section along z is
//! divided on its own, so section boundaries and aperture walls are grid planes; cells outside a
//! section's aperture are left out. Next to the re-entrant edges of a step or an iris, a strip's
//! edges and a via's faces the lines close in ([`Options`]).
//!
//! Each grid cell is split into six tetrahedra around its main diagonal (the Kuhn, or
//! Freudenthal, triangulation of the cube: H. W. Kuhn, *Some combinatorial lemmas in topology*,
//! IBM J. Res. Dev. 4(5):518–524, 1960). Every cell face is then cut along the diagonal from its
//! lowest to its highest corner, the same in both cells that share it, so the mesh is conforming
//! without any search.

use emcraft_model::mesh::TETS_PER_CELL;
use emcraft_model::{Aabb, Boundary, BoundaryFace, Grid, Mesh, Model, Point3, Span, Strip, Symmetry, Via, Waveguide, positive};

use crate::{MAX_TETS, MeshError, Result, Topology, signed_volume, too_large};

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
    /// The element size at strips' ends along z, as a fraction of `size`; `strip_edge` when
    /// `None`. An open end's corners are weaker singularities than a strip's long edges, and
    /// lines along z graded there cross the whole box: a long circuit of many coupled sections
    /// meshes far lighter with its ends graded less.
    pub strip_end: Option<f64>,
}

impl Options {
    /// Graded towards edges: a quarter of the size there, growing by half again per element.
    pub fn new(size: f64) -> Options {
        Options { size, edge: 0.25, growth: 1.5, size_y: None, strip_edge: 0.05, strip_end: None }
    }

    pub fn uniform(size: f64) -> Options {
        Options { size, edge: 1.0, growth: 1.5, size_y: None, strip_edge: 1.0, strip_end: None }
    }

    /// The options with a checked size and grading.
    fn checked(&self) -> Result<Options> {
        let size = positive("element size", self.size)?;
        let fraction = |f: f64| f.is_finite() && f > 0.0 && f <= 1.0;
        if !(fraction(self.edge) && fraction(self.strip_edge) && self.strip_end.is_none_or(fraction) && self.growth.is_finite() && self.growth > 1.0)
        {
            return Err(MeshError::Invalid { what: "grading".into(), why: "edge, strip_edge and strip_end must be in (0, 1] and growth > 1".into() });
        }
        Ok(Options { size, ..*self })
    }

    /// The element size at a grid line: `strip_edge` at a strip's edge, `edge` at a re-entrant
    /// edge (a step's, an iris', a via's), else 1 (fractions of the size).
    fn fraction(&self, at_strip_edge: bool, at_sharp_edge: bool) -> f64 {
        if at_strip_edge {
            self.strip_edge
        } else if at_sharp_edge {
            self.edge
        } else {
            1.0
        }
    }

    /// As [`Options::fraction`] along z, where a strip's edge is its end (`strip_end`).
    fn fraction_z(&self, at_strip_end: bool, at_sharp_edge: bool) -> f64 {
        Options { strip_edge: self.strip_end.unwrap_or(self.strip_edge), ..*self }.fraction(at_strip_end, at_sharp_edge)
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
/// plane too: only the side below it is meshed, its faces tagged with its wall. A via's faces are
/// grid planes graded like a step's edges, its cells left out like those outside an aperture.
pub fn waveguide_with(model: &Model, o: &Options) -> Result<Mesh> {
    let o = o.checked()?;
    let w = model.waveguide.as_ref().ok_or_else(|| MeshError::Invalid { what: "model".into(), why: "has no waveguide".into() })?;
    // The cap before any line is made (grading only adds to this count).
    let estimate = [w.width / o.size, w.height / o.size_y.unwrap_or(o.size), w.length() / o.size].iter().map(|n| n.ceil().max(1.0)).product::<f64>()
        * TETS_PER_CELL as f64;
    too_large(estimate)?;
    let size_y = match o.size_y {
        Some(h) => positive("element size across y", h)?,
        None => o.size,
    };
    if let Some(why) = model.symmetry_issue() {
        return Err(MeshError::Invalid { what: "symmetry".into(), why });
    }
    let features = Features::of(w);
    // Symmetry planes are grid planes; the lines beyond them are dropped (the half kept).
    let cut = |d: usize| model.symmetry.iter().find(|p| p.axis.index() == d).map(|p| p.at);
    let x_planes: Vec<f64> = cut(0).into_iter().collect();
    let y_planes: Vec<f64> = features.layer_tops.iter().copied().chain(cut(1)).collect();
    let across_x = Axis { span: w.width, step: o.size, planes: &x_planes, strip_edges: &features.strip_x, via_faces: &features.via_x };
    let across_y = Axis { span: w.height, step: size_y, planes: &y_planes, strip_edges: &features.strip_y, via_faces: &features.via_y };
    let x = below(across_x.lines(w, &|i| w.aperture(i), &o)?, cut(0), w.width);
    let y = below(across_y.lines(w, &|i| w.aperture_y(i), &o)?, cut(1), w.height);
    let along = along_z(model, w, &o, &features, [x.len() - 1, y.len() - 1].map(|n| n.max(1)))?;
    let grid = Grid { x, y, z: along.z, cells: Vec::new() };
    let materials = features
        .layer_tops
        .iter()
        .zip(&w.layers)
        .map(|(&top, l)| Ok(YLayer { top, material: model.material(&l.material)?.0 as u32 }))
        .collect::<Result<Vec<_>>>()?;
    let posts: Vec<Aabb> = w.vias.iter().map(Via::bounds).collect();
    let mut mesh =
        structured(grid, &Cells { layers: &along.layers, ylayers: &materials, posts: &posts, length: along.length, cuts: &model.symmetry });
    add_strips(&mut mesh, &w.strips, w.height);
    Ok(mesh)
}

/// The planes the guide's layers, strips and vias put into the grid: layers' tops (y), strips'
/// edges (x, at their y, and their ends along z) and vias' faces.
struct Features {
    layer_tops: Vec<f64>,
    strip_x: Vec<f64>,
    strip_y: Vec<f64>,
    strip_z: Vec<f64>,
    via_x: Vec<f64>,
    via_y: Vec<f64>,
    via_z: Vec<f64>,
}

impl Features {
    fn of(w: &Waveguide) -> Features {
        Features {
            layer_tops: w.layer_tops(),
            strip_x: w.strips.iter().flat_map(|s| [s.x0, s.x0 + s.width]).collect(),
            strip_y: w.strips.iter().map(|s| s.y).collect(),
            strip_z: w.strips.iter().flat_map(|s| [s.z0, s.z1]).collect(),
            via_x: w.vias.iter().flat_map(|v| [v.x0, v.x0 + v.wx]).collect(),
            via_y: w.vias.iter().flat_map(|v| [v.y0, v.y1]).collect(),
            via_z: w.vias.iter().flat_map(|v| [v.z0, v.z0 + v.wz]).collect(),
        }
    }
}

/// One axis of the cross-section: its span, the element size along it, and the planes on it.
struct Axis<'a> {
    span: f64,
    step: f64,
    /// Planes the lines pass through (layers' tops, symmetry planes).
    planes: &'a [f64],
    /// Strips' edges: planes the lines close in on (the field is singular at a strip's edge).
    strip_edges: &'a [f64],
    /// Vias' faces: planes the lines close in on as on a step's edges (a post's corners are
    /// re-entrant).
    via_faces: &'a [f64],
}

impl Axis<'_> {
    /// Grid lines through every aperture edge on the axis, closing in on the re-entrant edges (a
    /// narrower aperture's edge strictly inside its neighbour's), the strips' edges and the vias'
    /// faces.
    fn lines(&self, w: &Waveguide, aperture: &dyn Fn(usize) -> Option<Span>, o: &Options) -> Result<Vec<f64>> {
        let (span, tol) = (self.span, 1e-9 * self.span);
        let inside = |v: &f64| *v > tol && *v < span - tol;
        let mut breaks = vec![0.0, span];
        breaks.extend(self.planes.iter().chain(self.strip_edges).chain(self.via_faces).copied().filter(inside));
        let mut sharp: Vec<f64> = self.via_faces.iter().copied().filter(inside).collect();
        for i in 0..w.sections.len() {
            if let Some(a) = aperture(i) {
                breaks.push(a.start.clamp(0.0, span));
                breaks.push(a.end().clamp(0.0, span));
            }
        }
        for (narrow, wide) in aperture_steps(w, aperture, tol).into_iter().flatten() {
            sharp.extend([narrow.start, narrow.end()].into_iter().filter(|&e| e > wide.start + tol && e < wide.end() - tol));
        }
        breaks.sort_by(f64::total_cmp);
        breaks.dedup_by(|a, b| (*a - *b).abs() <= tol);
        if breaks.len() > 1000 {
            return Err(MeshError::Invalid { what: "waveguide".into(), why: "too many aperture edges".into() });
        }
        let near = |list: &[f64], v: f64| list.iter().any(|e| (e - v).abs() <= tol);
        let fraction = |v: f64| o.fraction(near(self.strip_edges, v), near(&sharp, v));
        let oo = Options { size: self.step, ..*o };
        let mut lines = vec![0.0];
        for b in breaks.windows(2) {
            lines.extend(graded(b[0], b[1], &oo, fraction(b[0]), fraction(b[1])));
        }
        Ok(lines)
    }
}

/// For each section boundary (index i ≥ 1, between sections i − 1 and i) where the aperture along
/// an axis changes, the narrower and the wider of the two apertures; `None` where it does not.
/// Index 0 (the guide's start) is always `None`.
fn aperture_steps(w: &Waveguide, aperture: &dyn Fn(usize) -> Option<Span>, tol: f64) -> Vec<Option<(Span, Span)>> {
    (0..w.sections.len())
        .map(|i| {
            let (p, q) = (aperture(i.checked_sub(1)?)?, aperture(i)?);
            (!p.same(q, tol)).then_some(if q.width < p.width { (q, p) } else { (p, q) })
        })
        .collect()
}

/// The lines up to a symmetry plane at `at` (the last one moved onto it); all of them without one.
fn below(mut lines: Vec<f64>, at: Option<f64>, span: f64) -> Vec<f64> {
    if let Some(at) = at {
        lines.retain(|v| *v <= at + 1e-9 * span);
        if let Some(last) = lines.last_mut() {
            *last = at;
        }
    }
    lines
}

/// The cell layers along z: each layer's material and apertures (x, y).
#[derive(Clone, Copy, Debug)]
struct ZLayer {
    material: u32,
    x: Span,
    y: Span,
}

/// A layer across y (a substrate): the cells whose centre lies below `top` are of `material`.
#[derive(Clone, Copy, Debug)]
struct YLayer {
    top: f64,
    material: u32,
}

/// The grid lines along z, the layers of cells between them, and the guide's length.
struct AlongZ {
    z: Vec<f64>,
    layers: Vec<ZLayer>,
    length: f64,
}

/// Each section divided on its own along z, closing in on the planes where the aperture steps,
/// on strips' ends and on vias' faces; `cross` is the number of cells across (x, y), for the cap
/// on the number of tetrahedra, checked before the lines are made.
fn along_z(model: &Model, w: &Waveguide, o: &Options, features: &Features, cross: [usize; 2]) -> Result<AlongZ> {
    // A step: the aperture changes across x or across y.
    let steps_x = aperture_steps(w, &|i| w.aperture(i), 1e-9 * w.width);
    let steps_y = aperture_steps(w, &|i| w.aperture_y(i), 1e-9 * w.height);
    let mut sharp_z: Vec<bool> = steps_x.iter().zip(&steps_y).map(|(x, y)| x.is_some() || y.is_some()).collect();
    sharp_z.push(false);
    let ends = w.interfaces();
    let tolz = 1e-9 * w.length();
    let (strip_z, via_z) = (&features.strip_z, &features.via_z);
    let near = |list: &[f64], z: f64| list.iter().any(|e| (e - z).abs() <= tolz);
    // A strip's end inside the guide is an edge; a section's aperture change or a via's face, a
    // step.
    let fraction = |(z, sharp): (f64, bool)| o.fraction_z(near(strip_z, z) && z > tolz && z < w.length() - tolz, sharp || near(via_z, z));
    let mut out = AlongZ { z: vec![0.0], layers: Vec::new(), length: 0.0 };
    let mut cells_z = 0usize;
    for (i, s) in w.sections.iter().enumerate() {
        // Strips' ends and vias' faces inside a section split it.
        let mut cuts: Vec<(f64, bool)> = vec![(ends[i], sharp_z[i]), (ends[i + 1], sharp_z[i + 1])];
        let within = |z: &&f64| **z > ends[i] + tolz && **z < ends[i + 1] - tolz;
        cuts.extend(strip_z.iter().chain(via_z).filter(within).map(|z| (*z, true)));
        cuts.sort_by(|a, b| a.0.total_cmp(&b.0));
        cuts.dedup_by(|a, b| (a.0 - b.0).abs() <= tolz);
        let (m, _) = model.material(&s.material)?;
        let layer = ZLayer { material: m as u32, x: w.aperture(i).unwrap_or(w.across_x()), y: w.aperture_y(i).unwrap_or(w.across_y()) };
        for c in cuts.windows(2) {
            let zs = graded(c[0].0, c[1].0, o, fraction(c[0]), fraction(c[1]));
            let n = zs.len();
            cells_z = cells_z.saturating_add(n);
            // The cap is checked before the lines are made.
            if cross[0].saturating_mul(cross[1]).saturating_mul(cells_z).saturating_mul(TETS_PER_CELL) > MAX_TETS {
                break;
            }
            out.z.extend(zs);
            out.layers.extend(std::iter::repeat_n(layer, n));
        }
        out.length += s.length;
    }
    let tets = cross[0].saturating_mul(cross[1]).saturating_mul(cells_z).saturating_mul(TETS_PER_CELL);
    if tets > MAX_TETS {
        return Err(MeshError::TooLarge { tets, limit: MAX_TETS });
    }
    if let Some(last) = out.z.last_mut() {
        *last = out.length;
    }
    Ok(out)
}

/// What the cells of a grid are: their layers along z, the layers across y, the conductors left
/// out (vias), the guide's length (its ports' planes) and the symmetry planes it ends on.
struct Cells<'a> {
    layers: &'a [ZLayer],
    ylayers: &'a [YLayer],
    posts: &'a [Aabb],
    length: f64,
    cuts: &'a [Symmetry],
}

/// The tetrahedral mesh of a grid: cells whose centre lies outside their layer's aperture or
/// inside a post are left out, the others split into six tetrahedra ([`KUHN`]), of their layer's
/// material unless a layer across y holds them; the boundary faces tagged ([`tag_boundary`]).
fn structured(mut grid: Grid, cells: &Cells) -> Mesh {
    let (nx, ny, nz) = (grid.x.len() - 1, grid.y.len() - 1, grid.z.len() - 1);
    let node = |i: usize, j: usize, k: usize| (i + (nx + 1) * (j + (ny + 1) * k)) as u32;
    let (xs, ys) = (&grid.x, &grid.y);
    let nodes: Vec<Point3> = grid.z.iter().flat_map(|&z| ys.iter().flat_map(move |&y| xs.iter().map(move |&x| Point3::new(x, y, z)))).collect();
    let mut tets = Vec::with_capacity(nx * ny * nz * TETS_PER_CELL);
    let mut tet_material = Vec::with_capacity(tets.capacity());
    let mut first_tet = Vec::with_capacity(nx * ny * nz);
    let all = Span::new(f64::NEG_INFINITY, f64::INFINITY);
    for k in 0..nz {
        let layer = cells.layers.get(k).copied().unwrap_or(ZLayer { material: 0, x: all, y: all });
        for j in 0..ny {
            let yc = 0.5 * (grid.y[j] + grid.y[j + 1]);
            let material = cells.ylayers.iter().find(|l| yc < l.top).map_or(layer.material, |l| l.material);
            for i in 0..nx {
                let c = Point3::new(0.5 * (grid.x[i] + grid.x[i + 1]), yc, 0.5 * (grid.z[k] + grid.z[k + 1]));
                let in_post = cells.posts.iter().any(|b| (0..3).all(|d| b.min[d] < c[d] && c[d] < b.max[d]));
                if !(layer.x.contains(c.x) && layer.y.contains(c.y)) || in_post {
                    first_tet.push(u32::MAX);
                    continue;
                }
                first_tet.push(tets.len() as u32);
                let corner = |c: usize| node(i + (c & 1), j + ((c >> 1) & 1), k + ((c >> 2) & 1));
                for pattern in KUHN {
                    let mut t = pattern.map(corner);
                    if signed_volume(&t.map(|n| nodes[n as usize])) < 0.0 {
                        t.swap(2, 3);
                    }
                    tets.push(t);
                    tet_material.push(material);
                }
            }
        }
    }
    grid.cells = first_tet;
    let mut mesh = Mesh { nodes, tets, tet_material, boundary: Vec::new(), grid: Some(grid), buckets: None, curved: Vec::new() };
    mesh.boundary = tag_boundary(&mesh, cells.length, cells.cuts);
    mesh
}

/// The faces held by one tetrahedron, tagged: at z = 0 and z = `length` the ports, on a symmetry
/// plane its wall, elsewhere a perfect conductor.
fn tag_boundary(mesh: &Mesh, length: f64, cuts: &[Symmetry]) -> Vec<BoundaryFace> {
    let topo = Topology::new(mesh);
    let tol = 1e-9 * length.max(f64::MIN_POSITIVE);
    let on_cut = |f: &[u32; 3], p: &Symmetry| {
        let (d, at) = (p.axis.index(), p.at);
        f.iter().all(|n| (mesh.nodes[*n as usize][d] - at).abs() <= 1e-9 * at.abs().max(f64::MIN_POSITIVE))
    };
    topo.faces
        .iter()
        .zip(&topo.face_tets)
        .filter(|(_, owners)| owners[1] == u32::MAX)
        .map(|(f, owners)| {
            let zs = f.map(|n| mesh.nodes[n as usize].z);
            let tag = if zs.iter().all(|z| z.abs() <= tol) {
                Boundary::Port(0)
            } else if zs.iter().all(|z| (z - length).abs() <= tol) {
                Boundary::Port(1)
            } else if let Some(p) = cuts.iter().find(|p| on_cut(f, p)) {
                Boundary::Symmetry(p.kind)
            } else {
                Boundary::Pec
            };
            BoundaryFace { nodes: *f, tet: owners[0], tag }
        })
        .collect()
}

/// Tags the faces inside the mesh that lie on a strip as conductors (each once, with one of its
/// two tetrahedra): the solver eliminates their tangential field as on the walls. A strip on a
/// symmetry plane y = b/2 lies on the mesh's boundary: its faces there become conductor instead
/// of the plane's wall.
fn add_strips(mesh: &mut Mesh, strips: &[Strip], height: f64) {
    if strips.is_empty() {
        return;
    }
    let tol = 1e-9 * height;
    let on = |mesh: &Mesh, f: &[u32; 3]| {
        let p = f.map(|n| mesh.nodes[n as usize]);
        Point3::centroid(p).is_some_and(|c| {
            strips.iter().any(|s| p.iter().all(|q| (q.y - s.y).abs() <= tol) && c.x > s.x0 && c.x < s.x0 + s.width && c.z > s.z0 && c.z < s.z1)
        })
    };
    for k in 0..mesh.boundary.len() {
        if matches!(mesh.boundary[k].tag, Boundary::Symmetry(_)) && on(mesh, &mesh.boundary[k].nodes) {
            mesh.boundary[k].tag = Boundary::Pec;
        }
    }
    let topo = Topology::new(mesh);
    for (f, owners) in topo.faces.iter().zip(&topo.face_tets) {
        if owners[1] != u32::MAX && on(mesh, f) {
            mesh.boundary.push(BoundaryFace { nodes: *f, tet: owners[0], tag: Boundary::Pec });
        }
    }
}
