//! Periodic structures' commands: the model's periodic boundaries (`model.periodic`), the
//! dispersion diagram (`solve.dispersion`, `results.dispersion`, `view.dispersion`) and its
//! chart, with the closed forms of a guide periodic along z where they apply.

use std::f64::consts::PI;
use std::sync::Arc;

use emcraft_model::units::C0;
use emcraft_model::{Periodic, Polarization};
use emcraft_render::chart::{Chart, Series, Style};
use emcraft_render::{Axis, Rgb};
use emcraft_solver::bloch::{Dispersion, MAX_BANDS, MAX_PHASES, Sweep};
use serde_json::{Value, json};

use crate::{Change, EngineError, Params, Result, Session};

/// An axis by name.
#[derive(serde::Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum AxisName {
    X,
    Y,
    Z,
}

fn axis_name(d: usize) -> &'static str {
    ["x", "y", "z"].get(d).copied().unwrap_or("?")
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
    let axis = match p.choice::<AxisName>("axis").optional()? {
        Some(AxisName::X) => 0,
        Some(AxisName::Y) => 1,
        Some(AxisName::Z) => 2,
        None => {
            if periodic.z {
                2
            } else if periodic.x {
                0
            } else {
                1
            }
        }
    };
    let near = p.frequency("near").or_maybe(s.model.band.map(|b| b.stop)).required_or("give `near`, or set the band: project.frequency")?;
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

/// The closed form's bands at phase `psi` for a guide periodic along z whose sections fill its
/// cross-section (empty: every TE/TM harmonic; loaded: the TE₁₀ Bloch band roots), with what
/// they are.
fn exact_bands(s: &Session, d: &Dispersion, psi: f64, count: usize, f_max: f64) -> Option<(Vec<f64>, &'static str)> {
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
    let d = s.results.dispersion.as_ref().ok_or_else(|| EngineError::failed("no dispersion diagram: solve first (solve.dispersion)"))?;
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

/// Band colours, in order.
const BAND_COLOURS: [Rgb; 6] = [Rgb(30, 90, 200), Rgb(200, 60, 40), Rgb(40, 150, 90), Rgb(150, 70, 180), Rgb(210, 140, 30), Rgb(60, 60, 60)];

/// The dispersion diagram: frequency against the phase per period, each band's solved points,
/// the closed form's curves where they apply and the light line f = c ψ/(2πL).
pub fn dispersion_chart(s: &Session) -> Result<Chart> {
    let d = s.results.dispersion.as_ref().ok_or_else(|| EngineError::failed("no dispersion diagram: solve first (solve.dispersion)"))?;
    let fu = s.display.frequency;
    let (p0, p1) = d.points.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p.psi.to_degrees()), b.max(p.psi.to_degrees())));
    let (p0, p1) = if p1 > p0 { (p0, p1) } else { (p0 - 10.0, p0 + 10.0) };
    let bands = d.points.iter().map(|p| p.frequencies.len()).max().unwrap_or(0);
    let f_top = d.points.iter().flat_map(|p| p.frequencies.iter().copied()).fold(0.0, f64::max);
    let mut series = Vec::new();
    // The closed form, finely.
    let n = 120;
    let mut exact: Vec<Vec<[f64; 2]>> = vec![Vec::new(); bands];
    let mut label = None;
    for k in 0..=n {
        let deg = p0 + (p1 - p0) * k as f64 / n as f64;
        if let Some((list, what)) = exact_bands(s, d, deg.to_radians(), bands, f_top * 1.5 + 1.0) {
            label = Some(what);
            for (b, f) in list.iter().take(bands).enumerate() {
                exact[b].push([deg, fu.from_si(*f)]);
            }
        }
    }
    if let Some(what) = label {
        for (b, pts) in exact.into_iter().enumerate().filter(|(_, p)| !p.is_empty()) {
            let c = BAND_COLOURS[b % BAND_COLOURS.len()];
            series.push(Series {
                label: if b == 0 { what.to_owned() } else { String::new() },
                color: Rgb(c.0 / 2 + 100, c.1 / 2 + 100, c.2 / 2 + 100),
                style: Style::Line,
                points: pts,
            });
        }
    }
    // The light line of the cell's slowest medium is the reference a slow-wave structure is read
    // against: here vacuum's, f = c ψ/(2πL).
    if d.period > 0.0 {
        let light = |deg: f64| fu.from_si(C0 * deg.to_radians() / (2.0 * PI * d.period));
        series.push(Series {
            label: "light line".into(),
            color: Rgb(120, 120, 120),
            style: Style::Line,
            points: vec![[p0.max(0.0), light(p0.max(0.0))], [p1, light(p1)]],
        });
    }
    for b in 0..bands {
        let pts: Vec<[f64; 2]> = d.points.iter().filter_map(|p| p.frequencies.get(b).map(|f| [p.psi.to_degrees(), fu.from_si(*f)])).collect();
        series.push(Series {
            label: format!("band {} (FEM)", b + 1),
            color: BAND_COLOURS[b % BAND_COLOURS.len()],
            style: Style::Markers,
            points: pts,
        });
    }
    let y1 = fu.from_si(f_top) * 1.1;
    Ok(Chart {
        title: format!("Dispersion along {}", axis_name(d.axis)),
        x: Axis::new("phase per period ψ (°)", p0, p1),
        y: Axis::new(&format!("frequency ({})", fu.symbol()), 0.0, if y1 > 0.0 { y1 } else { 1.0 }),
        series,
        marks: Vec::new(),
    })
}

pub fn view(s: &mut Session, p: &Params) -> Result<Value> {
    let path = p.string("path").required()?;
    let (w, h) = (p.side("width", s.limits.picture_side, 1000)?, p.side("height", s.limits.picture_side, 600)?);
    let c = emcraft_render::chart::draw(&dispersion_chart(s)?, w, h);
    crate::results::write_png(path, c.png())?;
    Ok(json!({"path": path, "width": c.w, "height": c.h}))
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
