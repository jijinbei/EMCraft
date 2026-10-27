//! Open problems: radiation, scattering, lumped ports and the time domain's antennas.

use serde_json::json;

use super::scratch;
use crate::*;

#[test]
fn open_problems_radiate_and_scatter() {
    let mut s = Session::new();
    // A layer needs a box, and room inside it.
    s.execute("geometry.solid", &json!({"shape": "sphere", "radius": 10})).unwrap();
    assert!(s.execute("model.open", &json!({"thickness": 3})).is_err());
    s.execute("geometry.solid", &json!({"shape": "box", "size": [40, 40, 40]})).unwrap();
    assert!(s.execute("model.open", &json!({"thickness": 25})).is_err());
    assert!(s.execute("model.open", &json!({"thickness": 10, "reflection": 2})).is_err());
    assert!(s.execute("model.open", &json!({"thickness": 10, "reflection": 0})).is_err());
    s.execute("model.open", &json!({"thickness": 10})).unwrap();
    s.execute("mesh.generate", &json!({"per_wavelength": 4, "frequency": 10})).unwrap();
    assert!(s.execute("solve.radiation", &json!({"frequency": 10})).is_err(), "no sources");
    assert!(s.execute("results.far_field", &json!({})).is_err(), "nothing solved");
    assert!(s.execute("source.dipole", &json!({"at": [50, 20, 20], "length": 1})).is_err(), "outside");
    assert!(s.execute("source.dipole", &json!({"at": [20, 20, 20], "length": 1, "direction": [0, 0, 0]})).is_err());
    s.execute("source.dipole", &json!({"at": [20.1, 20.2, 19.9], "length": 1})).unwrap();
    let r = s.execute("solve.radiation", &json!({"frequency": 10})).unwrap();
    // Coarse (λ/4, a third of a wavelength of layer): the plumbing, not the accuracy.
    let (p, exact) = (r["radiated_power"].as_f64().unwrap(), r["exact_radiated_power"].as_f64().unwrap());
    assert!((p / exact - 1.0).abs() < 0.2, "{p} {exact}");
    let ff = s.execute("results.far_field", &json!({"theta": [90, 0.0], "phi": 30})).unwrap();
    let d = ff["points"][0]["directivity"].as_f64().unwrap();
    assert!((d / 1.5 - 1.0).abs() < 0.2, "{d}");
    assert!(ff["points"][1]["directivity"].as_f64().unwrap() < 0.1, "the axis is a null");
    let path = scratch("pattern.png");
    s.execute("view.pattern", &json!({"path": path, "points": 37})).unwrap();
    assert!(std::fs::read(&path).unwrap().len() > 1000);
    // A plane wave on a conducting sphere: the Mie series' cross-section comes with it.
    s.execute("source.clear", &json!({})).unwrap();
    assert!(s.execute("source.plane_wave", &json!({"direction": [0, 0, 1], "polarization": [0, 0, 1]})).is_err(), "not transverse");
    s.execute("geometry.inclusion", &json!({"shape": "sphere", "radius": 3, "at": [20, 20, 20]})).unwrap();
    s.execute("model.open", &json!({"thickness": 10})).unwrap();
    s.execute("mesh.generate", &json!({"per_wavelength": 4, "frequency": 10})).unwrap();
    s.execute("source.plane_wave", &json!({})).unwrap();
    let r = s.execute("solve.radiation", &json!({"frequency": 10})).unwrap();
    let (sigma, exact) = (r["scattering_cross_section"].as_f64().unwrap(), r["exact_scattering_cross_section"].as_f64().unwrap());
    assert!((sigma / exact - 1.0).abs() < 0.3, "{sigma} {exact}");
    // Ports and an open boundary do not mix.
    let mut g = Session::new();
    g.execute("geometry.solid", &json!({"shape": "box", "size": [22.86, 10.16, 30]})).unwrap();
    g.execute("mesh.generate", &json!({"size": 5})).unwrap();
    g.execute("model.port.create", &json!({"name": "1", "end": "start"})).unwrap();
    assert!(g.execute("model.open", &json!({"thickness": 2})).is_err());
    s.execute("source.clear", &json!({})).unwrap();
    assert!(s.execute("model.port.create", &json!({"name": "1", "end": "start"})).is_err());
}

