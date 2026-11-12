//! The kinds of analysis as their cards and the step bar's switch name them: a glyph, a name,
//! what it gives in plain words and what it does (on hover), all to be translated.

use super::Solver;
use crate::glyph::Glyph;

/// A kind of analysis as its card shows it.
#[derive(Clone, Copy, Debug)]
pub struct KindCard {
    pub glyph: Glyph,
    pub name: crate::lang::Text,
    pub plain: crate::lang::Text,
    pub detail: crate::lang::Text,
}

/// The card of a kind of analysis.
///
/// ```
/// use emcraft_ui_core::steps::{Solver, kinds::card};
/// assert_eq!(card(Solver::Frequency).name.en(), "S-parameters");
/// ```
pub fn card(kind: Solver) -> KindCard {
    let (glyph, name, plain, detail) = match kind {
        Solver::Frequency => (
            Glyph::KindSweep,
            t!("S-parameters"),
            t!("What passes and what reflects, over the band"),
            t!("S-parameters between ports, swept over the band (solve.frequency)"),
        ),
        Solver::Eigenmodes => (
            Glyph::KindResonator,
            t!("Eigenmodes"),
            t!("The resonances of a closed cavity and their Q"),
            t!("A closed resonator's modes and Q; nothing drives it (solve.eigenmodes)"),
        ),
        Solver::Radiation => (
            Glyph::KindAntenna,
            t!("Radiation"),
            t!("How an antenna radiates: its far-field pattern"),
            t!("An antenna or a scatterer in an open boundary: fields and far field (solve.radiation)"),
        ),
        Solver::Pcb => (
            Glyph::Chart,
            t!("PCB signal and return path"),
            t!("Signal quality and ground continuity on a routed net"),
            t!("PCB geometry screening and ideal transmission-line estimates (pcb.inspect)"),
        ),
        Solver::TimeDomain => (
            Glyph::KindPulse,
            t!("Time domain"),
            t!("A pulse sent through, and what comes back in time"),
            t!("A pulse over the band: S-parameters by finite integration (solve.time_domain)"),
        ),
        Solver::System => (
            Glyph::Table,
            t!("System budget"),
            t!("Gain, noise and sensitivity along a receiver chain"),
            t!("An antenna, amplifiers, filters and lines in a chain: its budget stage by stage, no field solved (solve.system)"),
        ),
    };
    KindCard { glyph, name, plain, detail }
}
