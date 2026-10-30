//! EMCraft's templates, all in one place: the models the start page and `file.sample` open,
//! each a command script (`scripts/*.json`) whose dimensions are project variables (edited
//! with `project.variable`, which rebuilds the model), and the geometry builders those scripts
//! call when a dimension is a count (the iris filter's irises).
//!
//! A template is added here and nowhere else: its script in `scripts/`, where its parameters
//! show on the model in `scripts/<name>.marks.json`, a row in [`TEMPLATES`], and a builder
//! module when its shape needs one.
//!
//! ```
//! let t = emcraft_templates::find("iris-bandpass").map(|t| t.name);
//! assert_eq!(t, Some("iris-bandpass"));
//! assert!(emcraft_templates::find("nothing").is_none());
//! ```

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod iris;

pub use iris::{IrisFilter, MAX_IRISES, iris_sections};

/// A template.
pub struct Template {
    pub name: &'static str,
    pub label: &'static str,
    /// The command script (a JSON array of `{"command", "params"}`).
    pub script: &'static str,
    /// Where each parameter shows on the model: `{parameter: {"from": [x, y, z], "to": [x, y,
    /// z]}}`, a dimension between two points in the display units, each coordinate a number or
    /// an expression of the parameters (`"=(a+w)/2"`); `{}` for none.
    pub marks: &'static str,
}

pub const TEMPLATES: &[Template] = &[
    Template {
        name: "highpass",
        label: "Waveguide high-pass filter: WR-90 filled with εr = 2 on both sides of a 20 mm empty section, cut off below 6.56 GHz",
        script: include_str!("../scripts/highpass.json"),
        marks: include_str!("../scripts/highpass.marks.json"),
    },
    Template {
        name: "iris-bandpass",
        label: "Inductive-iris band-pass filter: three WR-90 cavities between four 2 mm irises (12.75, 9.25, 9.25, 12.75 mm), passband 9.75–10.25 GHz",
        script: include_str!("../scripts/iris-bandpass.json"),
        marks: include_str!("../scripts/iris-bandpass.marks.json"),
    },
    Template {
        name: "cavity",
        label: "Copper rectangular cavity, 22.86 × 10.16 × 30 mm: its four lowest resonances (TE101 at 8.24 GHz) and Q factors",
        script: include_str!("../scripts/cavity.json"),
        marks: include_str!("../scripts/cavity.marks.json"),
    },
    Template {
        name: "cylinder-cavity",
        label: "Copper cylindrical cavity, radius 10 mm, 30 mm long, meshed with unstructured tetrahedra: TE111 and TM010 against the Bessel zeros",
        script: include_str!("../scripts/cylinder-cavity.json"),
        marks: include_str!("../scripts/cylinder-cavity.marks.json"),
    },
    Template {
        name: "circular-guide",
        label: "Circular waveguide, radius 10 mm, 30 mm long, on an unstructured mesh: TE11 (cut-off 8.79 GHz) through ports closed by the Bessel modes",
        script: include_str!("../scripts/circular-guide.json"),
        marks: include_str!("../scripts/circular-guide.marks.json"),
    },
    Template {
        name: "coaxial-line",
        label: "PTFE coaxial line, conductors of radii 1 and 3.5 mm, 20 mm long: TEM through ports whose modes are solved on the annulus",
        script: include_str!("../scripts/coaxial-line.json"),
        marks: include_str!("../scripts/coaxial-line.marks.json"),
    },
    Template {
        name: "coupled-filter",
        label: "Parallel-coupled stripline band-pass filter: three half-wave resonators coupled edge to edge between 50 Ω feeds, PTFE between planes 4 mm apart, 4.66–5.34 GHz (Cohn 1958)",
        script: include_str!("../scripts/coupled-filter.json"),
        marks: "{}",
    },
    Template {
        name: "coupler",
        label: "Coupled-line directional coupler in stripline: two strips a quarter wave long at 5 GHz, each end a terminal of its own (four ports, 50 Ω), 10 dB coupled backwards, the far end of the other strip isolated (Pozar §7.6)",
        script: include_str!("../scripts/coupler.json"),
        marks: include_str!("../scripts/coupler.marks.json"),
    },
    Template {
        name: "inductive-post",
        label: "A 3 mm conducting post across WR-90, wall to wall, cut from the box by a Boolean difference: a strong shunt inductance",
        script: include_str!("../scripts/inductive-post.json"),
        marks: include_str!("../scripts/inductive-post.marks.json"),
    },
    Template {
        name: "loaded-sphere",
        label: "Spherical cavity, radius 10 mm, around a concentric ceramic sphere (εr = 4, radius 4 mm): two material regions joined node for node, the TM11 triplet at 11.26 GHz",
        script: include_str!("../scripts/loaded-sphere.json"),
        marks: include_str!("../scripts/loaded-sphere.marks.json"),
    },
    Template {
        name: "dipole",
        label: "A 1 mm current element at 10 GHz in a box lined with a perfectly matched layer: radiated power and directivity against the Hertzian dipole",
        script: include_str!("../scripts/dipole.json"),
        marks: "{}",
    },
    Template {
        name: "halfwave-dipole",
        label: "A half-wave wire dipole at 10 GHz (radius 0.5 mm, 1 mm gap) fed by a lumped port: input impedance, S11, power balance and pattern",
        script: include_str!("../scripts/halfwave-dipole.json"),
        marks: include_str!("../scripts/halfwave-dipole.marks.json"),
    },
    Template {
        name: "open-waveguide",
        label: "An open-ended WR-90 channel in a metal block radiating at 10 GHz, fed by a waveguide port on the channel's closed end: S11, gain and pattern",
        script: include_str!("../scripts/open-waveguide.json"),
        marks: "{}",
    },
    Template {
        name: "sphere-scattering",
        label: "A plane wave on a ceramic sphere (εr = 4, radius 5 mm) at 10 GHz: the scattered field and the radar cross-section against the Mie series",
        script: include_str!("../scripts/sphere-scattering.json"),
        marks: include_str!("../scripts/sphere-scattering.marks.json"),
    },
];

/// The template called `name`.
pub fn find(name: &str) -> Option<&'static Template> {
    TEMPLATES.iter().find(|t| t.name == name)
}

/// The templates' names, in [`TEMPLATES`]' order.
pub fn names() -> Vec<&'static str> {
    TEMPLATES.iter().map(|t| t.name).collect()
}
