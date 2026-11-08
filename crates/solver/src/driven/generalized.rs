//! The generalized scattering matrix of a driven model: every mode of every waveguide port
//! excited in turn (a unit wave of it in, all else matched) and read on every mode, each mode's
//! amplitude its own coefficient (unnormalized, as the port condition expands the field). The
//! driven solve's S is its first modes' block, in power waves; a periodic cell's Bloch wave
//! needs the whole matrix with the evanescent modes that carry the coupling between cells
//! (T. Itoh (ed.), *Numerical Techniques for Microwave and Millimeter-Wave Passive Structures*,
//! Wiley 1989, ch. 9; the body of revolution's [`crate::bor::generalized`]).

use num_complex::Complex64 as C;

use super::assembly::System;
use super::sparams::ModeTerm;
use super::{Driven, Result, check_frequency, invalid, solution};

/// The generalized scattering matrix at a frequency: the (port, mode) of each row and column,
/// the matrix row-major, each mode's propagation constant γ (1/m) and the power a unit
/// amplitude of it carries (2ωμ₀ times it, the real part of [`ModeTerm::proxy`]; 0 for an
/// evanescent mode).
#[derive(Clone, Debug, PartialEq)]
pub struct Generalized {
    pub modes: Vec<(usize, usize)>,
    pub s: Vec<C>,
    pub gamma: Vec<C>,
    pub power: Vec<f64>,
}

/// The most modes the generalized matrix takes in all.
pub const MAX_GENERALIZED_MODES: usize = 64;

impl Driven {
    /// The generalized scattering matrix at `frequency` (Hz): every mode of every port excited
    /// and read. Waveguide ports only, not a periodic cell.
    pub fn generalized(&self, frequency: f64) -> Result<Generalized> {
        check_frequency(frequency)?;
        if self.tie.is_some() || self.ports.is_empty() || !self.lumped.is_empty() || self.has_terminals() {
            return Err(invalid("generalized scattering matrix", "needs waveguide ports alone (no lumped or terminal ports, not a periodic cell)"));
        }
        let System { a, terms, border, .. } = self.system(frequency)?;
        let modes: Vec<(usize, usize)> = terms.iter().enumerate().flat_map(|(p, l)| (0..l.len()).map(move |m| (p, m))).collect();
        if modes.len() > MAX_GENERALIZED_MODES {
            return Err(invalid("generalized scattering matrix", format!("at most {MAX_GENERALIZED_MODES} modes in all")));
        }
        let n = self.dofs.unknowns;
        let rhs: Vec<Vec<C>> = modes
            .iter()
            .map(|&(p, m)| {
                let mut b = vec![C::new(0.0, 0.0); n + border];
                if let Some(t) = terms.get(p).and_then(|l| l.get(m)) {
                    for (i, bi) in t.excitation() {
                        b[i] = bi;
                    }
                }
                b
            })
            .collect();
        let system = System { a, rhs, terms, border };
        let f = self.factor(&system)?;
        let solved = solution::solve_each(&f, &system.a, &system.rhs, n, 0.0)?;
        let term = |k: usize| modes.get(k).and_then(|&(p, m)| system.terms.get(p)?.get(m));
        let w = modes.len();
        let mut s = vec![C::new(0.0, 0.0); w * w];
        for (q, x) in solved.fields.iter().enumerate() {
            for p in 0..w {
                if let Some(t) = term(p) {
                    s[p * w + q] = t.coefficient(x) - if p == q { 1.0 } else { 0.0 };
                }
            }
        }
        let gamma = (0..w).map(|k| term(k).map_or(C::new(0.0, 0.0), |t| t.gamma)).collect();
        let power = (0..w).map(|k| term(k).map_or(0.0, |t| t.proxy().re)).collect();
        Ok(Generalized { modes, s, gamma, power })
    }
}
