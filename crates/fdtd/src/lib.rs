//! EMCraft's time-domain solver: the finite integration technique on a tensor grid (T. Weiland,
//! *A discretization method for the solution of Maxwell's equations for six-component fields*,
//! AEÜ 31:116–120, 1977), which on a rectangular grid is the Yee scheme (K. S. Yee, *Numerical
//! solution of initial boundary value problems involving Maxwell's equations in isotropic
//! media*, IEEE Trans. Antennas Propag. 14(3):302–307, 1966): E on the edges of the primal grid,
//! H on the edges of the dual grid, leapfrogged in time. Grid lines need not be evenly spaced:
//! the curls use the primal edge lengths and the dual ones, and each E edge takes the
//! area-weighted permittivity of the cells around it.
//!
//! **Ports.** Each end of the guide is extended by its end section: a feed of a few cells and a
//! convolutional PML (J. A. Roden, S. D. Gedney, *Convolution PML (CPML): an efficient FDTD
//! implementation of the CFS–PML for arbitrary media*, Microw. Opt. Technol. Lett.
//! 27(5):334–339, 2000) that absorbs what leaves along z. A soft source with the TE₁₀ profile
//! in the port-1 feed launches a Gaussian-modulated pulse covering the band.
//!
//! **S-parameters.** The TE₁₀ amplitude is projected out of E_y on the model's end planes, z = 0
//! and z = L, every step and Fourier-transformed on the fly. A second run with the port-1 guide
//! uniform throughout gives the incident wave at z = 0 alone; then S₁₁ = (V₁ − V₁ʳᵉᶠ)/V₁ʳᵉᶠ
//! and S₂₁ = V₂/V₁ʳᵉᶠ (amplitudes, then power waves), already referred to the model's end faces,
//! with no de-embedding, and the grid's own dispersion in the feed cancels.
//!
//! **Dispersive materials** (Debye, Lorentz and Drude poles, [`emcraft_model::Pole`]) by the
//! auxiliary differential equation method (A. Taflove, S. C. Hagness, *Computational
//! Electrodynamics: The Finite-Difference Time-Domain Method*, 3rd ed., Artech House 2005, ch. 9,
//! §9.4): each pole's polarization P on the E edges it touches, D = ε₀ε∞E + ΣP, advanced with E.
//! A Debye pole, τ ∂ₜP + P = ε₀Δε E, is centred at n + ½ (trapezoidal), which couples P and E
//! at n + 1 through a scalar per edge, solved in place; a second-order pole (Lorentz, Drude),
//! ∂ₜ²P + γ∂ₜP + ω₀²P = ε₀ a E, is centred at n and gives P at n + 1 explicitly from E at n.
//! Ampère's law then reads ε₀ε∞ (Eⁿ⁺¹ − Eⁿ)/Δt = ∇ × Hⁿ⁺½ − (Pⁿ⁺¹ − Pⁿ)/Δt. An edge between
//! cells of different materials takes each pole weighted by the area of its cells, as it takes
//! ε∞.
//!
//! **Losses.** A loss tangent tan δ, constant over frequency in the frequency domain, has no
//! causal, finite-order time-domain form; the time domain realises it as a conductivity matched
//! at the centre f_r of the run's band, σ = 2π f_r ε₀ εr tan δ, so the material it solves is
//! ε = εr − jσ/(ωε₀): tan δ exactly at f_r, tan δ f_r/f elsewhere ([`Losses`]; the reply of
//! `solve.time_domain` names f_r). The conduction current is averaged over the step,
//! σ (Eⁿ⁺¹ + Eⁿ)/2, with ε scaled by (r/2) coth(r/2), r = σΔt/ε, which makes the update the
//! exponential one (Taflove & Hagness 2005, §3.12): second order, and a good conductor's field
//! decays at once rather than flipping sign every step. A magnetic loss tangent likewise becomes
//! a magnetic conductivity σ_m = 2π f_r μ₀ μr tan δ_m on the H faces, updated exponentially.
//! Walls of finite conductivity take Leontovich's surface impedance through branches of surface
//! current on the walls' edges (`surface`). (A causal model nearer a constant tan δ, such as
//! Djordjevic–Sarkar's wideband Debye sum, would change ε′ with frequency; the conductivity keeps
//! εr and is exactly what the references here are built for.)
//!
//! **Ports in lossy and dispersive media.** The ports' TE₁₀ propagation constant is complex
//! there, γ = √((π/a)² − k₀² ε(ω) μ(ω)) for the ε and μ the time domain realises; S₂₁ is
//! normalised by the modes' unconjugated powers ∝ ‖e‖ β/μ (β = −jγ), the pseudo-waves the
//! frequency-domain solver reads (R. B. Marks, D. F. Williams, *A general waveguide circuit
//! theory*, J. Res. NIST 97(5):533–562, 1992), so that equal port media cancel and
//! |S₂₁| = e^{−αL} along a uniform guide.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use emcraft_model::units::{angular, wavenumber};
use std::f64::consts::PI;

