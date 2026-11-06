//! Bodies of revolution: the eigenmodes of a cavity invariant under rotation about the z axis,
//! solved on its meridian half-plane (r, z), one azimuthal order m at a time.
//!
//! **The fields.** A field of the body goes as e^{−jmφ}: E = (E_t(r, z) + φ̂ E_φ(r, z)) e^{−jmφ},
//! E_t = (E_r, E_z). With ∂_φ → −jm the curl's components are (∇×E)_φ = ∂_z E_r − ∂_r E_z,
//! (∇×E)_r = −(jm E_z + ∂_z u)/r and (∇×E)_z = (∂_r u + jm E_r)/r, u = r E_φ. The unknowns are
//! E_t, by edge (Whitney) functions on the half-plane's triangles, and u, by nodal functions — the
//! hybrid edge/nodal choice of J.-F. Lee, G. M. Wilkins, R. Mittra, *Finite-element analysis of
//! axisymmetric cavity resonator using a hybrid edge element technique*, IEEE Trans. Microwave
//! Theory Tech. 41(11):1981–1987, 1993 (also A. D. Greenwood, J.-M. Jin, *A novel efficient
//! algorithm for scattering from a complex BOR using mixed finite elements and cylindrical PML*,
//! IEEE Trans. Antennas Propag. 47(4):620–629, 1999). Taken as v = −j u the problem is real:
//! per radian of φ,
//!
//! ∫∫ [ r (∂_z E_r − ∂_r E_z)² + (∇v + m E_t)² / r ] / μ_r dr dz = k₀² ∫∫ ε_r [ r |E_t|² + v² / r ] dr dz.
//!
//! **The axis.** u = r E_φ vanishes on it (v = 0 there). For m ≠ 0 the field has no E_z on the
//! axis (it would not be single-valued), so the edges along it are dropped; for m = 0 they are
//! kept. The 1/r term is finite only for fields whose ∇v + m E_t vanishes on the axis; a basis
//! function that does not is held to it by the term's weight, which grows as the mesh reaches the
//! axis (the quadrature's points are inside the triangles).
//!
//! **The gradients.** ∇(ψ e^{−jmφ}) has E_t = ∇_t ψ and v = −mψ: in the unknowns exactly the edge
//! differences of the nodal ψ and −m times it. They are the problem's null space (k₀ = 0) and
//! are projected out of the subspace iteration, as in the 3D cavity's (P. Arbenz, R. Geus,
//! *Multilevel preconditioned iterative eigensolvers for Maxwell eigenvalue problems*, Appl. Numer.
//! Math. 54(2):107–121, 2005): x ← x − G (GᵀMG)⁻¹ GᵀM x.
//!
//! **The solve.** Subspace iteration with the shift σ = k₀² of `near`: (K − σM) y = M x, the
//! gradients projected out, Rayleigh–Ritz on the subspace (the small problem by Cholesky and the
//! dense QR), at most [`MAX_ITERATIONS`] passes. The boundary off the axis is a perfect conductor.

use std::collections::BTreeMap;
use std::f64::consts::PI;

use emcraft_model::Point3;
use emcraft_model::units::C0;
use num_complex::Complex64 as C;

use crate::dense::Cholesky;
use crate::driven::{Result, invalid};
use crate::ldlt::Ldlt;
use crate::sparse::Csr;

mod field;
mod ports;

pub use field::{HalfPlaneField, MAX_FIELD_SIDE, field};
pub use ports::{BorPort, BorSweepPoint, MAX_PORT_MODES, s_parameters};

/// Passes of the subspace iteration before it stops.
pub const MAX_ITERATIONS: usize = 300;
/// The most triangles a profile may have.
pub const MAX_TRIANGLES: usize = 2_000_000;

/// The meridian half-plane of a body of revolution: points (r, z) in metres with r ≥ 0, the
/// triangles' corners, each triangle's material (εr, μr).
#[derive(Clone, Debug, Default)]
pub struct Profile {
    pub points: Vec<[f64; 2]>,
    pub triangles: Vec<[u32; 3]>,
    pub material: Vec<(f64, f64)>,
}

