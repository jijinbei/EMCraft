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
    // GᵀMG, factored for the projection.
    let mg: Vec<Vec<f64>> = gradients
        .iter()
        .map(|col| {
            let mut x = vec![0.0; d.count];
            for &(i, v) in col {
                x[i] = v;
            }
            mul(&mm, &x)
        })
        .collect();
    let gtmg = {
        let mut b = Csr::builder(d.psis);
        for c in &mg {
            b.row(gradients.iter().enumerate().filter_map(|(j, col)| {
                let v: f64 = col.iter().map(|&(i, g)| g * c[i]).sum();
                (v != 0.0).then_some((j, C::new(v, 0.0)))
            }));
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

/// A port of a body of revolution: the profile's boundary edges on the plane z = `z` from the
/// axis out (a circular guide's cross-section, its rim on the wall), with its `modes` lowest
/// modes of the order solved in its port condition.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BorPort {
    pub z: f64,
    pub modes: usize,
}

/// The S-parameters of a body of revolution at one frequency in one order m: between its ports'
/// first modes (named), and the unknowns.
#[derive(Clone, Debug, PartialEq)]
pub struct BorSweepPoint {
    pub frequency: f64,
    pub s: crate::SMatrix,
    pub modes: Vec<String>,
    pub unknowns: usize,
}

/// The most modes a port of a body of revolution takes.
pub const MAX_PORT_MODES: usize = 20;

/// A circular guide's mode of azimuthal order m in the half-plane's real form: (ẽ_r, ẽ_φ) such
/// that the port's projection ∫ E · e′ dS onto the mode, taken with its counter-rotating twin
/// e′ ∝ e^{+jmφ} (the reaction that pairs a field with a test function, E_φ W_φ = v w / r²), is
/// ∫ (E_r ẽ_r + v ẽ_φ / r) r dr.
#[derive(Clone, Copy, Debug)]
struct PortMode {
    te: bool,
    m: u32,
    radial: usize,
    kc: f64,
}

impl PortMode {
    /// (ẽ_r, ẽ_φ) at radius r: TE from H_z ∝ J_m(k_c r) (ẽ_r = m J_m/r, ẽ_φ = −k_c J_m′), TM
    /// from E_z ∝ J_m (ẽ_r = k_c J_m′, ẽ_φ = −m J_m/r): the twin's φ part, j its own, with the
    /// test side's v = +j r W_φ.
    fn field(&self, r: f64) -> [f64; 2] {
        let n = self.m as usize;
        let x = self.kc * r;
        let over = if x.abs() < 1e-8 { if n == 1 { 0.5 } else { 0.0 } } else { emcraft_fem::bessel::j(n, x) / x };
        let (a, b) = (f64::from(self.m) * self.kc * over, self.kc * emcraft_fem::bessel::jp(n, x));
        if self.te { [a, -b] } else { [b, -a] }
    }

    fn name(&self) -> String {
        format!("{}{}{}", if self.te { "TE" } else { "TM" }, self.m, self.radial)
    }

    /// κ with n̂ × ∇ × E_t = κ E_t leaving the port, for the medium's k (`None` at cut-off).
    fn kappa(&self, k: f64) -> Option<C> {
        let d = self.kc * self.kc - k * k;
        let g = if d >= 0.0 { C::new(d.sqrt(), 0.0) } else { C::new(0.0, (-d).sqrt()) };
        if g.norm() <= 1e-10 * k {
            return None;
        }
        Some(if self.te { g } else { -C::new(k * k, 0.0) / g })
    }
}

/// The `count` lowest modes of order m of a circle of radius `a`.
fn port_modes(a: f64, m: u32, count: usize) -> Vec<PortMode> {
    let n = m as usize;
    let x_max = 8.0 + 4.0 * count as f64 + f64::from(m);
    let mut all: Vec<PortMode> = Vec::new();
    for (te, zeros) in [
        (true, emcraft_fem::bessel::zeros(|x| emcraft_fem::bessel::jp(n, x), count + 1, x_max)),
        (false, emcraft_fem::bessel::zeros(|x| emcraft_fem::bessel::j(n, x), count + 1, x_max)),
    ] {
        all.extend(zeros.into_iter().filter(|x| *x > 1e-6).enumerate().map(|(k, x)| PortMode { te, m, radial: k + 1, kc: x / a }));
    }
    all.sort_by(|p, q| p.kc.total_cmp(&q.kc).then(q.te.cmp(&p.te)));
    all.truncate(count);
    all
}

/// Gauss–Legendre points and weights on [0, 1], five of them.
const GAUSS: [(f64, f64); 5] = [
    (0.046_910_077_030_668, 0.118_463_442_528_095),
    (0.230_765_344_947_158, 0.239_314_335_249_683),
    (0.5, 0.284_444_444_444_444),
    (0.769_234_655_052_842, 0.239_314_335_249_683),
    (0.953_089_922_969_332, 0.118_463_442_528_095),
];

/// The S-parameters of the profile's guide body in order `m` at `frequency` (Hz) between
/// `ports` (their first modes), the boundary elsewhere a perfect conductor and r = 0 the axis.
///
/// **The port condition** is the 3D ports' on the half-plane: the rank-one terms
/// (κ/μr‖ẽ‖²) b bᵀ with bᵢ = ∫ Nᵢ · ẽ r dr along the port's edges (E_r's edge functions with
/// ẽ_r, v's nodal ones with ẽ_φ / r · r), ‖ẽ‖² = ∫ (ẽ_r² + ẽ_φ²) r dr, the excitation 2κ b and
/// the modes' coefficients bᵀx/‖ẽ‖², all per radian of φ like the volume's terms; S between the
/// coefficients normalised to the modes' powers.
///
/// ```
/// use emcraft_solver::bor::{BorPort, Profile, s_parameters};
///
/// // Refused: no ports, a port on a plane with no edge.
/// let p = Profile { points: vec![[0.0, 0.0], [0.01, 0.0], [0.0, 0.01]], triangles: vec![[0, 1, 2]], material: vec![(1.0, 1.0)] };
/// assert!(s_parameters(&p, 1, &[], 12e9).is_err());
/// assert!(s_parameters(&p, 1, &[BorPort { z: 0.5, modes: 1 }], 12e9).is_err());
/// ```
pub fn s_parameters(p: &Profile, m: u32, ports: &[BorPort], frequency: f64) -> Result<BorSweepPoint> {
    if ports.is_empty() || ports.len() > 16 {
        return Err(invalid("ports", "1 to 16 ports on the profile's ends"));
    }
    if p.triangles.is_empty() || p.triangles.len() > MAX_TRIANGLES || p.material.len() != p.triangles.len() || m > 64 {
        return Err(invalid("profile", format!("must have 1 to {MAX_TRIANGLES} triangles, each with a material, and m at most 64")));
    }
    if !p.points.iter().all(|q| q[0].is_finite() && q[1].is_finite() && q[0] >= 0.0)
        || p.triangles.iter().flatten().any(|&n| n as usize >= p.points.len())
    {
        return Err(invalid("profile", "every point must be finite with r ≥ 0, every corner a point"));
    }
    crate::driven::check_frequency(frequency)?;
    let scale = p.points.iter().map(|q| q[0].abs().max(q[1].abs())).fold(0.0, f64::max).max(1e-300);
    let tol = 1e-9 * scale;
    let counts = boundary_edges(p);
    // Each port's edges (boundary edges on its plane), its radius and its medium.
    let mut port_edges: Vec<Vec<[u32; 2]>> = Vec::new();
    for pt in ports {
        if !(pt.z.is_finite() && (1..=MAX_PORT_MODES).contains(&pt.modes)) {
            return Err(invalid("ports", format!("a port needs a finite plane and 1 to {MAX_PORT_MODES} modes")));
        }
        let on = |n: u32| p.points.get(n as usize).is_some_and(|q| (q[1] - pt.z).abs() <= tol);
        let edges: Vec<[u32; 2]> = counts.iter().filter(|(e, c)| **c == 1 && on(e[0]) && on(e[1])).map(|(e, _)| *e).collect();
        if edges.is_empty() {
            return Err(invalid("ports", format!("no boundary edge of the profile lies on z = {} m", pt.z)));
        }
        port_edges.push(edges);
    }
    let open: std::collections::BTreeSet<[u32; 2]> = port_edges.iter().flatten().copied().collect();
    let d = numbering_open(p, m, &open);
    // Each port's vectors b per mode, its support, norms and κ.
    let k0 = 2.0 * PI * frequency / C0;
    struct Term {
        b: Vec<(usize, f64)>,
        norm: f64,
        kappa: C,
        name: String,
    }
    let mut terms: Vec<Vec<Term>> = Vec::new();
    for (pt, edges) in ports.iter().zip(&port_edges) {
        let r_of = |n: u32| p.points.get(n as usize).map_or(0.0, |q| q[0]);
        let a = edges.iter().flatten().map(|&n| r_of(n)).fold(0.0, f64::max);
        if !(a > tol) || edges.iter().flatten().map(|&n| r_of(n)).fold(f64::INFINITY, f64::min) > tol {
            return Err(invalid("ports", format!("the port on z = {} m must reach from the axis out", pt.z)));
        }
        // The medium of a triangle on the port.
        let tri = p.triangles.iter().position(|t| edges.iter().any(|e| t.contains(&e[0]) && t.contains(&e[1]))).unwrap_or(0);
        let (eps, mu) = p.material.get(tri).copied().unwrap_or((1.0, 1.0));
        let k = k0 * (eps * mu).sqrt();
        let mut list = Vec::new();
        for md in port_modes(a, m, pt.modes) {
            let mut b: BTreeMap<usize, f64> = BTreeMap::new();
            for e in edges {
                let (ra, rb) = (r_of(e[0]), r_of(e[1]));
                let len = (rb - ra).abs();
                for &(t, w) in &GAUSS {
                    let r = ra + t * (rb - ra);
                    let [er, ephi] = md.field(r);
                    // The Whitney function along its edge, from e[0] (the lower) to e[1]: 1/len
                    // along the edge, whose r runs one way or the other.
                    if let Some(&u) = d.edge.get(e) {
                        let along = if rb > ra { 1.0 } else { -1.0 } / len;
                        *b.entry(u).or_default() += w * len * along * er * r;
                    }
                    for (n, lam) in [(e[0], 1.0 - t), (e[1], t)] {
                        if let Some(Some(u)) = d.node.get(n as usize) {
                            *b.entry(*u).or_default() += w * len * lam * ephi;
                        }
                    }
                }
            }
            // ‖ẽ‖² over [0, a], finely.
            let samples = 400;
            let norm: f64 = (0..samples)
                .map(|i| {
                    let (lo, hi) = (a * i as f64 / samples as f64, a * (i + 1) as f64 / samples as f64);
                    GAUSS
                        .iter()
                        .map(|&(t, w)| {
                            let r = lo + t * (hi - lo);
                            let [x, y] = md.field(r);
                            w * (hi - lo) * (x * x + y * y) * r
                        })
                        .sum::<f64>()
                })
                .sum();
            let kappa = md.kappa(k).ok_or_else(|| invalid("frequency", format!("{frequency} Hz is at the cut-off of {}", md.name())))? / mu;
            list.push(Term { b: b.into_iter().collect(), norm, kappa, name: md.name() });
        }
        terms.push(list);
    }
    let support: Vec<Vec<usize>> = terms.iter().map(|l| l.iter().flat_map(|t| t.b.iter().map(|x| x.0)).collect()).collect();
    let (k, mm, _) = assemble_with(p, m, &d, &support)?;
    let mut a = k.clone();
    for (v, w) in a.val.iter_mut().zip(&mm.val) {
        *v -= w * (k0 * k0);
    }
    for t in terms.iter().flatten() {
        let f = t.kappa / t.norm;
        for &(i, bi) in &t.b {
            for &(j, bj) in &t.b {
                a.add(i, j, f * bi * bj);
            }
        }
    }
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
    let factor = Ldlt::factor(&a, &xyz)?;
    let n_ports = ports.len();
    let mut s = crate::SMatrix::zeros(n_ports);
    let first: Vec<&Term> = terms.iter().map(|l| &l[0]).collect();
    let proxy = |t: &Term| t.norm * t.kappa.im;
    for (q, tq) in first.iter().enumerate() {
        let mut x = vec![C::new(0.0, 0.0); d.count];
        for &(i, bi) in &tq.b {
            x[i] = 2.0 * tq.kappa * bi;
        }
        factor.solve(&mut x)?;
        for (pp, tp) in first.iter().enumerate() {
            let c: C = tp.b.iter().map(|&(i, bi)| bi * x[i]).sum::<C>() / tp.norm;
            let direct = if pp == q { c - 1.0 } else { c };
            let (wp, wq) = (proxy(tp), proxy(tq));
            s[(pp, q)] = if wp > 0.0 && wq > 0.0 { direct * (wp / wq).sqrt() } else { direct };
        }
    }
    Ok(BorSweepPoint { frequency, s, modes: first.iter().map(|t| t.name.clone()).collect(), unknowns: d.count })
}

#[cfg(test)]
mod port_tests {
    use super::*;

    /// A straight circular guide, radius 10 mm and 30 mm long, as its meridian rectangle in
    /// triangles of about `size`.
    fn guide(size: f64) -> Profile {
        let ring = [[0.0, 0.0], [0.01, 0.0], [0.01, 0.03], [0.0, 0.03]];
        let (points, triangles) = emcraft_mesh::planar::polygon(&ring, size).unwrap();
        let material = vec![(1.0, 1.0); triangles.len()];
        Profile { points, triangles, material }
    }

    /// The guide in order `m` at `f` Hz passes its first mode (Bessel root `x`) as e^{−jβL}.
    fn passes(m: u32, f: f64, x: f64, name: &str, tol: f64) {
        passes_at(m, f, x, name, tol, 0.5e-3);
    }

    fn passes_at(m: u32, f: f64, x: f64, name: &str, tol: f64, size: f64) {
        let ports = [BorPort { z: 0.0, modes: 2 }, BorPort { z: 0.03, modes: 2 }];
        let r = s_parameters(&guide(size), m, &ports, f).unwrap();
        assert_eq!(r.modes, [name, name]);
        let k = 2.0 * PI * f / C0;
        let want = C::from_polar(1.0, -(k * k - (x / 0.01).powi(2)).sqrt() * 0.03);
        let (s11, s21) = (r.s[(0, 0)], r.s[(1, 0)]);
        assert!(s11.norm() < tol && (s21 - want).norm() < tol, "m {m}: S11 {s11}, S21 {s21}, exact {want}");
    }

    #[test]
    fn a_circular_guide_passes_its_modes_on_the_half_plane() {
        // Lowest-order elements at 0.5 mm: TM01 1.2e-3 off e^{−jβL}, TE11 6.9e-3 (its 1/r
        // terms near the axis), the reflections below 3e-4.
        passes(0, 14e9, 2.404_825_557_695_773, "TM01", 2e-3);
        passes(1, 12e9, 1.841_183_781_340_659, "TE11", 1e-2);
        passes(2, 17e9, 3.054_236_928_227_14, "TE21", 2e-2);
    }

    #[test]
    #[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
    fn the_half_plane_s_converge_as_the_size_squared() {
        // TE11: 2.6e-2, 6.9e-3, 1.9e-3 at 1, 0.5, 0.25 mm; TM01: 5.1e-3, 1.2e-3, 3.0e-4.
        passes_at(1, 12e9, 1.841_183_781_340_659, "TE11", 2.5e-3, 0.25e-3);
        passes_at(0, 14e9, 2.404_825_557_695_773, "TM01", 4e-4, 0.25e-3);
    }
}