use emcraft_model::units::{EPS0, MU0};
use emcraft_model::{Conductivity, Hertz, Material, Model, Quantity, Span, Waveguide};
use num_complex::Complex64 as C;

pub use crate::device::{
    Axis, Cpml, CurrentPlan, NO_SLOT, OpenBatch, OpenPlan, OpenStepper, Plan, Poles, PortEdgePlan, PortPlan, SamplePlan, Stepper,
};
use crate::guide::{Grid, run};
pub use crate::solid::{MAX_EXCITED, s_parameters_solid};
use crate::yee::Settling;

pub mod axi;
pub mod bor;
pub mod device;
mod dispersion;
mod guide;
pub mod open;
pub mod pic;
mod planewave;
mod solid;
mod surface;
mod yee;

/// The most cells a grid may have.
pub const MAX_CELLS: usize = 20_000_000;
/// The most time steps a run may take.
pub const MAX_STEPS: usize = 2_000_000;

/// A guide run's defaults: PML cells at each end (reflecting below −60 dB for TE10 above
/// cut-off), feed cells between a port and the structure, the time step as a fraction of the
/// stability limit, the settling a transform must reach (a fraction of the incident wave's in a
/// check interval), and the most steps before giving up.
pub const DEFAULT_PML: usize = 16;
pub const DEFAULT_FEED: usize = 12;
pub const DEFAULT_COURANT: f64 = 0.99;
pub const DEFAULT_DECAY: f64 = 1e-5;
pub const DEFAULT_MAX_STEPS: usize = 200_000;

/// Two coordinates are the same within this fraction of the length they are measured on (a
/// grid's span, a face's size): far above the round-off of metres from display units.
pub(crate) const GEOMETRY_TOLERANCE: f64 = 1e-9;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum FdtdError {
    #[error("the model is not ready: {0}")]
    NotReady(String),
    #[error("invalid {what}: {why}")]
    Invalid { what: String, why: String },
    #[error("not supported yet: {0}")]
    Unsupported(String),
    #[error("the device failed: {0}")]
    Device(String),
    #[error("the grid would have {cells} cells, more than the limit of {limit}: use a larger cell")]
    TooLarge { cells: usize, limit: usize },
}

pub type Result<T> = std::result::Result<T, FdtdError>;

/// An invalid `what`, for `why`.
pub fn invalid(what: &str, why: impl Into<String>) -> FdtdError {
    FdtdError::Invalid { what: what.into(), why: why.into() }
}

/// `x` as a device's 32-bit index, refused as an invalid `what` when it does not fit.
pub fn to_u32(x: usize, what: &str) -> Result<u32> {
    u32::try_from(x).map_err(|_| invalid(what, format!("{x} does not fit 32 bits")))
}

/// Whether `frequencies` are some finite numbers > 0.
fn frequencies_ok(frequencies: &[f64]) -> bool {
    !frequencies.is_empty() && frequencies.iter().all(|f| Hertz::keeps(*f))
}