/// A mode: its frequency (Hz) and azimuthal order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BorMode {
    pub frequency: f64,
    pub m: u32,
}

/// The unknowns: the kept edges' then the kept nodes' (v).
struct Dofs {
    /// Per edge (sorted node pair), its unknown.
    edge: BTreeMap<[u32; 2], usize>,
    /// Per node, its v unknown.
    node: Vec<Option<usize>>,
    /// Per node, its gradient's column (ψ kept).
    psi: Vec<Option<usize>>,
    count: usize,
    psis: usize,
}

/// The edges on the profile's boundary (one triangle only), sorted.
fn boundary_edges(p: &Profile) -> BTreeMap<[u32; 2], usize> {
    let mut count: BTreeMap<[u32; 2], usize> = BTreeMap::new();
    for t in &p.triangles {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            *count.entry([a.min(b), a.max(b)]).or_insert(0) += 1;
        }
    }
    count
}

fn numbering(p: &Profile, m: u32) -> Dofs {
    numbering_open(p, m, &std::collections::BTreeSet::new())
}

/// [`numbering`] with the boundary edges `open` (ports) kept, not conductor.
fn numbering_open(p: &Profile, m: u32, open: &std::collections::BTreeSet<[u32; 2]>) -> Dofs {
    let tol = 1e-12 * p.points.iter().map(|q| q[0].abs().max(q[1].abs())).fold(0.0, f64::max).max(1e-300);
    let on_axis = |n: u32| p.points.get(n as usize).is_some_and(|q| q[0] <= tol);
    let counts = boundary_edges(p);
    let mut wall_node = vec![false; p.points.len()];
    let mut wall_edge = std::collections::BTreeSet::new();
    for (e, c) in &counts {
        // A boundary edge off the axis is the conductor (both ends on it: an edge of the axis).
        if *c == 1 && !(on_axis(e[0]) && on_axis(e[1])) && !open.contains(e) {
            wall_edge.insert(*e);
            for n in e {
                if let Some(w) = wall_node.get_mut(*n as usize) {
                    *w = true;
                }
            }
        }
    }
    let mut edge = BTreeMap::new();
    let mut count = 0;
    for e in counts.keys() {
        let axis = on_axis(e[0]) && on_axis(e[1]);
        if wall_edge.contains(e) || (axis && m != 0) {
            continue;
        }
        edge.insert(*e, count);
        count += 1;
    }
    let mut node = vec![None; p.points.len()];
    let mut psi = vec![None; p.points.len()];
    let mut psis = 0;
    for n in 0..p.points.len() as u32 {
        let (wall, axis) = (wall_node[n as usize], on_axis(n));
        if !wall && !axis {
            node[n as usize] = Some(count);
            count += 1;
        }
        // ψ: zero on the conductor; on the axis for m ≠ 0 (its v and E_z are zero there).
        if !wall && (m == 0 || !axis) {
            psi[n as usize] = Some(psis);
            psis += 1;
        }
    }
    Dofs { edge, node, psi, count, psis }
}

/// Dunavant's degree-5 rule on a triangle: barycentric points and weights (summing to 1).
const RULE: [([f64; 3], f64); 7] = {
    const A1: f64 = 0.059_715_871_789_770;
    const B1: f64 = 0.470_142_064_105_115;
    const W1: f64 = 0.132_394_152_788_506;
    const A2: f64 = 0.797_426_985_353_087;
    const B2: f64 = 0.101_286_507_323_456;
    const W2: f64 = 0.125_939_180_544_827;
    [
        ([1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0], 0.225),
        ([A1, B1, B1], W1),
        ([B1, A1, B1], W1),
        ([B1, B1, A1], W1),
        ([A2, B2, B2], W2),
        ([B2, A2, B2], W2),
        ([B2, B2, A2], W2),
    ]
};

