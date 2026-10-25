//! The commands.

use std::sync::Arc;

use emcraft_fem::Order;
use emcraft_model::units::{Frequency, Length, decibels};
use emcraft_model::{Axis, Combine, End, Feature, Inclusion, Material, Port, Section, Shape, Solid, Waveguide};
use emcraft_solver::{Complex64 as C, Driven};
use serde_json::{Value, json};

use crate::params::{get, integer, number, required_number, required_string, string};
use crate::view::{self, FieldView, Plane};
use crate::{CommandSpec, EngineError, ParamSpec, Quantity, Result, Session, Sweep};

const fn p(name: &'static str, kind: &'static str, quantity: Quantity, required: bool, doc: &'static str) -> ParamSpec {
    ParamSpec { name, kind, quantity, required, doc }
}

use Quantity::{Frequency as F, Length as L, None as N};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "units.set",
        label: "Display units",
        doc: "Sets the units parameters are read in and replies written in.",
        params: &[p("length", "string", N, false, "`mm` or `m`"), p("frequency", "string", N, false, "`GHz`, `MHz` or `Hz`")],
        run: units_set,
    },
    CommandSpec {
        id: "model.material.create",
        label: "Create material",
        doc: "A linear, isotropic material, lossless unless it has a loss tangent.",
        params: &[
            p("name", "string", N, true, "unique name"),
            p("eps_r", "number", N, true, "relative permittivity εr > 0"),
            p("mu_r", "number", N, false, "relative permeability μr > 0 (default 1)"),
            p("loss_tangent", "number", N, false, "dielectric loss tangent tan δ ≥ 0 (default 0): ε = εr(1 − j tan δ)"),
        ],
        run: material_create,
    },
    CommandSpec {
        id: "geometry.waveguide",
        label: "Rectangular waveguide",
        doc: "A rectangular waveguide along z with perfectly conducting walls: cross-section `width` (x) × `height` (y) from the origin, filled along z in sections, each with its own aperture across x (H-plane steps, inductive irises; neighbouring apertures must nest). Replaces any geometry, mesh and results.",
        params: &[
            p("width", "number", L, true, "broad wall a (x)"),
            p("height", "number", L, true, "narrow wall b (y)"),
            p(
                "sections",
                "array",
                N,
                true,
                "`[{\"length\", \"material\", \"width\"?, \"offset\"?}]` from z = 0: a section may narrow the broad wall to an aperture `width` wide (lengths), starting at x = `offset` (centred when left out)",
            ),
        ],
        run: waveguide,
    },
    CommandSpec {
        id: "geometry.solid",
        label: "Primitive body",
        doc: "A body of one material with conducting walls, meshed with unstructured tetrahedra: a box [0, x] × [0, y] × [0, z] (`size`), a cylinder about z from z = 0 (`radius`, `length`) or a sphere about the origin (`radius`). Its end faces (z = 0, z = length) can carry ports; a closed body is a cavity for `solve.eigenmodes`. Replaces any geometry, mesh and results.",
        params: &[
            p("shape", "string", N, true, "`box`, `cylinder` or `sphere`"),
            p("size", "array", L, false, "the box's [x, y, z]"),
            p("radius", "number", L, false, "the cylinder's or the sphere's radius"),
            p("length", "number", L, false, "the cylinder's length along z"),
            p("material", "string", N, false, "the filling (default vacuum)"),
        ],
        run: solid,
    },
    CommandSpec {
        id: "geometry.import",
        label: "Import a surface",
        doc: "A body bounded by a closed triangulated surface read from an STL (binary or ASCII) or 3MF file, meshed with unstructured tetrahedra like geometry.solid. Its triangles are grouped into faces by the angle between neighbours, so its sharp edges stay edges. The faces on its lowest and highest z planes can carry ports when they are a whole rectangle or a disc. Replaces any geometry, mesh and results.",
        params: &[
            p("path", "string", N, true, "the file (`.stl` or `.3mf`)"),
            p("unit", "number", L, false, "an STL file's length unit in display units (default 1, i.e. the display unit); 3MF files carry their own"),
            p("angle", "number", N, false, "neighbouring triangles turning by more than this many degrees are on different faces (default 30)"),
            p("material", "string", N, false, "the filling (default vacuum)"),
        ],
        run: import,
    },
    CommandSpec {
        id: "geometry.inclusion",
        label: "Inclusion",
        doc: "A body inside the solid: a dielectric region (`material`) or, without one, a conductor. The same shapes as geometry.solid, placed with their origin at `at`; it must lie inside the solid, apart from other inclusions. The regions are meshed apart and joined node for node across the interface.",
        params: &[
            p("shape", "string", N, true, "`box`, `cylinder` or `sphere`"),
            p("size", "array", L, false, "the box's [x, y, z]"),
            p("radius", "number", L, false, "the cylinder's or the sphere's radius"),
            p("length", "number", L, false, "the cylinder's length along z"),
            p("at", "array", L, false, "where its origin goes (default [0, 0, 0])"),
            p("material", "string", N, false, "its filling; a perfect conductor when left out"),
        ],
        run: inclusion,
    },
    CommandSpec {
        id: "geometry.feature",
        label: "Combine a body",
        doc: "Combines a primitive with the solid: `add` (union: a stub, a housing), `subtract` (difference: a post, a screw, a notch; its surface becomes conducting wall) or `intersect`. The same shapes as geometry.solid, its origin at `at` and its z axis turned onto `axis`. Faces flush with the solid's are handled (a post may run wall to wall); the surfaces must otherwise meet cleanly. A port's end face must stay whole.",
        params: &[
            p("combine", "string", N, true, "`add`, `subtract` or `intersect`"),
            p("shape", "string", N, true, "`box`, `cylinder` or `sphere`"),
            p("size", "array", L, false, "the box's [x, y, z] (in its own axes)"),
            p("radius", "number", L, false, "the cylinder's or the sphere's radius"),
            p("length", "number", L, false, "the cylinder's length along its axis"),
            p("at", "array", L, false, "where its origin goes (default [0, 0, 0])"),
            p("axis", "string", N, false, "`x`, `y` or `z` (default): where its own z axis points"),
        ],
        run: feature,
    },
    CommandSpec {
        id: "mesh.generate",
        label: "Mesh",
        doc: "Meshes the waveguide with tetrahedra of about `size` on a side (six per cell of a structured grid; every section boundary and aperture wall is a grid plane), graded towards the re-entrant edges of steps and irises. A solid is meshed without structure, its tetrahedra on curved walls and interfaces curved (quadratic geometry) unless `curved` is false.",
        params: &[
            p("size", "number", L, false, "target element size (required without `per_wavelength`; with it, the largest size)"),
            p(
                "per_wavelength",
                "number",
                N,
                false,
                "elements per wavelength in each material at `frequency` (λ = c/(f√(εr μr))): a solid's regions each at their own size, a waveguide at the smallest",
            ),
            p("frequency", "number", F, false, "the highest frequency the mesh must resolve (with `per_wavelength`)"),
            p("edge", "number", N, false, "element size at step and iris edges, as a fraction of `size` (default 0.25; 1 = uniform)"),
            p("growth", "number", N, false, "element growth away from those edges (default 1.5)"),
            p(
                "size_y",
                "number",
                L,
                false,
                "element size across the narrow wall (default `size`); an H-plane device's TE10 fields do not vary along y, so one layer (the height) is exact there",
            ),
            p(
                "curved",
                "boolean",
                N,
                false,
                "solids: bend the edges on curved surfaces onto them (default true; false leaves flat facets inscribed in the surface)",
            ),
        ],
        run: mesh_generate,
    },
    CommandSpec {
        id: "model.walls",
        label: "Wall conductivity",
        doc: "The conductivity of the guide's walls, irises and steps: losses by the Leontovich surface-impedance condition (good conductors, skin depth far below the guide). Without `conductivity` the walls are perfect conductors.",
        params: &[p("conductivity", "number", N, false, "σ in S/m (copper 5.8e7); leave out for perfect conductors")],
        run: walls,
    },
    CommandSpec {
        id: "model.port.create",
        label: "Create port",
        doc: "A waveguide port on an end face, matched to the guide's lowest modes. TE10 is excited and read; the other modes absorb what else reaches the port. With `face` the port is a rectangle of a z plane inside a solid instead — a guide's closed end inside a body, say, feeding an antenna in an open box; `end` then says which side of the plane the guide lies (start: above it, end: below it).",
        params: &[
            p("name", "string", N, true, "unique name"),
            p("end", "string", N, true, "`start` (z = 0) or `end` (z = length); with `face`, the side the guide lies"),
            p("modes", "integer", N, false, "modes in the port condition, 1 to 32 (default 4)"),
            p("face", "array", L, false, "[z, x0, y0, a, b]: the rectangle [x0, x0 + a] × [y0, y0 + b] of the plane at z"),
        ],
        run: port_create,
    },
    CommandSpec {
        id: "solve.frequency",
        label: "Frequency sweep",
        doc: "Solves the driven problem at each frequency, every port excited in turn by a unit TE10 wave: S-parameters and fields. Frequencies are `frequencies`, or `points` evenly from `start` to `stop`.",
        params: &[
            p("frequencies", "array", F, false, "the frequencies"),
            p("start", "number", F, false, "first frequency"),
            p("stop", "number", F, false, "last frequency"),
            p("points", "integer", N, false, "frequencies from start to stop (default 11)"),
            p("order", "integer", N, false, "element order: 1 (Whitney) or 2 (default)"),
        ],
        run: solve_frequency,
    },
    CommandSpec {
        id: "solve.time_domain",
        label: "Time-domain solve",
        doc: "The S-parameters (port 1 excited) by the time-domain solver: finite integration on a hexahedral grid of cells about `cell` on a side, a TE10 pulse over the band, PML-terminated feeds, Fourier transforms at the frequencies. An independent check of the frequency-domain FEM; lossless models only. For an open model (model.open) it solves the antenna instead: its lumped ports and dipoles on a uniform grid with a CPML on all sides, the ports' impedances and S11 and the radiated power at each frequency, the far field from a Huygens box (results.far_field and view.pattern with `solver` time_domain). Gaps and dipoles must lie along grid axes.",
        params: &[
            p("frequencies", "array", F, false, "the frequencies"),
            p("start", "number", F, false, "first frequency"),
            p("stop", "number", F, false, "last frequency"),
            p("points", "integer", N, false, "frequencies from start to stop (default 11)"),
            p("cell", "number", L, false, "the cell size (default 1 mm)"),
        ],
        run: solve_time_domain,
    },
    CommandSpec {
        id: "solve.eigenmodes",
        label: "Eigenmodes",
        doc: "The resonances of the guide closed at both ends by conducting walls (a cavity; ports are left out): the `count` modes nearest `near`, with Q factors from the walls' conductivity and the dielectrics' loss tangents by perturbation. Put `near` a little above the modes wanted: modes up to about √2 times it are found before the curl's null space.",
        params: &[
            p("near", "number", F, true, "the frequency the modes are sought around"),
            p("count", "integer", N, false, "modes, 1 to 50 (default 5)"),
            p("order", "integer", N, false, "element order: 1 or 2 (default)"),
        ],
        run: solve_eigenmodes,
    },
    CommandSpec {
        id: "results.eigenmodes",
        label: "Eigenmode table",
        doc: "The eigenmodes' frequencies and Q factors; for a cavity of one uniform section, the closed-form resonances too.",
        params: &[],
        run: eigenmode_table,
    },
    CommandSpec {
        id: "results.sparameters",
        label: "S-parameters",
        doc: "The S-parameter table of the last sweep: each S[p][q] (at port p for a wave into port q) as real, imaginary, magnitude, dB and phase (degrees).",
        params: &[],
        run: sparameters,
    },
    CommandSpec {
        id: "results.compare",
        label: "Compare with theory",
        doc: "Compares the sweep's S11 and S21 with the reference: the exact TE10 solution when every section has the full cross-section, mode matching (`modes` TE_m0 modes in the widest section) when the aperture changes. The errors per frequency and the worst.",
        params: &[p("modes", "integer", N, false, "TE_m0 modes in the widest section for mode matching, 10 to 1000 (default 160)")],
        run: compare,
    },
    CommandSpec {
        id: "results.field_error",
        label: "Field error against theory",
        doc: "The relative L2 errors ‖F − F_exact‖/‖F_exact‖ of E and H over the whole guide at a solved frequency, against the exact TE10 fields (port 1 excited).",
        params: &[p("frequency", "number", F, true, "a solved frequency")],
        run: field_error,
    },
    CommandSpec {
        id: "results.probe",
        label: "Probe fields",
        doc: "E (V/m) and H (A/m), complex, at points, for a solved frequency and an excited port.",
        params: &[
            p("frequency", "number", F, true, "a solved frequency"),
            p("at", "array", L, true, "points `[[x, y, z], …]`"),
            p("excitation", "string", N, false, "the excited port (default the first)"),
        ],
        run: probe,
    },
    CommandSpec {
        id: "view.field",
        label: "Field picture",
        doc: "Writes a PNG of the fields on a plane: a component as colour and the in-plane E and H as arrows, Re(F e^jωt) at a phase.",
        params: &[
            p("path", "string", N, true, "PNG file to write"),
            p("frequency", "number", F, false, "a solved frequency (the nearest is used); required unless `mode` is given"),
            p("mode", "integer", N, false, "an eigenmode (1 = the lowest) instead of a driven solution"),
            p("plane", "string", N, false, "`h` (H-plane, default), `e` (E-plane) or `cross` (cross-section)"),
            p("at", "number", L, false, "the plane's position along its normal (default the middle)"),
            p("component", "string", N, false, "colour: ex, ey (default), ez, hx, hy, hz, |e|, |h|"),
            p("arrows", "array", N, false, "vector fields drawn: `[\"e\", \"h\"]` (default both)"),
            p("phase", "number", N, false, "ωt in degrees (default 0)"),
            p("excitation", "string", N, false, "the excited port (default the first)"),
            p("width", "integer", N, false, "picture width in pixels (default 1100)"),
        ],
        run: view_field,
    },
    CommandSpec {
        id: "view.sparameters",
        label: "S-parameter chart",
        doc: "Writes a PNG of |S11| and |S21| in dB against frequency, the solved points over the exact TE10 curves, with the cut-off frequencies marked.",
        params: &[
            p("path", "string", N, true, "PNG file to write"),
            p("width", "integer", N, false, "pixels (default 1000)"),
            p("height", "integer", N, false, "pixels (default 600)"),
        ],
        run: view_sparameters,
    },
    CommandSpec {
        id: "model.open",
        label: "Open boundary",
        doc: "Lines every face of a box solid with a perfectly matched layer of `thickness` (conducting walls behind it), for radiation and scattering: an anisotropic medium of stretched coordinates, s = 1 − j(σ₀/k₀)(ξ/d)², designed for 1e-6 reflection at normal incidence. A quarter to half a wavelength is typical; leave as much again between what radiates and the layer (the Huygens surface for the far field lies between them, and wants a couple of elements' distance from sources' near fields). Without `thickness`, closes the box again.",
        params: &[
            p("thickness", "number", L, false, "the layer's thickness"),
            p("reflection", "number", N, false, "the reflection at normal incidence the layer's profile is designed for, 0 to 1 (default 1e-6)"),
        ],
        run: crate::open::model_open,
    },
    CommandSpec {
        id: "source.dipole",
        label: "Dipole source",
        doc: "A short current element (Hertzian dipole) driving an open problem: `current` (A, peak) over `length` along `direction` at `at`.",
        params: &[
            p("at", "array", L, true, "its position [x, y, z]"),
            p("direction", "array", N, false, "its axis (default [0, 0, 1])"),
            p("length", "number", L, true, "its length (the moment is current × length)"),
            p("current", "number", N, false, "its current in amperes, peak (default 1)"),
        ],
        run: crate::open::source_dipole,
    },
    CommandSpec {
        id: "source.plane_wave",
        label: "Plane wave",
        doc: "A plane wave of 1 V/m on the model's dielectric and conducting bodies: the solve finds the field they scatter (on conductors n × E_s = −n × E_inc), its radar cross-section and the power it carries away.",
        params: &[
            p("direction", "array", N, false, "the direction it travels (default [0, 0, 1])"),
            p("polarization", "array", N, false, "its electric field's direction, across `direction` (default [1, 0, 0])"),
        ],
        run: crate::open::source_plane_wave,
    },
    CommandSpec {
        id: "model.port.lumped",
        label: "Lumped port",
        doc: "A lumped (discrete) port: the feed of an antenna, a gap from `from` to `to` (usually between two conductors) across which a source of `voltage` at `phase` in series with `impedance` drives current from `from` towards `to`. solve.radiation reports its input (or, with several ports on, active) impedance, S11 against `impedance` and the power it delivers.",
        params: &[
            p("name", "string", N, true, "port name"),
            p("from", "array", L, true, "the gap's start [x, y, z]"),
            p("to", "array", L, true, "the gap's end [x, y, z]"),
            p("impedance", "number", N, false, "the source's internal impedance in ohms (default 50)"),
            p("voltage", "number", N, false, "the source's voltage in volts, peak (default 1)"),
            p("phase", "number", N, false, "the source's phase in degrees (default 0)"),
            p(
                "radius",
                "number",
                L,
                false,
                "spread the current round a band of this radius about the gap's line (a wire's radius, for a gap cut in a wire); default 0, the line itself",
            ),
        ],
        run: crate::open::port_lumped,
    },
    CommandSpec { id: "source.clear", label: "Clear sources", doc: "Removes every source.", params: &[], run: crate::open::source_clear },
    CommandSpec {
        id: "solve.radiation",
        label: "Radiation",
        doc: "Solves the open problem at one frequency: the field the sources radiate (or the dielectrics scatter), the power through a Huygens box between what radiates and the layer (a third of its thickness inside it, or halfway when that is nearer), with the closed form for a lone dipole (radiated power) or a lone dielectric or conducting sphere (the Mie series' scattering cross-section).",
        params: &[p("frequency", "number", F, true, "the frequency"), p("order", "integer", N, false, "element order: 1 or 2 (default)")],
        run: crate::open::solve_radiation,
    },
    CommandSpec {
        id: "results.far_field",
        label: "Far field",
        doc: "The far field (r e^{jkr} E_θ, E_φ) of the last radiation solve at polar angles `theta` and azimuth `phi` (degrees), by the near-to-far transform of the Huygens box's equivalent currents: directivity for sources, radar cross-section for a plane wave, with the closed forms where they apply.",
        params: &[
            p("theta", "array", N, false, "polar angles in degrees (default 0 to 180 by 10)"),
            p("phi", "number", N, false, "the azimuth in degrees (default 0)"),
            p(
                "solver",
                "string",
                N,
                false,
                "`fem` (solve.radiation) or `time_domain` (solve.time_domain); default whichever there is, the FEM's first",
            ),
            p("frequency", "number", F, false, "the time domain's frequency (the nearest solved)"),
        ],
        run: crate::open::results_far_field,
    },
    CommandSpec {
        id: "view.pattern",
        label: "Pattern chart",
        doc: "Writes a PNG of the far-field pattern against θ at azimuth `phi`: directivity in dBi for sources, radar cross-section in dBsm for a plane wave, over the closed form where it applies.",
        params: &[
            p("path", "string", N, true, "PNG file to write"),
            p("phi", "number", N, false, "the azimuth in degrees (default 0)"),
            p("points", "integer", N, false, "angles from 0 to 180° (default 91)"),
            p("width", "integer", N, false, "pixels (default 1000)"),
            p("height", "integer", N, false, "pixels (default 600)"),
            p(
                "solver",
                "string",
                N,
                false,
                "`fem` (solve.radiation) or `time_domain` (solve.time_domain); default whichever there is, the FEM's first",
            ),
            p("frequency", "number", F, false, "the time domain's frequency (the nearest solved)"),
        ],
        run: crate::open::view_pattern,
    },
    CommandSpec {
        id: "file.sample",
        label: "Open sample",
        doc: "Replaces the session with a sample model (`highpass`: the waveguide high-pass filter; `iris-bandpass`: an inductive-iris band-pass filter; `cavity`: a copper cavity's eigenmodes), built by its script; `solve: true` also runs its analysis.",
        params: &[p("name", "string", N, true, "sample name"), p("solve", "boolean", N, false, "run the sample's sweep (default false)")],
        run: file_sample,
    },
    CommandSpec { id: "model.summary", label: "Summary", doc: "The model, mesh and results in brief.", params: &[], run: summary },
];

