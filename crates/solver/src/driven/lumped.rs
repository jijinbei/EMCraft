//! Lumped ports: a gap between two points (or a band round it) carrying a source and an
//! impedance; its vector cᵢ = ∫ Nᵢ · ŝ dl couples the field to the gap's voltage.

use emcraft_model::units::angular;
use std::f64::consts::PI;

use emcraft_model::units::MU0;
use emcraft_model::{LumpedPort, Point3, Vec3};
use num_complex::Complex64 as C;

use super::assembly::Space;
use super::{Result, invalid};

/// A gap's line is inside a tetrahedron where every barycentric coordinate is above −this, and
/// two cuts along the line (fractions of its length) closer than this are one.
const BARYCENTRIC: f64 = 1e-12;

/// A barycentric coordinate changing by less than this along the line is constant on it (the
/// line runs parallel to that face).
const PARALLEL: f64 = 1e-15;

/// A lumped port prepared for solving.
#[derive(Clone, Debug)]
pub struct LumpedData {
    pub name: String,
    pub impedance: f64,
    pub voltage: C,
    /// (unknown, ∫ Nᵢ · ŝ dl) along the gap from `from` to `to`.
    pub c: Vec<(usize, f64)>,
}

impl LumpedData {
    /// The factor jωμ₀/R of the port's term (jωμ₀/R) c cᵀ at `frequency`: the current
    /// I = (V_s + cᵀx)/R through its gap.
    pub(crate) fn factor(&self, frequency: f64) -> C {
        let omega = angular(frequency);
        C::new(0.0, omega * MU0 / self.impedance)
    }
}

impl super::Driven {
    /// Without waveguide ports, one right-hand side (of `len` entries) per lumped port: its
    /// source V_s = 2√R driving −jωμ₀ (V_s/R) c, a power wave a = V_s/(2√R) = 1 (1 W
    /// available) into it, the other ports' sources off (their impedances still load them).
    pub(crate) fn lumped_excitations(&self, frequency: f64, len: usize) -> Vec<Vec<C>> {
        self.lumped
            .iter()
            .map(|l| {
                let mut b = vec![C::new(0.0, 0.0); len];
                let f = C::new(0.0, -angular(frequency) * MU0) * (2.0 / l.impedance.sqrt());
                for &(i, ci) in &l.c {
                    if let Some(x) = b.get_mut(i) {
                        *x += f * ci;
                    }
                }
                b
            })
            .collect()
    }

    /// S of the lumped ports from the field of each one's excitation
    /// ([`super::Driven::lumped_excitations`]): with the gap's line integral cᵀx, its current
    /// I = (V_s + cᵀx)/R and voltage V = −cᵀx, the wave out of port p is
    /// b = (V − R I)/(2√R) = −(V_s + 2cᵀx)/(2√R) (K. Kurokawa, *Power waves and the scattering
    /// matrix*, IEEE Trans. MTT 13(2):194–202, 1965), and S_pq = b_p for a unit wave into q.
    pub(crate) fn lumped_s(&self, fields: &[Vec<C>]) -> crate::smatrix::SMatrix {
        let n = self.lumped.len();
        crate::smatrix::SMatrix::from_fn(n, |p, q| {
            let (Some(l), Some(x)) = (self.lumped.get(p), fields.get(q)) else { return C::new(0.0, 0.0) };
            let line: C = l.c.iter().map(|&(i, ci)| x.get(i).copied().unwrap_or_default() * ci).sum();
            let source = if p == q { 2.0 * l.impedance.sqrt() } else { 0.0 };
            -(source + 2.0 * line) / (2.0 * l.impedance.sqrt())
        })
    }
}

/// A lumped element prepared for solving: its gap's line integrals and the element.
#[derive(Clone, Debug)]
pub struct ElementData {
    pub element: emcraft_model::LumpedElement,
    pub c: Vec<(usize, f64)>,
}

