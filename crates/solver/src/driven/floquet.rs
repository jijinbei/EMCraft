//! Floquet ports and the driven solve of a periodic cell.
//!
//! **Floquet modes.** Above and below a cell periodic in x (period a) and y (period b), any field
//! is a sum of plane waves, the Floquet harmonics (m, n): transverse wavevector
//! k_t,mn = (k_x + 2πm/a, k_y + 2πn/b), propagation constant β_mn = √(k² − |k_t,mn|²) (−jα below
//! cut-off), each in two polarizations relative to its plane of incidence, TE (E_t along
//! ẑ × k̂_t) and TM (E_t along k̂_t); at k_t = 0 the plane of incidence is the incident wave's
//! (azimuth φ). Their transverse fields e_mn = ê e^{−j k_t,mn · (r − r₀)} are orthogonal under
//! ∫ e · ē dS over the cell (A. K. Bhattacharyya, *Phased Array Antennas: Floquet Analysis,
//! Synthesis, BFNs, and Active Array Systems*, Wiley 2006; Jin 2014, periodic structures).
//!
//! **The port condition** is the waveguide ports' ([`super`]), with the conjugate product for
//! the modes' coefficients: n̂ × ∇ × E_t/μr = Σ (κ/μr) c_mn e_mn − 2(κ/μr) a e_inc, with
//! c_mn = ∫ ē_mn · E dS/‖e‖², κ_TE = jβ and κ_TM = jk²/β. In the weak form it adds the rank-one
//! terms (κ/μr‖e‖²) v qᵀ with v_i = ∫ Nᵢ · e dS and q = v̄: not symmetric, which the periodic
//! system is not anyway ([`super::periodic`]). They border the reduced matrix as the waveguide
//! ports' do, each with its own row and column.
//!
//! **Incidence.** The excited mode is the (0, 0) harmonic of the polarization asked for, with
//! k_t = k₁ sin θ (cos φ, sin φ), k₁ the wavenumber in the first port's medium; both ports
//! share this k_t (one Floquet phase for the cell), so port 2's incident wave arrives from above
//! with the same transverse wavevector. S_pq = c at port p for a unit wave from q, less the
//! incident wave, normalised to the modes' powers (|c|² ‖e‖² Re(1/Z)/2): co-polarised.

use std::f64::consts::PI;

use emcraft_model::units::C0;
use emcraft_model::{Model, Periodic, Polarization, Port, Vec3};
use num_complex::Complex64 as C;

use super::assembly::{FaceRule, Space};
use super::ports::{port_face_rules, tangential_trace};
use super::{Driven, FrequencyResult, NONE, Result, Timings, check_frequency, invalid, wavenumber};
use crate::smatrix::SMatrix;
use crate::sparse::Csr;

/// Harmonic orders searched for a port's modes: |m|, |n| up to this.
const MAX_ORDER: i32 = 6;

/// A Floquet port: the faces of one end of the cell, its medium and its modes' count.
#[derive(Clone, Debug)]
pub struct FloquetPort {
    pub eps_r: f64,
    pub mu_r: f64,
    pub(super) faces: Vec<usize>,
    /// The cell's corner (x0, y0) and periods (a, b).
    pub cell: [f64; 4],
    pub count: usize,
    pub polarization: Polarization,
    pub phi: f64,
}

/// One Floquet mode at a frequency.
#[derive(Clone, Debug, PartialEq)]
pub struct FloquetMode {
    pub m: i32,
    pub n: i32,
    pub polarization: Polarization,
    pub kx: f64,
    pub ky: f64,
    /// k² − |k_t|² (rad²/m²): > 0 propagating.
    pub beta2: f64,
}

impl FloquetMode {
    pub fn name(&self) -> String {
        let p = match self.polarization {
            Polarization::Te => "TE",
            Polarization::Tm => "TM",
        };
        format!("{p}({},{})", self.m, self.n)
    }

    /// The unit vector of E_t.
    fn direction(&self, phi: f64) -> Vec3 {
        let kt = self.kx.hypot(self.ky);
        let k = (self.beta2 + kt * kt).abs().sqrt();
        let (ux, uy) = if kt > 1e-9 * k.max(f64::MIN_POSITIVE) { (self.kx / kt, self.ky / kt) } else { (phi.cos(), phi.sin()) };
        match self.polarization {
            Polarization::Te => Vec3::new(-uy, ux, 0.0),
            Polarization::Tm => Vec3::new(ux, uy, 0.0),
        }
    }

