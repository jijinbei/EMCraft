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

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::f64::consts::PI;

use emcraft_model::units::C0;
use emcraft_model::{Model, Waveguide};
use num_complex::Complex64 as C;

pub use crate::device::{Plan, Stepper};
use crate::guide::{Grid, run};

pub mod device;
mod dispersion;
mod guide;
pub mod open;
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
    let port = |i: usize| -> Result<PortGuide> {
        let p = PortGuide::new(model, w, i)?;
        match frequencies.iter().find(|&&f| f <= p.cutoff()) {
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
    // Power waves: amplitude = V/‖e‖, power ∝ ‖e‖ β/μ.
    let mut s11 = Vec::new();
    let mut s21 = Vec::new();
    for (fi, &f) in frequencies.iter().enumerate() {
        let (b1, b2) = (p1.beta(f), p2.beta(f));
        if b1 == 0.0 || b2 == 0.0 {
            return Err(below(f));
        }
        let inc = reference.d1[fi];
        // The transform is linear: the reflected wave's is the difference of the two runs'.
        let refl = dut.d1[fi] - inc;
        let trans = dut.d2[fi];
        s11.push(refl / inc);
        // Amplitudes: V/‖e‖; power factor √((‖e₂‖ β2/μ2)/(‖e₁‖ β1/μ1)).
        let (n1, n2) = (p1.norm(), p2.norm());
        let amp = (trans / n2) / (inc / n1);
        s21.push(amp * ((n2 * b2 / p2.mu) / (n1 * b1 / p1.mu)).sqrt());
    }
    Ok(TdResult { frequencies: frequencies.to_vec(), s11, s21, cells: g.cells(), dt, steps: dut.steps, reference_steps: reference.steps, converged })
}

/// A port's guide: its aperture across x and y and its material.
struct PortGuide {
    width: f64,
    height: f64,
    eps: f64,
    mu: f64,
}

impl PortGuide {
    /// The guide of section `i`.
    fn new(model: &Model, w: &Waveguide, i: usize) -> Result<PortGuide> {
        let s = w.sections.get(i).ok_or_else(|| invalid("waveguide", "no sections"))?;
        let (_, m) = model.material(&s.material).map_err(|e| invalid("material", e.to_string()))?;
        let width = w.aperture(i).map_or(w.width, |a| a.width);
        let height = w.aperture_y(i).map_or(w.height, |a| a.width);
        Ok(PortGuide { width, height, eps: m.eps_r, mu: m.mu_r })
    }

    /// The TE₁₀ cut-off frequency.
    fn cutoff(&self) -> f64 {
        C0 / (2.0 * self.width * (self.eps * self.mu).sqrt())
    }

    /// The TE₁₀ propagation constant at `f` (0 below cut-off).
    fn beta(&self, f: f64) -> f64 {
        let k0 = 2.0 * PI * f / C0;
        (k0 * k0 * self.eps * self.mu - (PI / self.width).powi(2)).max(0.0).sqrt()
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
