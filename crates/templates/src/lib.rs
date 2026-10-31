//! EMCraft's templates, all in one place: the models the start page and `file.sample` open,
//! each a command script (`scripts/*.json`) whose dimensions are project variables (edited
//! with `project.variable`, which rebuilds the model), and the geometry builders those scripts
//! call when a dimension is a count (the iris filter's irises).
//!
//! A template is added here and nowhere else: its script in `scripts/`, where its parameters
//! show on the model in `scripts/<name>.marks.json`, a row in [`TEMPLATES`], and a builder
//! module when its shape needs one. A template whose shape is a SolidCraft design carries the
//! design in `designs/` (a row in [`DESIGNS`]); its script opens it by `geometry.import` with
//! `path` `template:<file>` ([`design`]), the template's parameters passed to the design's.
//!
//! ```
//! let t = emcraft_templates::find("iris-bandpass").map(|t| t.name);
//! assert_eq!(t, Some("iris-bandpass"));
//! assert!(emcraft_templates::find("nothing").is_none());
//! ```

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod iris;

pub use emcraft_model::Analysis;
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
    /// The kinds of analysis it is made for, the first its own (the start page lists it under
    /// each).
    pub kinds: &'static [Analysis],
}

pub const TEMPLATES: &[Template] = &[
    Template {
        name: "highpass",
        label: "Waveguide high-pass filter: WR-90 filled with εr = 2 on both sides of a 20 mm empty section, cut off below 6.56 GHz",
        script: include_str!("../scripts/highpass.json"),
        marks: include_str!("../scripts/highpass.marks.json"),
        kinds: &[Analysis::Frequency, Analysis::TimeDomain],
    },
    Template {
        name: "iris-bandpass",
        label: "Inductive-iris band-pass filter: three WR-90 cavities between four 2 mm irises (12.75, 9.25, 9.25, 12.75 mm), passband 9.75–10.25 GHz",
        script: include_str!("../scripts/iris-bandpass.json"),
        marks: include_str!("../scripts/iris-bandpass.marks.json"),
        kinds: &[Analysis::Frequency, Analysis::TimeDomain],
    },
    Template {
        name: "straight",
        label: "Straight rectangular waveguide, WR-90 (a × b = 22.86 × 10.16 mm) by default, 50 mm long, swept over its band (1.25 to 1.89 times the TE10 cut-off): set a and b to another EIA size (WR-62: 15.80 × 7.90)",
        script: include_str!("../scripts/straight.json"),
        marks: include_str!("../scripts/straight.marks.json"),
        kinds: &[Analysis::Frequency],
    },
    Template {
        name: "h-bend",
        label: "H-plane 90° bend in WR-90, its outer corner mitred by m on each wall (0: a square corner); ports on the arms' ends across y and x",
        script: include_str!("../scripts/h-bend.json"),
        marks: include_str!("../scripts/h-bend.marks.json"),
        kinds: &[Analysis::Frequency],
    },
    Template {
        name: "e-bend",
        label: "E-plane 90° bend in WR-90, its outer corner mitred by m on each wall (0: a square corner); ports on the arms' ends across y and x",
        script: include_str!("../scripts/e-bend.json"),
        marks: include_str!("../scripts/e-bend.marks.json"),
        kinds: &[Analysis::Frequency],
    },
    Template {
        name: "h-tee",
        label: "H-plane T-junction in WR-90: a side arm off the middle of a straight guide, three ports",
        script: include_str!("../scripts/h-tee.json"),
        marks: include_str!("../scripts/h-tee.marks.json"),
        kinds: &[Analysis::Frequency],
    },
    // Junctions, radius bends and couplers.
    Template {
        name: "h-bend-radius",
        label: "H-plane 90° radius bend in WR-90: inner radius r (28.6 mm: a mean radius of one guide wavelength at 10 GHz), each arc drawn in 18 straight sides; ports on the arms' ends across y and x",
        script: include_str!("../scripts/h-bend-radius.json"),
        marks: include_str!("../scripts/h-bend-radius.marks.json"),
        kinds: &[Analysis::Frequency],
    },
    Template {
        name: "e-bend-radius",
        label: "E-plane 90° radius bend in WR-90: inner radius r (35 mm: a mean radius of one guide wavelength at 10 GHz), each arc drawn in 18 straight sides; ports on the arms' ends across y and x",
        script: include_str!("../scripts/e-bend-radius.json"),
        marks: include_str!("../scripts/e-bend-radius.marks.json"),
        kinds: &[Analysis::Frequency],
    },
    Template {
        name: "e-tee",
        label: "E-plane T-junction in WR-90: a side arm off the broad wall of a straight guide, three ports; from the side arm the two ends are fed equally in antiphase",
        script: include_str!("../scripts/e-tee.json"),
        marks: include_str!("../scripts/e-tee.marks.json"),
        kinds: &[Analysis::Frequency],
    },
    Template {
        name: "magic-tee",
        label: "Magic tee (hybrid T) in WR-90: collinear arms 1 and 2, the H arm 3 and the E arm 4, each matched by posts at 9.5 GHz (two in the H arm, one across the E arm); 3 and 4 isolated, each split equally between 1 and 2, in phase from the H arm and in antiphase from the E arm",
        script: include_str!("../scripts/magic-tee.json"),
        marks: include_str!("../scripts/magic-tee.marks.json"),
        kinds: &[Analysis::Frequency],
    },
    Template {
        name: "cross-guide-coupler",
        label: "Cross-guide directional coupler: two WR-90 guides crossed at right angles over a common broad wall t thick, coupled through two round holes on the diagonal of their overlap; four ports, about 30 dB coupled",
        script: include_str!("../scripts/cross-guide-coupler.json"),
        marks: include_str!("../scripts/cross-guide-coupler.marks.json"),
        kinds: &[Analysis::Frequency],
    },
    Template {
        name: "cavity",
        label: "Copper rectangular cavity, 22.86 × 10.16 × 30 mm: its four lowest resonances (TE101 at 8.24 GHz) and Q factors",
        script: include_str!("../scripts/cavity.json"),
        marks: include_str!("../scripts/cavity.marks.json"),
        kinds: &[Analysis::Eigenmodes],
    },
    Template {
        name: "cylinder-cavity",
        label: "Copper cylindrical cavity, radius 10 mm, 30 mm long, meshed with unstructured tetrahedra: TE111 and TM010 against the Bessel zeros",
        script: include_str!("../scripts/cylinder-cavity.json"),
        marks: include_str!("../scripts/cylinder-cavity.marks.json"),
        kinds: &[Analysis::Eigenmodes],
    },
    Template {
        name: "circular-guide",
        label: "Circular waveguide, radius 10 mm, 30 mm long, on an unstructured mesh: TE11 (cut-off 8.79 GHz) through ports closed by the Bessel modes",
        script: include_str!("../scripts/circular-guide.json"),
        marks: include_str!("../scripts/circular-guide.marks.json"),
        kinds: &[Analysis::Frequency],
    },
    Template {
        name: "coaxial-line",
        label: "PTFE coaxial line, conductors of radii 1 and 3.5 mm, 20 mm long: TEM through ports whose modes are solved on the annulus",
        script: include_str!("../scripts/coaxial-line.json"),
        marks: include_str!("../scripts/coaxial-line.marks.json"),
        kinds: &[Analysis::Frequency],
    },
    Template {
        name: "coupled-filter",
        label: "Parallel-coupled stripline band-pass filter: three half-wave resonators coupled edge to edge between 50 Ω feeds, PTFE between planes 4 mm apart, 4.66–5.34 GHz (Cohn 1958)",
        script: include_str!("../scripts/coupled-filter.json"),
        marks: "{}",
        kinds: &[Analysis::Frequency],
    },
    Template {
        name: "coupler",
        label: "Coupled-line directional coupler in stripline: two strips a quarter wave long at 5 GHz, each end a terminal of its own (four ports, 50 Ω), 10 dB coupled backwards, the far end of the other strip isolated (Pozar §7.6)",
        script: include_str!("../scripts/coupler.json"),
        marks: include_str!("../scripts/coupler.marks.json"),
        kinds: &[Analysis::Frequency],
    },
    Template {
        name: "inductive-post",
        label: "A 3 mm conducting post across WR-90, wall to wall, cut from the box by a Boolean difference: a strong shunt inductance",
        script: include_str!("../scripts/inductive-post.json"),
        marks: include_str!("../scripts/inductive-post.marks.json"),
        kinds: &[Analysis::Frequency],
    },
    Template {
        name: "loaded-sphere",
        label: "Spherical cavity, radius 10 mm, around a concentric ceramic sphere (εr = 4, radius 4 mm): two material regions joined node for node, the TM11 triplet at 11.26 GHz",
        script: include_str!("../scripts/loaded-sphere.json"),
        marks: include_str!("../scripts/loaded-sphere.marks.json"),
        kinds: &[Analysis::Eigenmodes],
    },
    Template {
        name: "dipole",
        label: "A 1 mm current element at 10 GHz in a box lined with a perfectly matched layer: radiated power and directivity against the Hertzian dipole",
        script: include_str!("../scripts/dipole.json"),
        marks: "{}",
        kinds: &[Analysis::Radiation],
    },
    Template {
        name: "halfwave-dipole",
        label: "A half-wave wire dipole at 10 GHz (radius 0.5 mm, 1 mm gap) fed by a lumped port: input impedance, S11, power balance and pattern",
        script: include_str!("../scripts/halfwave-dipole.json"),
        marks: include_str!("../scripts/halfwave-dipole.marks.json"),
        kinds: &[Analysis::Radiation],
    },
    Template {
        name: "open-waveguide",
        label: "An open-ended WR-90 channel in a metal block radiating at 10 GHz, fed by a waveguide port on the channel's closed end: S11, gain and pattern",
        script: include_str!("../scripts/open-waveguide.json"),
        marks: "{}",
        kinds: &[Analysis::Radiation],
    },
    Template {
        name: "taper",
        label: "Linear taper from WR-90 to WR-75 (22.86 × 10.16 to 19.05 × 9.525 mm) over 40 mm, a SolidCraft design: its small reflection over the band both guides share, against the theory of small reflections",
        script: include_str!("../scripts/taper.json"),
        marks: include_str!("../scripts/taper.marks.json"),
        kinds: &[Analysis::Frequency],
    },
    Template {
        name: "termination",
        label: "Matched load: a lossy wedge (εr = 1.5, tan δ = 0.5) 90 mm long, 1 mm clear of the walls, against the short at the end of a WR-90 guide; one port: |S11| below 0.06 over the band",
        script: include_str!("../scripts/termination.json"),
        marks: include_str!("../scripts/termination.marks.json"),
        kinds: &[Analysis::Frequency],
    },
    Template {
        name: "horn",
        label: "Pyramidal horn fed by WR-90, a SolidCraft design: an A × B mouth h past a feed f long, walls t thick, radiating at 10 GHz in an open box: S11, gain and pattern",
        script: include_str!("../scripts/horn.json"),
        marks: include_str!("../scripts/horn.marks.json"),
        kinds: &[Analysis::Radiation],
    },
    Template {
        name: "sphere-scattering",
        label: "A plane wave on a ceramic sphere (εr = 4, radius 5 mm) at 10 GHz: the scattered field and the radar cross-section against the Mie series",
        script: include_str!("../scripts/sphere-scattering.json"),
        marks: include_str!("../scripts/sphere-scattering.marks.json"),
        kinds: &[Analysis::Radiation],
    },
];

