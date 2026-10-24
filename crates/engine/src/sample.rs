//! Sample models: command scripts built in, so `file.sample` and the desktop app's start page
//! open them without any file.

/// A sample.
pub struct Sample {
    pub name: &'static str,
    pub label: &'static str,
    /// The command script (a JSON array of `{"command", "params"}`).
    pub script: &'static str,
}

pub const SAMPLES: &[Sample] = &[
    Sample {
        name: "highpass",
        label: "Waveguide high-pass filter: WR-90 filled with εr = 2 on both sides of a 20 mm empty section, cut off below 6.56 GHz",
        script: include_str!("../samples/highpass.json"),
    },
    Sample {
        name: "iris-bandpass",
        label: "Inductive-iris band-pass filter: three WR-90 cavities between four 2 mm irises (12.75, 9.25, 9.25, 12.75 mm), passband 9.75–10.25 GHz",
        script: include_str!("../samples/iris-bandpass.json"),
    },
    Sample {
        name: "cavity",
        label: "Copper rectangular cavity, 22.86 × 10.16 × 30 mm: its four lowest resonances (TE101 at 8.24 GHz) and Q factors",
        script: include_str!("../samples/cavity.json"),
    },
    Sample {
        name: "cylinder-cavity",
        label: "Copper cylindrical cavity, radius 10 mm, 30 mm long, meshed with unstructured tetrahedra: TE111 and TM010 against the Bessel zeros",
        script: include_str!("../samples/cylinder-cavity.json"),
    },
];

pub fn find(name: &str) -> Option<&'static Sample> {
    SAMPLES.iter().find(|s| s.name == name)
}

pub fn names() -> Vec<&'static str> {
    SAMPLES.iter().map(|s| s.name).collect()
}
