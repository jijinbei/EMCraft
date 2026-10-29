//! Parameters read against the session: a port or terminal by name, a frequency that defaults to the band's
//! top.

use crate::{Params, Result, Session};

/// What to give when a frequency is missing and the band would have supplied it.
pub(crate) const SET_THE_BAND: &str = "or set the band: project.frequency";

/// The entry of S the parameter `name` names — a port, or a terminal `<port>:<k>` of a terminal
/// port —, if it is given: its index among S's entries and the entry.
pub(crate) fn entry(s: &Session, p: &Params, name: &str) -> Result<Option<(usize, emcraft_solver::SPort)>> {
    let Some(n) = p.string(name).optional()? else { return Ok(None) };
    match s.sparameter_entry(n) {
        Some(found) => Ok(Some(found)),
        None => {
            let names = s.sparameter_entries().into_iter().map(|e| e.name);
            Err(p.error(name, crate::text::unknown(format_args!("no port or terminal `{n}`"), "there are ", names)))
        }
    }
}

/// The entry of S `excitation` names, a port or a terminal (the first by default).
pub(crate) fn excitation(s: &Session, p: &Params) -> Result<usize> {
    Ok(entry(s, p, "excitation")?.map_or(0, |(k, _)| k))
}

/// The frequency parameter `name` (Hz), the band's top when it is absent; `hint` says what else
/// would do when there is no band either.
pub(crate) fn frequency_or_band_top(s: &Session, p: &Params, name: &str, hint: &str) -> Result<f64> {
    p.frequency(name).or_maybe(s.model.band.map(|b| b.stop)).required_or(hint)
}
