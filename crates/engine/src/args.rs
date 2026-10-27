//! Parameters read against the session: a port by name, a frequency that defaults to the band's
//! top.

use emcraft_model::Port;

use crate::{Params, Result, Session};

/// What to give when a frequency is missing and the band would have supplied it.
pub(crate) const SET_THE_BAND: &str = "or set the band: project.frequency";

/// The port parameter `name` names, if it is given.
pub(crate) fn port<'s>(s: &'s Session, p: &Params, name: &str) -> Result<Option<(usize, &'s Port)>> {
    Ok(p.string(name).optional()?.map(|n| s.model.port(n)).transpose()?)
}

/// The port `excitation` names (the first by default).
pub(crate) fn excitation(s: &Session, p: &Params) -> Result<usize> {
    Ok(port(s, p, "excitation")?.map_or(0, |(k, _)| k))
}

/// The frequency parameter `name` (Hz), the band's top when it is absent; `hint` says what else
/// would do when there is no band either.
pub(crate) fn frequency_or_band_top(s: &Session, p: &Params, name: &str, hint: &str) -> Result<f64> {
    p.frequency(name).or_maybe(s.model.band.map(|b| b.stop)).required_or(hint)
}