/// Run settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    /// The cell size (m), the grid's largest step.
    pub cell: f64,
    /// PML cells at each end.
    pub pml: usize,
    /// Cells between the PML and the source, and between the source and the port plane.
    pub feed: usize,
    /// Time step as a fraction of the stability limit.
    pub courant: f64,
    /// Stop when the transforms at the requested frequencies settle: none moves by more than this
    /// fraction of the incident wave's in a check interval.
    pub decay: f64,
    pub max_steps: usize,
}

impl Options {
    pub fn new(cell: f64) -> Options {
        Options { cell, pml: DEFAULT_PML, feed: DEFAULT_FEED, courant: DEFAULT_COURANT, decay: DEFAULT_DECAY, max_steps: DEFAULT_MAX_STEPS }
    }

    /// The settings checked: a finite cell > 0, a Courant number in (0, 1), at least 4 PML and
    /// 2 feed cells, 1 to [`MAX_STEPS`] steps and a decay > 0.
    pub fn validate(&self) -> Result<()> {
        if !(self.cell.is_finite() && self.cell > 0.0) {
            return Err(invalid("cell", "must be a finite number > 0"));
        }
        if !(self.courant > 0.0 && self.courant < 1.0)
            || self.pml < 4
            || self.feed < 2
            || self.max_steps == 0
            || self.max_steps > MAX_STEPS
            || !(self.decay > 0.0)
        {
            return Err(invalid("options", format!("{self:?}")));
        }
        Ok(())
    }
}

/// The result of a run.
#[derive(Clone, Debug, PartialEq)]
pub struct TdResult {
    pub frequencies: Vec<f64>,
    /// The model's ports, in its order.
    pub ports: usize,
    /// The ports excited (indices into the model's), one run each.
    pub inputs: Vec<usize>,
    /// Power waves referred to the ports' faces: S\[p\]\[inputs\[c\]\] at frequency k at
    /// (c · ports + p) · frequencies + k.
    pub s: Vec<C>,
    pub cells: usize,
    pub dt: f64,
    /// Steps of the device runs and of the reference runs.
    pub steps: usize,
    pub reference_steps: usize,
    /// Whether the transforms settled before `max_steps`.
    pub converged: bool,
    /// The frequency (Hz) at which the materials' loss tangents are matched ([`Losses`]).
    pub loss_reference: f64,
}

impl TdResult {
    /// S\[p\]\[inputs\[c\]\] at each frequency; empty out of range.
    pub fn column(&self, c: usize, p: usize) -> &[C] {
        let n = self.frequencies.len();
        let at = (c * self.ports + p) * n;
        self.s.get(at..at + n).unwrap_or(&[])
    }

    /// S₁₁ with port 1 excited first (the guide's own run).
    pub fn s11(&self) -> &[C] {
        self.column(0, 0)
    }

    /// S₂₁ with port 1 excited first.
    pub fn s21(&self) -> &[C] {
        self.column(0, 1)
    }

    /// Whether the S-parameters hold one value per frequency, port and input, the inputs
    /// distinct ports.
    pub fn consistent(&self) -> bool {
        let mut inputs = self.inputs.clone();
        inputs.sort_unstable();
        inputs.dedup();
        inputs.len() == self.inputs.len()
            && self.inputs.iter().all(|&q| q < self.ports)
            && self.ports.checked_mul(self.inputs.len()).and_then(|x| x.checked_mul(self.frequencies.len())) == Some(self.s.len())
    }
}

/// How the time domain realises the materials' loss tangents: as an electric and a magnetic
/// conductivity matched at the `reference` frequency (Hz), the centre of the run's band.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Losses {
    pub reference: f64,
}

/// The smallest σ/(ωε₀) at the band's top for walls of finite conductivity: Leontovich's
/// condition holds for good conductors only.
const GOOD_CONDUCTOR: f64 = 100.0;

