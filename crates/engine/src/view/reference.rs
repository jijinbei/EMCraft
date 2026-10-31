//! What solved S-parameters are compared with, and the cut-offs marked beside them: the exact
//! TE₁₀ cascade of a uniform guide, mode matching where the aperture changes, the closed forms of
//! empty circular and coaxial guides, and the plane-wave stack of a periodic cell.

use emcraft_model::Material;
use emcraft_solver::Complex64 as C;
use emcraft_theory::{EPlaneLine, ESection, HPlaneLine, HSection, Layer, Te10Line, te10_cutoff};

use crate::{EngineError, Result, Session};

/// A TE₁₀ line's section `length` long of material `m` at `frequency` (Hz): its permittivity
/// εr(ω) = ε′ − jε″ as εr (1 − j tan δ), tan δ = ε″/ε′, and its permeability μr(1 − j tan δ_m).
fn layer_at(length: f64, m: &Material, frequency: f64) -> Layer {
    let magnetic_loss_tangent = m.magnetic_loss_tangent.get();
    if !m.is_dispersive() {
        return Layer { length, eps_r: m.eps_r.get(), mu_r: m.mu_r.get(), loss_tangent: m.loss_tangent.get(), magnetic_loss_tangent };
    }
    let e = m.eps_at(frequency);
    Layer { length, eps_r: e.re, mu_r: m.mu_r.get(), loss_tangent: -e.im / e.re, magnetic_loss_tangent }
}

/// A uniform guide's sections (length, material) and its width: its exact TE₁₀ line at each
/// frequency.
#[derive(Clone, Debug)]
pub struct ExactLine {
    pub a: f64,
    pub sections: Vec<(f64, Material)>,
}

impl ExactLine {
    /// The line at `frequency` (Hz), dispersive materials evaluated there.
    pub fn at(&self, frequency: f64) -> Te10Line {
        Te10Line { a: self.a, layers: self.sections.iter().map(|(l, m)| layer_at(*l, m, frequency)).collect() }
    }
}

/// The exact TE₁₀ line of a waveguide of uniform cross-section.
pub fn te10_line(session: &Session) -> Result<ExactLine> {
    if session.model.periodic.is_some() {
        return Err(EngineError::failed("not supported yet: exact fields of a periodic cell"));
    }
    let w = session.model.waveguide.as_ref().ok_or_else(|| EngineError::failed("no waveguide"))?;
    if !w.uniform() {
        return Err(EngineError::failed(
            "not supported yet: exact fields of a guide whose aperture changes (the S-parameters are compared with mode matching)",
        ));
    }
    let mut sections = Vec::with_capacity(w.sections.len());
    for s in &w.sections {
        let (_, m) = session.model.material(&s.material)?;
        sections.push((s.length.get(), m.clone()));
    }
    Ok(ExactLine { a: w.width.get(), sections })
}

/// Mode matching takes constant permittivities: refuses a dispersive material.
fn constant(m: &Material) -> Result<&Material> {
    if m.is_magnetically_lossy() {
        return Err(EngineError::failed(format!(
            "not supported yet: a mode-matching reference for the magnetically lossy material `{}` (only uniform guides have one)",
            m.name
        )));
    }
    if m.is_dispersive() {
        return Err(EngineError::failed(format!(
            "not supported yet: a mode-matching reference for the dispersive material `{}` (only uniform guides have one)",
            m.name
        )));
    }
    Ok(m)
}

/// x'₁₁, the first zero of J₁' (Abramowitz & Stegun, Table 9.5): the circular guide's TE11
/// cut-off is x'₁₁/r.
pub(crate) const X11_PRIME: f64 = 1.841_183_781_340_659;

/// TE_m0 modes in the widest section when the reference is mode matching: converged to about
/// 10⁻⁵ in S for irises and steps of a few millimetres.
pub const MODE_MATCHING_MODES: usize = 160;

/// What a model's S-parameters are compared with: the exact TE₁₀ cascade when every section
/// has the full cross-section, mode matching when the aperture changes (H-plane steps, irises).
#[derive(Clone, Debug)]
pub enum Reference {
    Exact(ExactLine),
    ModeMatching(HPlaneLine),
    EPlane(EPlaneLine),
    /// A uniform guide `length` long whose first mode has cut-off wavenumber `cutoff` (0 for
    /// TEM) in a fill of εr μr = `index2`: S11 = 0, S21 = e^{−jβL}.
    Uniform {
        length: f64,
        cutoff: f64,
        index2: f64,
    },
    /// A periodic cell of plane layers under a plane wave (`crate::periodic`).
    Stack(emcraft_theory::floquet::Stack),
}

impl Reference {
    pub fn label(&self) -> &'static str {
        match self {
            Reference::Exact(_) | Reference::Uniform { .. } => "exact",
            Reference::ModeMatching(_) | Reference::EPlane(_) => "mode matching",
            Reference::Stack(_) => "exact (plane wave)",
        }
    }

    /// Whether the reference is mode matching: dense linear algebra per frequency, its accuracy
    /// set by the modes it takes.
    pub fn is_mode_matching(&self) -> bool {
        matches!(self, Reference::ModeMatching(_) | Reference::EPlane(_))
    }

    /// (S11, S21), power waves, with `modes` TE_m0 modes for mode matching.
    pub fn s(&self, frequency: f64, modes: usize) -> Result<(C, C)> {
        match self {
            Reference::Exact(l) => Ok(l.at(frequency).solve(frequency)?.s_power()),
            Reference::ModeMatching(l) => Ok(l.solve(frequency, modes)?),
            Reference::EPlane(l) => Ok(l.solve(frequency, modes)?),
            Reference::Stack(st) => Ok(st.s(frequency)?),
            Reference::Uniform { length, cutoff, index2 } => {
                let k = 2.0 * std::f64::consts::PI * frequency / emcraft_model::units::C0;
                let b2 = k * k * index2 - cutoff * cutoff;
                if !(b2 > 0.0) {
                    return Err(EngineError::failed(format!("{frequency} Hz is below the guide's cut-off")));
                }
                Ok((C::new(0.0, 0.0), C::from_polar(1.0, -b2.sqrt() * length)))
            }
        }
    }
}

