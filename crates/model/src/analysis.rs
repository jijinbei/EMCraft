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
    }
}

emcraft_macros::named_serde!(Analysis);

impl Analysis {
    /// The kind a model calls for when none is chosen: a resonator's eigenmodes when nothing
    /// drives it, radiation with sources, lumped ports or an open boundary, else a sweep.
    ///
    /// ```
    /// use emcraft_model::{Analysis, Model};
    /// let m = Model::default();
    /// assert_eq!(Analysis::infer(&m), Analysis::Eigenmodes);
    /// assert_eq!(m.analysis(), Analysis::Eigenmodes);
    /// assert_eq!(Analysis::parse("time_domain"), Some(Analysis::TimeDomain));
    /// assert_eq!(Analysis::parse("transient"), None);
    /// ```
    pub fn infer(m: &Model) -> Analysis {
        if m.ports.is_empty() && m.lumped.is_empty() && m.sources.is_empty() {
            Analysis::Eigenmodes
        } else if !m.sources.is_empty() || !m.lumped.is_empty() || m.open.is_some() {
            Analysis::Radiation
        } else {
            Analysis::Frequency
        }
    }
}
