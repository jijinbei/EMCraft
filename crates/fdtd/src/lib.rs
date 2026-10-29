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

use std::f64::consts::PI;

use emcraft_model::units::{C0, EPS0, MU0};
use emcraft_model::{Material, Model, Waveguide};
use num_complex::Complex64 as C;

pub use crate::device::{CurrentPlan, NO_SLOT, OpenBatch, OpenPlan, OpenStepper, Plan, Poles, PortEdgePlan, PortPlan, SamplePlan, Stepper};
use crate::guide::{Grid, run};

pub mod device;
mod dispersion;
mod guide;
pub mod open;
mod surface;
mod yee;

/// The most cells a grid may have.
pub const MAX_CELLS: usize = 20_000_000;
/// The most time steps a run may take.
pub const MAX_STEPS: usize = 2_000_000;

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

pub(crate) fn invalid(what: &str, why: impl Into<String>) -> FdtdError {
    FdtdError::Invalid { what: what.into(), why: why.into() }
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
        Options { cell, pml: 16, feed: 12, courant: 0.99, decay: 1e-5, max_steps: 200_000 }
    }
}

/// The result of a run.
#[derive(Clone, Debug, PartialEq)]
pub struct TdResult {
    pub frequencies: Vec<f64>,
    /// Power waves referred to the model's end faces.
    pub s11: Vec<C>,
    pub s21: Vec<C>,
    pub cells: usize,
    pub dt: f64,
    /// Steps of the device run and of the reference run.
    pub steps: usize,
    pub reference_steps: usize,
    /// Whether the transforms settled before `max_steps`.
    pub converged: bool,
    /// The frequency (Hz) at which the materials' loss tangents are matched ([`Losses`]).
    pub loss_reference: f64,
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

/// The largest wall conductivity (S/m) the time domain takes: far beyond any metal's (copper's
/// is 5.8e7), small enough that the walls' admittance stays finite.
pub const MAX_WALL_CONDUCTIVITY: f64 = 1e20;

impl Losses {
    pub fn new(reference: f64) -> Losses {
        Losses { reference }
    }

    /// The conductivity σ = ω_r ε₀ εr tan δ (S/m) and magnetic conductivity σ_m = ω_r μ₀ μr
    /// tan δ_m (Ω/m) standing for `m`'s loss tangents (εr is ε∞ with dispersion).
    pub fn of(&self, m: &Material) -> (f64, f64) {
        let w = 2.0 * PI * self.reference;
        (w * EPS0 * m.eps_r * m.loss_tangent, w * MU0 * m.mu_r * m.magnetic_loss_tangent)
    }

    /// The relative permittivity and permeability the time domain realises for `m` at `f`
    /// (Hz): ε∞ + Σ χ(ω) − jσ/(ωε₀) and μr − jσ_m/(ωμ₀).
    pub fn eps_mu(&self, m: &Material, f: f64) -> (C, C) {
        let r = self.reference / f;
        let eps = m.dispersion.iter().fold(C::new(m.eps_r, -m.eps_r * m.loss_tangent * r), |e, p| e + p.susceptibility(f));
        (eps, C::new(m.mu_r, -m.mu_r * m.magnetic_loss_tangent * r))
    }