fn units_set(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(l) = string(p, "length")? {
        s.display.length = match l {
            "mm" => Length::Millimetre,
            "m" => Length::Metre,
            _ => return Err(EngineError::Params("`length` must be mm or m".into())),
        };
    }
    if let Some(f) = string(p, "frequency")? {
        s.display.frequency = match f {
            "GHz" => Frequency::Gigahertz,
            "MHz" => Frequency::Megahertz,
            "Hz" => Frequency::Hertz,
            _ => return Err(EngineError::Params("`frequency` must be GHz, MHz or Hz".into())),
        };
    }
    Ok(json!({"length": s.display.length.symbol(), "frequency": s.display.frequency.symbol()}))
}

fn material_create(s: &mut Session, p: &Value) -> Result<Value> {
    let name = required_string(p, "name")?;
    let eps = required_number(p, "eps_r")?;
    let mu = number(p, "mu_r")?.unwrap_or(1.0);
    let tan_d = number(p, "loss_tangent")?.unwrap_or(0.0);
    s.model.add_material(Material::new(name, eps, mu)?.with_loss_tangent(tan_d)?)?;
    Ok(json!({"name": name}))
}

fn waveguide(s: &mut Session, p: &Value) -> Result<Value> {
    let width = s.positive_length(p, "width")?.ok_or_else(|| EngineError::Params("`width` is required".into()))?;
    let height = s.positive_length(p, "height")?.ok_or_else(|| EngineError::Params("`height` is required".into()))?;
    let list = get(p, "sections").and_then(Value::as_array).ok_or_else(|| EngineError::Params("`sections` must be an array".into()))?;
    if list.len() > 1000 {
        return Err(EngineError::Params("`sections`: at most 1000".into()));
    }
    let mut sections = Vec::with_capacity(list.len());
    for (i, sec) in list.iter().enumerate() {
        let length = s.positive_length(sec, "length").map_err(|e| EngineError::Params(format!("section {}: {e}", i + 1)))?;
        let length = length.ok_or_else(|| EngineError::Params(format!("section {}: `length` is required", i + 1)))?;
        let material = string(sec, "material")?.ok_or_else(|| EngineError::Params(format!("section {}: `material` is required", i + 1)))?;
        let width = s.positive_length(sec, "width").map_err(|e| EngineError::Params(format!("section {}: {e}", i + 1)))?;
        let offset = s.length(sec, "offset").map_err(|e| EngineError::Params(format!("section {}: {e}", i + 1)))?;
        let height = s.positive_length(sec, "height").map_err(|e| EngineError::Params(format!("section {}: {e}", i + 1)))?;
        let offset_y = s.length(sec, "offset_y").map_err(|e| EngineError::Params(format!("section {}: {e}", i + 1)))?;
        sections.push(Section { length, material: material.to_owned(), width, offset, height, offset_y });
    }
    s.model.set_waveguide(Waveguide { width, height, sections })?;
    s.sweep = None;
    Ok(json!({"length": s.show_length(s.model.waveguide.as_ref().map_or(0.0, |w| w.length()))}))
}

