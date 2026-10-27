//! Periodic structures' commands: the model's periodic boundaries (`model.periodic`) and the
//! dispersion diagram (`solve.dispersion`, `results.dispersion`), with the closed forms of a
//! guide periodic along z where they apply. Its chart is `view`'s.

use std::f64::consts::PI;
use std::sync::Arc;

use emcraft_model::{Periodic, Polarization};
use emcraft_solver::bloch::{Dispersion, MAX_BANDS, MAX_PHASES, Sweep};
use serde_json::{Value, json};

use crate::view::AXIS_NAMES;
pub use crate::view::dispersion_chart;
use crate::{Change, EngineError, Params, Result, Session};

/// An axis by name.
#[derive(serde::Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum AxisName {
    X,
    Y,
    Z,
}

impl AxisName {
    const fn index(self) -> usize {
        match self {
            AxisName::X => 0,
            AxisName::Y => 1,
            AxisName::Z => 2,
        }
    }
}

fn axis_name(d: usize) -> &'static str {
    AXIS_NAMES.get(d).copied().unwrap_or("?")
}

/// The model's periodic boundaries as a reply.
fn shown(p: &Option<Periodic>) -> Value {
    match p {
        None => json!({"periodic": false}),
        Some(p) => json!({
            "periodic": true,
            "x": p.x, "y": p.y, "z": p.z,
            "theta": p.theta.to_degrees(), "phi": p.phi.to_degrees(),
            "polarization": match p.polarization { Polarization::Te => "te", Polarization::Tm => "tm" },
        }),
    }
}

pub fn periodic(s: &mut Session, p: &Params) -> Result<Value> {
    if p.flag("clear").or(false)? {
        s.model.set_periodic(None)?;
        s.results.invalidate(Change::Geometry);
        return Ok(shown(&s.model.periodic));
    }
    let old = s.model.periodic;
    let angle = |name: &str, old: f64| -> Result<f64> { Ok(p.number(name).optional()?.map_or(old, f64::to_radians)) };
    let next = Periodic {
        x: p.flag("x").or(old.is_none_or(|o| o.x))?,
        y: p.flag("y").or(old.is_none_or(|o| o.y))?,
        z: p.flag("z").or(old.is_some_and(|o| o.z))?,
        theta: angle("theta", old.map_or(0.0, |o| o.theta))?,
        phi: angle("phi", old.map_or(0.0, |o| o.phi))?,
        polarization: p.choice("polarization").or(old.map_or(Polarization::Te, |o| o.polarization))?,
    };
    s.model.set_periodic(Some(next))?;
    s.results.invalidate(Change::Geometry);
    Ok(shown(&s.model.periodic))
}

pub fn dispersion(s: &mut Session, p: &Params) -> Result<Value> {
    let periodic = s.model.periodic.ok_or_else(|| EngineError::failed("no periodic boundaries: set them with model.periodic"))?;
    // Without `axis`: z when the cell is periodic along it, else x, else y.
    let axis = match p.choice::<AxisName>("axis").optional()? {
        Some(a) => a.index(),
        None if periodic.z => 2,
        None if periodic.x => 0,
        None => 1,
    };
    let near = crate::args::frequency_or_band_top(s, p, "near", "give `near`, or set the band: project.frequency")?;
    let from = p.number("from").or(0.0)?;
    let to = p.number("to").or(180.0)?;
    if !(from.is_finite() && to.is_finite() && from.abs() <= 360.0 && to.abs() <= 360.0) {
        return Err(p.error("from", "the phases must be finite, within ±360°"));
    }
    let sweep = Sweep {
        axis,
        from: from.to_radians(),
        to: to.to_radians(),
        points: p.integer("points", 1..=MAX_PHASES).or(11)?,
        bands: p.integer("bands", 1..=MAX_BANDS).or(4)?,
        near,
    };
    let order = p.order()?;
    let d = emcraft_solver::bloch::dispersion_each(&s.model, order, &sweep, |k, n| crate::progress::report(k, n, "phase"))?;
    s.results.dispersion = Some(Arc::new(d));
    let mut reply = dispersion_table(s)?;
    reply["order"] = json!(order.as_u8());
    Ok(reply)
}

pub fn results(s: &mut Session, _: &Params) -> Result<Value> {
    dispersion_table(s)
}

/// The dispersion diagram solved, or an error asking for one.
pub(crate) fn solved(s: &Session) -> Result<&Dispersion> {
    s.results.dispersion.as_deref().ok_or_else(|| EngineError::failed("no dispersion diagram: solve first (solve.dispersion)"))
}