/// A SolidCraft design a template is built from, compiled in: no file is read when a template
/// opens it.
pub struct Design {
    /// Its file name, `<name>.solidcraft`.
    pub file: &'static str,
    pub bytes: &'static [u8],
}

/// How a script names a template's design: `template:<file>` (`geometry.import`'s `path`).
pub const DESIGN_SCHEME: &str = "template:";

/// The designs templates are built from, each written with SolidCraft's document API by the
/// program kept beside it (`designs/make-<name>.rs.txt`).
pub const DESIGNS: &[Design] = &[
    Design { file: "taper.solidcraft", bytes: include_bytes!("../designs/taper.solidcraft") },
    Design { file: "horn.solidcraft", bytes: include_bytes!("../designs/horn.solidcraft") },
];

/// The design a path names, `template:<file>`; `None` for any other path (a file of the
/// user's) or a design no template has.
///
/// ```
/// let taper = emcraft_templates::design("template:taper.solidcraft");
/// assert!(taper.is_some_and(|b| !b.is_empty()));
/// assert!(emcraft_templates::design("taper.solidcraft").is_none());
/// assert!(emcraft_templates::design("template:nothing.solidcraft").is_none());
/// ```
pub fn design(path: &str) -> Option<&'static [u8]> {
    let file = path.strip_prefix(DESIGN_SCHEME)?;
    DESIGNS.iter().find(|d| d.file == file).map(|d| d.bytes)
}

/// The template called `name`.
pub fn find(name: &str) -> Option<&'static Template> {
    TEMPLATES.iter().find(|t| t.name == name)
}

/// The templates made for `kind`, in [`TEMPLATES`]' order.
///
/// ```
/// use emcraft_templates::{Analysis, made_for};
/// assert!(made_for(Analysis::Eigenmodes).any(|t| t.name == "cavity"));
/// assert!(!made_for(Analysis::Eigenmodes).any(|t| t.name == "dipole"));
/// ```
pub fn made_for(kind: Analysis) -> impl Iterator<Item = &'static Template> {
    TEMPLATES.iter().filter(move |t| t.kinds.contains(&kind))
}

/// The templates' names, in [`TEMPLATES`]' order.
pub fn names() -> Vec<&'static str> {
    TEMPLATES.iter().map(|t| t.name).collect()
}