/// A shape from `shape`, `size`, `radius` and `length`.
fn shape_of(s: &Session, p: &Value) -> Result<Shape> {
    let req = |s: &Session, name: &str| -> Result<f64> {
        s.positive_length(p, name)?.ok_or_else(|| EngineError::Params(format!("`{name}` is required for this shape")))
    };
    Ok(match required_string(p, "shape")? {
        "box" => Shape::Box { size: point(s, p, "size")?.ok_or_else(|| EngineError::Params("`size` must be [x, y, z]".into()))? },
        "cylinder" => Shape::Cylinder { radius: req(s, "radius")?, length: req(s, "length")? },
        "sphere" => Shape::Sphere { radius: req(s, "radius")? },
        _ => return Err(EngineError::Params("`shape` must be box, cylinder or sphere".into())),
    })
}

/// Three lengths `[x, y, z]` in display units, in metres.
fn point(s: &Session, p: &Value, name: &str) -> Result<Option<[f64; 3]>> {
    let Some(v) = get(p, name) else { return Ok(None) };
    let list = v.as_array().filter(|a| a.len() == 3).ok_or_else(|| EngineError::Params(format!("`{name}` must be [x, y, z]")))?;
    let mut out = [0.0; 3];
    for (d, v) in out.iter_mut().zip(list) {
        *d = s.display.length.to_si(v.as_f64().filter(|x| x.is_finite()).ok_or_else(|| EngineError::Params(format!("`{name}` must be numbers")))?);
    }
    Ok(Some(out))
}

