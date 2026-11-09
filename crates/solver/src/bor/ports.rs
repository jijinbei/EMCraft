//! A body of revolution's ports: S-parameters between circular-guide modes on its ends.

use super::*;

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
    Ok(solved(p, m, ports, frequency, None, None)?.0)
}

/// The generalized scattering matrix between every mode of every port (row-major, the modes by
/// port in order), each mode's amplitude its own (unnormalized), with the first modes' point.
pub fn generalized(p: &Profile, m: u32, ports: &[BorPort], frequency: f64) -> Result<(BorSweepPoint, Vec<C>)> {
    let mut gsm = Vec::new();
    let point = solved(p, m, ports, frequency, None, Some(&mut gsm))?.0;
    Ok((point, gsm))
}

/// [`s_parameters`] with the unknowns' numbering and each excitation's solution (port q's first
/// mode, a unit wave in), the profile lined by `layer` when it is open.
pub(super) fn solved(
    p: &Profile,
    m: u32,
    ports: &[BorPort],
    frequency: f64,
    layer: Option<&super::open::Layer>,
    gsm: Option<&mut Vec<C>>,
) -> Result<(BorSweepPoint, Dofs, Vec<Vec<C>>)> {
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
    let (k, mm, _) = assemble_with(p, m, &d, &support, layer.map(|l| (l, k0)))?;
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
    if let Some(out) = gsm {
        // Every mode of every port excited in turn (a unit wave in), each read on every mode:
        // the generalized scattering matrix, its amplitudes as the modes' own (no power
        // normalization), modes by port in order.
        let all: Vec<&Term> = terms.iter().flatten().collect();
        let n = all.len();
        out.clear();
        out.resize(n * n, C::new(0.0, 0.0));
        for (q, tq) in all.iter().enumerate() {
            let mut x = vec![C::new(0.0, 0.0); d.count];
            for &(i, bi) in &tq.b {
                x[i] = 2.0 * tq.kappa * bi;
            }
            factor.solve(&mut x)?;
            for (pp, tp) in all.iter().enumerate() {
                let c: C = tp.b.iter().map(|&(i, bi)| bi * x[i]).sum::<C>() / tp.norm;
                out[pp * n + q] = if pp == q { c - 1.0 } else { c };
            }
        }
    }
    let n_ports = ports.len();
    let mut s = crate::SMatrix::zeros(n_ports);
    let first: Vec<&Term> = terms.iter().map(|l| &l[0]).collect();
    let proxy = |t: &Term| t.norm * t.kappa.im;
    let mut solutions = Vec::with_capacity(n_ports);
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
        solutions.push(x);
    }
    let point = BorSweepPoint { frequency, s, modes: first.iter().map(|t| t.name.clone()).collect(), unknowns: d.count };
    Ok((point, d, solutions))
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
        // Circular guide: e^{−jβL}, β = √(k² − (x/r)²), x = Bessel zero (Marcuvitz, Waveguide
        // Handbook, §2.3; A&S Table 9.5).
        // Lowest-order elements at 0.5 mm: TM01 1.2e-3 off e^{−jβL}, TE11 6.9e-3 (its 1/r
        // terms near the axis), the reflections below 3e-4.
        passes(0, 14e9, 2.404_825_557_695_773, "TM01", 2e-3);
        passes(1, 12e9, 1.841_183_781_340_659, "TE11", 1e-2);
        passes(2, 17e9, 3.054_236_928_227_14, "TE21", 2e-2);
    }

    #[test]
    fn a_travelling_tm01_has_no_azimuthal_e_and_a_steady_magnitude() {
        let ports = [BorPort { z: 0.0, modes: 2 }, BorPort { z: 0.03, modes: 2 }];
        let f = field(&guide(0.5e-3), 0, &ports, 14e9, 0, [21, 61]).unwrap();
        let at = |i: usize, j: usize| f.e[j * f.nx + i].unwrap();
        // Lowest-order edge functions vary within a triangle (about 10 % point to point): the
        // means over the guide's two halves agree.
        let mean = |js: std::ops::Range<usize>| js.clone().map(|j| at(10, j)[0].norm()).sum::<f64>() / js.len() as f64;
        let (lo, hi) = (mean(5..30), mean(31..56));
        assert!(hi > 0.0 && (hi - lo).abs() / hi < 0.03, "|E_r| at r = 5 mm: {lo} and {hi} over the two halves");
        let ephi = f.e.iter().flatten().map(|e| e[2].norm()).fold(0.0f64, f64::max);
        assert!(ephi < 1e-6 * hi, "E_φ {ephi}");
    }

    #[test]
    #[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
    fn the_half_plane_s_converge_as_the_size_squared() {
        // Circular guide: e^{−jβL}, β = √(k² − (x/r)²), x = Bessel zero (Marcuvitz, Waveguide
        // Handbook, §2.3; A&S Table 9.5).
        // TE11: 2.6e-2, 6.9e-3, 1.9e-3 at 1, 0.5, 0.25 mm; TM01: 5.1e-3, 1.2e-3, 3.0e-4.
        passes_at(1, 12e9, 1.841_183_781_340_659, "TE11", 2.5e-3, 0.25e-3);
        passes_at(0, 14e9, 2.404_825_557_695_773, "TM01", 4e-4, 0.25e-3);
    }
}