impl ElementData {
    /// The factor jωμ₀Y(ω) of the element's term jωμ₀Y c cᵀ: the current I = Y V through its gap
    /// for the gap's voltage V = −cᵀx, as a lumped port's with no source and Y = 1/R.
    pub(crate) fn factor(&self, frequency: f64) -> C {
        let omega = angular(frequency);
        C::new(0.0, omega * MU0) * self.element.admittance(frequency)
    }
}

/// The model's lumped element `e`: its gap's line integrals, along the line or across its sheet.
pub(super) fn prepare_element(e: &emcraft_model::LumpedElement, space: &Space) -> Result<ElementData> {
    let c = match e.across {
        Some(across) => sheet_vector(space, e.from, e.to, across)?,
        None => line_vector(space, e.from, e.to)?,
    };
    if c.is_empty() {
        return Err(invalid(format!("lumped element `{}`", e.name), "its gap lies outside the mesh (inside a conductor?)"));
    }
    Ok(ElementData { element: e.clone(), c })
}

/// The model's lumped port `lp`: its gap's line integrals.
pub(super) fn prepare(lp: &LumpedPort, space: &Space) -> Result<LumpedData> {
    let c = match lp.across {
        Some(across) => sheet_vector(space, lp.from, lp.to, across)?,
        None => band_vector(space, lp.from, lp.to, lp.radius)?,
    };
    if c.is_empty() {
        return Err(invalid(format!("lumped port `{}`", lp.name), "its gap lies outside the mesh (inside a conductor?)"));
    }
    Ok(LumpedData { name: lp.name.clone(), impedance: lp.impedance.get(), voltage: C::new(lp.voltage[0], lp.voltage[1]), c })
}

/// The port's vector: the line integral along the gap, or for a band of radius r its average
/// over 16 lines evenly round the band (the trapezoid rule, spectrally accurate round a
/// circle): the voltage is the band's mean line integral and the current spreads evenly round
/// it.
fn band_vector(space: &Space, a: Point3, b: Point3, r: f64) -> Result<Vec<(usize, f64)>> {
    if r == 0.0 {
        return line_vector(space, a, b);
    }
    let s = (b - a).normalized().ok_or_else(|| invalid("lumped port", "its two ends coincide"))?;
    // Two unit vectors across the line.
    let helper = Vec3::unit(if s.x.abs() < 0.9 { 0 } else { 1 });
    let u = s.cross(helper).normalized().unwrap_or_else(|| Vec3::unit(0));
    let v = s.cross(u);
    let n = 16;
    let mut out: std::collections::BTreeMap<usize, f64> = std::collections::BTreeMap::new();
    for k in 0..n {
        let phi = 2.0 * PI * k as f64 / n as f64;
        let off = (u * phi.cos() + v * phi.sin()) * r;
        for (i, c) in line_vector(space, a + off, b + off)? {
            *out.entry(i).or_default() += c / n as f64;
        }
    }
    Ok(out.into_iter().collect())
}

/// The lines across a sheet port: its voltage the mean of their line integrals.
const SHEET_LINES: usize = 8;

/// A sheet's vector: the mean of the line integrals along [`SHEET_LINES`] lines parallel to the
/// gap, at the midpoints of equal stretches from `−across` to `+across` about it (the midpoint
/// rule): the voltage is the sheet's mean and the current spreads evenly across it.
fn sheet_vector(space: &Space, a: Point3, b: Point3, across: Vec3) -> Result<Vec<(usize, f64)>> {
    let n = SHEET_LINES;
    let mut out: std::collections::BTreeMap<usize, f64> = std::collections::BTreeMap::new();
    for k in 0..n {
        let off = across * ((2 * k + 1) as f64 / n as f64 - 1.0);
        for (i, c) in line_vector(space, a + off, b + off)? {
            *out.entry(i).or_default() += c / n as f64;
        }
    }
    Ok(out.into_iter().collect())
}

/// The stretch `from`..`to` (fractions of the segment) of a segment inside tetrahedron `tet`.
#[derive(Clone, Copy)]
struct Piece {
    from: f64,
    to: f64,
    tet: usize,
}

