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
    // Coarse (λ/4, a third of a wavelength of layer): the plumbing, not the accuracy (the
    // oracle's dipole-radiation). The Hertzian dipole's power comes with it: η₀k²(Il)²/12π.
    let (p, exact) = (r["radiated_power"].as_f64().unwrap(), r["exact_radiated_power"].as_f64().unwrap());
    let k = 2.0 * std::f64::consts::PI * 10e9 / emcraft_model::units::C0;
    let hertzian = emcraft_model::units::ETA0 * (k * 1e-3).powi(2) / (12.0 * std::f64::consts::PI);
    assert!(p > 0.0 && (exact / hertzian - 1.0).abs() < 1e-9, "{p} {exact} {hertzian}");
    let ff = s.execute("results.far_field", &json!({"theta": [90, 0.0], "phi": 30})).unwrap();
    assert!(ff["points"][1]["directivity"].as_f64().unwrap() < 0.1 * ff["points"][0]["directivity"].as_f64().unwrap(), "the axis is a null");
    let path = scratch("pattern.png");
    s.execute("view.pattern", &json!({"path": path, "points": 37})).unwrap();
    assert!(std::fs::read(&path).unwrap().len() > 1000);
    // The cut through the equator (θ = 90°, φ varying) is flat; the z dipole's field is E_θ,
    // E_φ's share far below it.
    let cut = |component| view::PatternView { cut: view::Cut::Theta, angle: 90.0, component, points: 37, ..Default::default() };
    let fem = |c: emcraft_render::chart::Chart| c.series.into_iter().find(|x| x.key == "fem").unwrap().points;
    let theta = fem(view::pattern_chart_in(&s, &cut(view::FarComponent::Theta)).unwrap());
    let phi = fem(view::pattern_chart_in(&s, &cut(view::FarComponent::Phi)).unwrap());
    let (lo, hi) = theta.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p[1]), b.max(p[1])));
    assert!(hi - lo < 1.5 && (theta[36][0] - 360.0).abs() < 1e-9, "{lo} {hi}");
    assert!(phi.iter().all(|p| p[1] < lo - 10.0), "E_φ is small");
    s.execute("view.pattern", &json!({"path": path, "cut": "theta", "theta": 90, "component": "phi", "points": 37})).unwrap();
    assert!(s.execute("view.pattern", &json!({"path": path, "cut": "theta", "theta": 200})).is_err());
    assert!(s.execute("view.pattern", &json!({"path": path, "component": "z"})).is_err());
    // Drawn round a centre: every point within the outer ring (the top) and outside the centre
    // (the range below it), on a frame of rings, no axes; along an axis, the range is the
    // y axis's span; a range out of bounds refused.
    let polar = view::pattern_chart_in(&s, &view::PatternView { polar: true, range: 20.0, points: 37, ..Default::default() }).unwrap();
    assert!(!polar.look.frame && polar.look.equal && polar.series.iter().any(|x| x.key == "grid"));
    assert!(fem(polar).iter().all(|p| p[0].hypot(p[1]) <= 1.0 + 1e-12));
    let along = view::pattern_chart_in(&s, &view::PatternView { range: 30.0, points: 37, ..Default::default() }).unwrap();
    assert!((along.y.max - along.y.min - 30.0).abs() < 1e-9 && along.look.frame);
    s.execute("view.pattern", &json!({"path": path, "polar": true, "range": 60})).unwrap();
    assert!(s.execute("view.pattern", &json!({"path": path, "range": 5})).is_err());
    // A plane wave on a conducting sphere: the Mie series' cross-section comes with it.
    s.execute("source.clear", &json!({})).unwrap();
    assert!(s.execute("source.plane_wave", &json!({"direction": [0, 0, 1], "polarization": [0, 0, 1]})).is_err(), "not transverse");
    s.execute("geometry.inclusion", &json!({"shape": "sphere", "radius": 3, "at": [20, 20, 20]})).unwrap();
    s.execute("model.open", &json!({"thickness": 10})).unwrap();
    // λ/3 and first order, for speed: the plumbing (how close σ comes to the Mie series is the
    // oracle's pec-sphere-scattering).
    s.execute("mesh.generate", &json!({"per_wavelength": 3, "frequency": 10})).unwrap();
    s.execute("source.plane_wave", &json!({})).unwrap();
    let r = s.execute("solve.radiation", &json!({"frequency": 10, "order": 1})).unwrap();
    let (sigma, exact) = (r["scattering_cross_section"].as_f64().unwrap(), r["exact_scattering_cross_section"].as_f64().unwrap());
    assert!(sigma > 0.0 && exact > 0.0, "{sigma} {exact}");
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
    // Coarse (λ/4) and first order, a fifth of second order's unknowns: the plumbing, not the
    // accuracy (the oracle's dipole-radiation-spherical-pml). Measured: the power 2 % high, the
    // directivity 1.20 on the equator, 0.23 on the axis (second order: 1.59 and 0.01).
    let r = s.execute("solve.radiation", &json!({"frequency": 10, "order": 1})).unwrap();
    let (p, exact) = (r["radiated_power"].as_f64().unwrap(), r["exact_radiated_power"].as_f64().unwrap());
    assert!(p > 0.0 && exact > 0.0, "{p} {exact}");
    let ff = s.execute("results.far_field", &json!({"theta": [90, 0.0], "phi": 30})).unwrap();
    let d = ff["points"][0]["directivity"].as_f64().unwrap();
    assert!(ff["points"][1]["directivity"].as_f64().unwrap() < 0.3 * d, "the axis is a null");
    // The time domain's grid is a box: refused.
    assert!(s.execute("solve.time_domain", &json!({"frequencies": [10]})).is_err());
}