/// K and M of the profile in the unknowns, and the gradients G (columns: ψ).
fn assemble(p: &Profile, m: u32, d: &Dofs) -> Result<(Csr, Csr, Vec<Vec<(usize, f64)>>)> {
    assemble_with(p, m, d, &[])
}

/// [`assemble`] on a pattern that also couples each group of `extra` (a port's unknowns).
fn assemble_with(p: &Profile, m: u32, d: &Dofs, extra: &[Vec<usize>]) -> Result<(Csr, Csr, Vec<Vec<(usize, f64)>>)> {
    let mf = f64::from(m);
    let mut groups: Vec<Vec<usize>> = Vec::with_capacity(p.triangles.len());
    // Per triangle: its unknowns with, per local function, (unknown, kind) for the element.
    let mut locals: Vec<Vec<(usize, Local)>> = Vec::with_capacity(p.triangles.len());
    for t in &p.triangles {
        let mut l = Vec::with_capacity(6);
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            if let Some(&u) = d.edge.get(&[a.min(b), a.max(b)]) {
                // The Whitney function from the lower node to the higher.
                let (i, j) = if a < b { (k, (k + 1) % 3) } else { ((k + 1) % 3, k) };
                l.push((u, Local::Edge(i, j)));
            }
        }
        for (k, &n) in t.iter().enumerate() {
            if let Some(Some(u)) = d.node.get(n as usize) {
                l.push((*u, Local::Node(k)));
            }
        }
        groups.push(l.iter().map(|x| x.0).collect());
        locals.push(l);
    }
    groups.extend(extra.iter().cloned());
    let pattern = Csr::from_groups(d.count, groups.iter().map(Vec::as_slice));
    let (mut k, mut mm) = (pattern.zeroed(), pattern.zeroed());
    for (ti, t) in p.triangles.iter().enumerate() {
        let corner = |k: usize| p.points.get(t[k] as usize).copied().ok_or_else(|| invalid("profile", "a triangle's corner is not a point"));
        let q = [corner(0)?, corner(1)?, corner(2)?];
        let (eps, mu) = p.material.get(ti).copied().unwrap_or((1.0, 1.0));
        let det = (q[1][0] - q[0][0]) * (q[2][1] - q[0][1]) - (q[2][0] - q[0][0]) * (q[1][1] - q[0][1]);
        let area = 0.5 * det.abs();
        if !(area > 0.0) {
            return Err(invalid("profile", format!("triangle {} has no area", ti + 1)));
        }
        // ∇λ_i (constant).
        let g = [0, 1, 2].map(|i| {
            let (j, k) = ((i + 1) % 3, (i + 2) % 3);
            [(q[j][1] - q[k][1]) / det, (q[k][0] - q[j][0]) / det]
        });
        let l = &locals[ti];
        for &(lam, w) in &RULE {
            let r = lam[0] * q[0][0] + lam[1] * q[1][0] + lam[2] * q[2][0];
            let wa = w * area;
            // Each local function's E_t, its curl (∂_z E_r − ∂_r E_z), its v, and its ∇v + m E_t.
            let fields: Vec<([f64; 2], f64, f64, [f64; 2])> = l
                .iter()
                .map(|(_, f)| match *f {
                    Local::Edge(i, j) => {
                        let e = [lam[i] * g[j][0] - lam[j] * g[i][0], lam[i] * g[j][1] - lam[j] * g[i][1]];
                        // ∂_z W_r − ∂_r W_z = −2 (g_i × g_j).
                        let curl = -2.0 * (g[i][0] * g[j][1] - g[i][1] * g[j][0]);
                        (e, curl, 0.0, [mf * e[0], mf * e[1]])
                    }
                    Local::Node(i) => ([0.0; 2], 0.0, lam[i], g[i]),
                })
                .collect();
            for (a, fa) in l.iter().zip(&fields) {
                for (b, fb) in l.iter().zip(&fields) {
                    let kv = (r * fa.1 * fb.1 + (fa.3[0] * fb.3[0] + fa.3[1] * fb.3[1]) / r) / mu;
                    let mv = eps * (r * (fa.0[0] * fb.0[0] + fa.0[1] * fb.0[1]) + fa.2 * fb.2 / r);
                    k.add(a.0, b.0, C::new(wa * kv, 0.0));
                    mm.add(a.0, b.0, C::new(wa * mv, 0.0));
                }
            }
        }
    }
    // G: ψ_p's gradient, the edges' differences (to the higher node) and v = −m ψ.
    let mut gradients: Vec<Vec<(usize, f64)>> = vec![Vec::new(); d.psis];
    for (e, &u) in &d.edge {
        for (n, s) in [(e[0], -1.0), (e[1], 1.0)] {
            if let Some(Some(c)) = d.psi.get(n as usize) {
                gradients[*c].push((u, s));
            }
        }
    }
    if m != 0 {
        for (n, c) in d.psi.iter().enumerate() {
            if let (Some(c), Some(Some(u))) = (c, d.node.get(n)) {
                gradients[*c].push((*u, -mf));
            }
        }
    }
    Ok((k, mm, gradients))
}