#[test]
fn spherical_layers_radiate() {
    let mut s = Session::new();
    // A spherical layer needs a sphere, and room inside it; unknown shapes are refused.
    s.execute("geometry.solid", &json!({"shape": "box", "size": [40, 40, 40]})).unwrap();
    assert!(s.execute("model.open", &json!({"thickness": 10, "shape": "sphere"})).is_err());
    s.execute("geometry.solid", &json!({"shape": "sphere", "radius": 20})).unwrap();
    assert!(s.execute("model.open", &json!({"thickness": 10, "shape": "ellipsoid"})).is_err());
    assert!(s.execute("model.open", &json!({"thickness": 15, "shape": "sphere"})).is_err());
    let r = s.execute("model.open", &json!({"thickness": 8, "shape": "sphere"})).unwrap();
    assert_eq!(r["shape"], "sphere");
    assert!(s.execute("model.port.create", &json!({"name": "1", "end": "start"})).is_err(), "no waveguide ports");
    assert!(s.execute("geometry.feature", &json!({"combine": "add", "shape": "box", "size": [5, 5, 5], "at": [0, 0, 0]})).is_err());
    s.execute("mesh.generate", &json!({"per_wavelength": 4, "frequency": 10})).unwrap();
    // A source in the layer is refused at the solve.
    s.execute("source.dipole", &json!({"at": [0, 0, 16], "length": 1})).unwrap();
    assert!(s.execute("solve.radiation", &json!({"frequency": 10})).is_err());
    s.execute("source.clear", &json!({})).unwrap();
    s.execute("source.dipole", &json!({"at": [0.1, 0.2, -0.1], "length": 1})).unwrap();
    let r = s.execute("solve.radiation", &json!({"frequency": 10})).unwrap();
    // Coarse (λ/4): the plumbing, not the accuracy (the oracle's dipole-radiation-spherical-pml).
    let (p, exact) = (r["radiated_power"].as_f64().unwrap(), r["exact_radiated_power"].as_f64().unwrap());
    assert!((p / exact - 1.0).abs() < 0.2, "{p} {exact}");
    let ff = s.execute("results.far_field", &json!({"theta": [90, 0.0], "phi": 30})).unwrap();
    let d = ff["points"][0]["directivity"].as_f64().unwrap();
    assert!((d / 1.5 - 1.0).abs() < 0.2, "{d}");
    assert!(ff["points"][1]["directivity"].as_f64().unwrap() < 0.1, "the axis is a null");
    // The time domain's grid is a box: refused.
    assert!(s.execute("solve.time_domain", &json!({"frequencies": [10]})).is_err());
}

#[test]
fn a_lumped_gap_radiates_as_a_hertzian_dipole() {
    let mut s = Session::new();
    s.execute("geometry.solid", &json!({"shape": "box", "size": [40, 40, 40]})).unwrap();
    s.execute("model.open", &json!({"thickness": 10})).unwrap();
    s.execute("mesh.generate", &json!({"per_wavelength": 4, "frequency": 10})).unwrap();
    // Refused: outside, no gap, no impedance, a duplicate name.
    assert!(s.execute("model.port.lumped", &json!({"name": "x", "from": [20, 20, 50], "to": [20, 20, 51]})).is_err());
    assert!(s.execute("model.port.lumped", &json!({"name": "x", "from": [20, 20, 20], "to": [20, 20, 20]})).is_err());
    assert!(s.execute("model.port.lumped", &json!({"name": "x", "from": [20, 20, 19.5], "to": [20, 20, 20.5], "impedance": -50})).is_err());
    // A current source: 1 A through a 1 mm gap.
    s.execute("model.port.lumped", &json!({"name": "gap", "from": [20.1, 20.2, 19.4], "to": [20.1, 20.2, 20.4], "impedance": 1e9, "voltage": 1e9}))
        .unwrap();
    assert!(s.execute("model.port.lumped", &json!({"name": "gap", "from": [21, 20, 19], "to": [21, 20, 20]})).is_err());
    let r = s.execute("solve.radiation", &json!({"frequency": 10})).unwrap();
    // Re Z = 80π²(l/λ)², the power balance, coarse as the mesh is.
    let r11 = r["ports"][0]["impedance"]["re"].as_f64().unwrap();
    assert!((r11 / 0.8785 - 1.0).abs() < 0.1, "{r11}");
    assert!((r["radiation_efficiency"].as_f64().unwrap() - 1.0).abs() < 0.03);
    let p = &r["ports"][0];
    let (zr, zi) = (p["impedance"]["re"].as_f64().unwrap(), p["impedance"]["im"].as_f64().unwrap());
    let expect = (((zr - 1e9).powi(2) + zi * zi) / ((zr + 1e9).powi(2) + zi * zi)).sqrt();
    assert!((p["s11"]["mag"].as_f64().unwrap() - expect).abs() < 1e-9);
}