#[test]
fn a_lumped_gap_radiates_as_a_hertzian_dipole() {
    // Hertzian dipole Re Z = 2P/I² = 80π²(l/λ)² = 0.8785 Ω for l = 1 mm at 10 GHz (Balanis, Antenna
    // Theory, 4th ed., §4.2).
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
    approx::assert_abs_diff_eq!(r["radiation_efficiency"].as_f64().unwrap(), 1.0, epsilon = 0.03);
    let p = &r["ports"][0];
    let (zr, zi) = (p["impedance"]["re"].as_f64().unwrap(), p["impedance"]["im"].as_f64().unwrap());
    let expect = (((zr - 1e9).powi(2) + zi * zi) / ((zr + 1e9).powi(2) + zi * zi)).sqrt();
    approx::assert_abs_diff_eq!(p["s11"]["mag"].as_f64().unwrap(), expect, epsilon = 1e-9);
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
    approx::assert_abs_diff_eq!(s11, expect, epsilon = 1e-12);
    assert!(p["induced_emf_impedance"]["re"].as_f64().is_some());
}

#[test]
fn far_fields_give_gains_and_polarization() {
    // Two 1 mm gaps crossed at the centre, x and y, driven by currents in quadrature (the y one
    // 90° behind): on the z axis E ∝ x̂ − jŷ, right-hand circular (IEEE) — axial ratio 0 dB,
    // the left-hand share far below. Gains: G = D·P_rad/P_in, G_realized = D·P_rad/P_avail, and
    // P_in = P_avail (1 − |S11|²) for a source in series with its impedance, so their ratio is
    // the mismatch (Balanis, Antenna Theory, 4th ed., §2.9).
    let mut s = Session::new();
    s.execute("geometry.solid", &json!({"shape": "box", "size": [40, 40, 40]})).unwrap();
    s.execute("model.open", &json!({"thickness": 10})).unwrap();
    s.execute("mesh.generate", &json!({"per_wavelength": 4, "frequency": 10})).unwrap();
    s.execute("model.port.lumped", &json!({"name": "x", "from": [19.5, 20, 20], "to": [20.5, 20, 20], "impedance": 1e9, "voltage": 1e9})).unwrap();
    s.execute(
        "model.port.lumped",
        &json!({"name": "y", "from": [20, 19.5, 20], "to": [20, 20.5, 20], "impedance": 1e9, "voltage": 1e9, "phase": -90}),
    )
    .unwrap();
    s.execute("solve.radiation", &json!({"frequency": 10})).unwrap();
    let ff = s.execute("results.far_field", &json!({"theta": [0, 90], "phi": 0})).unwrap();
    let top = &ff["points"][0];
    let num = |v: &serde_json::Value, k: &str| v[k].as_f64().unwrap_or_else(|| panic!("{k}: {v}"));
    assert_eq!(top["sense"], "right", "{top}");
    assert!(num(top, "axial_ratio_db") < 1.0, "{top}");
    assert!(num(top, "directivity_rhcp_dbi") - num(top, "directivity_lhcp_dbi") > 20.0, "{top}");
    approx::assert_abs_diff_eq!(num(top, "directivity_rhcp_dbi"), num(top, "directivity_dbi"), epsilon = 0.05);
    // In the plane of the gaps at φ = 0 the y gap's field reaches (E_φ), the x gap's hardly (it
    // points along the gap): nearly linear, the ellipse tens of dB long.
    let side = &ff["points"][1];
    assert!(num(side, "axial_ratio_db") > 20.0, "{side}");
    // Ludwig 3 about x̂ at φ = 0: E_φ is cross-polar; about ŷ, co-polar.
    approx::assert_abs_diff_eq!(num(side, "directivity_cross_dbi"), num(side, "directivity_dbi"), epsilon = 0.1);
    let about_y = s.execute("results.far_field", &json!({"theta": [90], "phi": 0, "co_polar": 90})).unwrap();
    approx::assert_abs_diff_eq!(num(&about_y["points"][0], "directivity_co_dbi"), num(side, "directivity_cross_dbi"), epsilon = 1e-9);
    // A z current element's field is E_θ alone: linear exactly as rounding allows.
    let ff = emcraft_solver::radiation::FarField {
        theta: 1.0,
        phi: 0.3,
        e_theta: emcraft_solver::Complex64::new(1.0, 0.0),
        e_phi: emcraft_solver::Complex64::new(0.0, 0.0),
    };
    assert!(ff.axial_ratio().is_infinite());
    // The gains against the powers the solve reports.
    let r = s.execute("solve.radiation", &json!({"frequency": 10})).unwrap();
    let ff = s.execute("results.far_field", &json!({"theta": [0]})).unwrap();
    let row = &ff["points"][0];
    let (d, g, gr) = (num(row, "directivity"), num(row, "gain"), num(row, "realized_gain"));
    approx::assert_abs_diff_eq!(g / d, num(&r, "radiation_efficiency"), epsilon = 1e-12);
    let input = num(&r, "input_power");
    let mismatch: f64 = r["ports"].as_array().unwrap().iter().map(|p| num(p, "power") / (1.0 - p["s11"]["mag"].as_f64().unwrap().powi(2))).sum();
    // 1 − |S11|² is a few 1e-9 against a gigaohm source: eight digits are left of it.
    approx::assert_abs_diff_eq!(gr / g, input / mismatch, epsilon = 1e-6 * input / mismatch);
    assert!(s.execute("results.far_field", &json!({"co_polar": 1000})).is_err());
    let path = scratch("pattern-gain.png");
    s.execute("view.pattern", &json!({"path": path, "quantity": "realized_gain", "component": "rhcp", "points": 19})).unwrap();
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
    // Hertzian dipole: Re Z = 80π²(l/λ)² = 3.514 Ω for 2 mm at 10 GHz, directivity 3/2 (Balanis,
    // Antenna Theory, 4th ed., §4.2).
    let mut s = Session::new();
    s.execute("geometry.solid", &json!({"shape": "box", "size": [40, 40, 40]})).unwrap();
    s.execute("model.open", &json!({"thickness": 10})).unwrap();
    s.execute("model.port.lumped", &json!({"name": "gap", "from": [20, 20, 19], "to": [20, 20, 21], "impedance": 1e9, "voltage": 1e9})).unwrap();
    let r = s.execute("solve.time_domain", &json!({"frequencies": [10], "cell": 2})).unwrap();
    let p = &r["points"][0];
    // 80π²(l/λ)² for 2 mm: 3.51 Ω, coarse grid.
    let r11 = p["ports"][0]["impedance"]["re"].as_f64().unwrap();
    assert!((r11 / 3.514 - 1.0).abs() < 0.15, "{r11}");
    approx::assert_abs_diff_eq!(p["radiation_efficiency"].as_f64().unwrap(), 1.0, epsilon = 0.05);
    let ff = s.execute("results.far_field", &json!({"theta": [90], "solver": "time_domain"})).unwrap();
    approx::assert_abs_diff_eq!(ff["points"][0]["directivity"].as_f64().unwrap() / 1.5, 1.0, epsilon = 0.1);
    assert!(s.execute("results.far_field", &json!({"solver": "fem"})).is_err(), "no FEM solve yet");
    assert!(s.execute("results.far_field", &json!({"solver": "magic"})).is_err());
    let path = scratch("td-pattern.png");
    s.execute("view.pattern", &json!({"path": path, "points": 19})).unwrap();
}

