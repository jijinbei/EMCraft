//! Dispersive and lossy E edges by the auxiliary differential equation method (Taflove &
//! Hagness 2005, §9.4): each pole's polarization P on the edges it touches, advanced with E
//! (the crate's documentation gives the time centring of each kind of pole); a conductivity σ
//! as the time-averaged conduction current σ (Eⁿ⁺¹ + Eⁿ)/2 with ε scaled to make the update
//! exponential (Taflove & Hagness §3.12; [`exponential_fit`]); and a wall's surface admittance
//! as first-order branches of surface current ([`crate::surface`]).

use std::ops::Range;

use emcraft_model::units::EPS0;
use emcraft_model::{Pole, SecondOrder};

use crate::yee::Field;

/// One pole's update on one edge (its weight on the edge folded in).
#[derive(Clone, Copy, Debug)]
pub(crate) enum Term {
    /// Pⁿ⁺¹ = ka Pⁿ + kb (Eⁿ⁺¹ + Eⁿ).
    Debye { ka: f64, kb: f64 },
    /// Pⁿ⁺¹ = c1 Pⁿ − c2 Pⁿ⁻¹ + c3 Eⁿ.
    Second { c1: f64, c2: f64, c3: f64 },
    /// A branch of surface current, entering Ampère's law as −(Qⁿ⁺¹ + Qⁿ) (a current averaged
    /// over the step, not a polarization's change): Qⁿ⁺¹ = ka Qⁿ + kb (Eⁿ⁺¹ + Eⁿ).
    Surface { ka: f64, kb: f64 },
}

impl Term {
    /// The pole's update for a time step `dt`, weighted by `w` (the share of the edge's area in
    /// its cells).
    pub(crate) fn new(pole: Pole, w: f64, dt: f64) -> Term {
        match (pole, pole.second_order()) {
            (Pole::Debye { delta_eps, tau }, _) => {
                let (delta_eps, tau) = (delta_eps.get(), tau.get());
                Term::Debye { ka: (2.0 * tau - dt) / (2.0 * tau + dt), kb: w * EPS0 * delta_eps * dt / (2.0 * tau + dt) }
            }
            (_, Some(SecondOrder { strength: a, omega0: w0, gamma: g })) => {
                let d = 1.0 + 0.5 * g * dt;
                Term::Second { c1: (2.0 - w0 * w0 * dt * dt) / d, c2: (1.0 - 0.5 * g * dt) / d, c3: w * dt * dt * EPS0 * a / d }
            }
            _ => Term::Second { c1: 0.0, c2: 0.0, c3: 0.0 },
        }
    }
}

/// A dispersive or lossy edge: ε₀ε∞ (the edge's average), σΔt/2 (its conductivity's) and its
/// terms.
struct Slot {
    eps: f64,
    loss: f64,
    terms: Range<usize>,
}

/// The dispersive or lossy E edges of one component: each edge's slot, and per slot its terms,
/// with their states now and a step before.
pub(crate) struct Dispersion {
    slot_of: Vec<u32>,
    slots: Vec<Slot>,
    terms: Vec<Term>,
    p: Field,
    p_prev: Field,
}

/// An edge without a slot in `slot_of`.
pub const NO_SLOT: u32 = u32::MAX;

/// The dispersive or lossy E edges of the three components as plain arrays, for a device: each
/// edge's slot, each slot's ε₀ε∞, σΔt/2 and terms, each term's kind and coefficients. The update
/// of an edge is [`Dispersion::step`]'s.
#[derive(Clone, Debug, PartialEq)]
pub struct Poles {
    /// Per component, each edge's slot ([`NO_SLOT`] if it has none).
    pub slot: [Vec<u32>; 3],
    /// Per slot: ε₀ε∞ and σΔt/2.
    pub eps: Vec<f64>,
    pub loss: Vec<f64>,
    /// Slot s has the terms `start[s]..start[s + 1]`.
    pub start: Vec<u32>,
    /// Per term: 0 a Debye pole, k = (ka, kb, 0); 1 a second-order pole, k = (c1, c2, c3); 2 a
    /// branch of surface current, k = (ka, kb, 0).
    pub kind: Vec<u32>,
    pub k: Vec<[f64; 3]>,
}

impl Poles {
    /// Whether the slots fit lattices of `lens` edges per component, and the terms the slots.
    pub fn fit(&self, lens: [usize; 3]) -> bool {
        let slots = self.eps.len();
        let terms = self.kind.len();
        self.slot.iter().zip(lens).all(|(s, l)| s.len() == l && s.iter().all(|&x| x == NO_SLOT || (x as usize) < slots))
            && self.loss.len() == slots
            && self.start.len() == slots + 1
            && self.start.iter().all(|&t| t as usize <= terms)
            && self.start.windows(2).all(|w| w[0] <= w[1])
            && self.k.len() == terms
    }