fn inclusion(s: &mut Session, p: &Value) -> Result<Value> {
    let shape = shape_of(s, p)?;
    let at = point(s, p, "at")?.unwrap_or([0.0; 3]);
    let material = string(p, "material")?.map(str::to_owned);
    s.model.add_inclusion(Inclusion { shape, at, material })?;
    s.sweep = None;
    s.eigen = None;
    Ok(json!({"inclusions": s.model.inclusions.len()}))
}

/// Largest geometry file read.
const MAX_IMPORT_BYTES: u64 = 512 << 20;

fn import(s: &mut Session, p: &Value) -> Result<Value> {
    let path = required_string(p, "path")?;
    let unit = s.display.length.to_si(number(p, "unit")?.unwrap_or(1.0));
    if !(unit.is_finite() && unit > 0.0) {
        return Err(EngineError::Params("`unit` must be a positive length".into()));
    }
    let angle = number(p, "angle")?.unwrap_or(30.0);
    if !(angle.is_finite() && angle > 0.0 && angle < 180.0) {
        return Err(EngineError::Params("`angle` must be between 0 and 180 degrees".into()));
    }
    let len = std::fs::metadata(path).map_err(|e| EngineError::Failed(format!("`{path}`: {e}")))?.len();
    if len > MAX_IMPORT_BYTES {
        return Err(EngineError::Failed(format!("`{path}` is {} MB; at most {} MB are read", len >> 20, MAX_IMPORT_BYTES >> 20)));
    }
    let bytes = std::fs::read(path).map_err(|e| EngineError::Failed(format!("`{path}`: {e}")))?;
    let mut surface = emcraft_io::read(path, &bytes, unit).map_err(|e| EngineError::Failed(e.to_string()))?;
    let faces = emcraft_io::split_faces(&mut surface, angle);
    let material = string(p, "material")?.unwrap_or("vacuum").to_owned();
    let triangles = surface.triangles.len();
    let shape = Shape::Surface { name: surface.name, positions: surface.positions, triangles: surface.triangles, tags: surface.tags };
    s.model.set_solid(Solid { shape, material })?;
    s.sweep = None;
    s.eigen = None;
    s.time_domain = None;
    let (lo, hi) = s.model.bounds().unwrap_or(([0.0; 3], [0.0; 3]));
    Ok(json!({
        "triangles": triangles,
        "faces": faces,
        "volume": s.model.solid.as_ref().map(|x| x.shape.volume()),
        "bounds": [lo.map(|v| s.display.length.from_si(v)), hi.map(|v| s.display.length.from_si(v))],
    }))
}