    /// γ = jβ: (α, β), β > 0 propagating, α > 0 decaying.
    fn gamma(&self) -> C {
        if self.beta2 >= 0.0 { C::new(0.0, self.beta2.sqrt()) } else { C::new((-self.beta2).sqrt(), 0.0) }
    }

    /// κ with n̂ × ∇ × E_t = κ E_t for the mode leaving the port: γ (TE), −k²/γ (TM).
    fn kappa(&self, k: f64) -> Option<C> {
        let g = self.gamma();
        if !(g.norm() > 1e-10 * k) {
            return None;
        }
        Some(match self.polarization {
            Polarization::Te => g,
            Polarization::Tm => -C::new(k * k, 0.0) / g,
        })
    }
}

/// A Floquet mode's term at a frequency, on the full unknowns.
#[derive(Clone, Debug)]
pub struct FloquetTerm {
    pub name: String,
    /// v_i = ∫ Nᵢ · e dS (the coefficient reads q = v̄).
    pub v: Vec<(usize, C)>,
    pub factor: C,
    pub rhs: C,
    pub norm: f64,
    /// ‖e‖² Re(κ/μr)·(propagating): 2ωμ₀ times the power of a unit wave.
    pub proxy: f64,
    pub propagating: bool,
}

impl FloquetTerm {
    /// The mode's coefficient c = q·x/‖e‖² in the full field `x`.
    pub fn coefficient(&self, x: &[C]) -> C {
        self.v.iter().map(|&(i, vi)| vi.conj() * x.get(i).copied().unwrap_or_default()).sum::<C>() / self.norm
    }
}

/// Port `p` of a model periodic in x and y as a Floquet port on its end `faces`.
pub(super) fn prepare(model: &Model, periodic: &Periodic, p: &Port, faces: &[usize]) -> Result<FloquetPort> {
    let what = || format!("port `{}`", p.name);
    if !(periodic.x && periodic.y) {
        return Err(invalid(what(), "not supported yet: ports on a cell periodic along one axis only (Floquet ports need x and y periodic)"));
    }
    if p.deembed != 0.0 || p.reference.is_some() {
        return Err(invalid(what(), "not supported yet: de-embedding or a reference impedance on a Floquet port"));
    }
    if p.face.is_some() {
        return Err(invalid(what(), "a Floquet port lies on an end of the cell, not on a face inside it"));
    }
    let w = model.waveguide.as_ref().ok_or_else(|| invalid("model", "has no waveguide"))?;
    if !w.layers.is_empty() {
        return Err(invalid(what(), "not supported yet: a Floquet port on a layered cell (its face must be one material)"));
    }
    let full = |o: Option<f64>| o.is_none();
    let sec = match p.end {
        emcraft_model::End::Start => w.sections.first(),
        emcraft_model::End::End => w.sections.last(),
    }
    .ok_or_else(|| invalid("waveguide", "has no sections"))?;
    if !(full(sec.width) || sec.width.is_some_and(|x| (x - w.width).abs() <= 1e-9 * w.width))
        || !(full(sec.height) || sec.height.is_some_and(|y| (y - w.height).abs() <= 1e-9 * w.height))
    {
        return Err(invalid(what(), "a Floquet port needs the whole cell's cross-section at its end"));
    }
    let m = model.port_material(p.end)?;
    if m.loss_tangent != 0.0 || m.is_dispersive() {
        return Err(invalid(what(), "not supported yet: a Floquet port in a lossy or dispersive medium"));
    }
    Ok(FloquetPort {
        eps_r: m.eps_r,
        mu_r: m.mu_r,
        faces: faces.to_vec(),
        cell: [0.0, 0.0, w.width, w.height],
        count: p.modes as usize,
        polarization: periodic.polarization,
        phi: periodic.phi,
    })
}