/// The far-field chart's cut through the strongest direction: none before a solve.
#[test]
fn no_strongest_azimuth_before_a_solve() {
    assert_eq!(crate::open::strongest_azimuth(&Session::new()), None);
}

/// A patch antenna's broadside lies off its board's plane (the model's x–z, which φ = 0 cuts
/// along): the strongest azimuth is 90° (7.2 dBi there against about −4 dBi in the plane).
#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn a_patch_radiates_strongest_off_its_board() {
    let mut s = Session::new();
    s.execute("file.sample", &json!({"name": "patch-antenna"})).unwrap();
    s.execute("solve.radiation", &json!({"frequency": 10})).unwrap();
    assert_eq!(crate::open::strongest_azimuth(&s), Some(90.0));
}

#[test]
fn a_radiation_sweep_keeps_the_antennas_terminals_and_the_last_solution() {
    // The coarse 1 mm gap of a_lumped_gap_radiates_as_a_hertzian_dipole, on 50 Ω, at two
    // frequencies: each as solve.radiation gives it, bit for bit, and the gap's S11 kept.
    let mut s = Session::new();
    s.execute("geometry.solid", &json!({"shape": "box", "size": [40, 40, 40]})).unwrap();
    s.execute("model.open", &json!({"thickness": 10})).unwrap();
    s.execute("mesh.generate", &json!({"per_wavelength": 4, "frequency": 10})).unwrap();
    s.execute("model.port.lumped", &json!({"name": "gap", "from": [20.1, 20.2, 19.4], "to": [20.1, 20.2, 20.4]})).unwrap();
    let r = s.execute("solve.radiation_sweep", &json!({"frequencies": [9, 10], "name": "gap"})).unwrap();
    assert_eq!(r["kept"], "gap");
    let one = s.execute("solve.radiation", &json!({"frequency": 9})).unwrap();
    assert_eq!(r["points"][0]["ports"][0]["impedance"], one["ports"][0]["impedance"]);
    // The antenna as the source of a circuit: its impedance read back from the kept S11.
    let z = &r["points"][1]["ports"][0]["impedance"];
    let n = s
        .execute("circuit.cascade", &json!({"blocks": [{"type": "line", "length": 1}], "source": "gap", "start": 9, "stop": 10, "points": 2}))
        .unwrap();
    assert_eq!(n["noise"]["source"], "gap");
    assert!(z["re"].as_f64().unwrap() > 0.0);
    // Too many frequencies, or a source that is not a one-port: refused.
    assert!(s.execute("solve.radiation_sweep", &json!({"start": 9, "stop": 10, "points": 65})).is_err());
    assert!(s.execute("circuit.cascade", &json!({"blocks": [{"type": "line", "length": 1}], "source": "nothing", "start": 9, "stop": 10})).is_err());
}
