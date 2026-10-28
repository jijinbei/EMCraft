//! The ports' conditions at one frequency: each mode's rank-one term, its excitation and how
//! its coefficient is read from a field.

use std::borrow::Cow;
use std::f64::consts::PI;

use emcraft_model::units::{C0, MU0};
use num_complex::Complex64 as C;

use super::assembly::port_rule_points;
use super::ports::{NumericPort, PortModes};
use super::{Driven, Result, invalid, wavenumber};

/// One mode's part of a port condition at one frequency: the matrix gains factor·v vᵀ, a unit
/// incident wave drives rhs·v, and the mode's coefficient in a field x is vᵀx/norm.
#[derive(Clone, Debug)]
pub struct PortTerm<'a> {
    pub name: String,
    pub v: Cow<'a, [(usize, f64)]>,
    pub factor: C,
    pub rhs: C,
    pub norm: f64,
    /// 2ωμ₀ times the power a unit wave of the mode carries (0 for an evanescent one).
    pub proxy: f64,
    pub propagating: bool,
    /// The line's characteristic impedance (Ω) for a port's first mode, where defined.
    pub impedance: Option<f64>,
    /// The mode's propagation constant γ = α + jβ (1/m): it goes as e^{−γz} along the line.
    pub gamma: C,
}

impl PortTerm<'_> {
    /// The mode's coefficient c = vᵀx/norm in the field coefficients `x`.
    pub fn coefficient(&self, x: &[C]) -> C {
        self.v.iter().map(|&(i, vi)| x[i] * vi).sum::<C>() / self.norm
    }

    /// The right-hand side of a unit incident wave of the mode: (unknown, rhs·vᵢ).
    pub(crate) fn excitation(&self) -> impl Iterator<Item = (usize, C)> + '_ {
        self.v.iter().map(|&(i, vi)| (i, self.rhs * vi))
    }
}

