//! `results.active`: an array's active reflection coefficients. With every element driven at
//! once — element n by the wave a_n = w_n e^{−jk₀ û·r_n}, which steers the beam toward û (θ, φ)
//! with e^{jωt} — the wave coming back at element m is Σ_n S_mn a_n, so the reflection it sees is
//!
//! Γ_m(θ, φ) = Σ_n S_mn a_n / a_m
//!
//! (D. M. Pozar, "The active element pattern", IEEE Trans. Antennas Propag. 42(8):1176–1178,
//! 1994; R. C. Hansen, *Phased Array Antennas*, 2nd ed., Wiley 2009, §7.2): the mismatch an
//! element of a scanned array works into, which coupling makes depend on the scan.

use emcraft_model::units::C0;
use serde_json::{Value, json};

use crate::network::source;
use crate::{EngineError, Params, Result, Session};
use emcraft_solver::Complex64 as C;

/// The most scan directions one reading takes.
const MAX_ANGLES: usize = 361;

/// `results.active`.
pub fn active(s: &mut Session, p: &Params) -> Result<Value> {
    let src = source(s, p)?;
    let d = &src.data;
    let n = d.outputs();
    if d.inputs().len() != n || n < 2 {
        return Err(EngineError::failed(format!("an array of 2 or more elements, each excited; this has {n} entries, {} excited", d.inputs().len())));
    }
    let positions = positions(s, p, &src.names)?;
    let weights = weights(p, n)?;
    let angles = angles(p)?;
    let fs = d.frequencies();
    let mut points = Vec::with_capacity(fs.len());
    for (k, &f) in fs.iter().enumerate() {
        let k0 = 2.0 * std::f64::consts::PI * f / C0;
        let scans: Vec<Value> = angles
            .iter()
            .map(|&(theta, phi)| {
                let u = direction(theta, phi);
                let a: Vec<C> =
                    positions.iter().zip(&weights).map(|(r, w)| *w * C::from_polar(1.0, -k0 * (u[0] * r[0] + u[1] * r[1] + u[2] * r[2]))).collect();
                let gammas: Vec<Value> = (0..n)
                    .map(|m| {
                        let back: C = (0..n).map(|q| d.get(k, m, q).unwrap_or_default() * a[q]).sum();
                        let g = if a[m].norm() > 0.0 { back / a[m] } else { C::new(f64::NAN, f64::NAN) };
                        json!({"entry": src.names.get(m), "mag": g.norm(), "db": 20.0 * g.norm().log10(), "deg": g.arg().to_degrees()})
                    })
                    .collect();
                json!({"theta": theta, "phi": phi, "elements": gammas})
            })
            .collect();
        points.push(json!({"frequency": s.show_frequency(f), "scan": scans}));
    }
    Ok(json!({"entries": src.names, "points": points}))
}

/// The unit vector toward (θ, φ), degrees: (sin θ cos φ, sin θ sin φ, cos θ).
fn direction(theta: f64, phi: f64) -> [f64; 3] {
    let (t, f) = (theta.to_radians(), phi.to_radians());
    [t.sin() * f.cos(), t.sin() * f.sin(), t.cos()]
}

/// Each entry's element position (m): `positions` (display units, one per entry), else the
/// middles of the model's lumped ports when the entries are they, by name.
fn positions(s: &Session, p: &Params, names: &[String]) -> Result<Vec<[f64; 3]>> {
    if p.has("positions") {
        let list = p.list("positions", names.len().max(1)).required()?;
        if list.len() != names.len() {
            return Err(p.error("positions", format!("needs one position per entry: {} ({})", names.len(), names.join(", "))));
        }
        return list.iter().map(|v| p.point_of("positions", v).map(|q| [q.x, q.y, q.z])).collect();
    }
    names
        .iter()
        .map(|name| {
            let port = s.model.lumped.iter().find(|l| &l.name == name).ok_or_else(|| {
                p.error(
                    "positions",
                    format!("is needed: `{}` is not a lumped port of the model, whose middle would be its place", crate::text::clip(name, 40)),
                )
            })?;
            let m = port.from.midpoint(port.to);
            Ok([m.x, m.y, m.z])
        })
        .collect()
}

/// The elements' amplitude taper `weights` (default all 1), finite and ≥ 0, not all 0.
fn weights(p: &Params, n: usize) -> Result<Vec<f64>> {
    if !p.has("weights") {
        return Ok(vec![1.0; n]);
    }
    let list = p.list("weights", n).required()?;
    let w: Vec<f64> = list.iter().map(|v| p.number_of("weights", v)).collect::<Result<_>>()?;
    if w.len() != n || w.iter().any(|x| !(x.is_finite() && *x >= 0.0)) || w.iter().all(|x| *x == 0.0) {
        return Err(p.error("weights", format!("needs {n} finite amplitudes ≥ 0, not all 0")));
    }
    Ok(w)
}

/// The scan directions (θ, φ in degrees): `angles` as [[θ, φ], …], else `theta` (one value or
/// `start`/`stop`/`points` of θ) at `phi` (default 0); broadside (0, 0) by default.
fn angles(p: &Params) -> Result<Vec<(f64, f64)>> {
    let finite = |x: f64, what: &str| if x.is_finite() { Ok(x) } else { Err(p.error(what, "must be a finite angle in degrees")) };
    if p.has("angles") {
        let list = p.list("angles", MAX_ANGLES).required()?;
        return list
            .iter()
            .map(|v| {
                let pair = v.as_array().filter(|a| a.len() == 2).ok_or_else(|| p.error("angles", "each is [θ, φ] in degrees"))?;
                Ok((finite(p.number_of("angles", &pair[0])?, "angles")?, finite(p.number_of("angles", &pair[1])?, "angles")?))
            })
            .collect();
    }
    let phi = finite(p.number("phi").or(0.0)?, "phi")?;
    let thetas = if p.has("start") || p.has("stop") {
        let (a, b) = (p.number("start").required()?, p.number("stop").required()?);
        let n = p.integer("points", 2..=MAX_ANGLES).or(19)?;
        crate::numbers::evenly_spaced(finite(a, "start")?, finite(b, "stop")?, n)
    } else {
        vec![finite(p.number("theta").or(0.0)?, "theta")?]
    };
    Ok(thetas.into_iter().map(|t| (t, phi)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broadside_is_up_the_axis_and_endfire_along_x() {
        let z = direction(0.0, 0.0);
        assert!((z[2] - 1.0).abs() < 1e-15 && z[0].abs() < 1e-15);
        let x = direction(90.0, 0.0);
        assert!((x[0] - 1.0).abs() < 1e-15 && x[2].abs() < 1e-15);
    }
}
