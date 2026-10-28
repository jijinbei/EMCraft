//! Terminal ports: a port whose cross-section holds N conductors besides the ground (coupled
//! strips) read per conductor instead of per mode.
//!
//! The port carries N quasi-TEM modes, one per conductor (the even and odd modes of two strips,
//! [`crate::portmode`]); each is excited in turn and read at every port, which gives S in the
//! modes' power waves â, b̂ (each mode's wave scaled by the square root of the power a unit
//! wave carries). The conductors' voltages and currents are the modes' sums,
//! V = T_V (â + b̂), I = T_I (â − b̂), where column i of T_I holds mode i's conductor currents
//! when it carries 1 W and, the modes being power-orthogonal, T_V = 2 (T_Iᴴ)⁻¹ makes the
//! modes' powers the conductors' (½ T_Vᵀ T_I* = 1; D. F. Williams, L. A. Hayden, R. B. Marks,
//! *A complete multimode equivalent-circuit theory for electrical design*, J. Res. NIST
//! 102(4):405–423, 1997, the power-current voltage definition; for the homogeneous TEM lines
//! the modes' currents are orthogonal and V_i = Z_i I_i, the modal impedances; the modal
//! transformation as in G. Faché, F. Olyslager, D. De Zutter, *Electromagnetic and Circuit
//! Modelling of Multiconductor Transmission Lines*, Oxford 1993, ch. 4). Each conductor's
//! terminal waves referred to a real reference impedance R are the power waves
//! a = (V + R I)/(2√(2R)), b = (V − R I)/(2√(2R)) (K. Kurokawa, *Power waves and the scattering
//! matrix*, IEEE Trans. MTT 13(2):194–202, 1965, peak phasors), so
//! a = P â + Q b̂, b = Q â + P b̂ with P = (T_V + R T_I)/(2√(2R)), Q = (T_V − R T_I)/(2√(2R)),
//! block by port, and S_t = (Q + P S)(P + Q S)⁻¹. A lossless reciprocal structure keeps a
//! unitary, symmetric S_t (T_I real).
//!
//! De-embedding moves each mode's plane by its own γ before the conversion (the modes travel
//! apart along an inhomogeneous line). Ports read in their first mode keep their own waves, or
//! the reference impedance's as in [`Driven::refer`], in the same transformation.

use std::f64::consts::PI;

use emcraft_model::DEFAULT_TERMINAL_IMPEDANCE;
use emcraft_model::units::MU0;
use num_complex::Complex64 as C;

use super::terms::PortTerm;
use super::{Driven, Result, invalid};
use crate::smatrix::SMatrix;
use crate::zdense;

/// One entry of S: a port read in its first mode, or one terminal (conductor) of a terminal
/// port.
#[derive(Clone, Debug, PartialEq)]
pub struct SPort {
    /// The port's name, or `<port>:<k>` for terminal k (from 1) of a terminal port.
    pub name: String,
    /// The port's index.
    pub port: usize,
    /// The terminal (from 0) of a terminal port.
    pub terminal: Option<usize>,
    /// The impedance (Ω) the entry's waves are referred to: the port's, or for a terminal 50 Ω
    /// when the port sets none; `None` for a port's own mode's waves.
    pub reference: Option<f64>,
}

impl Driven {
    /// S's entries in order: each port's, its terminals' for a terminal port.
    pub fn entries(&self) -> Vec<SPort> {
        let mut out = Vec::new();
        for (i, p) in self.ports.iter().enumerate() {
            if p.terminals >= 2 {
                let r = p.reference.unwrap_or(DEFAULT_TERMINAL_IMPEDANCE);
                out.extend((0..p.terminals).map(|k| SPort { name: format!("{}:{}", p.name, k + 1), port: i, terminal: Some(k), reference: Some(r) }));
            } else {
                out.push(SPort { name: p.name.clone(), port: i, terminal: None, reference: p.reference });
            }
        }
        out
    }

    /// The excitations, one per entry of S: (port, mode). A terminal port excites each of its
    /// first modes, one per conductor.
    pub(crate) fn excitations(&self) -> Vec<(usize, usize)> {
        self.entries().iter().map(|e| (e.port, e.terminal.unwrap_or(0))).collect()
    }

    /// Whether any port is read per conductor.
    pub fn has_terminals(&self) -> bool {
        self.ports.iter().any(|p| p.terminals >= 2)
    }