impl Driven {
    /// Every port's terms at `frequency`, refusing it where a port's first mode does not
    /// propagate.
    pub(crate) fn checked_terms(&self, frequency: f64) -> Result<Vec<Vec<PortTerm<'_>>>> {
        let k0 = wavenumber(frequency);
        let mut terms = Vec::with_capacity(self.ports.len());
        for p in 0..self.ports.len() {
            // (A closed-form mode is refused before its κ, singular at the cut-off.)
            let below = self.below_cutoff(p, k0);
            let list = if below { Vec::new() } else { self.port_terms(p, frequency)? };
            if below || !list.first().is_some_and(|t| t.propagating) {
                return Err(invalid(
                    "frequency",
                    format!(
                        "{frequency} Hz is below the cut-off of the first mode at port `{}`: no wave can carry power into the port",
                        self.ports[p].name
                    ),
                ));
            }
            terms.push(list);
        }
        Ok(terms)
    }

    /// The power (W) a unit TE₁₀ wave carries into port `p`: ½ ‖e‖² β/(ωμ).
    pub fn incident_power(&self, p: usize, frequency: f64) -> Option<f64> {
        let port = self.ports.get(p)?;
        let omega = 2.0 * PI * frequency;
        match &port.modes {
            PortModes::Analytic { eps_r, mu_r, modes, fraction } => {
                let (mode, _) = modes.first()?;
                let k = omega / C0 * (eps_r * mu_r).sqrt();
                Some(0.5 * mode.norm() * fraction * mode.gamma(k).1 / (omega * MU0 * mu_r))
            }
            PortModes::Numeric(_) => Some(0.5 * self.port_terms(p, frequency).ok()?.first()?.proxy / (omega * MU0)),
            PortModes::Floquet(fp) => {
                let periodic = self.periodic.as_ref()?;
                let (kx, ky) = super::floquet::incidence(periodic, fp, frequency);
                let terms = self.floquet_terms(frequency, kx, ky).ok()?;
                Some(0.5 * terms.get(p)?.first()?.proxy / (omega * MU0))
            }
        }
    }

    /// Whether a closed-form port's first mode is at or below its cut-off at k₀ (numerical ports
    /// tell by their terms).
    fn below_cutoff(&self, p: usize, k0: f64) -> bool {
        match self.ports.get(p).map(|p| &p.modes) {
            Some(PortModes::Analytic { eps_r, mu_r, modes, .. }) => modes.first().is_none_or(|(m, _)| k0 * (eps_r * mu_r).sqrt() <= m.cutoff()),
            _ => false,
        }
    }

    /// Port `p`'s modes as terms of its condition at `frequency` (the first is the one excited and
    /// read). Closed forms: n̂ × ∇ × E_t = κ E_t, factor κ/(μr‖e‖²); numerical modes: H_t =
    /// ŝ × g β/(ωμ), factor jβ/D, with v_i = ∫ μr⁻¹ Nᵢ · g dS and D = ∫ μr⁻¹ e · g dS
    /// ([`crate::portmode`]).
    pub fn port_terms(&self, p: usize, frequency: f64) -> Result<Vec<PortTerm<'_>>> {
        let port = self.ports.get(p).ok_or_else(|| invalid("port", format!("no port {p}")))?;
        let k0 = wavenumber(frequency);
        match &port.modes {
            PortModes::Analytic { eps_r, mu_r, modes, fraction } => {
                let k = k0 * (eps_r * mu_r).sqrt();
                modes
                    .iter()
                    .map(|(mode, v)| {
                        let norm = mode.norm() * fraction;
                        let (kr, ki) = mode.kappa(k).ok_or_else(|| {
                            invalid("frequency", format!("{} Hz is at the cut-off of mode {} at port `{}`", frequency, mode.name(), port.name))
                        })?;
                        let kappa = C::new(kr, ki) / mu_r;
                        let gamma = mode.gamma(k);
                        Ok(PortTerm {
                            name: mode.name(),
                            v: Cow::Borrowed(v.as_slice()),
                            factor: kappa / norm,
                            rhs: 2.0 * kappa,
                            norm,
                            proxy: norm * gamma.1 / mu_r,
                            propagating: gamma.1 > 0.0,
                            impedance: None,
                            gamma: C::new(gamma.0, gamma.1),
                        })
                    })
                    .collect()
            }
            PortModes::Numeric(n) => self.numeric_terms(n, k0),
            PortModes::Floquet(_) => Err(invalid(
                format!("port `{}`", port.name),
                "a Floquet port's modes are complex: its terms come with the periodic solve (not supported yet: the fast sweep and the symmetric solver paths)",
            )),
        }
    }

    fn numeric_terms(&self, n: &NumericPort, k0: f64) -> Result<Vec<PortTerm<'static>>> {
        let modes = n.section.modes_split(k0, n.count)?;
        let index: std::collections::HashMap<usize, usize> = n.support.iter().enumerate().map(|(k, &u)| (u, k)).collect();
        let mut v = vec![vec![0.0; n.support.len()]; modes.len()];
        let space = self.space();
        for (t, &f) in n.faces.iter().enumerate() {
            let b = self.mesh.boundary.get(f).ok_or_else(|| invalid("port", "a face is missing from the mesh"))?;
            let face = space.face_rule(b, port_rule_points(self.order))?;
            let inv_mu = 1.0 / n.section.mu_of(t);
            for (l, wa, _) in face.points {
                let x = face.el.point(l);
                let values = face.el.values(l);
                let gs = modes.iter().map(|m| n.section.fields(m, t, x).map(|(_, g)| g)).collect::<Result<Vec<_>>>()?;
                for (u, val) in face.unknowns.iter().zip(&values) {
                    let Some(&k) = index.get(&(*u as usize)) else { continue };
                    for (vm, g) in v.iter_mut().zip(&gs) {
                        vm[k] += wa * inv_mu * val.dot(*g);
                    }
                }
            }
        }
        let impedance = match modes.first() {
            Some(m) => n.section.impedance_whole(m, k0, &self.symmetry)?,
            None => None,
        };
        Ok(modes
            .iter()
            .zip(v)
            .enumerate()
            .map(|(i, (m, vm))| {
                let beta = m.beta();
                let jb = C::new(0.0, 1.0) * beta;
                PortTerm {
                    impedance: if i == 0 { impedance } else { None },
                    name: format!("mode {}", i + 1),
                    v: Cow::Owned(n.support.iter().copied().zip(vm).collect()),
                    factor: jb / m.d,
                    rhs: 2.0 * jb,
                    norm: m.d,
                    proxy: if m.beta2 > 0.0 { beta.re * m.d } else { 0.0 },
                    propagating: m.beta2 > 0.0,
                    gamma: jb,
                }
            })
            .collect())
    }
}
