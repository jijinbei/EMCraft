//! Lumped circuit elements (`model.element.*`): passive R, L and C loads across gaps.

use emcraft_model::{Arrangement, LumpedElement};
use serde_json::{Value, json};

use crate::Result;
use crate::params::Params;
use crate::session::Session;

/// `model.element.create`: a resistor, inductor and capacitor (in Ω, nH and pF) across the gap
/// from `from` to `to`, in series or in parallel.
pub fn create(s: &mut Session, p: &Params) -> Result<Value> {
    let width = p.positive_length("width").optional()?;
    let from = p.point("from").required()?;
    let to = p.point("to").required()?;
    // A sheet's half width across the gap, along `across` (default the axis after the gap's).
    let across = match width {
        None => None,
        Some(w) => {
            let along = to - from;
            let dir = p.vector("across").or(emcraft_model::Vec3::unit((along.dominant_axis() + 1) % 3))?;
            let dir = dir.normalized().ok_or_else(|| p.error("across", "must not be zero"))?;
            Some(dir * (0.5 * w))
        }
    };
    let e = lumped(p, p.string("name").required()?.to_owned(), from, to, across)?;
    s.model.add_element(e)?;
    Ok(json!({"elements": s.model.elements.len()}))
}

/// Henries per nanohenry and farads per picofarad: the units a command's `inductance` and
/// `capacitance` are read in (`resistance` in ohms).
const NANOHENRY: f64 = 1e-9;
const PICOFARAD: f64 = 1e-12;

/// A lumped element named `name` across `from`–`to` (m), its `resistance` (Ω), `inductance`
/// (nH) and `capacitance` (pF) — each a finite number ≥ 0, 0 (the default) for a part it does
/// not have — and `arrangement` (series by default) read from `p`; its sheet `across`.
pub(crate) fn lumped(
    p: &Params,
    name: String,
    from: emcraft_model::Point3,
    to: emcraft_model::Point3,
    across: Option<emcraft_model::Vec3>,
) -> Result<LumpedElement> {
    let part = |key: &str, si: f64| -> Result<f64> {
        let v = p.number(key).or(0.0)?;
        if !(v.is_finite() && v >= 0.0) {
            return Err(p.error(key, "must be a finite number ≥ 0"));
        }
        Ok(v * si)
    };
    Ok(LumpedElement {
        name,
        from,
        to,
        resistance: part("resistance", 1.0)?,
        inductance: part("inductance", NANOHENRY)?,
        capacitance: part("capacitance", PICOFARAD)?,
        arrangement: p.pick("arrangement").or(Arrangement::Series)?,
        across,
    })
}

/// `model.element.remove`: the element named `name`.
pub fn remove(s: &mut Session, p: &Params) -> Result<Value> {
    let name = p.string("name").required()?;
    s.model.remove_element(name).ok_or_else(|| p.error("name", format!("names no element `{name}`")))?;
    Ok(json!({"elements": s.model.elements.len()}))
}

/// The elements as the summary lists them, in display units.
pub fn summary(s: &Session) -> Value {
    let list: Vec<Value> = s
        .model
        .elements
        .iter()
        .map(|e| {
            json!({"name": e.name, "resistance": e.resistance, "inductance_nh": e.inductance * 1e9, "capacitance_pf": e.capacitance * 1e12, "arrangement": e.arrangement.id()})
        })
        .collect();
    Value::Array(list)
}
