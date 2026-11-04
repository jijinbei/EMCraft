//! Lumped circuit elements (`model.element.*`): passive R, L and C loads across gaps.

use emcraft_model::{Arrangement, LumpedElement};
use serde_json::{Value, json};

use crate::Result;
use crate::params::Params;
use crate::session::Session;

/// `model.element.create`: a resistor, inductor and capacitor (in Ω, nH and pF) across the gap
/// from `from` to `to`, in series or in parallel.
pub fn create(s: &mut Session, p: &Params) -> Result<Value> {
    let part = |name: &str, si: f64| -> Result<f64> {
        let v = p.number(name).or(0.0)?;
        if !(v.is_finite() && v >= 0.0) {
            return Err(p.error(name, "must be a finite number ≥ 0"));
        }
        Ok(v * si)
    };
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
    let e = LumpedElement {
        name: p.string("name").required()?.to_owned(),
        from,
        to,
        resistance: part("resistance", 1.0)?,
        inductance: part("inductance", 1e-9)?,
        capacitance: part("capacitance", 1e-12)?,
        arrangement: p.pick("arrangement").or(Arrangement::Series)?,
        across,
    };
    s.model.add_element(e)?;
    Ok(json!({"elements": s.model.elements.len()}))
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