    /// The three components' slotted edges (`edges` per component), `None` if there are none.
    pub(crate) fn of(d: &[Dispersion; 3], edges: [usize; 3]) -> Option<Poles> {
        if !d.iter().any(Dispersion::any) {
            return None;
        }
        let mut poles =
            Poles { slot: [Vec::new(), Vec::new(), Vec::new()], eps: Vec::new(), loss: Vec::new(), start: vec![0], kind: Vec::new(), k: Vec::new() };
        for (a, d) in d.iter().enumerate() {
            let base = poles.eps.len() as u32;
            poles.slot[a] = if d.slot_of.is_empty() {
                vec![NO_SLOT; edges[a]]
            } else {
                d.slot_of.iter().map(|&s| if s == NO_SLOT { NO_SLOT } else { s + base }).collect()
            };
            for slot in &d.slots {
                poles.eps.push(slot.eps);
                poles.loss.push(slot.loss);
                for t in slot.terms.clone() {
                    let (kind, k) = match d.terms[t] {
                        Term::Debye { ka, kb } => (0, [ka, kb, 0.0]),
                        Term::Second { c1, c2, c3 } => (1, [c1, c2, c3]),
                        Term::Surface { ka, kb } => (2, [ka, kb, 0.0]),
                    };
                    poles.kind.push(kind);
                    poles.k.push(k);
                }
                poles.start.push(poles.kind.len() as u32);
            }
        }
        Some(poles)
    }
}

impl Dispersion {
    /// No slotted edges.
    pub(crate) fn none() -> Dispersion {
        Dispersion { slot_of: Vec::new(), slots: Vec::new(), terms: Vec::new(), p: Field::new([0; 3]), p_prev: Field::new([0; 3]) }
    }

    /// Gives edge `id` of a component of `edges` edges a slot: ε₀ε∞ `eps`, σΔt/2 `loss` and
    /// `terms`.
    pub(crate) fn add_edge(&mut self, edges: usize, id: usize, eps: f64, loss: f64, terms: impl IntoIterator<Item = Term>) {
        if self.slot_of.is_empty() {
            self.slot_of = vec![NO_SLOT; edges];
        }
        let start = self.terms.len();
        self.terms.extend(terms);
        if let Some(s) = self.slot_of.get_mut(id) {
            *s = self.slots.len() as u32;
        }
        self.slots.push(Slot { eps, loss, terms: start..self.terms.len() });
    }

    /// Whether any edge has a slot.
    pub(crate) fn any(&self) -> bool {
        !self.slots.is_empty()
    }

    /// Whether edge `id` has a slot (is dispersive or lossy).
    pub(crate) fn has(&self, id: usize) -> bool {
        self.slot_of.get(id).is_some_and(|&s| s != NO_SLOT)
    }

    /// The change of Eⁿ⁺¹ on edge `id` per unit of Δt ∇×H (1/(ε₀ε∞ + σΔt/2 + Σ kb) of its
    /// solve); `None` for an edge without a slot.
    pub(crate) fn gain(&self, id: usize) -> Option<f64> {
        let slot = *self.slot_of.get(id)?;
        let Slot { eps, loss, terms } = self.slots.get(slot as usize)?;
        let kb: f64 = terms.clone().map(|t| if let Term::Debye { kb, .. } | Term::Surface { kb, .. } = self.terms[t] { kb } else { 0.0 }).sum();
        Some(1.0 / (eps + loss + kb))
    }

    /// Adds `delta` to Eⁿ⁺¹ on edge `id` after its step, as a current impressed in that step
    /// would have (Δt J standing in Ampère's law beside Δt ∇×H, `delta` = −Δt J × [`Self::gain`]):
    /// the terms that took Eⁿ⁺¹ follow it. Returns whether the edge has a slot.
    pub(crate) fn inject(&self, e: &Field, id: usize, delta: f64) -> bool {
        let Some(Slot { terms, .. }) = self.slot_of.get(id).and_then(|&s| self.slots.get(s as usize)) else {
            return false;
        };
        e.set(id, e.get(id) + delta);
        for t in terms.clone() {
            if let Term::Debye { kb, .. } | Term::Surface { kb, .. } = self.terms[t] {
                self.p.set(t, self.p.get(t) + kb * delta);
            }
        }
        true
    }

    /// Allocates the terms' states, once every edge is added.
    pub(crate) fn finish(&mut self) {
        self.p = Field::new([self.terms.len(), 1, 1]);
        self.p_prev = Field::new([self.terms.len(), 1, 1]);
    }

