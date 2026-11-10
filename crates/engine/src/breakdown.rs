//! `results.breakdown`: the power a structure carries before its peak field reaches breakdown.
//! The fields scale with the square root of the power, so with the largest field E₁ for 1 W
//! incident at a port, the field reaches the breakdown strength E_b at P = (E_b/E₁)² W
//! (D. M. Pozar, *Microwave Engineering*, 4th ed., Wiley 2012, §3.3 — the power a TE10 guide
//! carries, P = ab|E₀|²/(4Z_TE), whose inverse gives the empty guide's handling; air breaks
//! down near 3 MV/m at sea level). The largest field is the mesh's: at a metal edge it is
//! singular and grows as the mesh refines, so a sharp corner reads lower than it is.

use serde_json::{Value, json};

use crate::args::excitation;
use crate::{EngineError, Params, Result, Session};

/// Air's breakdown field (MV/m) when none is given: dry air at sea level, gaps of centimetres.
pub const AIR_BREAKDOWN_MV_PER_M: f64 = 3.0;

/// The strongest breakdown field a reading takes (MV/m): far above any dielectric's.
const MAX_BREAKDOWN_MV_PER_M: f64 = 1e4;

/// `results.breakdown`.
pub fn breakdown(s: &mut Session, p: &Params) -> Result<Value> {
    let eb = p.positive("breakdown").or(AIR_BREAKDOWN_MV_PER_M)?;
    if eb > MAX_BREAKDOWN_MV_PER_M {
        return Err(p.error("breakdown", format!("at most {MAX_BREAKDOWN_MV_PER_M} MV/m")));
    }
    let k = excitation(s, p)?;
    let sw = s.results().sweep()?;
    let entries = sw.entries();
    let port = entries.get(k).ok_or_else(|| EngineError::failed("no such excitation"))?;
    let entry = port.name.clone();
    let only = p.frequency("frequency").optional()?;
    let picked: Vec<_> = sw.results.iter().filter(|r| only.is_none_or(|f| crate::numbers::same_frequency(r.frequency, f))).collect();
    if picked.is_empty() {
        return Err(p.error("frequency", "is not one the sweep solved"));
    }
    let mut points = Vec::with_capacity(picked.len());
    for r in picked {
        crate::progress::check()?;
        let x = r.fields.get(k).ok_or_else(|| EngineError::failed("the sweep kept no field for this excitation"))?;
        let Some(peak) = sw.driven.peak_e(x, r.frequency)? else { continue };
        // The solved field is the excitation's unit wave: scaled to 1 W.
        let watts = sw
            .driven
            .entry_power(port, r.frequency)
            .ok_or_else(|| EngineError::failed("the port's mode carries no power at this frequency (below its cut-off)"))?;
        let field = peak.value / watts.sqrt();
        let power = (eb * 1e6 / field).powi(2);
        let at = [peak.at.x, peak.at.y, peak.at.z].map(|c| s.show_length(c));
        points.push(json!({"frequency": s.show_frequency(r.frequency), "peak_field": field, "at": at, "max_power": power}));
    }
    Ok(json!({"excitation": entry, "breakdown": eb, "points": points}))
}