#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn lumped_ports_feed_antennas() {
    let mut s = Session::new();
    s.execute("geometry.solid", &json!({"shape": "box", "size": [60, 60, 60]})).unwrap();
    s.execute("geometry.inclusion", &json!({"shape": "cylinder", "radius": 0.5, "length": 7, "at": [30, 30, 22.5]})).unwrap();
    s.execute("geometry.inclusion", &json!({"shape": "cylinder", "radius": 0.5, "length": 7, "at": [30, 30, 30.5]})).unwrap();
    s.execute("model.open", &json!({"thickness": 15})).unwrap();
    s.execute("mesh.generate", &json!({"per_wavelength": 3, "frequency": 10})).unwrap();
    s.execute("model.port.lumped", &json!({"name": "feed", "from": [30, 30, 29.5], "to": [30, 30, 30.5], "radius": 0.5})).unwrap();
    let r = s.execute("solve.radiation", &json!({"frequency": 10})).unwrap();
    // Coarse (λ/3): the plumbing. The power balance holds whatever the mesh.
    let eff = r["radiation_efficiency"].as_f64().unwrap();
    assert!((eff - 1.0).abs() < 0.03, "{eff}");
    let p = &r["ports"][0];
    let z = (p["impedance"]["re"].as_f64().unwrap(), p["impedance"]["im"].as_f64().unwrap());
    assert!(z.0 > 50.0 && z.0 < 200.0, "{z:?}");
    let s11 = p["s11"]["mag"].as_f64().unwrap();
    let (zr, zi) = z;
    let expect = (((zr - 50.0).powi(2) + zi * zi) / ((zr + 50.0).powi(2) + zi * zi)).sqrt();
    assert!((s11 - expect).abs() < 1e-12);
    assert!(p["induced_emf_impedance"]["re"].as_f64().is_some());
}

#[test]
fn ports_on_faces_go_with_open_boundaries() {
    let mut s = Session::new();
    // A face port needs a solid; with an open box ports must be on faces.
    s.execute("geometry.waveguide", &json!({"width": 22.86, "height": 10.16, "sections": [{"length": 10, "material": "vacuum"}]})).unwrap();
    assert!(s.execute("model.port.create", &json!({"name": "1", "end": "start", "face": [5, 0, 0, 22.86, 10.16]})).is_err());
    s.execute("geometry.solid", &json!({"shape": "box", "size": [60, 60, 60]})).unwrap();
    s.execute("model.open", &json!({"thickness": 10})).unwrap();
    assert!(s.execute("model.port.create", &json!({"name": "1", "end": "start"})).is_err());
    assert!(s.execute("model.port.create", &json!({"name": "1", "end": "start", "face": [5, 0, 0, -1, 10]})).is_err());
    assert!(s.execute("model.port.create", &json!({"name": "1", "end": "start", "face": [5, 0, 0]})).is_err());
    // A face where the mesh has no boundary: refused when solving, nothing panics.
    s.execute("model.port.create", &json!({"name": "1", "end": "start", "face": [30, 20, 20, 20, 10]})).unwrap();
    s.execute("mesh.generate", &json!({"per_wavelength": 3, "frequency": 10})).unwrap();
    let e = s.execute("solve.radiation", &json!({"frequency": 10})).unwrap_err();
    assert!(e.to_string().contains("no boundary face"), "{e}");
    // Eigenmodes of an open model are refused.
    assert!(s.execute("solve.eigenmodes", &json!({"near": 10})).is_err());
}

#[test]
fn the_time_domain_solves_open_problems() {
    let mut s = Session::new();
    s.execute("geometry.solid", &json!({"shape": "box", "size": [40, 40, 40]})).unwrap();
    s.execute("model.open", &json!({"thickness": 10})).unwrap();
    s.execute("model.port.lumped", &json!({"name": "gap", "from": [20, 20, 19], "to": [20, 20, 21], "impedance": 1e9, "voltage": 1e9})).unwrap();
    let r = s.execute("solve.time_domain", &json!({"frequencies": [10], "cell": 2})).unwrap();
    let p = &r["points"][0];
    // 80π²(l/λ)² for 2 mm: 3.51 Ω, coarse grid.
    let r11 = p["ports"][0]["impedance"]["re"].as_f64().unwrap();
    assert!((r11 / 3.514 - 1.0).abs() < 0.15, "{r11}");
    assert!((p["radiation_efficiency"].as_f64().unwrap() - 1.0).abs() < 0.05);
    let ff = s.execute("results.far_field", &json!({"theta": [90], "solver": "time_domain"})).unwrap();
    assert!((ff["points"][0]["directivity"].as_f64().unwrap() / 1.5 - 1.0).abs() < 0.1);
    assert!(s.execute("results.far_field", &json!({"solver": "fem"})).is_err(), "no FEM solve yet");
    assert!(s.execute("results.far_field", &json!({"solver": "magic"})).is_err());
    let path = scratch("td-pattern.png");
    s.execute("view.pattern", &json!({"path": path, "points": 19})).unwrap();
}