/// The reference the model's S-parameters are compared with, or why it has none.
pub fn reference(session: &Session) -> Result<Reference> {
    if session.model.periodic.is_some() {
        return crate::periodic::stack_reference(session).map(Reference::Stack);
    }
    if session.model.waveguide.is_none()
        && let Some(solid) = session.model.solid.as_ref()
    {
        return body_reference(session, solid);
    }
    let w = session.model.waveguide.as_ref().ok_or_else(|| EngineError::failed("no waveguide"))?;
    if !w.plain() {
        return Err(EngineError::failed("not supported yet: a reference for a layered guide or one with strips"));
    }
    if w.uniform() {
        return Ok(Reference::Exact(te10_line(session)?));
    }
    if w.full_width() {
        let mut sections = Vec::with_capacity(w.sections.len());
        for (i, s) in w.sections.iter().enumerate() {
            let m = constant(session.model.material(&s.material)?.1)?;
            let emcraft_model::Span { start: y0, width: height } = w.aperture_y(i).unwrap_or(w.across_y());
            sections.push(ESection {
                length: s.length.get(),
                eps_r: m.eps_r.get(),
                mu_r: m.mu_r.get(),
                loss_tangent: m.loss_tangent.get(),
                y0,
                height,
            });
        }
        return Ok(Reference::EPlane(EPlaneLine { a: w.width.get(), sections }));
    }
    if !w.full_height() {
        return Err(EngineError::failed("not supported yet: a reference for guides whose width and height both change"));
    }
    let mut sections = Vec::with_capacity(w.sections.len());
    for (i, s) in w.sections.iter().enumerate() {
        let m = constant(session.model.material(&s.material)?.1)?;
        let emcraft_model::Span { start: x0, width } = w.aperture(i).unwrap_or(w.across_x());
        sections.push(HSection { length: s.length.get(), eps_r: m.eps_r.get(), mu_r: m.mu_r.get(), x0, width, loss_tangent: m.loss_tangent.get() });
    }
    Ok(Reference::ModeMatching(HPlaneLine { sections }))
}

/// The reference of a body between two ports: an empty box, circular guide or coaxial line.
fn body_reference(session: &Session, solid: &emcraft_model::Solid) -> Result<Reference> {
    let (_, m) = session.model.material(&solid.material)?;
    let plain = session.model.inclusions.is_empty() && !m.is_lossy();
    let index2 = m.eps_r.get() * m.mu_r.get();
    match (&solid.shape, &session.model.features[..]) {
        // A box between two ports is an empty guide.
        (emcraft_model::Shape::Box { size }, []) if session.model.inclusions.is_empty() => {
            Ok(Reference::Exact(ExactLine { a: size[0], sections: vec![(size[2], m.clone())] }))
        }
        // An empty circular guide: TE11, cut off at x'₁₁/r (the first zero of J₁').
        (emcraft_model::Shape::Cylinder { radius, length }, []) if plain => {
            Ok(Reference::Uniform { length: length.get(), cutoff: X11_PRIME / radius.get(), index2 })
        }
        // A coaxial line: a cylinder through the whole length on the axis, subtracted: TEM.
        (emcraft_model::Shape::Cylinder { radius, length }, [f])
            if plain
                && f.combine == emcraft_model::Combine::Subtract
                && f.axis == emcraft_model::Axis::Z
                && f.at.x == 0.0
                && f.at.y == 0.0
                && f.at.z <= 0.0
                && matches!(f.shape, emcraft_model::Shape::Cylinder { radius: r, length: l } if r < *radius && f.at.z + l >= length.get()) =>
        {
            Ok(Reference::Uniform { length: length.get(), cutoff: 0.0, index2 })
        }
        _ => Err(EngineError::failed("not supported yet: a reference for this body's ports")),
    }
}

/// The TE10 cut-offs (Hz) of the waveguide's sections, each once, with what tells them apart:
/// the section's material, or its width where it is narrowed.
pub fn section_cutoffs(session: &Session) -> Vec<(f64, String)> {
    let mut out: Vec<(f64, String)> = Vec::new();
    let Some(w) = session.model.waveguide.as_ref() else { return out };
    // A periodic cell's walls are not walls: no guide cut-offs.
    if session.model.periodic.is_some() {
        return out;
    }
    let lu = session.display.length;
    for (i, sec) in w.sections.iter().enumerate() {
        let (Ok((_, m)), Some(emcraft_model::Span { width, .. })) = (session.model.material(&sec.material), w.aperture(i)) else { continue };
        let fc = te10_cutoff(width, m.eps_r.get(), m.mu_r.get());
        if !out.iter().any(|(x, _)| (x - fc).abs() < 1e-6 * fc) {
            let name = if (width - w.width).abs() <= 1e-9 * w.width {
                sec.material.clone()
            } else {
                format!("{} {}", emcraft_render::axes::format(lu.from_si(width)), lu.symbol())
            };
            out.push((fc, name));
        }
    }
    out
}
