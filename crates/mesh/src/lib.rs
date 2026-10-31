//! Meshing for EMCraft: structured tetrahedral meshes of a rectangular waveguide
//! ([`waveguide`]), unstructured meshes of primitive bodies ([`solid`]) with Boolean features
//! ([`csg`]) and adaptive refinement ([`adapt`]), the global numbering of edges and faces the
//! vector elements need ([`Topology`]), and point location ([`locate`]).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod adapt;
pub mod cavity;
mod clip;
mod crease;
pub mod csg;
mod flush;
mod shared;
pub mod solid;
mod topology;
mod waveguide;

pub use topology::{LOCAL_EDGES, LOCAL_FACES, NO_TET, Topology};
pub use waveguide::{Options, waveguide, waveguide_with};

use emcraft_model::geom::triple;
use emcraft_model::mesh::TETS_PER_CELL;
use emcraft_model::{Aabb, Mesh, Point3};

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

impl MeshError {
    /// An [`MeshError::Invalid`] about `what`.
    pub(crate) fn invalid(what: &str, why: impl Into<String>) -> Self {
        MeshError::Invalid { what: what.into(), why: why.into() }
    }
}

/// The marker of a cell left out of a structured grid (`emcraft_model::mesh::Grid::cells`).
pub(crate) const CELL_LEFT_OUT: u32 = u32::MAX;

pub type Result<T> = std::result::Result<T, MeshError>;

/// Refuses an estimated number of tetrahedra above [`MAX_TETS`] (or not a number) before any is
/// made.
fn too_large(estimate: f64) -> Result<()> {
    if estimate <= MAX_TETS as f64 { Ok(()) } else { Err(MeshError::TooLarge { tets: estimate.min(usize::MAX as f64) as usize, limit: MAX_TETS }) }
}

/// The least shape a tetrahedron may have: six times its volume over the cube of its longest
/// edge (1/√2 for the regular one). Below it, a tetrahedron is flat to rounding — no element
/// built on it is meaningful.
pub const MIN_TET_SHAPE: f64 = 1e-12;

/// Refuses a mesh with a degenerate or inverted tetrahedron (corners missing or not finite, its
/// volume not positive, or flat to rounding: [`MIN_TET_SHAPE`]), so it never reaches a solver.
/// The error names the first such tetrahedron and where it lies.
pub fn check_tetrahedra(mesh: &Mesh) -> Result<()> {
    let bad = |t: usize, why: String| MeshError::Invalid {
        what: "mesh".into(),
        why: format!(
            "tetrahedron {} {why}; the geometry probably has faces that touch or nearly touch: let bodies reach into each other or keep them apart, or try another element size",
            t + 1
        ),
    };
    for t in 0..mesh.tets.len() {
        let c = mesh.corners(t).ok_or_else(|| bad(t, "has a missing corner".into()))?;
        if !c.iter().all(|p| p.is_finite()) {
            return Err(bad(t, "has a corner that is not a finite point".into()));
        }
        let longest = LOCAL_EDGES.iter().map(|&[i, j]| (c[j] - c[i]).norm()).fold(0.0f64, f64::max);
        let six_v = triple(c[1] - c[0], c[2] - c[0], c[3] - c[0]);
        if !(six_v > MIN_TET_SHAPE * longest.powi(3)) {
            let at = Point3::centroid(c).unwrap_or(c[0]);
            let what = if six_v < 0.0 { "is inverted" } else { "is degenerate" };
            return Err(bad(t, format!("{what} (volume {:.3e} m³, near ({:.6e}, {:.6e}, {:.6e}) m)", six_v / 6.0, at.x, at.y, at.z)));
        }
    }
    Ok(())
}

fn signed_volume(p: &[Point3; 4]) -> f64 {
    triple(p[1] - p[0], p[2] - p[0], p[3] - p[0]) / 6.0
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
    let mut best = Best::default();
    for &k in &ks {
        for &j in &js {
            for &i in &is {
                let first = *g.cells.get(i + nx * (j + ny * k))?;
                if first == CELL_LEFT_OUT {
                    continue;
                }
                for t in first as usize..first as usize + TETS_PER_CELL {
                    best.offer(mesh, t, p)?;
                }
            }
        }
    }
    // Inside a cell that was left out: not in the mesh.
    best.inside()
}