impl Losses {
    pub fn new(reference: f64) -> Losses {
        Losses { reference }
    }

    /// The conductivity σ = ω_r ε₀ εr tan δ (S/m) and magnetic conductivity σ_m = ω_r μ₀ μr
    /// tan δ_m (Ω/m) standing for `m`'s loss tangents (εr is ε∞ with dispersion).
    pub fn of(&self, m: &Material) -> (f64, f64) {
        let w = angular(self.reference);
        (w * EPS0 * m.eps_r.get() * m.loss_tangent.get(), w * MU0 * m.mu_r.get() * m.magnetic_loss_tangent.get())
    }

    /// The relative permittivity and permeability the time domain realises for `m` at `f`
    /// (Hz): ε∞ + Σ χ(ω) − jσ/(ωε₀) and μr − jσ_m/(ωμ₀).
    pub fn eps_mu(&self, m: &Material, f: f64) -> (C, C) {
        let r = self.reference / f;
        let eps = m.dispersion.iter().fold(C::new(m.eps_r.get(), -m.eps_r.get() * m.loss_tangent.get() * r), |e, p| e + p.susceptibility(f));
        (eps, C::new(m.mu_r.get(), -m.mu_r.get() * m.magnetic_loss_tangent.get() * r))
    }

    /// The walls' conductivity `sigma` (S/m, in range by its type) checked against the band: a
    /// good conductor up to `f_max`.
    pub(crate) fn wall(sigma: Conductivity, f_max: f64) -> Result<f64> {
        let sigma = sigma.get();
        if sigma < GOOD_CONDUCTOR * 2.0 * PI * f_max * EPS0 {
            return Err(invalid(
                "wall conductivity",
                format!("{sigma} S/m is not a good conductor at {f_max:e} Hz (σ/ωε₀ < {GOOD_CONDUCTOR}): the surface-impedance condition needs one"),
            ));
        }
        Ok(sigma)
    }
}

/// The broadband S-parameters (port 1 excited) of the model's guide at `frequencies` (Hz).
pub fn s_parameters(model: &Model, o: &Options, frequencies: &[f64]) -> Result<TdResult> {
    s_parameters_with(model, o, frequencies, None)
}

/// The guide's two ports, checked for a run at `frequencies` (each above both ports' TE₁₀
/// cut-off), and the losses matched at the band's centre.
fn ports(model: &Model, o: &Options, frequencies: &[f64]) -> Result<(PortGuide, PortGuide, Losses)> {
    o.validate()?;
    if !frequencies_ok(frequencies) {
        return Err(invalid("frequencies", "must be finite numbers > 0"));
    }
    let w = model.waveguide().ok_or_else(|| FdtdError::NotReady("no waveguide".into()))?;
    if !model.domain.symmetry.is_empty() {
        return Err(FdtdError::Unsupported("symmetry planes in the time domain (clear them: model.symmetry with clear true)".into()));
    }
    if !model.elements.is_empty() {
        return Err(FdtdError::Unsupported("lumped elements in the time domain".into()));
    }
    if !w.plain() {
        return Err(FdtdError::Unsupported("layers and strips in the time domain (its ports carry TE10 only)".into()));
    }
    // Every frequency above both ports' TE₁₀ cut-off, checked before any run.
    let below = |f: f64| invalid("frequency", format!("{f} Hz is below a port's TE10 cut-off"));
    let losses = Losses::new(yee::Band::of(frequencies).fc);
    let port = |i: usize| -> Result<PortGuide> {
        let p = PortGuide::new(model, w, i, losses)?;
        match frequencies.iter().find(|&&f| !p.propagates(f)) {
            Some(&f) => Err(below(f)),
            None => Ok(p),
        }
    };
    let (p1, p2) = (port(0)?, port(w.sections.len().saturating_sub(1))?);
    Ok((p1, p2, losses))
}