impl FloquetPort {
    /// The port's `count` modes at wavenumber `k` (its medium's) for the incident transverse
    /// wavevector (kx, ky): the (0, 0) harmonic of the excited polarization first, then the
    /// other's, then the rest by their cut-off |k_t,mn| (TE before TM, then m, then n).
    pub fn modes(&self, k: f64, kx: f64, ky: f64) -> Vec<FloquetMode> {
        let [_, _, a, b] = self.cell;
        let first = self.polarization;
        let other = match first {
            Polarization::Te => Polarization::Tm,
            Polarization::Tm => Polarization::Te,
        };
        let mut list = Vec::new();
        for m in -MAX_ORDER..=MAX_ORDER {
            for n in -MAX_ORDER..=MAX_ORDER {
                let (kxm, kyn) = (kx + 2.0 * PI * m as f64 / a, ky + 2.0 * PI * n as f64 / b);
                let kt2 = kxm * kxm + kyn * kyn;
                for pol in [Polarization::Te, Polarization::Tm] {
                    list.push(FloquetMode { m, n, polarization: pol, kx: kxm, ky: kyn, beta2: k * k - kt2 });
                }
            }
        }
        let rank = |md: &FloquetMode| -> (u8, f64, u8, i32, i32) {
            let zero = md.m == 0 && md.n == 0;
            let lead = if zero && md.polarization == first {
                0
            } else if zero && md.polarization == other {
                1
            } else {
                2
            };
            (lead, -md.beta2, (md.polarization == Polarization::Tm) as u8, md.m, md.n)
        };
        list.sort_by(|x, y| rank(x).partial_cmp(&rank(y)).unwrap_or(std::cmp::Ordering::Equal));
        list.truncate(self.count.max(1));
        list
    }

    /// The modes' terms at `frequency` for (kx, ky), on the full unknowns.
    fn terms(&self, space: &Space, frequency: f64, kx: f64, ky: f64) -> Result<Vec<FloquetTerm>> {
        let k = wavenumber(frequency) * (self.eps_r * self.mu_r).sqrt();
        let modes = self.modes(k, kx, ky);
        let [x0, y0, a, b] = self.cell;
        let norm = a * b;
        let n = space.dofs.unknowns;
        let mut acc: Vec<std::collections::BTreeMap<usize, C>> = vec![Default::default(); modes.len()];
        let dirs: Vec<Vec3> = modes.iter().map(|m| m.direction(self.phi)).collect();
        for face in port_face_rules(*space, &self.faces) {
            let FaceRule { el, unknowns, points, .. } = face?;
            for (l, wa, _) in points {
                let x = el.point(l);
                let values = el.values(l);
                let traced = tangential_trace(&values);
                for ((md, dir), map) in modes.iter().zip(&dirs).zip(acc.iter_mut()) {
                    let arg = md.kx * (x.x - x0) + md.ky * (x.y - y0);
                    let ph = C::new(arg.cos(), -arg.sin()) * wa;
                    for ((u, v), _) in unknowns.iter().zip(&values).zip(&traced).filter(|((u, _), t)| **u != NONE && **t && (**u as usize) < n) {
                        *map.entry(*u as usize).or_default() += ph * v.dot(*dir);
                    }
                }
            }
        }
        modes
            .iter()
            .zip(acc)
            .map(|(md, v)| {
                let kappa =
                    md.kappa(k).ok_or_else(|| invalid("frequency", format!("{frequency} Hz is at the cut-off of Floquet mode {}", md.name())))?
                        / self.mu_r;
                let propagating = md.beta2 > 0.0;
                Ok(FloquetTerm {
                    name: md.name(),
                    v: v.into_iter().collect(),
                    factor: kappa / norm,
                    rhs: 2.0 * kappa,
                    norm,
                    proxy: if propagating { norm * kappa.im } else { 0.0 },
                    propagating,
                })
            })
            .collect()
    }
}

/// The incident transverse wavevector (k_x, k_y) at `frequency` from the model's angles, in the
/// first port's medium.
pub(super) fn incidence(periodic: &Periodic, first: &FloquetPort, frequency: f64) -> (f64, f64) {
    let k1 = 2.0 * PI * frequency / C0 * (first.eps_r * first.mu_r).sqrt();
    let kt = k1 * periodic.theta.sin();
    (kt * periodic.phi.cos(), kt * periodic.phi.sin())
}

