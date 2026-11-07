//! The kind of analysis a job is set up for: a frequency sweep of S-parameters, a resonator's
//! eigenmodes, an open problem's radiation, or the time domain. Chosen at the job's start
//! (`project.analysis`), or, while none is chosen, inferred from what the model holds.

use crate::model::Model;

emcraft_macros::named_enum! {
    /// A kind of analysis: its id (`project.analysis`'s `kind`) and its label.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum Analysis {
        /// S-parameters between ports, swept over the band.
        Frequency = "frequency" => "Frequency sweep (S-parameters)",
        /// A closed resonator's modes: nothing drives it.
        Eigenmodes = "eigenmodes" => "Eigenmodes (resonances)",
        /// An open problem: an antenna or a scatterer in a perfectly matched layer.
        Radiation = "radiation" => "Radiation (open problem)",
        /// A pulse in time, its spectrum over the band.
        TimeDomain = "time_domain" => "Time domain",
        /// Routed PCB signal quality and geometric reference-copper screening.
        Pcb = "pcb" => "PCB signal and return path",
    }
}

emcraft_macros::named_serde!(Analysis);

impl Analysis {
    /// The kind a model calls for when none is chosen: a board's route diagnosis while it has no
    /// region solved by the finite elements; radiation with sources, lumped ports or an open
    /// boundary, else a sweep — also before anything drives it (most jobs are; a
    /// resonator is chosen, as its templates do).
    ///
    /// ```
    /// use emcraft_model::{Analysis, Model};
    /// let m = Model::default();
    /// assert_eq!(Analysis::infer(&m), Analysis::Frequency);
    /// assert_eq!(m.analysis(), Analysis::Frequency);
    /// assert_eq!(Analysis::parse("time_domain"), Some(Analysis::TimeDomain));
    /// assert_eq!(Analysis::parse("transient"), None);
    /// ```
    pub fn infer(m: &Model) -> Analysis {
        // A board's route diagnosis until a region is set; then a model as any other.
        if m.pcb().is_some_and(|b| b.region.is_none()) {
            Analysis::Pcb
        } else if !m.sources.is_empty() || !m.lumped.is_empty() || m.domain.open.is_some() {
            Analysis::Radiation
        } else {
            Analysis::Frequency
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pcb::{Board, Region};

    /// A board is a route diagnosis until a region is set; then it is a model as any other: a
    /// sweep with waveguide-like ports, radiation once open (an antenna on the board).
    #[test]
    fn a_board_with_a_region_is_inferred_as_any_model() {
        let board = Board { name: "b".into(), layers: vec![], segments: vec![], vias: vec![], copper: vec![], notes: vec![], region: None };
        let mut m = Model::new();
        m.set_pcb(Some(board.clone()));
        assert_eq!(Analysis::infer(&m), Analysis::Pcb);
        let region = Region { lo: [0.0, 0.0], hi: [0.01, 0.01], above: 0.005, below: 0.005 };
        m.set_pcb(Some(Board { region: Some(region), ..board }));
        assert_eq!(Analysis::infer(&m), Analysis::Frequency);
        m.domain.open = Some(crate::Open { thickness: crate::Metres::new(0.006).unwrap(), reflection: None, shape: crate::OpenShape::Box });
        assert_eq!(Analysis::infer(&m), Analysis::Radiation);
    }
}
