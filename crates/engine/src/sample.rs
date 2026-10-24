//! Sample models: command scripts built in, so `file.sample` and the desktop app's start page
//! open them without any file.

/// A sample.
pub struct Sample {
    pub name: &'static str,
    pub label: &'static str,
    /// The command script (a JSON array of `{"command", "params"}`).
    pub script: &'static str,
}

pub const SAMPLES: &[Sample] = &[Sample {
    name: "highpass",
    label: "Waveguide high-pass filter: WR-90 filled with εr = 2 on both sides of a 20 mm empty section, cut off below 6.56 GHz",
    script: include_str!("../samples/highpass.json"),
}];

pub fn find(name: &str) -> Option<&'static Sample> {
    SAMPLES.iter().find(|s| s.name == name)
}

pub fn names() -> Vec<&'static str> {
    SAMPLES.iter().map(|s| s.name).collect()
}