/// `a` bordered by one row and column per (column vector, row vector, f): column s·col, row
/// s·row, diagonal −s²/f, s = √(|f|‖a‖) (as the symmetric ports' border, see `assembly`).
fn bordered(a: &Csr, borders: &[(Vec<(usize, C)>, Vec<(usize, C)>, C)]) -> Result<Csr> {
    let (n, m) = (a.n, borders.len());
    let norm = a.max_abs();
    let scale: Vec<f64> = borders.iter().map(|(_, _, f)| (f.norm() * norm).sqrt()).collect();
    let mut extra: Vec<Vec<(usize, C)>> = vec![Vec::new(); n];
    for (k, (col, _, _)) in borders.iter().enumerate() {
        for &(i, vi) in col {
            extra.get_mut(i).ok_or_else(|| invalid("port", "a mode's vector refers to a missing unknown"))?.push((n + k, vi * scale[k]));
        }
    }
    let mut out = Csr { n: n + m, ptr: Vec::with_capacity(n + m + 1), col: Vec::new(), val: Vec::new() };
    out.ptr.push(0);
    for (i, row) in extra.iter().enumerate() {
        let (lo, hi) = (a.ptr[i], a.ptr[i + 1]);
        out.col.extend_from_slice(&a.col[lo..hi]);
        out.val.extend_from_slice(&a.val[lo..hi]);
        out.col.extend(row.iter().map(|(j, _)| *j));
        out.val.extend(row.iter().map(|(_, v)| *v));
        out.ptr.push(out.col.len());
    }
    for (k, (col, row, f)) in borders.iter().enumerate() {
        if !(f.norm() > 0.0 && scale[k].is_finite()) {
            return Err(invalid("port", "a mode with no coupling"));
        }
        // The pattern stays symmetric: the row holds every unknown the column does.
        let rowmap: std::collections::BTreeMap<usize, C> = row.iter().copied().collect();
        let mut idx: Vec<usize> = col.iter().map(|(i, _)| *i).chain(row.iter().map(|(i, _)| *i)).collect();
        idx.sort_unstable();
        idx.dedup();
        for i in idx {
            out.col.push(i);
            out.val.push(rowmap.get(&i).copied().unwrap_or_default() * scale[k]);
        }
        out.col.push(n + k);
        out.val.push(-(scale[k] * scale[k]) / f);
        out.ptr.push(out.col.len());
    }
    // Rows whose column lacked an entry of the border's row: add the structural zero.
    let mut fixed = Csr { n: out.n, ptr: vec![0], col: Vec::new(), val: Vec::new() };
    let mut need: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (k, (col, row, _)) in borders.iter().enumerate() {
        let have: std::collections::BTreeSet<usize> = col.iter().map(|(i, _)| *i).collect();
        for (i, _) in row {
            if !have.contains(i)
                && let Some(l) = need.get_mut(*i)
            {
                l.push(n + k);
            }
        }
    }
    for i in 0..out.n {
        let mut entries: Vec<(usize, C)> = (out.ptr[i]..out.ptr[i + 1]).map(|e| (out.col[e], out.val[e])).collect();
        if let Some(l) = need.get(i) {
            entries.extend(l.iter().map(|&j| (j, C::new(0.0, 0.0))));
        }
        entries.sort_by_key(|e| e.0);
        entries.dedup_by_key(|e| e.0);
        fixed.col.extend(entries.iter().map(|e| e.0));
        fixed.val.extend(entries.iter().map(|e| e.1));
        fixed.ptr.push(fixed.col.len());
    }
    Ok(fixed)
}

impl Driven {
    /// The Floquet ports' terms at `frequency` for (kx, ky).
    pub(super) fn floquet_terms(&self, frequency: f64, kx: f64, ky: f64) -> Result<Vec<Vec<FloquetTerm>>> {
        let space = self.space();
        self.ports
            .iter()
            .map(|p| match &p.modes {
                super::PortModes::Floquet(fp) => fp.terms(&space, frequency, kx, ky),
                _ => Err(invalid(format!("port `{}`", p.name), "a periodic cell's ports must be Floquet ports")),
            })
            .collect()
    }