    /// The walls' conductivity `sigma` checked: finite, > 0 and a good conductor up to `f_max`.
    pub(crate) fn wall(&self, sigma: f64, f_max: f64) -> Result<f64> {
        if !(sigma.is_finite() && sigma > 0.0 && sigma <= MAX_WALL_CONDUCTIVITY) {
            return Err(invalid("wall conductivity", format!("must be a finite number from 0 to {MAX_WALL_CONDUCTIVITY:e} S/m, got {sigma}")));
        }
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

/// [`s_parameters`] with the leapfrog on `device` (the CPU's threads when `None`).
pub fn s_parameters_with(model: &Model, o: &Options, frequencies: &[f64], device: Option<&dyn Stepper>) -> Result<TdResult> {
    if !(o.cell.is_finite() && o.cell > 0.0) {
        return Err(invalid("cell", "must be a finite number > 0"));
    }
    if !(o.courant > 0.0 && o.courant < 1.0) || o.pml < 4 || o.feed < 2 || o.max_steps == 0 || o.max_steps > MAX_STEPS || !(o.decay > 0.0) {
        return Err(invalid("options", format!("{o:?}")));
    }
    if frequencies.is_empty() || frequencies.iter().any(|f| !(f.is_finite() && *f > 0.0)) {
        return Err(invalid("frequencies", "must be finite numbers > 0"));
    }
    let w = model.waveguide.as_ref().ok_or_else(|| FdtdError::NotReady("no waveguide".into()))?;
    if !model.symmetry.is_empty() {
        return Err(FdtdError::Unsupported("symmetry planes in the time domain (clear them: model.symmetry with clear true)".into()));
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
    // The incident run: the port-1 guide throughout.
    let gr = Grid::new(model, o, true, frequencies)?;
    let mut settling = Settling::new(o.decay);
    let (reference, dt) = run(&gr, o, frequencies, device, |d1, _| settling.settled(d1, &[], peak(d1)))?;
    let scale = peak(&reference.d1);
    let g = Grid::new(model, o, false, frequencies)?;
    let mut settling = Settling::new(o.decay);
    let mut converged = false;
    let (dut, dt2) = run(&g, o, frequencies, device, |d1, d2| {
        converged = settling.settled(d1, d2, scale);
        converged
    })?;
    if (dt - dt2).abs() > 1e-12 * dt {
        return Err(invalid("grid", "the incident and device runs differ in time step"));
    }
    // Power waves (pseudo-waves in lossy ports): amplitude = V/‖e‖, power ∝ ‖e‖ β/μ.
    let mut s11 = Vec::new();
    let mut s21 = Vec::new();
    for (fi, &f) in frequencies.iter().enumerate() {
        let (y1, y2) = (p1.admittance(f), p2.admittance(f));
        if y1.norm() == 0.0 || y2.norm() == 0.0 {
            return Err(below(f));
        }
        let inc = reference.d1[fi];
        // The transform is linear: the reflected wave's is the difference of the two runs'.
        let refl = dut.d1[fi] - inc;
        let trans = dut.d2[fi];
        s11.push(refl / inc);
        // Amplitudes: V/‖e‖; power factor √((‖e₂‖ β2/μ2)/(‖e₁‖ β1/μ1)), real for lossless ports
        // and on the principal branch otherwise.
        let (n1, n2) = (p1.norm(), p2.norm());
        let amp = (trans / n2) / (inc / n1);
        let ratio = (y2 * n2) / (y1 * n1);
        s21.push(amp * if ratio.im == 0.0 { C::new(ratio.re.sqrt(), 0.0) } else { ratio.sqrt() });
    }
    Ok(TdResult {
        frequencies: frequencies.to_vec(),
        s11,
        s21,
        cells: g.cells(),
        dt,
        steps: dut.steps,
        reference_steps: reference.steps,
        converged,
        loss_reference: losses.reference,
    })
}

/// A port's guide: its aperture across x and y and its material, with its losses as the time
/// domain realises them.
struct PortGuide {
    width: f64,
    height: f64,
    material: Material,
    losses: Losses,
}

impl PortGuide {
    /// The guide of section `i`.
    fn new(model: &Model, w: &Waveguide, i: usize, losses: Losses) -> Result<PortGuide> {
        let s = w.sections.get(i).ok_or_else(|| invalid("waveguide", "no sections"))?;
        let (_, m) = model.material(&s.material).map_err(|e| invalid("material", e.to_string()))?;
        let width = w.aperture(i).map_or(w.width, |a| a.width);
        let height = w.aperture_y(i).map_or(w.height, |a| a.width);
        Ok(PortGuide { width, height, material: m.clone(), losses })
    }

    /// k² − (π/a)² at `f`, k² = k₀² ε μ.
    fn kz2(&self, f: f64) -> C {
        let k0 = 2.0 * PI * f / C0;
        let (eps, mu) = self.losses.eps_mu(&self.material, f);
        eps * mu * (k0 * k0) - (PI / self.width).powi(2)
    }

    /// Whether TE₁₀ propagates at `f`: Re(k²) above the cut-off's (k² real without losses).
    fn propagates(&self, f: f64) -> bool {
        self.kz2(f).re > 0.0
    }

    /// The TE₁₀ β/μr at `f`, β = √(k² − (π/a)²) on the branch with Im β ≤ 0 (a wave that
    /// decays along +z), 0 below cut-off without losses.
    fn admittance(&self, f: f64) -> C {
        let kz2 = self.kz2(f);
        let (_, mu) = self.losses.eps_mu(&self.material, f);
        if kz2.im == 0.0 && mu.im == 0.0 {
            return C::new(kz2.re.max(0.0).sqrt() / mu.re, 0.0);
        }
        let b = kz2.sqrt();
        (if b.im > 0.0 { -b } else { b }) / mu
    }

    /// The TE₁₀ profile's norm ‖e‖ = width·height/2.
    fn norm(&self) -> f64 {
        0.5 * self.width * self.height
    }
}

/// The largest magnitude of a transform.
fn peak(d: &[C]) -> f64 {
    d.iter().fold(0.0f64, |m, c| m.max(c.norm()))
}

/// Whether a run's transforms have settled: no frequency moved by more than `decay` of the
/// incident wave's largest since the last check.
struct Settling {
    decay: f64,
    last: Option<(Vec<C>, Vec<C>)>,
}

impl Settling {
    fn new(decay: f64) -> Settling {
        Settling { decay, last: None }
    }

    /// Checks the transforms at the two planes against the last check's, `scale` the incident
    /// wave's largest.
    fn settled(&mut self, d1: &[C], d2: &[C], scale: f64) -> bool {
        let moved = self.last.as_ref().map(|(p1, p2)| {
            let m1 = p1.iter().zip(d1).fold(0.0f64, |m, (a, b)| m.max((a - b).norm()));
            let m2 = p2.iter().zip(d2).fold(0.0f64, |m, (a, b)| m.max((a - b).norm()));
            m1.max(m2)
        });
        self.last = Some((d1.to_vec(), d2.to_vec()));
        moved.is_some_and(|m| m <= self.decay * scale)
    }
}