/// The candidate tetrahedron that holds a point best: the largest least barycentric
/// coordinate, the first offered among equals.
#[derive(Default)]
struct Best(Option<(usize, [f64; 4], f64)>);

impl Best {
    /// Weighs tetrahedron `t` for `p`; `None` when its corners are missing.
    fn offer(&mut self, mesh: &Mesh, t: usize, p: Point3) -> Option<()> {
        let c = mesh.corners(t)?;
        let l = barycentric(&c, p);
        let worst = l.iter().copied().fold(f64::INFINITY, f64::min);
        if self.0.as_ref().is_none_or(|b| worst > b.2) {
            self.0 = Some((t, l, worst));
        }
        Some(())
    }

    /// The best tetrahedron and `p`'s barycentric coordinates in it, when `p` lies inside it
    /// (within 10⁻⁹).
    fn inside(self) -> Option<(usize, [f64; 4])> {
        self.0.filter(|b| b.2 >= -1e-9).map(|(t, l, _)| (t, l))
    }
}

/// The tetrahedron of an unstructured mesh containing `p` (within 10⁻⁹ in barycentric terms).
fn locate_in_buckets(mesh: &Mesh, p: Point3) -> Option<(usize, [f64; 4])> {
    let b = mesh.buckets.as_ref()?;
    if !p.is_finite() {
        return None;
    }
    for d in 0..3 {
        let f = (p[d] - b.lo[d]) / b.step[d];
        if !(f >= -1e-9 && f <= b.dims[d] as f64 + 1e-9) {
            return None;
        }
    }
    let ijk = [0, 1, 2].map(|d| solid::bucket_index(b, p[d], d));
    let cell = ijk[0] + b.dims[0] * (ijk[1] + b.dims[1] * ijk[2]);
    let (s, e) = (*b.start.get(cell)? as usize, *b.start.get(cell + 1)? as usize);
    let mut best = Best::default();
    for &t in b.items.get(s..e)? {
        best.offer(mesh, t as usize, p)?;
    }
    best.inside()
}