/// [`s_parameters`] with the leapfrog on `device` (the CPU's threads when `None`).
pub fn s_parameters_with(model: &Model, o: &Options, frequencies: &[f64], device: Option<&dyn Stepper>) -> Result<TdResult> {
    let (p1, p2, losses) = ports(model, o, frequencies)?;
    let below = |f: f64| invalid("frequency", format!("{f} Hz is below a port's TE10 cut-off"));
    // The incident run: the port-1 guide throughout.
    let gr = Grid::new(model, o, true, frequencies)?;
    let mut settling = Settling::new(o.decay);
    let (reference, dt) = run(&gr, o, frequencies, device, |d1, _| settling.settled(d1.to_vec(), peak(d1)))?;
    let scale = peak(&reference.d1);
    let g = Grid::new(model, o, false, frequencies)?;
    let mut settling = Settling::new(o.decay);
    let mut converged = false;
    let (dut, dt2) = run(&g, o, frequencies, device, |d1, d2| {
        converged = settling.settled(d1.iter().chain(d2).copied().collect(), scale);
        converged
    })?;
    if (dt - dt2).abs() > 1e-12 * dt {
        return Err(invalid("grid", "the incident and device runs differ in time step"));
    }
    // Power waves (pseudo-waves in lossy ports): amplitude = V/‖e‖, power ∝ ‖e‖ β/μ.
    let mut s11 = Vec::new();
    let mut s21 = Vec::new();
    for (fi, &f) in frequencies.iter().enumerate() {
        let inc = reference.d1[fi];
        // The transform is linear: the reflected wave's is the difference of the two runs'.
        s11.push((dut.d1[fi] - inc) / inc);
        s21.push(transmission(dut.d2[fi], &p2, inc, &p1, f).ok_or_else(|| below(f))?);
    }
    Ok(TdResult {
        frequencies: frequencies.to_vec(),
        ports: 2,
        inputs: vec![0],
        s: s11.into_iter().chain(s21).collect(),
        cells: g.cells(),
        dt,
        steps: dut.steps,
        reference_steps: reference.steps,
        converged,
        loss_reference: losses.reference,
    })
}

/// A chirped pulse's run through the guide: the TE₁₀ projections (V·m) of the wave launched
/// at port 1 (its plane in a run of the port-1 guide throughout) and of the wave out of port 2,
/// `dt` (s) apart from the pulse's launch, its peak at `t0` (s).
#[derive(Clone, Debug, PartialEq)]
pub struct ChirpRun {
    pub dt: f64,
    pub t0: f64,
    pub incident: Vec<f64>,
    pub transmitted: Vec<f64>,
    pub cells: usize,
}

/// A Gaussian pulse of full width at half power `width` (s) at `f0` (Hz), chirped at `rate`
/// (Hz/s), through the model's guide in the time domain: the direct check of a pulse made from
/// the S-parameters (both ports must be the same guide, whose wave the projections compare).
/// Its band, f₀ ± (|rate| width + 1/width) (the Gaussian's spectrum 10⁻⁶ down past the
/// sweep), must lie above the ports' cut-off.
pub fn chirp_through(model: &Model, o: &Options, f0: f64, width: f64, rate: f64) -> Result<ChirpRun> {
    if !(f0.is_finite() && f0 > 0.0 && width.is_finite() && width > 0.0 && rate.is_finite()) {
        return Err(invalid("pulse", "the carrier and width must be positive and the chirp finite"));
    }
    let half = rate.abs() * width + 1.0 / width;
    if !(half < f0) {
        return Err(invalid("pulse", "its band reaches 0 Hz: lengthen it or chirp less"));
    }
    let band = [f0 - half, f0, f0 + half];
    let (p1, p2, _) = ports(model, o, &band)?;
    if p1.width != p2.width || p1.height != p2.height || p1.material != p2.material {
        return Err(FdtdError::Unsupported("a chirped pulse between ports of different guides".into()));
    }
    let pulse = yee::Pulse::chirped(width, f0, rate);
    let reference = Grid::new(model, o, true, &band)?;
    let (incident, dt) = guide::chirped(&reference, o, yee::Band::of(&band), pulse, None)?;
    let g = Grid::new(model, o, false, &band)?;
    let (through, _) = guide::chirped(&g, o, yee::Band::of(&band), pulse, None)?;
    Ok(ChirpRun {
        dt,
        t0: pulse.t0,
        incident: incident.iter().map(|v| v[0]).collect(),
        transmitted: through.iter().map(|v| v[1]).collect(),
        cells: g.cells(),
    })
}