/// The closed form's bands at phase `psi` for a guide periodic along z whose sections fill its
/// cross-section (empty: every TE/TM harmonic; loaded: the TE₁₀ Bloch band roots), with what
/// they are.
pub(crate) fn exact_bands(s: &Session, d: &Dispersion, psi: f64, count: usize, f_max: f64) -> Option<(Vec<f64>, &'static str)> {
    let m = &s.model;
    let p = m.periodic?;
    if d.axis != 2 || p.x || p.y {
        return None;
    }
    let w = m.waveguide.as_ref()?;
    if !(w.uniform() && w.plain() && m.symmetry.is_empty()) {
        return None;
    }
    let mats: Vec<(f64, f64, f64)> =
        w.sections.iter().map(|sec| m.material(&sec.material).ok().map(|(_, mat)| (sec.length, mat.eps_r, mat.mu_r))).collect::<Option<_>>()?;
    let first = mats.first()?;
    if mats.iter().all(|x| x.1 == first.1 && x.2 == first.2) {
        let bands = emcraft_theory::floquet::empty_guide_bands(w.width, w.height, w.length(), first.1 * first.2, psi, count);
        return Some((bands, "exact (TE/TM harmonics)"));
    }
    if psi <= 0.0 || psi >= PI {
        return None;
    }
    Some((emcraft_theory::floquet::bloch_frequencies(w.width, &mats, psi, f_max), "exact TE₁₀ Bloch"))
}

/// The diagram as a table: each phase's β and bands, and the closed form's where it applies.
pub(crate) fn dispersion_table(s: &Session) -> Result<Value> {
    let d = solved(s)?;
    let per_unit = s.display.length.to_si(1.0);
    let f_max = d.points.iter().flat_map(|p| p.frequencies.iter().copied()).fold(0.0, f64::max) * 1.5;
    let rows: Vec<Value> = d
        .points
        .iter()
        .map(|pt| {
            let mut row = json!({
                "phase": pt.psi.to_degrees(),
                "beta": pt.beta * per_unit,
                "frequencies": pt.frequencies.iter().map(|f| s.show_frequency(*f)).collect::<Vec<_>>(),
                "iterations": pt.iterations,
            });
            if let Some((bands, what)) = exact_bands(s, d, pt.psi, pt.frequencies.len(), f_max.max(1.0)) {
                row["exact"] = json!(bands.iter().take(pt.frequencies.len()).map(|f| s.show_frequency(*f)).collect::<Vec<_>>());
                row["exact_kind"] = json!(what);
            }
            row
        })
        .collect();
    Ok(json!({
        "axis": axis_name(d.axis),
        "period": s.show_length(d.period),
        "unknowns": d.unknowns,
        "frequency_unit": s.display.frequency.symbol(),
        "length_unit": s.display.length.symbol(),
        "points": rows,
    }))
}