    /// The new E on edge `id` from E now and the bracketed curl term (∇ × H less the CPML's),
    /// advancing the edge's terms; `None` for an edge without a slot.
    ///
    /// ε₀ε∞ (Eⁿ⁺¹ − Eⁿ) + σΔt (Eⁿ⁺¹ + Eⁿ)/2 = Δt ∇×H − Σ (Pⁿ⁺¹ − Pⁿ) − Σ (Qⁿ⁺¹ + Qⁿ), P the
    /// poles' polarizations and Q the surface branches': each linear in Eⁿ⁺¹, solved per edge.
    #[inline]
    pub(crate) fn step(&self, id: usize, e: f64, curl: f64, dt: f64) -> Option<f64> {
        let slot = *self.slot_of.get(id)?;
        let Slot { eps, loss, terms } = self.slots.get(slot as usize)?;
        let (mut kb_sum, mut rhs) = (*loss, dt * curl);
        for t in terms.clone() {
            let p = self.p.get(t);
            match self.terms[t] {
                Term::Debye { ka, kb } => {
                    kb_sum += kb;
                    rhs -= (ka - 1.0) * p;
                }
                Term::Surface { ka, kb } => {
                    kb_sum += kb;
                    rhs -= (ka + 1.0) * p;
                }
                Term::Second { c1, c2, c3 } => {
                    let next = c1 * p - c2 * self.p_prev.get(t) + c3 * e;
                    rhs -= next - p;
                    self.p_prev.set(t, p);
                    self.p.set(t, next);
                }
            }
        }
        let e_new = ((eps - kb_sum) * e + rhs) / (eps + kb_sum);
        for t in terms.clone() {
            if let Term::Debye { ka, kb } | Term::Surface { ka, kb } = self.terms[t] {
                self.p.set(t, ka * self.p.get(t) + kb * (e_new + e));
            }
        }
        Some(e_new)
    }
}

/// A second-order pole is explicit in time: stable while its frequencies stay well below 1/Δt
/// (ω₀Δt < 2 and the plasma term likewise; refused well before).
pub(crate) fn check_poles<'a>(poles: impl Iterator<Item = &'a Pole>, dt: f64) -> crate::Result<()> {
    for pole in poles {
        if let Some(SecondOrder { strength: a, omega0: w0, .. }) = pole.second_order()
            && !(w0 * dt < 0.5 && a.sqrt() * dt < 0.5)
        {
            return Err(crate::FdtdError::Unsupported(format!("a pole too fast for the time step ({dt:e} s): {pole:?}")));
        }
    }
    Ok(())
}

/// The factor on ε that turns the semi-implicit lossy update into the exponential one: with
/// r = σΔt/ε, (ε − σΔt/2)/(ε + σΔt/2) becomes e^{−r} and Δt/(ε + σΔt/2) becomes (1 − e^{−r})/σ
/// when ε is scaled by (r/2) coth(r/2) = 1 + r²/12 + … (Taflove & Hagness §3.12): the same to
/// second order for small r, and for large r the field in a good conductor decays at once
/// instead of flipping sign every step.
pub(crate) fn exponential_fit(r: f64) -> f64 {
    if r < 1e-4 { 1.0 + r * r / 12.0 } else { 0.5 * r / (0.5 * r).tanh() }
}

/// The exponential update of H with magnetic conductivity σ_m (Ω/m): (dt/μ times the update's
/// gain, the decay), H ← decay H − c ∇×E; (dt/μ, 1) without loss.
pub(crate) fn magnetic_update(dt: f64, mu: f64, sigma_m: f64) -> (f64, f64) {
    let r = sigma_m * dt / mu;
    if r < 1e-12 {
        return (dt / mu, 1.0);
    }
    // (1 − e^{−r})/r without cancellation.
    (dt / mu * -(-r).exp_m1() / r, (-r).exp())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_exponential_fit_matches_the_exponential_update() {
        for r in [0.0, 1e-6, 1e-3, 0.5, 2.0, 40.0, 1e8] {
            let (eps, loss) = (exponential_fit(r), 0.5 * r);
            let (ca, cb) = ((eps - loss) / (eps + loss), 1.0 / (eps + loss));
            let exact_cb = if r == 0.0 { 1.0 } else { -(-r).exp_m1() / r };
            assert!((ca - (-r).exp()).abs() < 1e-12 && (cb - exact_cb).abs() < 1e-12 * exact_cb.max(1e-300), "{r}: {ca} {cb}");
        }
        let (c, d) = magnetic_update(1.0, 2.0, 0.0);
        assert_eq!((c, d), (0.5, 1.0));
        let (c, d) = magnetic_update(1.0, 1.0, 1e9);
        assert!(d == 0.0 && (c - 1e-9).abs() < 1e-21);
    }
}
