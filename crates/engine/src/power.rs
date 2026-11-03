//! Power fractions of the reported, power-normalized S entries (Kurokawa 1965; see
//! docs/solvers/literature.md). Missing power is a signed balance, not automatically absorption:
//! higher port modes, radiation, dissipation and numerical error can all contribute.
use crate::{EngineError, Params, Result, Session};
use emcraft_solver::SMatrix;
use serde_json::{Value, json};

/// The fractions of the incident power at each port for excitation `excitation`: one column of a
/// power-wave S matrix, |S[p, q]|².
///
/// ```
/// use emcraft_solver::{Complex64, SMatrix};
/// // A matched through line: everything goes on to the other port.
/// let through = SMatrix::from_fn(2, |p, q| Complex64::new(if p != q { 1.0 } else { 0.0 }, 0.0));
/// assert_eq!(emcraft_engine::power::fractions(&through, 0).unwrap(), vec![0.0, 1.0]);
/// assert!(emcraft_engine::power::fractions(&through, 2).is_err());
/// ```
pub fn fractions(s: &SMatrix, excitation: usize) -> Result<Vec<f64>> {
    if excitation >= s.ports() {
        return Err(EngineError::failed("no such excitation in S"));
    }
    (0..s.ports())
        .map(|p| {
            let value = s.get(p, excitation).map_or(f64::NAN, |v| v.norm_sqr());
            if value.is_finite() { Ok(value) } else { Err(EngineError::failed("non-finite S entry: power fractions unavailable")) }
        })
        .collect()
}

/// The power fractions at one solved frequency (Hz) for excitation `excitation`: the frequency
/// must be one solved, never interpolated or replaced by a neighbour.
pub fn at(s: &Session, frequency: f64, excitation: usize) -> Result<Value> {
    let sw = s.results().sweep()?;
    let r = sw.at(frequency, false).ok_or_else(|| EngineError::failed("frequency not solved"))?;
    let entries = sw.entries();
    if excitation >= entries.len() {
        return Err(EngineError::failed("no such excitation"));
    }
    // The lossy-port solver can use pseudo-waves: their |S|² is not real power. Refuse
    // that interpretation instead of giving a plausible but physically wrong budget. (Lumped
    // ports' waves are power waves of real impedances: their |S|² is power.)
    let lumped = sw.driven.ports.is_empty();
    for entry in entries.iter().filter(|_| !lumped) {
        let terms = sw.driven.port_terms(entry.port, frequency)?;
        let gamma = terms.first().ok_or_else(|| EngineError::failed("port has no mode"))?.gamma;
        if gamma.im <= 0.0 || (entry.reference.is_none() && gamma.re.abs() > 1e-9 * gamma.im.abs().max(1.0)) {
            return Err(EngineError::failed("not supported yet: real power fractions for evanescent or lossy-port pseudo-waves"));
        }
    }
    let powers = fractions(&r.s, excitation)?;
    let transmitted: f64 = powers.iter().enumerate().filter(|(p, _)| *p != excitation).map(|(_, v)| v).sum();
    let accounted: f64 = powers.iter().sum();
    let outputs: Vec<Value> = powers
        .iter()
        .enumerate()
        .map(|(p, value)| {
            let branch = (p != excitation && transmitted > 0.0).then(|| value / transmitted);
            json!({
                "port": entries.get(p).map(|e| e.name.as_str()),
                "s": format!("S{}{}", p + 1, excitation + 1),
                "reflected": p == excitation,
                "incident_fraction": value,
                "branch_fraction": branch,
            })
        })
        .collect();
    Ok(json!({
        "frequency": s.show_frequency(r.frequency),
        "excitation": entries[excitation].name,
        "reflected": powers[excitation],
        "transmitted": transmitted,
        "accounted": accounted,
        "unaccounted": 1.0 - accounted,
        "outputs": outputs,
    }))
}

pub fn command(s: &mut Session, p: &Params) -> Result<Value> {
    let frequency = p
        .frequency("frequency")
        .optional()?
        .or_else(|| s.results().sweep.as_ref()?.results.last().map(|r| r.frequency))
        .ok_or(EngineError::NotSolved { what: "S-parameters", run: "solve.frequency" })?;
    let excitation = crate::args::excitation(s, p)?;
    at(s, frequency, excitation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use emcraft_solver::Complex64 as C;
    #[test]
    fn independently_constructed_lossless_three_way_split() {
        // Householder scattering matrix 2 vv^T-I is real, symmetric and unitary.
        let v = [0.5f64.sqrt(), 0.3f64.sqrt(), 0.2f64.sqrt()];
        let s = SMatrix::from_fn(3, |p, q| C::new(2.0 * v[p] * v[q] - f64::from(p == q), 0.0));
        let p = fractions(&s, 0).unwrap();
        assert!(p[0] < 1e-28 && (p[1] - 0.6).abs() < 1e-14 && (p[2] - 0.4).abs() < 1e-14);
        assert!((p.iter().sum::<f64>() - 1.0).abs() < 1e-14);
        assert!(fractions(&s, 3).is_err());
        assert!(fractions(&SMatrix::from_fn(1, |_, _| C::new(f64::NAN, 0.0)), 0).is_err());
    }
}