fn feature(s: &mut Session, p: &Value) -> Result<Value> {
    let combine = match required_string(p, "combine")? {
        "add" => Combine::Add,
        "subtract" => Combine::Subtract,
        "intersect" => Combine::Intersect,
        _ => return Err(EngineError::Params("`combine` must be add, subtract or intersect".into())),
    };
    let shape = shape_of(s, p)?;
    let at = point(s, p, "at")?.unwrap_or([0.0; 3]);
    let axis = match string(p, "axis")?.unwrap_or("z") {
        "x" => Axis::X,
        "y" => Axis::Y,
        "z" => Axis::Z,
        _ => return Err(EngineError::Params("`axis` must be x, y or z".into())),
    };
    s.model.add_feature(Feature { combine, shape, at, axis })?;
    s.sweep = None;
    s.eigen = None;
    s.time_domain = None;
    Ok(json!({"features": s.model.features.len()}))
}

fn solid(s: &mut Session, p: &Value) -> Result<Value> {
    let shape = shape_of(s, p)?;
    if let Shape::Box { size } = shape
        && size.iter().any(|v| !(*v > 0.0))
    {
        return Err(EngineError::Params("`size` must be positive".into()));
    }
    let material = string(p, "material")?.unwrap_or("vacuum").to_owned();
    s.model.set_solid(Solid { shape, material })?;
    s.sweep = None;
    s.eigen = None;
    s.time_domain = None;
    Ok(json!({"volume": s.model.solid.as_ref().map(|x| x.shape.volume())}))
}

/// The element size in each material (by index): `per_wavelength` elements per wavelength at
/// `frequency`, at most `size`; or `size` alone.
fn material_sizes(s: &Session, p: &Value) -> Result<(f64, Vec<Option<f64>>)> {
    let size = s.positive_length(p, "size")?;
    let Some(n) = number(p, "per_wavelength")? else {
        return Ok((size.ok_or_else(|| EngineError::Params("`size` (or `per_wavelength` with `frequency`) is required".into()))?, Vec::new()));
    };
    if !(n.is_finite() && (1.0..=1000.0).contains(&n)) {
        return Err(EngineError::Params("`per_wavelength` must be between 1 and 1000".into()));
    }
    let f = s.frequency(p, "frequency")?.ok_or_else(|| EngineError::Params("`per_wavelength` needs the `frequency` the mesh must resolve".into()))?;
    let per: Vec<Option<f64>> = s
        .model
        .materials
        .iter()
        .map(|m| {
            let h = emcraft_model::units::C0 / (f * (m.eps_r * m.mu_r).sqrt()) / n;
            Some(size.map_or(h, |c| c.min(h)))
        })
        .collect();
    let default = per.first().copied().flatten().unwrap_or(f64::NAN);
    Ok((default, per))
}