/// A port's guide: its aperture across x and y and its material, with its losses as the time
/// domain realises them.
pub(crate) struct PortGuide {
    pub(crate) width: f64,
    pub(crate) height: f64,
    pub(crate) material: Material,
    pub(crate) losses: Losses,
}

impl PortGuide {
    /// The guide of section `i`.
    fn new(model: &Model, w: &Waveguide, i: usize, losses: Losses) -> Result<PortGuide> {
        let s = w.sections.get(i).ok_or_else(|| invalid("waveguide", "no sections"))?;
        let (_, m) = model.material(&s.material).map_err(|e| invalid("material", e.to_string()))?;
        let [Span { width, .. }, Span { width: height, .. }] = w.opening(i);
        Ok(PortGuide { width, height, material: m.clone(), losses })
    }

    /// k² − (π/a)² at `f`, k² = k₀² ε μ.
    pub(crate) fn kz2(&self, f: f64) -> C {
        let k0 = wavenumber(f);
        let (eps, mu) = self.losses.eps_mu(&self.material, f);
        eps * mu * (k0 * k0) - (PI / self.width).powi(2)
    }

    /// Whether TE₁₀ propagates at `f`: Re(k²) above the cut-off's (k² real without losses).
    pub(crate) fn propagates(&self, f: f64) -> bool {
        self.kz2(f).re > 0.0
    }

    /// The TE₁₀ β/μr at `f`, β = √(k² − (π/a)²) on the branch with Im β ≤ 0 (a wave that
    /// decays along +z), 0 below cut-off without losses.
    pub(crate) fn admittance(&self, f: f64) -> C {
        let kz2 = self.kz2(f);
        let (_, mu) = self.losses.eps_mu(&self.material, f);
        if kz2.im == 0.0 && mu.im == 0.0 {
            return C::new(kz2.re.max(0.0).sqrt() / mu.re, 0.0);
        }
        let b = kz2.sqrt();
        (if b.im > 0.0 { -b } else { b }) / mu
    }

    /// The TE₁₀ profile's norm ‖e‖ = width·height/2.
    pub(crate) fn norm(&self) -> f64 {
        0.5 * self.width * self.height
    }
}

/// The power wave out of port `out` (its TE₁₀ projection's transform `v`) per power wave in
/// at port `inc` (`v_inc`) at `f`: amplitudes V/‖e‖, and the power factor
/// √((‖e_out‖ β_out/μ_out)/(‖e_in‖ β_in/μ_in)), real for lossless ports and on the principal
/// branch otherwise. `None` below either port's cut-off.
pub(crate) fn transmission(v: C, out: &PortGuide, v_inc: C, inc: &PortGuide, f: f64) -> Option<C> {
    let (y_in, y_out) = (inc.admittance(f), out.admittance(f));
    if y_in.norm() == 0.0 || y_out.norm() == 0.0 {
        return None;
    }
    let (n_in, n_out) = (inc.norm(), out.norm());
    let amp = (v / n_out) / (v_inc / n_in);
    let ratio = (y_out * n_out) / (y_in * n_in);
    Some(amp * if ratio.im == 0.0 { C::new(ratio.re.sqrt(), 0.0) } else { ratio.sqrt() })
}

/// The largest magnitude of a transform.
fn peak(d: &[C]) -> f64 {
    d.iter().fold(0.0f64, |m, c| m.max(c.norm()))
}
