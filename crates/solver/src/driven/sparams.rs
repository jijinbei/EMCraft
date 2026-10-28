//! S-parameters read from the solutions of each port's excitation, referred to the ports'
//! impedances and moved to their reference planes. The full solve and the fast sweep
//! ([`crate::sweep`]) read S the same way.

use num_complex::Complex64 as C;

use super::terms::PortTerm;
use super::{Driven, Result, invalid};
use crate::smatrix::SMatrix;

/// S read from the solutions of each port's excitation (`fields[q]`): the first mode's
/// coefficient at each port, less the incident wave, as power waves of the modes (scaled by
/// the ports' power proxies).
///
/// In lossy port media the proxies are complex (the modes' unconjugated powers, see
/// [`PortTerm::proxy`]) and √(P_p/P_q) is taken on the principal branch: the waves are then the
/// pseudo-waves of R. B. Marks and D. F. Williams (*A general waveguide circuit theory*, J. Res.
/// NIST 97(5):533–562, 1992) referred to each port's own characteristic (wave) impedance, with
/// the normalisation that keeps S symmetric for a reciprocal structure. For lossless ports they
/// are the power waves; for equal port media the normalisation cancels and S is the ratio of the
/// modes' amplitudes, |S21| = e^{−αL} along a uniform line. (Power waves of a lossy line would
/// not be reciprocal, Marks & Williams §4.)
///
/// `excitations` lists (port, mode) per entry of S, each excited by a unit wave of its mode in
/// `fields` (the first mode of each port, or each conductor's mode of a terminal port).
pub(crate) fn read_s(terms: &[Vec<PortTerm<'_>>], excitations: &[(usize, usize)], fields: &[Vec<C>]) -> SMatrix {
    let term = |e: usize| excitations.get(e).and_then(|&(p, m)| terms.get(p)?.get(m));
    let proxy = |r: usize| term(r).map_or(C::new(1.0, 0.0), |t| t.proxy);
    let mut s = SMatrix::zeros(excitations.len());
    for (q, x) in fields.iter().enumerate() {
        for p in 0..excitations.len() {
            let Some(t) = term(p) else { continue };
            let c = t.coefficient(x);
            let direct = if p == q { c - 1.0 } else { c };
            let (pp, pq) = (proxy(p), proxy(q));
            s[(p, q)] = if pp.im == 0.0 && pq.im == 0.0 { direct * (pp.re / pq.re).sqrt() } else { direct * (pp / pq).sqrt() };
        }
    }
    s
}

impl Driven {
    /// S from the solutions `fields` of each excitation ([`Driven::excitations`]), with each
    /// entry's line impedance (where a port's first mode has one): read ([`read_s`]), referred to
    /// the ports' reference impedances ([`Driven::refer`]) and de-embedded
    /// ([`Driven::deembed`]); with terminal ports, de-embedded per mode and converted to the
    /// terminals' waves, `fields` then made the entries' ([`Driven::to_terminals`]).
    pub(crate) fn s_parameters(&self, terms: &[Vec<PortTerm<'_>>], fields: &mut Vec<Vec<C>>, frequency: f64) -> Result<(SMatrix, Vec<Option<f64>>)> {
        let excitations = self.excitations();
        let modal = read_s(terms, &excitations, fields);
        if self.has_terminals() {
            let impedances = self
                .entries()
                .iter()
                .map(|e| if e.terminal.is_some() { None } else { terms.get(e.port).and_then(|l| l.first()).and_then(|t| t.impedance) })
                .collect();
            let s = self.to_terminals(&modal, terms, frequency, fields)?;
            return Ok((s, impedances));
        }
        let impedances: Vec<Option<f64>> = terms.iter().map(|l| l.first().and_then(|t| t.impedance)).collect();
        let s = self.deembed(self.refer(modal, &impedances)?, terms);
        Ok((s, impedances))
    }

    /// S at the ports' reference planes moved `deembed` into the model along their lines: the
    /// waves there are e^{γd} larger, so S′_pq = S_pq e^{γ_p d_p + γ_q d_q} for each port's
    /// first mode (D. M. Pozar, *Microwave Engineering*, 4th ed., §4.3, shifting reference
    /// planes).
    pub(crate) fn deembed(&self, s: SMatrix, terms: &[Vec<PortTerm<'_>>]) -> SMatrix {
        if self.ports.iter().all(|p| p.deembed == 0.0) {
            return s;
        }
        let shift = |p: usize| match (self.ports.get(p), terms.get(p).and_then(|l| l.first())) {
            (Some(port), Some(t)) => (t.gamma * port.deembed).exp(),
            _ => C::new(1.0, 0.0),
        };
        SMatrix::from_fn(s.ports(), |p, q| s[(p, q)] * shift(p) * shift(q))
    }

    /// S of the modes' power waves referred to the ports' reference impedances R (those without
    /// one keep their mode's): with each port's line impedance Z, V = √Z (a + b) and
    /// I = (a − b)/√Z, the new waves a′ = P a + Q b, b′ = Q a + P b, P = (Z + R)/(2√(ZR)),
    /// Q = (Z − R)/(2√(ZR)), so S′ = (Q + P S)(P + Q S)⁻¹ (K. Kurokawa, *Power waves and the
    /// scattering matrix*, IEEE Trans. MTT 13(2):194–202, 1965).
    pub(crate) fn refer(&self, s: SMatrix, impedances: &[Option<f64>]) -> Result<SMatrix> {
        let n = s.ports();
        let mut p = vec![1.0; n];
        let mut q = vec![0.0; n];
        let mut any = false;
        for (i, port) in self.ports.iter().enumerate() {
            let Some(r) = port.reference else { continue };
            let z = impedances.get(i).copied().flatten().ok_or_else(|| {
                invalid(
                    format!("port `{}`", port.name),
                    "has a reference impedance, but its mode has no line impedance (one conductor inside the ground is needed)",
                )
            })?;
            let root = 2.0 * (z * r).sqrt();
            p[i] = (z + r) / root;
            q[i] = (z - r) / root;
            any = true;
        }
        if !any {
            return Ok(s);
        }
        s.renormalized(&p, &q).ok_or_else(|| invalid("reference impedances", "a singular renormalisation"))
    }
}