fn mesh_generate(s: &mut Session, p: &Value) -> Result<Value> {
    let (size, per_material) = material_sizes(s, p)?;
    let sizes_reply = |s: &Session| -> Value {
        let mut out = serde_json::Map::new();
        for (m, h) in s.model.materials.iter().zip(&per_material) {
            if let Some(h) = h {
                out.insert(m.name.clone(), json!(s.display.length.from_si(*h)));
            }
        }
        Value::Object(out)
    };
    if s.model.waveguide.is_none() && s.model.solid.is_some() {
        let curved = match p.get("curved") {
            None | Some(Value::Null) => true,
            Some(Value::Bool(b)) => *b,
            Some(v) => return Err(EngineError::Params(format!("`curved` must be true or false, got {v}"))),
        };
        let sizes = emcraft_mesh::solid::Sizes { default: size, per_material: per_material.clone() };
        let mesh = emcraft_mesh::solid::solid_sized(&s.model, &sizes, curved)?;
        let mut reply = json!({"nodes": mesh.nodes.len(), "tetrahedra": mesh.tets.len(), "unstructured": true, "curved_edges": mesh.curved.len()});
        if !per_material.is_empty() {
            reply["sizes"] = sizes_reply(s);
        }
        s.model.mesh = Some(mesh);
        s.sweep = None;
        return Ok(reply);
    }
    // A structured grid has one size: the smallest the sections' materials ask for.
    let size = match &s.model.waveguide {
        Some(w) if !per_material.is_empty() => {
            let mut h = f64::INFINITY;
            for sec in &w.sections {
                let (m, _) = s.model.material(&sec.material)?;
                h = h.min(per_material.get(m).copied().flatten().unwrap_or(size));
            }
            h
        }
        _ => size,
    };
    let mut o = emcraft_mesh::Options::new(size);
    o.edge = number(p, "edge")?.unwrap_or(o.edge);
    o.growth = number(p, "growth")?.unwrap_or(o.growth);
    o.size_y = s.positive_length(p, "size_y")?;
    let mesh = emcraft_mesh::waveguide_with(&s.model, &o)?;
    let mut reply = json!({"nodes": mesh.nodes.len(), "tetrahedra": mesh.tets.len()});
    if !per_material.is_empty() {
        reply["size"] = json!(s.display.length.from_si(size));
    }
    s.model.mesh = Some(mesh);
    s.sweep = None;
    Ok(reply)
}

fn walls(s: &mut Session, p: &Value) -> Result<Value> {
    let sigma = number(p, "conductivity")?;
    s.model.set_wall_conductivity(sigma)?;
    s.sweep = None;
    Ok(json!({"conductivity": sigma}))
}

fn port_create(s: &mut Session, p: &Value) -> Result<Value> {
    let name = required_string(p, "name")?;
    let end = match required_string(p, "end")? {
        "start" => End::Start,
        "end" => End::End,
        _ => return Err(EngineError::Params("`end` must be start or end".into())),
    };
    let modes = integer(p, "modes", 1..=emcraft_model::MAX_PORT_MODES as u64)?.unwrap_or(4) as u32;
    let face = match get(p, "face") {
        None => None,
        Some(v) => {
            let list = v.as_array().filter(|a| a.len() == 5).ok_or_else(|| EngineError::Params("`face` must be [z, x0, y0, a, b]".into()))?;
            let mut f = [0.0; 5];
            for (o, v) in f.iter_mut().zip(list) {
                *o =
                    s.display.length.to_si(v.as_f64().filter(|x| x.is_finite()).ok_or_else(|| EngineError::Params("`face` must be numbers".into()))?);
            }
            Some(emcraft_model::PortFace { z: f[0], x0: f[1], y0: f[2], a: f[3], b: f[4] })
        }
    };
    s.model.add_port(Port { name: name.to_owned(), end, modes, face })?;
    s.sweep = None;
    Ok(json!({"name": name}))
}

fn frequencies(s: &Session, p: &Value) -> Result<Vec<f64>> {
    let mut out = Vec::new();
    if let Some(list) = get(p, "frequencies") {
        let list = list.as_array().ok_or_else(|| EngineError::Params("`frequencies` must be an array".into()))?;
        if list.len() > s.limits.frequencies {
            return Err(EngineError::Params(format!("at most {} frequencies", s.limits.frequencies)));
        }
        for v in list {
            let f =
                v.as_f64().filter(|f| f.is_finite() && *f > 0.0).ok_or_else(|| EngineError::Params("`frequencies` must be numbers > 0".into()))?;
            out.push(s.display.frequency.to_si(f));
        }
    } else {
        let start = s.frequency(p, "start")?.ok_or_else(|| EngineError::Params("give `frequencies`, or `start` and `stop`".into()))?;
        let stop = s.frequency(p, "stop")?.ok_or_else(|| EngineError::Params("give `frequencies`, or `start` and `stop`".into()))?;
        let n = integer(p, "points", 1..=s.limits.frequencies as u64)?.unwrap_or(11) as usize;
        if stop < start {
            return Err(EngineError::Params("`stop` must not be below `start`".into()));
        }
        for k in 0..n {
            out.push(if n == 1 { start } else { start + (stop - start) * k as f64 / (n - 1) as f64 });
        }
    }
    if out.is_empty() {
        return Err(EngineError::Params("no frequencies".into()));
    }
    out.sort_by(f64::total_cmp);
    out.dedup();
    Ok(out)
}

fn solve_frequency(s: &mut Session, p: &Value) -> Result<Value> {
    let freqs = frequencies(s, p)?;
    let order = Order::from_u8(integer(p, "order", 1..=2)?.unwrap_or(2) as u8).unwrap_or(Order::Two);
    let driven = Driven::new(&s.model, order)?;
    let mut results = Vec::with_capacity(freqs.len());
    for f in &freqs {
        results.push(driven.solve(*f)?);
    }
    let unknowns = driven.dofs.unknowns;
    s.sweep = Some(Sweep { order, driven: Arc::new(driven), results });
    let mut reply = sparameters(s, &Value::Null)?;
    if let Some(o) = reply.as_object_mut() {
        o.insert("unknowns".into(), json!(unknowns));
        o.insert("order".into(), json!(order.as_u8()));
    }
    Ok(reply)
}

fn solve_time_domain(s: &mut Session, p: &Value) -> Result<Value> {
    let freqs = frequencies(s, p)?;
    let cell = s.positive_length(p, "cell")?.unwrap_or(emcraft_model::units::Length::Millimetre.to_si(1.0));
    if s.model.open.is_some() {
        return crate::open::solve_time_domain_open(s, &freqs, cell);
    }
    let r = emcraft_fdtd::s_parameters(&s.model, &emcraft_fdtd::Options::new(cell), &freqs)?;
    let points: Vec<Value> = r
        .frequencies
        .iter()
        .zip(r.s11.iter().zip(&r.s21))
        .map(|(f, (a, b))| json!({"frequency": s.show_frequency(*f), "s11": complex(*a), "s21": complex(*b)}))
        .collect();
    let reply = json!({"cells": r.cells, "steps": r.steps, "reference_steps": r.reference_steps, "converged": r.converged, "frequency_unit": s.display.frequency.symbol(), "points": points});
    s.time_domain = Some(r);
    Ok(reply)
}