/// The tetrahedra whose bounding boxes meet `region`: from the buckets of an unstructured mesh,
/// every tetrahedron otherwise (a box test still drops the far ones).
pub fn tets_near(mesh: &Mesh, region: Aabb) -> Vec<usize> {
    let Aabb { min: lo, max: hi } = region;
    let meets = |t: usize| mesh.corners(t).is_some_and(|c| Aabb::around(c).unwrap_or_default().overlaps(region));
    let Some(bk) = mesh.buckets.as_ref() else { return (0..mesh.tets.len()).filter(|&t| meets(t)).collect() };
    let cell = |p: Point3, d: usize| solid::bucket_index(bk, p[d], d);
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
    use emcraft_model::{Boundary, Material, Model, Section, Waveguide};

    /// A length (m), checked.
    fn metres(v: f64) -> emcraft_model::Metres {
        emcraft_model::Metres::new(v).unwrap()
    }

    fn model() -> Model {
        let mut m = Model::new();
        m.add_material(Material::new("ptfe", 2.0, 1.0).unwrap()).unwrap();
        let s = |l: f64, m: &str| Section::new(metres(l), m);
        m.set_waveguide(Waveguide {
            width: metres(0.02286),
            height: metres(0.01016),
            sections: vec![s(0.015, "ptfe"), s(0.02, "vacuum"), s(0.015, "ptfe")],
            layers: Vec::new(),
            strips: Vec::new(),
            vias: Vec::new(),
        })
        .unwrap();
        m
    }

    fn tri_area(mesh: &Mesh, f: &[u32; 3]) -> f64 {
        let p = f.map(|n| mesh.nodes[n as usize]);
        0.5 * (p[1] - p[0]).cross(p[2] - p[0]).norm()
    }

    /// Regression: a mesh with a flat tetrahedron reached the solver, which refused it with
    /// "degenerate tetrahedron (volume -0e0)". The mesh is refused where it is made.
    #[test]
    fn degenerate_and_inverted_tetrahedra_are_refused() {
        let mesh = waveguide(&model(), metres(0.004)).unwrap();
        assert_eq!(check_tetrahedra(&mesh), Ok(()));
        let mut flat = mesh.clone();
        flat.tets[3][1] = flat.tets[3][0];
        let e = check_tetrahedra(&flat).unwrap_err().to_string();
        assert!(e.contains("tetrahedron 4 is degenerate"), "{e}");
        let mut inverted = mesh.clone();
        inverted.tets[7].swap(0, 1);
        let e = check_tetrahedra(&inverted).unwrap_err().to_string();
        assert!(e.contains("tetrahedron 8 is inverted"), "{e}");
        // A corner moved onto the plane of the other three, or to no place at all.
        let mut squashed = mesh.clone();
        let [a, b, c, d] = squashed.tets[0].map(|n| squashed.nodes[n as usize]);
        let n = (b - a).cross(c - a).normalized().unwrap();
        squashed.nodes[squashed.tets[0][3] as usize] = d - n * n.dot(d - a);
        assert!(check_tetrahedra(&squashed).is_err());
        let mut lost = mesh;
        lost.nodes[lost.tets[0][2] as usize] = Point3::new(f64::NAN, 0.0, 0.0);
        assert!(check_tetrahedra(&lost).is_err());
    }

    #[test]
    fn volume_is_the_guide_and_every_tet_is_positive() {
        let m = model();
        let mesh = waveguide(&m, metres(0.003)).unwrap();
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
        let mesh = waveguide(&m, metres(0.004)).unwrap();
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
        let full = waveguide(&m, metres(0.004)).unwrap();
        m.set_symmetry(vec![Symmetry { axis: Axis::X, at: a / 2.0, kind: SymmetryKind::Magnetic }]).unwrap();
        let half = waveguide(&m, metres(0.004)).unwrap();
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
        let quarter = waveguide(&m, metres(0.004)).unwrap();
        assert!((volumes(&quarter).iter().sum::<f64>() - a * b * l / 4.0).abs() < 1e-12 * a * b * l);
        assert!((area(&quarter, Boundary::Symmetry(SymmetryKind::Electric)) - a / 2.0 * l).abs() < 1e-13);
        assert!((area(&quarter, Boundary::Symmetry(SymmetryKind::Magnetic)) - b / 2.0 * l).abs() < 1e-13);
        // A guide no longer symmetric is refused.
        let mut w = m.waveguide.clone().unwrap();
        w.sections[1].width = Some(metres(0.01));
        w.sections[1].offset = Some(0.0);
        m.set_waveguide(w).unwrap();
        assert!(waveguide(&m, metres(0.004)).is_err());
    }

    #[test]
    fn locate_finds_the_containing_tet() {
        let m = model();
        let mesh = waveguide(&m, metres(0.004)).unwrap();
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
        let iris =
            Section { length: metres(0.002), material: "vacuum".into(), width: Some(metres(0.01)), offset: None, height: None, offset_y: None };
        m.set_waveguide(Waveguide {
            width: metres(0.02286),
            height: metres(0.01016),
            sections: vec![Section::new(metres(0.01), "vacuum"), iris, Section::new(metres(0.01), "vacuum")],
            layers: Vec::new(),
            strips: Vec::new(),
            vias: Vec::new(),
        })
        .unwrap();
        let mesh = waveguide(&m, metres(0.003)).unwrap();
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
        let iris =
            Section { length: metres(0.002), material: "vacuum".into(), width: Some(metres(0.01)), offset: None, height: None, offset_y: None };
        m.set_waveguide(Waveguide {
            width: metres(0.02286),
            height: metres(0.01016),
            sections: vec![Section::new(metres(0.01), "vacuum"), iris, Section::new(metres(0.01), "vacuum")],
            layers: Vec::new(),
            strips: Vec::new(),
            vias: Vec::new(),
        })
        .unwrap();
        let o = Options::new(metres(0.004));
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
        assert_eq!(waveguide_with(&uniform_guide, &o).unwrap(), waveguide_with(&uniform_guide, &Options::uniform(metres(0.004))).unwrap());
        assert!(
            waveguide_with(&m, &Options { size: metres(0.004), edge: 0.0, growth: 1.5, size_y: None, strip_edge: 0.05, strip_end: None }).is_err()
        );
        assert!(
            waveguide_with(&m, &Options { size: metres(0.004), edge: 0.5, growth: 1.0, size_y: None, strip_edge: 0.05, strip_end: None }).is_err()
        );
    }

    #[test]
    fn vias_leave_their_cells_out_and_strips_on_a_symmetry_plane_stay_conductors() {
        use emcraft_model::{Axis, Strip, Symmetry, SymmetryKind, Via};
        let (a, b, l) = (0.012, 0.002, 0.006);
        let mut m = Model::new();
        let strip = Strip { y: b / 2.0, x0: 0.002, width: metres(0.002), z0: 0.0, z1: l };
        let via = Via { x0: 0.0071, z0: 0.0023, wx: metres(0.0013), wz: metres(0.0017), y0: 0.0, y1: b };
        m.set_waveguide(Waveguide {
            width: metres(a),
            height: metres(b),
            sections: vec![Section::new(metres(l), "vacuum")],
            layers: Vec::new(),
            strips: vec![strip],
            vias: vec![via],
        })
        .unwrap();
        let mesh = waveguide(&m, metres(0.001)).unwrap();
        // The post's faces are grid planes, its cells gone, its four sides walls.
        let g = mesh.grid.as_ref().unwrap();
        for (lines, v) in [(&g.x, 0.0071), (&g.x, 0.0084), (&g.z, 0.0023), (&g.z, 0.004)] {
            assert!(lines.iter().any(|x| (x - v).abs() < 1e-15), "{v}");
        }
        let total: f64 = volumes(&mesh).iter().sum();
        let exact = a * b * l - via.wx * via.wz * b;
        assert!((total - exact).abs() < 1e-12 * exact, "{total} {exact}");
        let area = |mesh: &Mesh, tag: Boundary| mesh.boundary.iter().filter(|f| f.tag == tag).map(|f| tri_area(mesh, &f.nodes)).sum::<f64>();
        let walls = 2.0 * (a + b) * l - 2.0 * via.wx * via.wz + 2.0 * (via.wx + via.wz) * b;
        // The strip's faces inside the mesh are tagged once each.
        assert!((area(&mesh, Boundary::Pec) - walls - strip.width * l).abs() < 1e-12, "{}", area(&mesh, Boundary::Pec));
        assert!(locate(&mesh, Point3::new(0.0075, 0.001, 0.003)).is_none() && locate(&mesh, Point3::new(0.0065, 0.001, 0.003)).is_some());
        // Halved on the strip's plane by a magnetic wall: the strip's faces there are conductor,
        // the rest of the plane the wall.
        m.set_symmetry(vec![Symmetry { axis: Axis::Y, at: b / 2.0, kind: SymmetryKind::Magnetic }]).unwrap();
        let half = waveguide(&m, metres(0.001)).unwrap();
        let plane = a * l - via.wx * via.wz;
        assert!((area(&half, Boundary::Symmetry(SymmetryKind::Magnetic)) - (plane - strip.width * l)).abs() < 1e-12);
        let half_walls = (2.0 * b / 2.0 + a) * l - via.wx * via.wz + 2.0 * (via.wx + via.wz) * b / 2.0;
        assert!((area(&half, Boundary::Pec) - half_walls - strip.width * l).abs() < 1e-12, "{}", area(&half, Boundary::Pec));
        // A via under the strip only (y1 below the plane) has no image: refused.
        let mut w = m.waveguide.clone().unwrap();
        w.vias[0].y1 = b / 2.0;
        m.set_waveguide(w).unwrap();
        assert!(waveguide(&m, metres(0.001)).is_err());
    }

    #[test]
    fn hostile_sizes_are_refused() {
        let m = model();
        // A size of zero, below or not finite is no `Metres`: it cannot reach the mesher.
        assert!([0.0, f64::NAN, -1.0].into_iter().all(|h| emcraft_model::Metres::new(h).is_err()));
        assert!(matches!(waveguide(&m, metres(1e-6)), Err(MeshError::TooLarge { .. })));
        assert!(waveguide(&Model::new(), metres(0.001)).is_err());
    }
}
