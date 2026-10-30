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

use super::assembly::{Space, border_columns};
use super::ports::for_each_port_point;
use super::solution::solve_each;
use super::sparams::{ModeTerm, read_s};
use super::{Driven, FrequencyResult, Result, check_frequency, invalid, wavenumber};
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

impl ModeTerm for FloquetTerm {
    /// The mode's coefficient c = q·x/‖e‖² in the full field `x`.
    fn coefficient(&self, x: &[C]) -> C {
        self.v.iter().map(|&(i, vi)| vi.conj() * x.get(i).copied().unwrap_or_default()).sum::<C>() / self.norm
    }

    fn proxy(&self) -> C {
        C::new(self.proxy, 0.0)
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
    if m.is_lossy() {
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
        let mut acc: Vec<std::collections::BTreeMap<usize, C>> = vec![Default::default(); modes.len()];
        let dirs: Vec<Vec3> = modes.iter().map(|m| m.direction(self.phi)).collect();
        for_each_port_point(*space, &self.faces, emcraft_model::Frame::new(emcraft_model::Axis::Z), |x, wa, traced| {
            for ((md, dir), map) in modes.iter().zip(&dirs).zip(acc.iter_mut()) {
                let arg = md.kx * (x.x - x0) + md.ky * (x.y - y0);
                let ph = C::new(arg.cos(), -arg.sin()) * wa;
                for &(u, v) in traced {
                    *map.entry(u).or_default() += ph * v.dot(*dir);
                }
            }
        })?;
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

/// One Floquet mode's border of the reduced matrix: its column (test side, Tᴴ v), its row
/// (trial side, Tᵀ v̄) and its factor.
struct Border {
    column: Vec<(usize, C)>,
    row: Vec<(usize, C)>,
    factor: C,
}

/// `a` bordered by one row and column per [`Border`]: column s·column, row s·row, diagonal
/// −s²/f, s = √(|f|‖a‖) (as the symmetric ports' border, [`border_columns`]); the pattern kept
/// symmetric with structural zeros.
fn bordered(a: &Csr, borders: &[Border]) -> Result<Csr> {
    let n = a.n;
    let (mut out, scale) = border_columns(a, borders.iter().map(|b| (b.column.iter().copied(), b.factor)))?;
    for (k, (b, s)) in borders.iter().zip(&scale).enumerate() {
        // The pattern stays symmetric: the row holds every unknown the column does.
        let rowmap: std::collections::BTreeMap<usize, C> = b.row.iter().copied().collect();
        let mut idx: Vec<usize> = b.column.iter().map(|(i, _)| *i).chain(b.row.iter().map(|(i, _)| *i)).collect();
        idx.sort_unstable();
        idx.dedup();
        for i in idx {
            out.push(i, rowmap.get(&i).copied().unwrap_or_default() * *s);
        }
        out.push(n + k, -(s * s) / b.factor);
        out.end_row();
    }
    Ok(out.finish().with_symmetric_pattern())
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
        let tie = self.tie.as_ref().ok_or_else(|| invalid("periodic solve", "the model is not periodic"))?;
        let t0 = std::time::Instant::now();
        let PeriodicSystem { a, rhs, terms, psi, border } = self.periodic_system(frequency)?;
        let assembly = t0.elapsed().as_secs_f64();
        let f = self.factor_lu(&a, &tie.points, border)?;
        let mut solved = solve_each(&f, &a, &rhs, tie.reduced, assembly)?;
        solved.fields = solved.fields.iter().map(|x| tie.expand(x, psi)).collect();
        let excitations: Vec<(usize, usize)> = (0..terms.len()).map(|p| (p, 0)).collect();
        let s = read_s(&terms, &excitations, &solved.fields);
        Ok(solved.result(frequency, s, vec![None; terms.len()]))
    }

    /// The periodic cell's system at `frequency`: Tᴴ A T bordered by every Floquet mode, the
    /// right-hand side of each port's excitation by its first mode, the modes' terms and the
    /// phases per period.
    fn periodic_system(&self, frequency: f64) -> Result<PeriodicSystem> {
        let (tie, periodic) = match (&self.tie, &self.periodic) {
            (Some(t), Some(p)) => (t, p),
            _ => return Err(invalid("periodic solve", "the model is not periodic")),
        };
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
        let reduced = tie.reduce(&self.unbordered(frequency)?.val, psi);
        let borders: Vec<Border> = terms
            .iter()
            .flatten()
            .map(|t| {
                let conj: Vec<(usize, C)> = t.v.iter().map(|(i, v)| (*i, v.conj())).collect();
                Border { column: tie.reduce_test(&t.v, psi), row: tie.reduce_trial(&conj, psi), factor: t.factor }
            })
            .collect();
        let border = borders.len();
        let a = bordered(&reduced, &borders)?;
        let rhs: Vec<Vec<C>> = terms
            .iter()
            .map(|list| {
                let mut r = vec![C::new(0.0, 0.0); tie.reduced + border];
                if let Some(t) = list.first() {
                    for (i, v) in tie.reduce_test(&t.v, psi) {
                        r[i] = v * t.rhs;
                    }
                }
                r
            })
            .collect();
        Ok(PeriodicSystem { a, rhs, terms, psi, border })
    }
}

/// A periodic cell's linear system at one frequency (see [`Driven::periodic_system`]).
struct PeriodicSystem {
    a: Csr,
    rhs: Vec<Vec<C>>,
    terms: Vec<Vec<FloquetTerm>>,
    psi: [f64; 3],
    border: usize,
}
