//! Dispersive materials on the E edges by the auxiliary differential equation method (Taflove
//! & Hagness 2005, §9.4): each pole's polarization P on the edges it touches, advanced with E
//! (the crate's documentation gives the time centring of each kind of pole).

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
}

impl Term {
    /// The pole's update for a time step `dt`, weighted by `w` (the share of the edge's area in
    /// its cells).
    pub(crate) fn new(pole: Pole, w: f64, dt: f64) -> Term {
        match (pole, pole.second_order()) {
            (Pole::Debye { delta_eps, tau }, _) => {
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

/// A dispersive edge: ε₀ε∞ (the edge's average) and its terms.
struct Slot {
    eps: f64,
    terms: Range<usize>,
}

/// The dispersive E edges of one component: each edge's slot, and per slot its terms, with
/// their polarizations now and a step before.
pub(crate) struct Dispersion {
    slot_of: Vec<u32>,
    slots: Vec<Slot>,
    terms: Vec<Term>,
    p: Field,
    p_prev: Field,
}

/// An edge without dispersion in `slot_of`.
pub const NO_SLOT: u32 = u32::MAX;

/// The dispersive E edges of the three components as plain arrays, for a device: each edge's
/// slot, each slot's ε₀ε∞ and terms, each term's kind and coefficients. The update of an edge is
/// [`Dispersion::step`]'s.
#[derive(Clone, Debug, PartialEq)]
pub struct Poles {
    /// Per component, each edge's slot ([`NO_SLOT`] if it is not dispersive).
    pub slot: [Vec<u32>; 3],
    /// Per slot: ε₀ε∞.
    pub eps: Vec<f64>,
    /// Slot s has the terms `start[s]..start[s + 1]`.
    pub start: Vec<u32>,
    /// Per term: 0 a Debye pole, k = (ka, kb, 0); 1 a second-order pole, k = (c1, c2, c3).
    pub kind: Vec<u32>,
    pub k: Vec<[f64; 3]>,
}

impl Poles {
    /// The three components' dispersive edges (`edges` per component), `None` if there are none.
    pub(crate) fn of(d: &[Dispersion; 3], edges: [usize; 3]) -> Option<Poles> {
        if !d.iter().any(Dispersion::any) {
            return None;
        }
        let mut poles = Poles { slot: [Vec::new(), Vec::new(), Vec::new()], eps: Vec::new(), start: vec![0], kind: Vec::new(), k: Vec::new() };
        for (a, d) in d.iter().enumerate() {
            let base = poles.eps.len() as u32;
            poles.slot[a] = if d.slot_of.is_empty() {
                vec![NO_SLOT; edges[a]]
            } else {
                d.slot_of.iter().map(|&s| if s == NO_SLOT { NO_SLOT } else { s + base }).collect()
            };
            for slot in &d.slots {
                poles.eps.push(slot.eps);
                for t in slot.terms.clone() {
                    let (kind, k) = match d.terms[t] {
                        Term::Debye { ka, kb } => (0, [ka, kb, 0.0]),
                        Term::Second { c1, c2, c3 } => (1, [c1, c2, c3]),
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
    /// No dispersive edges.
    pub(crate) fn none() -> Dispersion {
        Dispersion { slot_of: Vec::new(), slots: Vec::new(), terms: Vec::new(), p: Field::new([0; 3]), p_prev: Field::new([0; 3]) }
    }

    /// Makes edge `id` of a component of `edges` edges dispersive, with ε₀ε∞ `eps` and `terms`.
    pub(crate) fn add_edge(&mut self, edges: usize, id: usize, eps: f64, terms: impl IntoIterator<Item = Term>) {
        if self.slot_of.is_empty() {
            self.slot_of = vec![NO_SLOT; edges];
        }
        let start = self.terms.len();
        self.terms.extend(terms);
        if let Some(s) = self.slot_of.get_mut(id) {
            *s = self.slots.len() as u32;
        }
        self.slots.push(Slot { eps, terms: start..self.terms.len() });
    }

    /// Whether any edge is dispersive.
    pub(crate) fn any(&self) -> bool {
        !self.slots.is_empty()
    }

    /// Allocates the polarizations, once every edge is added.
    pub(crate) fn finish(&mut self) {
        self.p = Field::new([self.terms.len(), 1, 1]);
        self.p_prev = Field::new([self.terms.len(), 1, 1]);
    }

    /// The new E on edge `id` from E now and the bracketed curl term (∇ × H less the CPML's),
    /// advancing the edge's polarizations; `None` for an edge without dispersion.
    #[inline]
    pub(crate) fn step(&self, id: usize, e: f64, curl: f64, dt: f64) -> Option<f64> {
        let slot = *self.slot_of.get(id)?;
        let Slot { eps, terms } = self.slots.get(slot as usize)?;
        let (mut kb_sum, mut rhs) = (0.0, dt * curl);
        for t in terms.clone() {
            let p = self.p.get(t);
            match self.terms[t] {
                Term::Debye { ka, kb } => {
                    kb_sum += kb;
                    rhs -= (ka - 1.0) * p;
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
            if let Term::Debye { ka, kb } = self.terms[t] {
                self.p.set(t, ka * self.p.get(t) + kb * (e_new + e));
            }
        }
        Some(e_new)
    }
}