    /// Solves the periodic cell at one frequency: Tᴴ A T bordered by the Floquet modes, by LU.
    pub(super) fn solve_periodic(&self, frequency: f64) -> Result<FrequencyResult> {
        check_frequency(frequency)?;
        let (tie, periodic) = match (&self.tie, &self.periodic) {
            (Some(t), Some(p)) => (t, p),
            _ => return Err(invalid("periodic solve", "the model is not periodic")),
        };
        let t0 = std::time::Instant::now();
        let first = match self.ports.first().map(|p| &p.modes) {
            Some(super::PortModes::Floquet(fp)) => fp,
            _ => return Err(super::SolveError::NotReady("no Floquet ports: create them with model.port.create on the cell's ends".into())),
        };
        let (kx, ky) = incidence(periodic, first, frequency);
        let psi = tie.phases(kx, ky, 0.0);
        let terms = self.floquet_terms(frequency, kx, ky)?;
        for (p, list) in self.ports.iter().zip(&terms) {
            if !list.first().is_some_and(|t| t.propagating) {
                return Err(invalid(
                    "frequency",
                    format!("{frequency} Hz: the excited Floquet mode does not propagate at port `{}` (beyond grazing incidence there)", p.name),
                ));
            }
        }
        let a = self.unbordered(frequency)?;
        let b = tie.reduce(&a.val, psi);
        let borders: Vec<(Vec<(usize, C)>, Vec<(usize, C)>, C)> = terms
            .iter()
            .flatten()
            .map(|t| {
                let conj: Vec<(usize, C)> = t.v.iter().map(|(i, v)| (*i, v.conj())).collect();
                (tie.reduce_test(&t.v, psi), tie.reduce_trial(&conj, psi), t.factor)
            })
            .collect();
        let border = borders.len();
        let a = bordered(&b, &borders)?;
        let nr = tie.reduced;
        let rhs: Vec<Vec<C>> = terms
            .iter()
            .map(|list| {
                let mut r = vec![C::new(0.0, 0.0); nr + border];
                if let Some(t) = list.first() {
                    for (i, v) in tie.reduce_test(&t.v, psi) {
                        r[i] = v * t.rhs;
                    }
                }
                r
            })
            .collect();
        let assembly = t0.elapsed().as_secs_f64();
        let budget = crate::ldlt::memory_budget();
        let made;
        let an = match self.analysis.get().filter(|an| an.fits(&a)) {
            Some(an) => an,
            None => {
                let points: Vec<emcraft_model::Point3> =
                    tie.points.iter().copied().chain(std::iter::repeat_n(emcraft_model::Point3::ORIGIN, border)).collect();
                made = crate::ldlt::Analysis::new(&a, &points, border)?;
                // Kept for the next frequencies (the pattern does not change with them).
                self.analysis.get_or_init(|| made.clone())
            }
        };
        let f = crate::lu::Lu::factor_analysed(&a, an, budget)?;
        let t1 = std::time::Instant::now();
        let mut fields = Vec::with_capacity(rhs.len());
        let mut worst: f64 = 0.0;
        for b in &rhs {
            let (mut x, e) = f.solve_refined(&a, b, 4)?;
            x.truncate(nr);
            worst = worst.max(e);
            fields.push(tie.expand(&x, psi));
        }
        let proxy = |r: usize| terms.get(r).and_then(|l| l.first()).map_or(1.0, |t| t.proxy);
        let mut s = SMatrix::zeros(terms.len());
        for (q, x) in fields.iter().enumerate() {
            for (p, list) in terms.iter().enumerate() {
                let Some(t) = list.first() else { continue };
                let c = t.coefficient(x);
                let direct = if p == q { c - 1.0 } else { c };
                s[(p, q)] = direct * (proxy(p) / proxy(q)).sqrt();
            }
        }
        Ok(FrequencyResult {
            frequency,
            s,
            impedances: vec![None; terms.len()],
            fields,
            backward_error: worst,
            perturbed_pivots: f.perturbed,
            factor_entries: f.entries,
            largest_front: f.largest_front,
            timings: Timings { assembly, analysis: 0.0, factor: f.numeric_seconds, solve: t1.elapsed().as_secs_f64() },
        })
    }
}