/// How far past either end a gap's line may run on to the conductor there, a share of its length.
const REACH: f64 = 0.5;

/// The gap's line from `a` to `b` run on past either end, through the air, to the conductor
/// that end faces when one lies within [`REACH`] of its length. A line ending short of its
/// terminal — a band's line off a wire's axis where the wire's end is not square to the gap (a
/// bent wire's), or a point set a little off the metal — leaves its current's charge in the
/// air: a capacitance in series with the gap that grows as the mesh resolves it (a wire loop's
/// second-order reactance ran 65 Ω capacitive with a band's lines ending 1–10 µm short of the
/// wire's slanted ends). An end with no conductor within reach, or already on one, stays.
fn onto_conductors(space: &Space, a: Point3, b: Point3) -> (Point3, Point3) {
    let d = b - a;
    let (from, to) = (a - d * REACH, b + d * REACH);
    // The stretches of the longer line in the mesh, joined where they touch.
    let mut spans: Vec<(f64, f64)> = pieces(space, from, to).iter().map(|p| (p.from, p.to)).collect();
    spans.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut joined: Vec<(f64, f64)> = Vec::new();
    for (lo, hi) in spans {
        match joined.last_mut() {
            Some(last) if lo <= last.1 + BARYCENTRIC => last.1 = last.1.max(hi),
            _ => joined.push((lo, hi)),
        }
    }
    // The gap's own ends and its middle, as fractions of the longer line.
    let whole = 1.0 + 2.0 * REACH;
    let (ta, tb) = (REACH / whole, (1.0 + REACH) / whole);
    let Some(&(lo, hi)) = joined.iter().find(|s| s.0 <= 0.5 && 0.5 <= s.1) else { return (a, b) };
    let at = |t: f64| from + (to - from) * t;
    let start = if lo > BARYCENTRIC && lo < ta { at(lo) } else { a };
    let end = if hi < 1.0 - BARYCENTRIC && hi > tb { at(hi) } else { b };
    (start, end)
}

/// The stretches of the segment from `a` to `b` inside each tetrahedron it crosses (barycentric
/// coordinates are linear along it), as fractions of the segment.
fn pieces(space: &Space, a: Point3, b: Point3) -> Vec<Piece> {
    let mesh = space.mesh;
    let mut pieces: Vec<Piece> = Vec::new();
    // Only the tetrahedra whose boxes meet the segment's.
    for t in emcraft_mesh::tets_near(mesh, emcraft_model::Aabb::new(a.min(b), a.max(b))) {
        let Some(c) = mesh.corners(t) else { continue };
        // Barycentric coordinates of a and b in the straight tetrahedron.
        let e = [1, 2, 3].map(|i| c[i] - c[0]);
        let det = emcraft_model::geom::triple(e[0], e[1], e[2]);
        if det == 0.0 {
            continue;
        }
        let g = [e[1].cross(e[2]), e[2].cross(e[0]), e[0].cross(e[1])];
        let bary = |p: Point3| {
            let l = g.map(|gi| gi.dot(p - c[0]) / det);
            [1.0 - l[0] - l[1] - l[2], l[0], l[1], l[2]]
        };
        let (la, lb) = (bary(a), bary(b));
        let (mut t0, mut t1) = (0.0f64, 1.0f64);
        let tol = BARYCENTRIC;
        for i in 0..4 {
            let (u, v) = (la[i], lb[i]);
            // u + t (v − u) ≥ −tol.
            if (v - u).abs() < PARALLEL {
                if u < -tol {
                    t1 = -1.0;
                }
            } else {
                let t = (-tol - u) / (v - u);
                if v > u { t0 = t0.max(t) } else { t1 = t1.min(t) }
            }
        }
        if t1 - t0 > BARYCENTRIC {
            pieces.push(Piece { from: t0.max(0.0), to: t1.min(1.0), tet: t });
        }
    }
    pieces
}