/// The plane-wave reference of a cell periodic in x and y whose sections fill it (plane layers):
/// the transfer-matrix S of the incidence and polarization set.
pub fn stack_reference(s: &Session) -> Result<emcraft_theory::floquet::Stack> {
    let m = &s.model;
    let p = m.periodic.ok_or_else(|| EngineError::failed("the model is not periodic"))?;
    let w = m.waveguide.as_ref().ok_or_else(|| EngineError::failed("no waveguide"))?;
    if !(p.x && p.y && !p.z && w.uniform() && w.plain()) {
        return Err(EngineError::failed("not supported yet: a reference for this periodic cell (one of plane layers periodic in x and y has one)"));
    }
    let layers = w
        .sections
        .iter()
        .map(|sec| {
            let (_, mat) = m.material(&sec.material)?;
            if mat.is_dispersive() {
                return Err(EngineError::failed("not supported yet: a plane-wave reference with dispersive layers"));
            }
            Ok(emcraft_theory::floquet::StackLayer {
                thickness: sec.length,
                eps_r: emcraft_solver::Complex64::new(mat.eps_r, -mat.eps_r * mat.loss_tangent),
                mu_r: mat.mu_r,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let polarization = match p.polarization {
        Polarization::Te => emcraft_theory::floquet::Polarization::Te,
        Polarization::Tm => emcraft_theory::floquet::Polarization::Tm,
    };
    Ok(emcraft_theory::floquet::Stack { layers, theta: p.theta, polarization })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    fn scratch(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("emcraft-engine-periodic-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        dir.join(name).to_string_lossy().into_owned()
    }

    /// A 10 × 10 mm cell: 6 mm of air, a 5 mm slab of εr = 4, 6 mm of air.
    fn slab_cell(s: &mut Session) {
        s.execute("model.material.create", &json!({"name": "slab", "eps_r": 4})).unwrap();
        s.execute(
            "geometry.waveguide",
            &json!({"width": 10, "height": 10, "sections": [{"length": 6, "material": "vacuum"}, {"length": 5, "material": "slab"}, {"length": 6, "material": "vacuum"}]}),
        )
        .unwrap();
        s.execute("mesh.generate", &json!({"size": 2.5})).unwrap();
        s.execute("model.port.create", &json!({"name": "below", "end": "start", "modes": 2})).unwrap();
        s.execute("model.port.create", &json!({"name": "above", "end": "end", "modes": 2})).unwrap();
    }

    #[test]
    fn a_slab_cell_matches_the_plane_wave_through_it() {
        let mut s = Session::new();
        slab_cell(&mut s);
        let r = s.execute("model.periodic", &json!({"theta": 30, "phi": 20, "polarization": "tm"})).unwrap();
        assert_eq!(r["x"], true);
        assert_eq!(r["polarization"], "tm");
        assert!((r["theta"].as_f64().unwrap() - 30.0).abs() < 1e-12);
        s.execute("solve.frequency", &json!({"frequencies": [8, 10, 12]})).unwrap();
        let c = s.execute("results.compare", &json!({})).unwrap();
        assert_eq!(c["reference"], "exact (plane wave)");
        assert!(c["worst"].as_f64().unwrap() < 1e-2, "{c}");
        let path = scratch("slab.png");
        s.execute("view.sparameters", &json!({"path": path, "width": 600, "height": 400})).unwrap();
        assert!(std::fs::read(&path).unwrap().len() > 1000);
        s.execute("model.periodic", &json!({"clear": true})).unwrap();
        assert!(s.model.periodic.is_none() && s.results.sweep.is_none());
    }

    #[test]
    fn a_guide_periodic_along_z_has_its_dispersion() {
        let mut s = Session::new();
        s.execute("geometry.waveguide", &json!({"width": 22.86, "height": 10.16, "sections": [{"length": 10, "material": "vacuum"}]})).unwrap();
        s.execute("mesh.generate", &json!({"size": 3.5})).unwrap();
        s.execute("model.periodic", &json!({"x": false, "y": false, "z": true})).unwrap();
        let r = s.execute("solve.dispersion", &json!({"points": 3, "bands": 2, "near": 15})).unwrap();
        assert_eq!(r["axis"], "z");
        let pts = r["points"].as_array().unwrap();
        assert_eq!(pts.len(), 3);
        for p in pts {
            let (f, e) = (p["frequencies"].as_array().unwrap(), p["exact"].as_array().unwrap());
            for (a, b) in f.iter().zip(e) {
                let (a, b) = (a.as_f64().unwrap(), b.as_f64().unwrap());
                assert!((a - b).abs() < 2e-3 * b, "{p}");
            }
        }
        // β = ψ/L per mm at the zone's edge: π/10.
        assert!((pts[2]["beta"].as_f64().unwrap() - std::f64::consts::PI / 10.0).abs() < 1e-12);
        let path = scratch("dispersion.png");
        s.execute("view.dispersion", &json!({"path": path, "width": 600, "height": 400})).unwrap();
        assert!(std::fs::read(&path).unwrap().len() > 1000);
        assert!(s.execute("solve.eigenmodes", &json!({"near": 10})).is_err(), "a periodic cell has a diagram, not eigenmodes");
    }

    #[test]
    fn periodic_boundaries_refuse_what_they_cannot_hold() {
        let mut s = Session::new();
        assert!(s.execute("model.periodic", &json!({})).is_err(), "no geometry");
        s.execute("geometry.solid", &json!({"shape": "box", "size": [10, 10, 10]})).unwrap();
        let e = s.execute("model.periodic", &json!({})).unwrap_err().to_string();
        assert!(e.contains("not supported yet"), "{e}");
        let mut s = Session::new();
        slab_cell(&mut s);
        assert!(s.execute("model.periodic", &json!({"z": true})).is_err(), "z with ports");
        assert!(s.execute("model.periodic", &json!({"theta": 90})).is_err());
        assert!(s.execute("model.periodic", &json!({"theta": "x"})).is_err());
        assert!(s.execute("solve.dispersion", &json!({"near": 10})).is_err(), "not periodic yet");
        s.execute("model.periodic", &json!({"x": true, "y": false})).unwrap();
        assert!(s.execute("solve.frequency", &json!({"frequencies": [10]})).is_err(), "Floquet ports need x and y");
        assert!(s.execute("solve.dispersion", &json!({"axis": "z", "near": 10})).is_err(), "z is not periodic");
    }
}