    /// The modal S `sm` (of the excitations' power waves, at the ports' faces) de-embedded per
    /// mode and converted to the entries' waves (see the module docs), with each excitation's
    /// field in `fields` (a unit wave of its mode) replaced by the field of a unit wave (1 W
    /// available on the whole line) at its entry, the other entries' incident waves zero.
    pub(crate) fn to_terminals(&self, sm: &SMatrix, terms: &[Vec<PortTerm<'_>>], frequency: f64, fields: &mut Vec<Vec<C>>) -> Result<SMatrix> {
        let ex = self.excitations();
        let n = ex.len();
        if sm.ports() != n || fields.len() != n {
            return Err(invalid("terminals", "the excitations do not match the ports' entries"));
        }
        let term = |e: usize| ex.get(e).and_then(|&(p, m)| terms.get(p)?.get(m)).ok_or_else(|| invalid("terminals", "a mode's term is missing"));
        // Each mode moved to its port's reference plane: S′ = D S D, D = e^{γd}.
        let mut shift = Vec::with_capacity(n);
        for (e, &(p, _)) in ex.iter().enumerate() {
            let d = self.ports.get(p).map_or(0.0, |x| x.deembed);
            shift.push(if d == 0.0 { C::new(1.0, 0.0) } else { (term(e)?.gamma * d).exp() });
        }
        let s = SMatrix::from_fn(n, |i, j| sm[(i, j)] * shift[i] * shift[j]);
        let (mut p, mut q) = (vec![C::new(0.0, 0.0); n * n], vec![C::new(0.0, 0.0); n * n]);
        let mut start = 0;
        for (pi, port) in self.ports.iter().enumerate() {
            if port.terminals >= 2 {
                let k = port.terminals;
                let mut t_i = vec![C::new(0.0, 0.0); k * k];
                for m in 0..k {
                    let col = terms.get(pi).and_then(|l| l.get(m)).and_then(|t| t.current.as_ref()).filter(|c| c.len() == k);
                    let col = col.ok_or_else(|| invalid(format!("port `{}`", port.name), "a terminal port's mode has no conductor currents"))?;
                    for (r, c) in col.iter().enumerate() {
                        t_i[r * k + m] = *c;
                    }
                }
                let t_v = voltages(&t_i, k).ok_or_else(|| {
                    invalid(format!("port `{}`", port.name), "its modes' conductor currents are not independent: the terminals cannot be told apart")
                })?;
                let r = port.reference.unwrap_or(DEFAULT_TERMINAL_IMPEDANCE);
                let scale = 1.0 / (2.0 * (2.0 * r).sqrt());
                for row in 0..k {
                    for m in 0..k {
                        let (v, i) = (t_v[row * k + m], t_i[row * k + m]);
                        p[(start + row) * n + start + m] = (v + r * i) * scale;
                        q[(start + row) * n + start + m] = (v - r * i) * scale;
                    }
                }
                start += k;
            } else {
                let (pp, qq) = match port.reference {
                    None => (1.0, 0.0),
                    Some(r) => {
                        let z = terms.get(pi).and_then(|l| l.first()).and_then(|t| t.impedance).ok_or_else(|| {
                            invalid(
                                format!("port `{}`", port.name),
                                "has a reference impedance, but its mode has no line impedance (one conductor inside the ground is needed)",
                            )
                        })?;
                        let root = 2.0 * (z * r).sqrt();
                        ((z + r) / root, (z - r) / root)
                    }
                };
                p[start * n + start] = C::new(pp, 0.0);
                q[start * n + start] = C::new(qq, 0.0);
                start += 1;
            }
        }
        let (st, inv) = s.transformed(&p, &q).ok_or_else(|| invalid("terminals", "a singular conversion to the terminals' waves"))?;
        // The fields: entry k's unit wave is the modes' incident waves D X e_k at their faces,
        // each mode's field per unit power wave its unit coefficient's over √P (P: the power a
        // unit coefficient carries on the whole line).
        let omega = 2.0 * PI * frequency;
        let whole = 2f64.powi(self.symmetry.len() as i32);
        let mut weight = Vec::with_capacity(n);
        for e in 0..n {
            let power = term(e)?.proxy.re / (2.0 * omega * MU0) * whole;
            if !(power > 0.0 && power.is_finite()) {
                return Err(invalid("terminals", "a port's mode carries no power"));
            }
            weight.push(shift[e] / power.sqrt());
        }
        let old = std::mem::take(fields);
        let len = old.first().map_or(0, Vec::len);
        *fields = (0..n)
            .map(|k| {
                let mut x = vec![C::new(0.0, 0.0); len];
                for (j, f) in old.iter().enumerate() {
                    let c = weight[j] * inv[j * n + k];
                    if c != C::new(0.0, 0.0) {
                        x.iter_mut().zip(f).for_each(|(xi, fi)| *xi += c * fi);
                    }
                }
                x
            })
            .collect();
        Ok(st)
    }
}

/// T_V = 2 (T_Iᴴ)⁻¹ of the k × k row-major `t_i`; `None` when it is singular.
fn voltages(t_i: &[C], k: usize) -> Option<Vec<C>> {
    let h: Vec<Vec<C>> = (0..k).map(|r| (0..k).map(|c| t_i[c * k + r].conj()).collect()).collect();
    let mut out = vec![C::new(0.0, 0.0); k * k];
    for col in 0..k {
        let e: Vec<C> = (0..k).map(|i| C::new(if i == col { 2.0 } else { 0.0 }, 0.0)).collect();
        let x = zdense::solve(h.clone(), e)?;
        for (r, v) in x.into_iter().enumerate() {
            if !(v.re.is_finite() && v.im.is_finite()) {
                return None;
            }
            out[r * k + col] = v;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_conductor_gives_the_line_impedance() {
        // One conductor: T_I = √(2/Z), T_V = √(2Z).
        let z: f64 = 62.0;
        let t_v = voltages(&[C::new((2.0 / z).sqrt(), 0.0)], 1).unwrap();
        assert!((t_v[0].re - (2.0 * z).sqrt()).abs() < 1e-12);
        assert!(voltages(&[C::new(0.0, 0.0)], 1).is_none());
    }

    #[test]
    fn an_ideal_coupled_line_converts_to_the_closed_form_coupler() {
        // Two like strips: even and odd modes, currents (1, 1)/√2 and (1, −1)/√2 per unit
        // amplitude, each a matched line of length θ: the modal S of four ports (two modes at
        // each end) is S_m = [[0, T], [T, 0]], T = diag(e^{−jθ}). Converted to terminals at
        // Z0 = √(Ze Zo) it is the ideal coupler (Pozar §7.6).
        let (ze, zo, theta) = (69.37f64, 36.04f64, 0.9f64);
        let z0 = (ze * zo).sqrt();
        let t = C::from_polar(1.0, -theta);
        let zero = C::new(0.0, 0.0);
        let sm = SMatrix::from_fn(4, |i, j| if (i < 2) != (j < 2) && i % 2 == j % 2 { t } else { zero });
        // Mode m carrying 1 W: per-conductor current magnitude |I| with 2P = Z_m Σ|I|² = Z_m 2|I|².
        let ie = (1.0 / ze).sqrt();
        let io = (1.0 / zo).sqrt();
        let t_i = [C::new(ie, 0.0), C::new(io, 0.0), C::new(ie, 0.0), C::new(-io, 0.0)];
        let t_v = voltages(&t_i, 2).unwrap();
        // V_i = Z_i I_i for orthogonal currents.
        assert!((t_v[0].re - ze * ie).abs() < 1e-12 && (t_v[3].re + zo * io).abs() < 1e-12);
        let scale = 1.0 / (2.0 * (2.0 * z0).sqrt());
        let (mut p, mut q) = (vec![zero; 16], vec![zero; 16]);
        for b in [0, 2] {
            for r in 0..2 {
                for m in 0..2 {
                    p[(b + r) * 4 + b + m] = (t_v[r * 2 + m] + z0 * t_i[r * 2 + m]) * scale;
                    q[(b + r) * 4 + b + m] = (t_v[r * 2 + m] - z0 * t_i[r * 2 + m]) * scale;
                }
            }
        }
        let (st, _) = sm.transformed(&p, &q).unwrap();
        let c = (ze - zo) / (ze + zo);
        let j = C::new(0.0, 1.0);
        let (sin, cos) = (theta.sin(), theta.cos());
        let root = (1.0 - c * c).sqrt();
        let den = root * cos + j * sin;
        let coupled = j * c * sin / den;
        let through = root / den;
        // Entries: 0 = 1:1 (input), 1 = 1:2 (coupled), 2 = 2:1 (through), 3 = 2:2 (isolated).
        assert!(st[(0, 0)].norm() < 1e-12 && st[(3, 0)].norm() < 1e-12, "{st:?}");
        assert!((st[(1, 0)] - coupled).norm() < 1e-12, "{} {coupled}", st[(1, 0)]);
        assert!((st[(2, 0)] - through).norm() < 1e-12, "{} {through}", st[(2, 0)]);
        assert!(st.reciprocity_error() < 1e-12 && (st.largest_singular_value() - 1.0).abs() < 1e-9);
    }
}
