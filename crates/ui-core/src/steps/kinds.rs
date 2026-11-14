//! The kinds of analysis as their cards and the tree's head name them: a glyph, a name and what
//! it gives in plain words, all to be translated.

use super::Solver;
use crate::glyph::Glyph;

/// A kind of analysis as its card shows it.
#[derive(Clone, Copy, Debug)]
pub struct KindCard {
    pub glyph: Glyph,
    pub name: crate::lang::Text,
    pub plain: crate::lang::Text,
}

/// The card of a kind of analysis.
///
/// ```
/// use emcraft_ui_core::steps::{Solver, kinds::card};
/// assert_eq!(card(Solver::Frequency).name.en(), "S-parameters");
/// ```
pub fn card(kind: Solver) -> KindCard {
    let (glyph, name, plain) = match kind {
        Solver::Frequency => (Glyph::KindSweep, t!("S-parameters"), t!("What passes and what reflects, over the band")),
        Solver::Eigenmodes => (Glyph::KindResonator, t!("Eigenmodes"), t!("The resonances of a closed cavity and their Q")),
        Solver::Radiation => (Glyph::KindAntenna, t!("Radiation"), t!("How an antenna radiates: its far-field pattern")),
        Solver::Pcb => (Glyph::Chart, t!("PCB signal and return path"), t!("Signal quality and ground continuity on a routed net")),
        Solver::TimeDomain => (Glyph::KindPulse, t!("Time domain"), t!("A pulse sent through, and what comes back in time")),
        Solver::System => (Glyph::Table, t!("System budget"), t!("Gain, noise and sensitivity along a receiver chain")),
        Solver::Particles => (Glyph::FieldLines, t!("Particles (PIC)"), t!("Electrons in the fields they drive themselves, in time")),
    };
    KindCard { glyph, name, plain }
}