fn solve_eigenmodes(s: &mut Session, p: &Value) -> Result<Value> {
    let near = s.frequency(p, "near")?.ok_or_else(|| EngineError::Params("`near` is required".into()))?;
    let count = integer(p, "count", 1..=crate::eigen_max())?.unwrap_or(5) as usize;
    let order = Order::from_u8(integer(p, "order", 1..=2)?.unwrap_or(2) as u8).unwrap_or(Order::Two);
    let e = emcraft_solver::eigen::eigenmodes(&s.model, order, count, near)?;
    s.eigen = Some(Arc::new(e));
    eigenmode_table(s, &Value::Null)
}

fn eigenmode_table(s: &mut Session, _: &Value) -> Result<Value> {
    let e = s.eigen.as_ref().ok_or_else(|| EngineError::Failed("no eigenmodes: solve first (solve.eigenmodes)".into()))?;
    let modes: Vec<Value> = e
        .modes
        .iter()
        .enumerate()
        .map(|(i, m)| json!({"mode": i + 1, "frequency": s.show_frequency(m.frequency), "q": m.q(), "q_walls": m.q_walls, "q_dielectric": m.q_dielectric}))
        .collect();
    let mut reply =
        json!({"frequency_unit": s.display.frequency.symbol(), "unknowns": e.driven.dofs.unknowns, "iterations": e.iterations, "modes": modes});
    // The closed form for a primitive cavity.
    if let Some(solid) = s.model.solid.as_ref()
        && s.model.inclusions.is_empty()
        && s.model.features.is_empty()
        && let Ok((_, m)) = s.model.material(&solid.material)
    {
        let er = m.eps_r * m.mu_r;
        let list = match solid.shape {
            Shape::Box { size } => emcraft_theory::cavity_resonances(size[0], size[1], size[2], er, 12),
            Shape::Cylinder { radius, length } => emcraft_theory::bessel::cylinder_cavity_resonances(radius, length, er, 12),
            Shape::Sphere { radius } => emcraft_theory::bessel::sphere_cavity_resonances(radius, er, 12),
            Shape::Surface { .. } => Vec::new(),
        };
        if !list.is_empty() {
            reply["exact"] = json!(list.into_iter().map(|(f, name)| json!({"mode": name, "frequency": s.show_frequency(f)})).collect::<Vec<_>>());
        }
    }
    // The closed form for a cavity of one full-width section.
    if let Some(w) = s.model.waveguide.as_ref()
        && w.sections.len() == 1
        && w.uniform()
        && let Ok((_, m)) = s.model.material(&w.sections[0].material)
    {
        let exact: Vec<Value> = emcraft_theory::cavity_resonances(w.width, w.height, w.length(), m.eps_r * m.mu_r, 12)
            .into_iter()
            .map(|(f, name)| json!({"mode": name, "frequency": s.show_frequency(f)}))
            .collect();
        reply["exact"] = json!(exact);
    }
    Ok(reply)
}

fn complex(c: C) -> Value {
    json!({"re": c.re, "im": c.im, "mag": c.norm(), "db": decibels(c.norm()), "deg": c.arg().to_degrees()})
}

fn sweep(s: &Session) -> Result<&Sweep> {
    s.sweep.as_ref().ok_or_else(|| EngineError::Failed("no results: solve first (solve.frequency)".into()))
}

fn sparameters(s: &mut Session, _: &Value) -> Result<Value> {
    let sw = sweep(s)?;
    let names: Vec<&str> = s.model.ports.iter().map(|p| p.name.as_str()).collect();
    let rows: Vec<Value> = sw
        .results
        .iter()
        .map(|r| {
            let mut m = serde_json::Map::new();
            m.insert("frequency".into(), json!(s.show_frequency(r.frequency)));
            for (i, row) in r.s.iter().enumerate() {
                for (j, v) in row.iter().enumerate() {
                    m.insert(format!("s{}{}", i + 1, j + 1), complex(*v));
                }
            }
            m.insert("backward_error".into(), json!(r.backward_error));
            Value::Object(m)
        })
        .collect();
    Ok(json!({"ports": names, "frequency_unit": s.display.frequency.symbol(), "points": rows}))
}

fn compare(s: &mut Session, p: &Value) -> Result<Value> {
    let modes = integer(p, "modes", 10..=1000)?.map_or(view::MODE_MATCHING_MODES, |n| n as usize);
    let sw = sweep(s)?;
    let reference = view::reference(s)?;
    let mut rows = Vec::new();
    let mut worst: f64 = 0.0;
    for r in &sw.results {
        let (s11, s21) = reference.s(r.frequency, modes)?;
        let (f11, f21) = (r.s[0][0], r.s.get(1).map_or(C::new(0.0, 0.0), |row| row[0]));
        let (e11, e21) = ((f11 - s11).norm(), (f21 - s21).norm());
        worst = worst.max(e11).max(e21);
        rows.push(json!({
            "frequency": s.show_frequency(r.frequency),
            "s11": complex(f11), "s21": complex(f21),
            "exact_s11": complex(s11), "exact_s21": complex(s21),
            "error_s11": e11, "error_s21": e21,
        }));
    }
    Ok(json!({"reference": reference.label(), "points": rows, "worst": worst}))
}

fn field_error(s: &mut Session, p: &Value) -> Result<Value> {
    let f = s.frequency(p, "frequency")?.ok_or_else(|| EngineError::Params("`frequency` is required".into()))?;
    let sw = sweep(s)?;
    let r = sw.at(f, false).ok_or_else(|| EngineError::Params(format!("{} {} was not solved", s.show_frequency(f), s.display.frequency.symbol())))?;
    let x = r.fields.first().ok_or_else(|| EngineError::Failed("no field".into()))?;
    let exact = view::te10_line(s)?.solve(r.frequency).map_err(|e| EngineError::Failed(e.to_string()))?;
    let (e, en) = sw.driven.l2_error(x, r.frequency, emcraft_solver::Field::E, |q| exact.e(q[0], q[1], q[2]))?;
    let (h, hn) = sw.driven.l2_error(x, r.frequency, emcraft_solver::Field::H, |q| exact.h(q[0], q[1], q[2]))?;
    Ok(json!({"frequency": s.show_frequency(r.frequency), "e": e / en, "h": h / hn}))
}

