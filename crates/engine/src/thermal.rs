//! `results.thermal`: the steady temperature a solved field's losses raise
//! ([`emcraft_solver::driven::Driven::temperature`]): `power` watts incident at `excitation`,
//! the walls a heat sink at `wall_temperature`.

use emcraft_model::material::AIR_THERMAL_CONDUCTIVITY;
use serde_json::{Value, json};

use crate::args::excitation;
use crate::{EngineError, Params, Result, Session};

/// The most power (W) a reading scales to: far above any guide's.
const MAX_POWER: f64 = 1e9;

/// `results.thermal`.
pub fn thermal(s: &mut Session, p: &Params) -> Result<Value> {
    let power = p.positive("power").or(1.0)?;
    if power > MAX_POWER {
        return Err(p.error("power", format!("at most {MAX_POWER} W")));
    }
    let sink = p.number("wall_temperature").or(20.0)?;
    if !(sink.is_finite() && sink > -273.15) {
        return Err(p.error("wall_temperature", "must be above absolute zero (°C)"));
    }
    let k = excitation(s, p)?;
    let sw = s.results().sweep()?;
    let r = match p.frequency("frequency").optional()? {
        Some(f) => sw.at(f, false).ok_or_else(|| p.error("frequency", "is not one the sweep solved"))?,
        None if sw.results.len() == 1 => sw.results.first().ok_or_else(|| EngineError::failed("the sweep is empty"))?,
        None => return Err(p.error("frequency", "is needed: the sweep solved several")),
    };
    let entries = sw.entries();
    let entry = entries.get(k).ok_or_else(|| EngineError::failed("no such excitation"))?;
    let watts = sw
        .driven
        .entry_power(entry, r.frequency)
        .ok_or_else(|| EngineError::failed("the port's mode carries no power at this frequency (below its cut-off)"))?;
    let x = r.fields.get(k).ok_or_else(|| EngineError::failed("the sweep kept no field for this excitation"))?;
    let mesh = s.model.mesh.as_ref().ok_or_else(|| EngineError::failed("no mesh"))?;
    // Each material's conductivity: its own, air's for vacuum; a material of the mesh without
    // one is named.
    let materials = &s.model.materials;
    let mut of_material = Vec::with_capacity(materials.len());
    for m in materials {
        of_material.push(match m.thermal_conductivity {
            Some(k) => Some(k.get()),
            None if m.name == emcraft_model::Material::vacuum().name => Some(AIR_THERMAL_CONDUCTIVITY),
            None => None,
        });
    }
    for &mi in &mesh.tet_material {
        if of_material.get(mi as usize).copied().flatten().is_none() {
            let name = materials.get(mi as usize).map_or("?", |m| m.name.as_str());
            return Err(EngineError::failed(format!(
                "material `{name}` has no thermal conductivity: model.material.edit with `thermal_conductivity`"
            )));
        }
    }
    let conductivity = |t: usize| mesh.tet_material.get(t).and_then(|m| of_material.get(*m as usize).copied().flatten()).unwrap_or(0.0);
    let rise = sw.driven.temperature(x, r.frequency, (power / watts).sqrt(), conductivity)?;
    // Each material's hottest node.
    let mut hottest = vec![f64::NEG_INFINITY; materials.len()];
    for (tet, &mi) in mesh.tets.iter().zip(&mesh.tet_material) {
        for &v in tet {
            if let (Some(h), Some(Some(r))) = (hottest.get_mut(mi as usize), rise.rise.get(v as usize)) {
                *h = h.max(*r);
            }
        }
    }
    let by_material: Vec<Value> = materials
        .iter()
        .zip(&hottest)
        .filter(|(_, h)| h.is_finite())
        .map(|(m, h)| json!({"material": m.name, "max_temperature": sink + h}))
        .collect();
    let at = [rise.at.x, rise.at.y, rise.at.z].map(|c| s.show_length(c));
    Ok(json!({
        "frequency": s.show_frequency(r.frequency),
        "excitation": entry.name,
        "power": power,
        "heat": rise.heat,
        "max_temperature": sink + rise.peak,
        "max_rise": rise.peak,
        "at": at,
        "materials": by_material,
    }))
}