/// ∫ Nᵢ · ŝ dl along the gap's line from `a` to `b` (ŝ its direction), run on to the conductors
/// at its ends ([`onto_conductors`]), for every unknown: each stretch between the breakpoints
/// of its [`pieces`] is integrated once, in one of the tetrahedra holding it (the tangential
/// component along the line is the same in all of them), by four-point Gauss rules.
fn line_vector(space: &Space, a: Point3, b: Point3) -> Result<Vec<(usize, f64)>> {
    let (a, b) = onto_conductors(space, a, b);
    let d = b - a;
    let len = d.norm();
    let s = d / len;
    let pieces = pieces(space, a, b);
    let mut cuts: Vec<f64> = pieces.iter().flat_map(|p| [p.from, p.to]).collect();
    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|x, y| (*x - *y).abs() < BARYCENTRIC);
    let (x, w) = emcraft_fem::quadrature::gauss_legendre(4);
    let mut out: std::collections::BTreeMap<usize, f64> = std::collections::BTreeMap::new();
    for win in cuts.windows(2) {
        let (t0, t1) = (win[0], win[1]);
        let mid = 0.5 * (t0 + t1);
        let Some(&Piece { tet, .. }) = pieces.iter().find(|p| p.from <= mid && mid <= p.to) else { continue };
        let el = space.element(tet)?;
        let unknowns = space.unknowns(tet);
        for (xi, wi) in x.iter().zip(&w) {
            let t = t0 + xi * (t1 - t0);
            for (u, v) in unknowns.iter().zip(el.values(el.barycentric(a + d * t))) {
                if let Some(u) = u.get() {
                    *out.entry(u).or_default() += wi * (t1 - t0) * len * v.dot(s);
                }
            }
        }
    }
    Ok(out.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driven::Dofs;
    use emcraft_fem::Order;
    use emcraft_mesh::Topology;
    use emcraft_model::Mesh;

    /// The unit cube of air in six tetrahedra (Kuhn's), the metal all round it.
    fn cube() -> Mesh {
        let nodes = (0..8).map(|i| Point3::new((i & 1) as f64, ((i >> 1) & 1) as f64, ((i >> 2) & 1) as f64)).collect();
        let paths = [[1, 2, 4], [1, 4, 2], [2, 1, 4], [2, 4, 1], [4, 1, 2], [4, 2, 1]];
        let tets = paths.iter().map(|p| [0, p[0], p[0] + p[1], 7]).collect();
        Mesh { nodes, tets, tet_material: vec![0; 6], ..Mesh::default() }
    }

    #[test]
    fn a_gap_line_ending_short_of_the_metal_runs_on_to_it() {
        let mesh = cube();
        let topo = Topology::new(&mesh);
        let dofs = Dofs::new(Order::Two, &mesh, &topo, &[]).unwrap();
        let space = Space { mesh: &mesh, topo: &topo, dofs: &dofs };
        let p = |z: f64| Point3::new(0.3, 0.4, z);
        // 0.1 short of the floor and ceiling: run on to them, the same line as one drawn there.
        let (a, b) = onto_conductors(&space, p(0.1), p(0.9));
        assert!(a.distance(p(0.0)) < 1e-9 && b.distance(p(1.0)) < 1e-9, "{a:?} {b:?}");
        let (short, whole) = (line_vector(&space, p(0.1), p(0.9)).unwrap(), line_vector(&space, p(0.0), p(1.0)).unwrap());
        assert_eq!(short.len(), whole.len());
        for ((i, x), (j, y)) in short.iter().zip(&whole) {
            assert_eq!(i, j);
            approx::assert_abs_diff_eq!(x, y, epsilon = 1e-12);
        }
        // Metal beyond reach (half the line's length), or none short of the line's ends: stays.
        assert_eq!(onto_conductors(&space, p(0.45), p(0.55)), (p(0.45), p(0.55)));
        assert_eq!(onto_conductors(&space, p(-0.2), p(1.2)), (p(-0.2), p(1.2)));
    }
}