fn excitation(s: &Session, p: &Value) -> Result<usize> {
    match string(p, "excitation")? {
        None => Ok(0),
        Some(name) => Ok(s.model.port(name)?.0),
    }
}

fn probe(s: &mut Session, p: &Value) -> Result<Value> {
    let f = s.frequency(p, "frequency")?.ok_or_else(|| EngineError::Params("`frequency` is required".into()))?;
    let q = excitation(s, p)?;
    let sw = sweep(s)?;
    let r = sw.at(f, false).ok_or_else(|| EngineError::Params(format!("{} {} was not solved", s.show_frequency(f), s.display.frequency.symbol())))?;
    let x = r.fields.get(q).ok_or_else(|| EngineError::Failed("no field for that port".into()))?;
    let pts = get(p, "at").and_then(Value::as_array).ok_or_else(|| EngineError::Params("`at` must be an array of [x, y, z]".into()))?;
    if pts.len() > 100_000 {
        return Err(EngineError::Params("`at`: at most 100000 points".into()));
    }
    let mut out = Vec::with_capacity(pts.len());
    for pt in pts {
        let c = pt.as_array().filter(|a| a.len() == 3).ok_or_else(|| EngineError::Params("each point is [x, y, z]".into()))?;
        let mut xyz = [0.0; 3];
        for (d, v) in xyz.iter_mut().zip(c) {
            *d = s
                .display
                .length
                .to_si(v.as_f64().filter(|v| v.is_finite()).ok_or_else(|| EngineError::Params("coordinates must be finite numbers".into()))?);
        }
        let (e, h) = sw.driven.fields(x, r.frequency, xyz).ok_or_else(|| EngineError::Params(format!("{pt} is outside the mesh")))?;
        out.push(
            json!({"e": e.iter().map(|c| json!([c.re, c.im])).collect::<Vec<_>>(), "h": h.iter().map(|c| json!([c.re, c.im])).collect::<Vec<_>>()}),
        );
    }
    Ok(json!({"frequency": s.show_frequency(r.frequency), "points": out}))
}

pub(crate) fn write_png(path: &str, png: Option<Vec<u8>>) -> Result<()> {
    if path.trim().is_empty() {
        return Err(EngineError::Params("`path` must not be empty".into()));
    }
    let bytes = png.ok_or_else(|| EngineError::Failed("could not encode the picture".into()))?;
    std::fs::write(path, bytes).map_err(|e| EngineError::Failed(format!("cannot write {path}: {e}")))
}

/// A field view from command parameters.
pub fn field_view(s: &Session, p: &Value) -> Result<FieldView> {
    let plane = match string(p, "plane")? {
        None => Plane::H,
        Some(n) => Plane::parse(n).ok_or_else(|| EngineError::Params("`plane` must be h, e or cross".into()))?,
    };
    let mut v = match integer(p, "mode", 1..=crate::eigen_max())? {
        Some(m) => FieldView::mode(m as usize - 1, plane),
        None => FieldView::new(s.frequency(p, "frequency")?.ok_or_else(|| EngineError::Params("`frequency` or `mode` is required".into()))?, plane),
    };
    v.at = s.length(p, "at")?;
    if let Some(c) = string(p, "component")? {
        v.component = c.to_ascii_lowercase();
    }
    if let Some(list) = get(p, "arrows") {
        let list = list.as_array().ok_or_else(|| EngineError::Params("`arrows` must be an array of \"e\" and \"h\"".into()))?;
        v.show_e = list.iter().any(|x| x.as_str() == Some("e"));
        v.show_h = list.iter().any(|x| x.as_str() == Some("h"));
    }
    v.phase = number(p, "phase")?.unwrap_or(0.0).to_radians();
    v.excitation = excitation(s, p)?;
    Ok(v)
}

fn view_field(s: &mut Session, p: &Value) -> Result<Value> {
    let path = required_string(p, "path")?;
    let width = integer(p, "width", 200..=s.limits.picture_side as u64)?.unwrap_or(1100) as usize;
    let v = field_view(s, p)?;
    let plot = view::field_plot(s, &v)?;
    let c = emcraft_render::plot::draw(&plot, width);
    write_png(path, c.png())?;
    Ok(json!({"path": path, "width": c.w, "height": c.h}))
}

fn view_sparameters(s: &mut Session, p: &Value) -> Result<Value> {
    let path = required_string(p, "path")?;
    let side = 200..=s.limits.picture_side as u64;
    let w = integer(p, "width", side.clone())?.unwrap_or(1000) as usize;
    let h = integer(p, "height", side)?.unwrap_or(600) as usize;
    let chart = view::sparameter_chart(s)?;
    let c = emcraft_render::chart::draw(&chart, w, h);
    write_png(path, c.png())?;
    Ok(json!({"path": path, "width": c.w, "height": c.h}))
}

fn file_sample(s: &mut Session, p: &Value) -> Result<Value> {
    let name = required_string(p, "name")?;
    let sample = crate::sample::find(name).ok_or_else(|| {
        EngineError::Params(format!("unknown sample `{}` (known: {})", name.chars().take(40).collect::<String>(), crate::sample::names().join(", ")))
    })?;
    let solve = get(p, "solve").and_then(Value::as_bool).unwrap_or(false);
    let mut fresh = Session { limits: s.limits, ..Session::new() };
    let script: Value = serde_json::from_str(sample.script).map_err(|e| EngineError::Failed(format!("sample {name}: {e}")))?;
    let steps: Vec<Value> = script
        .as_array()
        .map(|a| {
            a.iter().filter(|st| solve || !st.get("command").and_then(Value::as_str).is_some_and(|c| c.starts_with("solve."))).cloned().collect()
        })
        .unwrap_or_default();
    fresh.run_script(&Value::Array(steps))?;
    *s = fresh;
    Ok(s.summary())
}

fn summary(s: &mut Session, _: &Value) -> Result<Value> {
    Ok(s.summary())
}
