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

mod topology;

pub use topology::{LOCAL_EDGES, LOCAL_FACES, Topology};

use emcraft_model::mesh::TETS_PER_CELL;
use emcraft_model::{Boundary, BoundaryFace, Grid, Mesh, Model, positive};

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

fn signed_volume(p: &[[f64; 3]; 4]) -> f64 {
    let d = |k: usize, i: usize| p[k][i] - p[0][i];
    let (a, b, c) = ([d(1, 0), d(1, 1), d(1, 2)], [d(2, 0), d(2, 1), d(2, 2)], [d(3, 0), d(3, 1), d(3, 2)]);
    (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0])) / 6.0
}

/// Meshes the model's waveguide with tetrahedra of about `size` (m) on a side: x and y divided
/// evenly, each section along z divided on its own so section boundaries are grid planes. Faces
/// at z = 0 and z = length are tagged as ports 0 and 1, every other boundary face as a perfect
/// conductor.
pub fn waveguide(model: &Model, size: f64) -> Result<Mesh> {
    let size = positive("element size", size)?;
    let w = model.waveguide.as_ref().ok_or_else(|| MeshError::Invalid { what: "model".into(), why: "has no waveguide".into() })?;
    let nx = divisions(w.width, size, 2);
    let ny = divisions(w.height, size, 1);
    let mut z = vec![0.0];
    let mut materials = Vec::new();
    let mut at = 0.0;
    let mut cells_z = 0usize;
    for s in &w.sections {
        let n = divisions(s.length, size, 1);
        cells_z = cells_z.saturating_add(n);
        // The cap is checked before the lines are made.
        if nx.saturating_mul(ny).saturating_mul(cells_z).saturating_mul(TETS_PER_CELL) > MAX_TETS {
            break;
        }
        z.extend(line(at, s.length, n).skip(1));
        let (m, _) = model.material(&s.material)?;
        materials.extend(std::iter::repeat_n(m as u32, n));
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
    let grid = Grid { x: line(0.0, w.width, nx).collect(), y: line(0.0, w.height, ny).collect(), z };
    Ok(structured(grid, &materials, length))
}

/// The tetrahedral mesh of a grid whose cell layers along z have the given materials.
fn structured(grid: Grid, layer_material: &[u32], length: f64) -> Mesh {
    let (nx, ny, nz) = (grid.x.len() - 1, grid.y.len() - 1, grid.z.len() - 1);
    let node = |i: usize, j: usize, k: usize| (i + (nx + 1) * (j + (ny + 1) * k)) as u32;
    let mut nodes = Vec::with_capacity((nx + 1) * (ny + 1) * (nz + 1));
    for &z in &grid.z {
        for &y in &grid.y {
            for &x in &grid.x {
                nodes.push([x, y, z]);
            }
        }
    }
    let mut tets = Vec::with_capacity(nx * ny * nz * TETS_PER_CELL);
    let mut tet_material = Vec::with_capacity(tets.capacity());
    for k in 0..nz {
        let material = layer_material.get(k).copied().unwrap_or(0);
        for j in 0..ny {
            for i in 0..nx {
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
    let mut mesh = Mesh { nodes, tets, tet_material, boundary: Vec::new(), grid: Some(grid) };
    let topo = Topology::new(&mesh);
    let tol = 1e-9 * length.max(f64::MIN_POSITIVE);
    for (f, owners) in topo.faces.iter().zip(&topo.face_tets) {
        if owners[1] != u32::MAX {
            continue;
        }
        let zs = f.map(|n| mesh.nodes[n as usize][2]);
        let tag = if zs.iter().all(|z| z.abs() <= tol) {
            Boundary::Port(0)
        } else if zs.iter().all(|z| (z - length).abs() <= tol) {
            Boundary::Port(1)
        } else {
            Boundary::Pec
        };
        mesh.boundary.push(BoundaryFace { nodes: *f, tet: owners[0], tag });
    }
    mesh
}

/// Barycentric coordinates of `p` in a tetrahedron.
pub fn barycentric(c: &[[f64; 3]; 4], p: [f64; 3]) -> [f64; 4] {
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
pub fn locate(mesh: &Mesh, p: [f64; 3]) -> Option<(usize, [f64; 4])> {
    let g = mesh.grid.as_ref()?;
    let (nx, ny) = (g.x.len().checked_sub(1)?, g.y.len().checked_sub(1)?);
    let span = |l: &[f64]| (l.first().copied().unwrap_or(0.0), l.last().copied().unwrap_or(0.0));
    for (l, v) in [(&g.x, p[0]), (&g.y, p[1]), (&g.z, p[2])] {
        let (lo, hi) = span(l);
        let tol = 1e-9 * (hi - lo).abs();
        if !(v >= lo - tol && v <= hi + tol) {
            return None;
        }
    }
    let (i, j, k) = (cell_index(&g.x, p[0])?, cell_index(&g.y, p[1])?, cell_index(&g.z, p[2])?);
    let cell = i + nx * (j + ny * k);
    let mut best: Option<(usize, [f64; 4], f64)> = None;
    for t in cell * TETS_PER_CELL..(cell + 1) * TETS_PER_CELL {
        let c = mesh.corners(t)?;
        let l = barycentric(&c, p);
        let worst = l.iter().copied().fold(f64::INFINITY, f64::min);
        if best.as_ref().is_none_or(|b| worst > b.2) {
            best = Some((t, l, worst));
        }
    }
    best.map(|(t, l, _)| (t, l))
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
        let s = |l: f64, m: &str| Section { length: l, material: m.into() };
        m.set_waveguide(Waveguide { width: 0.02286, height: 0.01016, sections: vec![s(0.015, "ptfe"), s(0.02, "vacuum"), s(0.015, "ptfe")] })
            .unwrap();
        m
    }

    fn tri_area(mesh: &Mesh, f: &[u32; 3]) -> f64 {
        let p = f.map(|n| mesh.nodes[n as usize]);
        let u = [p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]];
        let v = [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]];
        let c = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        0.5 * (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt()
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
    fn locate_finds_the_containing_tet() {
        let m = model();
        let mesh = waveguide(&m, 0.004).unwrap();
        for p in [[0.011, 0.005, 0.025], [0.0, 0.0, 0.0], [0.02286, 0.01016, 0.05], [0.001, 0.009, 0.0349]] {
            let (t, l) = locate(&mesh, p).unwrap();
            assert!(l.iter().all(|&x| x >= -1e-12), "{l:?}");
            assert!((l.iter().sum::<f64>() - 1.0).abs() < 1e-12);
            let c = mesh.corners(t).unwrap();
            for d in 0..3 {
                let q: f64 = (0..4).map(|i| l[i] * c[i][d]).sum();
                assert!((q - p[d]).abs() < 1e-14);
            }
        }
        assert!(locate(&mesh, [0.03, 0.0, 0.0]).is_none());
        assert!(locate(&mesh, [f64::NAN, 0.0, 0.0]).is_none());
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