#[derive(Clone, Copy)]
enum Local {
    Edge(usize, usize),
    Node(usize),
}

fn real(v: &[C]) -> Vec<f64> {
    v.iter().map(|c| c.re).collect()
}

fn mul(a: &Csr, x: &[f64]) -> Vec<f64> {
    real(&a.mul(&x.iter().map(|v| C::new(*v, 0.0)).collect::<Vec<_>>()))
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// The `count` modes of order `m` nearest `near` (Hz) of the profile's cavity, lowest first.
///
/// ```
/// use emcraft_solver::bor::{Profile, eigenmodes};
///
/// // Refused: no triangles, a point left of the axis.
/// assert!(eigenmodes(&Profile::default(), 0, 1, 1e10).is_err());
/// let p = Profile { points: vec![[-1.0, 0.0], [1.0, 0.0], [0.0, 1.0]], triangles: vec![[0, 1, 2]], material: vec![(1.0, 1.0)] };
/// assert!(eigenmodes(&p, 0, 1, 1e10).is_err());
/// ```
pub fn eigenmodes(p: &Profile, m: u32, count: usize, near: f64) -> Result<Vec<BorMode>> {
    if p.triangles.is_empty() || p.triangles.len() > MAX_TRIANGLES || p.material.len() != p.triangles.len() {
        return Err(invalid("profile", format!("must have 1 to {MAX_TRIANGLES} triangles, each with a material")));
    }
    if !p.points.iter().all(|q| q[0].is_finite() && q[1].is_finite() && q[0] >= 0.0) {
        return Err(invalid("profile", "every point must be finite with r ≥ 0 (the half-plane right of the axis)"));
    }
    if !p.material.iter().all(|(e, u)| e.is_finite() && u.is_finite() && *e > 0.0 && *u > 0.0) {
        return Err(invalid("profile", "every material needs finite εr > 0 and μr > 0"));
    }
    if p.triangles.iter().flatten().any(|&n| n as usize >= p.points.len()) {
        return Err(invalid("profile", "a triangle's corner is not a point"));
    }
    if !(1..=50).contains(&count) || m > 64 {
        return Err(invalid("modes", "1 to 50 modes, of order m from 0 to 64"));
    }
    crate::driven::check_frequency(near)?;
    let d = numbering(p, m);
    if d.count == 0 {
        return Err(invalid("profile", "has no unknowns: no interior to the conductor"));
    }
    let (k, mm, gradients) = assemble(p, m, &d)?;
    let sigma = (2.0 * PI * near / C0).powi(2);
    // A point per unknown for the ordering: an edge's middle, a node.
    let at = |n: u32| p.points.get(n as usize).map_or(Point3::ORIGIN, |q| Point3::new(q[0], q[1], 0.0));
    let mut xyz = vec![Point3::ORIGIN; d.count];
    for (e, &u) in &d.edge {
        xyz[u] = at(e[0]).lerp(at(e[1]), 0.5);
    }
    for (n, u) in d.node.iter().enumerate() {
        if let Some(u) = u {
            xyz[*u] = at(n as u32);
        }
    }
    let mut shifted = k.clone();
    for (v, w) in shifted.val.iter_mut().zip(&mm.val) {
        *v -= w * sigma;
    }
    let factor = Ldlt::factor(&shifted, &xyz)?;
    // GᵀMG, factored for the projection, column by column in sparse form: (M g_c)'s entries
    // reach only the unknowns next to g_c's, and Gᵀ of them only the gradients there.
    let mut by_unknown: Vec<Vec<(usize, f64)>> = vec![Vec::new(); d.count];
    for (j, col) in gradients.iter().enumerate() {
        for &(i, g) in col {
            if let Some(r) = by_unknown.get_mut(i) {
                r.push((j, g));
            }
        }
    }
    let gtmg = {
        let mut b = Csr::builder(d.psis);
        for col in &gradients {
            let mut mg: BTreeMap<usize, f64> = BTreeMap::new();
            for &(i, g) in col {
                for e in mm.row(i) {
                    *mg.entry(mm.col[e]).or_insert(0.0) += mm.val[e].re * g;
                }
            }
            let mut row: BTreeMap<usize, f64> = BTreeMap::new();
            for (k, v) in mg {
                for &(j, g) in by_unknown.get(k).into_iter().flatten() {
                    *row.entry(j).or_insert(0.0) += g * v;
                }
            }
            b.row(row.into_iter().filter(|(_, v)| *v != 0.0).map(|(j, v)| (j, C::new(v, 0.0))));
        }
        b.finish()
    };
    let psi_at: Vec<Point3> = {
        let mut v = vec![Point3::ORIGIN; d.psis];
        for (n, c) in d.psi.iter().enumerate() {
            if let Some(c) = c {
                v[*c] = at(n as u32);
            }
        }
        v
    };
    let gfactor = if d.psis > 0 { Some(Ldlt::factor(&gtmg, &psi_at)?) } else { None };
    let project = |x: &mut Vec<f64>| -> Result<()> {
        let Some(f) = &gfactor else { return Ok(()) };
        let mx = mul(&mm, x);
        let mut y: Vec<C> = gradients.iter().map(|col| C::new(col.iter().map(|&(i, g)| g * mx[i]).sum(), 0.0)).collect();
        f.solve(&mut y)?;
        for (col, yc) in gradients.iter().zip(&y) {
            for &(i, g) in col {
                x[i] -= g * yc.re;
            }
        }
        Ok(())
    };
    // The subspace: twice the modes wanted, deterministic start vectors.
    let q = (2 * count).max(count + 4).min(d.count);
    let mut state = 0x2545_f491_4f6c_dd1du64;
    let mut basis: Vec<Vec<f64>> = (0..q)
        .map(|_| {
            (0..d.count)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    (state >> 11) as f64 / (1u64 << 53) as f64 - 0.5
                })
                .collect()
        })
        .collect();
    let mut last: Vec<f64> = Vec::new();
    for _ in 0..MAX_ITERATIONS {
        for x in &mut basis {
            project(x)?;
            let mut y: Vec<C> = mul(&mm, x).into_iter().map(|v| C::new(v, 0.0)).collect();
            factor.solve(&mut y)?;
            *x = real(&y);
            project(x)?;
        }
        // Rayleigh–Ritz: Kr z = λ Mr z.
        let kx: Vec<Vec<f64>> = basis.iter().map(|x| mul(&k, x)).collect();
        let mx: Vec<Vec<f64>> = basis.iter().map(|x| mul(&mm, x)).collect();
        let n = basis.len();
        let kr: Vec<f64> = (0..n * n).map(|ij| dot(&basis[ij / n], &kx[ij % n])).collect();
        let mr: Vec<f64> = (0..n * n).map(|ij| dot(&basis[ij / n], &mx[ij % n])).collect();
        let chol = Cholesky::new(&mr, n).ok_or_else(|| invalid("BOR eigenmodes", "the subspace lost its independence"))?;
        // A = L⁻¹ Kr L⁻ᵀ, column by column (Kr symmetric).
        let cols: Vec<Vec<f64>> = (0..n).map(|j| chol.lower_solve(&(0..n).map(|i| kr[i * n + j]).collect::<Vec<_>>())).collect();
        let rows: Vec<Vec<f64>> = (0..n).map(|i| chol.lower_solve(&(0..n).map(|j| cols[j][i]).collect::<Vec<_>>())).collect();
        let a: Vec<f64> = (0..n * n).map(|ij| 0.5 * (rows[ij / n][ij % n] + rows[ij % n][ij / n])).collect();
        let mut pairs = crate::dense::eigen(&a, n).ok_or_else(|| invalid("BOR eigenmodes", "the projected problem did not converge"))?;
        pairs.retain(|e| e.im == 0.0 && !e.vector.is_empty());
        pairs.sort_by(|x, y| (x.re - sigma).abs().total_cmp(&(y.re - sigma).abs()));
        // The new basis: the Ritz vectors, nearest the shift first.
        let mut next = Vec::with_capacity(pairs.len());
        for e in &pairs {
            let z = chol.upper_solve(&e.vector);
            let mut x = vec![0.0; d.count];
            for (zj, b) in z.iter().zip(&basis) {
                for (xi, bi) in x.iter_mut().zip(b) {
                    *xi += zj * bi;
                }
            }
            let norm = dot(&x, &mul(&mm, &x)).sqrt();
            if norm > 0.0 && norm.is_finite() {
                x.iter_mut().for_each(|v| *v /= norm);
                next.push(x);
            }
        }
        if next.len() < count {
            return Err(invalid("BOR eigenmodes", "the subspace lost its independence"));
        }
        basis = next;
        let values: Vec<f64> = pairs.iter().take(count).map(|e| e.re).collect();
        let settled = values.len() == last.len() && values.iter().zip(&last).all(|(a, b)| (a - b).abs() <= 1e-11 * a.abs().max(sigma));
        last = values;
        if settled {
            let mut modes: Vec<BorMode> = last.iter().map(|&k2| BorMode { frequency: k2.max(0.0).sqrt() * C0 / (2.0 * PI), m }).collect();
            modes.sort_by(|a, b| a.frequency.total_cmp(&b.frequency));
            return Ok(modes);
        }
    }
    Err(invalid("BOR eigenmodes", format!("did not converge in {MAX_ITERATIONS} passes")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rectangle [0, a] × [0, d] of the half-plane in `nr` × `nz` cells, each split by its
    /// diagonal: a closed cylindrical cavity's profile.
    fn pillbox(a: f64, d: f64, nr: usize, nz: usize) -> Profile {
        annulus(0.0, a, d, nr, nz)
    }

    /// As [`pillbox`], the profile from r = `b` (a coaxial cavity's inner conductor) to `a`.
    fn annulus(b: f64, a: f64, d: f64, nr: usize, nz: usize) -> Profile {
        let id = |i: usize, j: usize| (j * (nr + 1) + i) as u32;
        let points = (0..=nz).flat_map(|j| (0..=nr).map(move |i| [b + (a - b) * i as f64 / nr as f64, d * j as f64 / nz as f64])).collect();
        let mut triangles = Vec::new();
        for j in 0..nz {
            for i in 0..nr {
                triangles.push([id(i, j), id(i + 1, j), id(i + 1, j + 1)]);
                triangles.push([id(i, j), id(i + 1, j + 1), id(i, j + 1)]);
            }
        }
        let material = vec![(1.0, 1.0); triangles.len()];
        Profile { points, triangles, material }
    }

    /// k (1/m) of a pillbox's mode from the Bessel zero `x` of its order and p half waves.
    fn k(x: f64, a: f64, p: f64, d: f64) -> f64 {
        ((x / a).powi(2) + (p * PI / d).powi(2)).sqrt()
    }

    fn frequencies(m: u32, count: usize, near: f64, cells: (usize, usize)) -> Vec<f64> {
        let p = pillbox(0.01, 0.015, cells.0, cells.1);
        eigenmodes(&p, m, count, near).unwrap().iter().map(|md| md.frequency).collect()
    }

    /// A pillbox of radius 10 mm and length 15 mm against its closed forms (D. M. Pozar,
    /// *Microwave Engineering*, 4th ed., Wiley 2012, §6.3; Bessel zeros from M. Abramowitz,
    /// I. A. Stegun, *Handbook of Mathematical Functions*, 1964, Table 9.5): m = 0 TM010, TM011,
    /// TE011; m = 1 TE111, TM110; m = 2 TE211.
    #[test]
    fn a_pillbox_resonates_at_its_closed_forms_for_each_azimuthal_order() {
        let (a, d) = (0.01, 0.015);
        let f = |kk: f64| kk * C0 / (2.0 * PI);
        let (x01, x11, xp01, xp11, xp21) =
            (2.404_825_557_695_773, 3.831_705_970_207_512, 3.831_705_970_207_512, 1.841_183_781_340_659, 3.054_236_928_227_14);
        let cases: [(u32, Vec<f64>); 3] = [
            (0, vec![f(k(x01, a, 0.0, d)), f(k(x01, a, 1.0, d)), f(k(xp01, a, 1.0, d))]),
            (1, vec![f(k(xp11, a, 1.0, d)), f(k(x11, a, 0.0, d))]),
            (2, vec![f(k(xp21, a, 1.0, d))]),
        ];
        for (m, exact) in cases {
            let mut exact = exact;
            exact.sort_by(f64::total_cmp);
            let got = frequencies(m, exact.len(), exact[0] * 0.95, (24, 36));
            for (g, e) in got.iter().zip(&exact) {
                assert!((g - e).abs() / e < 5e-3, "m = {m}: {got:?} against {exact:?}");
            }
        }
    }

    /// Second order: halving the cells quarters each order's error (TM010, TE111, TE211).
    #[test]
    fn the_error_falls_as_the_square_of_the_cells() {
        let (a, d) = (0.01, 0.015);
        let f = |x: f64, p: f64| k(x, a, p, d) * C0 / (2.0 * PI);
        for (m, exact) in [(0u32, f(2.404_825_557_695_773, 0.0)), (1, f(1.841_183_781_340_659, 1.0)), (2, f(3.054_236_928_227_14, 1.0))] {
            let error = |s: usize| (frequencies(m, 1, exact * 0.97, (12 * s, 18 * s))[0] - exact).abs() / exact;
            let ratio = error(1) / error(2);
            assert!((3.5..4.5).contains(&ratio), "m = {m}: the error fell by {ratio}");
        }
    }

    /// A coaxial cavity (conductors of radii 2 and 10 mm, 15 mm long, shorted at both ends): its
    /// TEM resonances at p c / 2d whatever the radii, off the axis (Pozar §6.3).
    #[test]
    fn a_coaxial_cavity_resonates_in_its_tem_mode_at_half_waves() {
        let d = 0.015;
        let profile = annulus(0.002, 0.01, d, 16, 30);
        // The mode nearest each half wave count (the guide's TM01 lies between the first two).
        for p in [1.0, 2.0] {
            let exact = p * C0 / (2.0 * d);
            let got = eigenmodes(&profile, 0, 1, exact).unwrap()[0].frequency;
            assert!((got - exact).abs() / exact < 1e-3, "p = {p}: {got} against {exact}");
        }
    }
}
