//! The region off the origin: a box region anywhere (a block) solves what the same box at the
//! origin does, in the frequency and the time domain (ADR 0005, the automatic region's ground).

use serde_json::{Value, json};

use crate::Session;

/// A 40 mm box lined by 10 mm, with `put` run on it (a source or a lumped port at its middle), at
/// the origin; and the same moved by (−20, −20, −20) mm so its middle is the origin.
fn both(put: &dyn Fn(&mut Session, f64)) -> [Session; 2] {
    let mut a = Session::new();
    a.execute("geometry.solid", &json!({"shape": "box", "size": [40, 40, 40]})).unwrap();
    a.execute("model.open", &json!({"thickness": 10})).unwrap();
    put(&mut a, 20.0);
    let mut b = Session::new();
    b.execute("geometry.solid", &json!({"shape": "box", "size": [40, 40, 40]})).unwrap();
    b.model = b.model.translated(emcraft_model::Vec3::new(-0.02, -0.02, -0.02)).unwrap();
    b.execute("model.open", &json!({"thickness": 10})).unwrap();
    put(&mut b, 0.0);
    [a, b]
}

fn number(v: &Value, key: &str) -> f64 {
    v[key].as_f64().unwrap_or_else(|| panic!("{key} in {v}"))
}

/// **Setup**: a 1 mm current element at the middle of a 40 mm box lined by 10 mm at 10 GHz, the box
/// at the origin and centred on it. **Closed form**: the Hertzian dipole's η₀(kIl)²/(12π) (Balanis
/// §4.2) — both boxes' radiated power within the coarse mesh's 10 % of it (λ/4 elements: their
/// meshes differ, 0.427 and 0.483 W against 0.45), the moved one's directivity broadside 3/2.
#[test]
fn a_box_region_off_the_origin_radiates_as_at_it() {
    let [mut a, mut b] = both(&|s, c| {
        s.execute("source.dipole", &json!({"at": [c + 0.1, c + 0.2, c - 0.1], "length": 1})).unwrap();
        s.execute("mesh.generate", &json!({"per_wavelength": 4, "frequency": 10})).unwrap();
    });
    let (ra, rb) =
        (a.execute("solve.radiation", &json!({"frequency": 10})).unwrap(), b.execute("solve.radiation", &json!({"frequency": 10})).unwrap());
    let (pa, pb, exact) = (number(&ra, "radiated_power"), number(&rb, "radiated_power"), number(&ra, "exact_radiated_power"));
    assert!((pa / exact - 1.0).abs() < 0.1 && (pb / exact - 1.0).abs() < 0.1, "{pa} {pb} against {exact}");
    let d = b.execute("results.far_field", &json!({"theta": [90], "phi": 30})).unwrap();
    approx::assert_abs_diff_eq!(number(&d["points"][0], "directivity") / 1.5, 1.0, epsilon = 0.1);
}

/// **Setup**: a 2 mm gap fed at the middle of the same boxes in the time domain (2 mm cells).
/// **Closed form**: the short dipole's radiation resistance 80π²(l/λ)² = 3.51 Ω (Balanis §4.2) —
/// the moved box's the same as the box at the origin's to rounding (the run is the same grid,
/// moved), its Huygens box moved with it.
#[test]
fn a_box_region_off_the_origin_rings_as_at_it() {
    let [mut a, mut b] = both(&|s, c| {
        s.execute("model.port.lumped", &json!({"name": "gap", "from": [c, c, c - 1.0], "to": [c, c, c + 1.0], "impedance": 1e9, "voltage": 1e9}))
            .unwrap();
    });
    let p = |s: &mut Session| {
        s.execute("solve.time_domain", &json!({"frequencies": [10], "cell": 2})).unwrap()["points"][0]["ports"][0]["impedance"]["re"]
            .as_f64()
            .unwrap()
    };
    let (ra, rb) = (p(&mut a), p(&mut b));
    assert!((ra / 3.514 - 1.0).abs() < 0.15, "{ra}");
    assert!((ra / rb - 1.0).abs() < 1e-9, "{ra} {rb}");
    let ff = b.execute("results.far_field", &json!({"theta": [90], "solver": "time_domain"})).unwrap();
    approx::assert_abs_diff_eq!(number(&ff["points"][0], "directivity") / 1.5, 1.0, epsilon = 0.1);
}

/// **Setup**: a 1 mm current element placed with no region, the band 8–10 GHz.
/// **Rule** (ADR 0005): the region starts automatic round it — margin and layer a quarter of the
/// wavelength at 10 GHz (7.4948 mm, rounded up to 7.5 mm each) — and follows the band; then the dipole's
/// η₀(kIl)²/(12π) (Balanis §4.2) within 5 %: a quarter-wavelength margin and layer leave it
/// mesh-sensitive (+4.1 % at λ/8, −0.65 % at λ/10, +0.31 % at λ/12).
#[test]
fn a_dipole_placed_with_no_region_gets_one_round_it() {
    let mut s = Session::new();
    s.execute("source.dipole", &json!({"at": [5, -3, 2], "length": 1})).unwrap();
    assert!(matches!(s.model.domain.region, emcraft_model::Region::Auto { .. }));
    s.execute("project.setup", &json!({"kind": "radiation", "frequency": 10})).unwrap();
    let r = s.execute("model.region", &json!({"mode": "auto"})).unwrap();
    // λ/4 at 10 GHz, 7.4948 mm, rounded up to two digits.
    let quarter = 7.5;
    let lo = r["box"][0][0].as_f64().unwrap();
    assert!((lo - (5.0 - 2.0 * quarter)).abs() < 1e-6, "{r}");
    approx::assert_abs_diff_eq!(r["layer"].as_f64().unwrap(), quarter, epsilon = 1e-9);
    s.execute("mesh.generate", &json!({"per_wavelength": 8, "frequency": 10})).unwrap();
    let p = s.execute("solve.radiation", &json!({"frequency": 10})).unwrap();
    let (got, exact) = (number(&p, "radiated_power"), number(&p, "exact_radiated_power"));
    assert!((got / exact - 1.0).abs() < 0.05, "{got} against {exact}");
    // A body placed far off grows it; pinning keeps it as it is.
    s.execute("geometry.body", &json!({"shape": "sphere", "radius": 2, "at": [40, 0, 0], "material": "metal"})).unwrap();
    let hi = s.model.domain.solid.as_ref().and_then(|x| x.shape.box_bounds()).unwrap().max.x * 1e3;
    assert!((hi - (42.0 + 2.0 * quarter)).abs() < 1e-6, "{hi}");
    s.execute("model.region", &json!({"mode": "pinned"})).unwrap();
    s.execute("geometry.body", &json!({"shape": "sphere", "radius": 1, "at": [-5, 0, 0], "material": "metal"})).unwrap();
    assert_eq!(s.model.domain.solid.as_ref().and_then(|x| x.shape.box_bounds()).unwrap().max.x * 1e3, hi);
    // An end port has no place in an automatic region.
    assert!(s.execute("model.region", &json!({"mode": "sideways"})).is_err());
}

/// **Setup**: a WR-90 guide 30 mm long drawn as an interior body centred on the z axis (a box
/// from (−11.43, −5.08, 0) mm), its ends ports, at 10 GHz; a metal post placed before it stays.
/// **Closed form**: the matched straight guide's TE10: S11 = 0, S21 = e^{−jβL},
/// β = √(k₀² − (π/a)²) (Pozar §3.3).
#[test]
fn an_interior_body_is_the_region_and_keeps_the_bodies() {
    let mut s = Session::new();
    s.execute("geometry.body", &json!({"shape": "box", "size": [22.86, 10.16, 30], "at": [-11.43, -5.08, 0], "material": "interior"})).unwrap();
    assert_eq!(s.model.domain.region, emcraft_model::Region::Pinned);
    assert!(s.model.domain.open.is_none());
    s.execute("model.port.create", &json!({"name": "1", "end": "start"})).unwrap();
    s.execute("model.port.create", &json!({"name": "2", "end": "end"})).unwrap();
    s.execute("mesh.generate", &json!({"size": 3})).unwrap();
    let r = s.execute("solve.frequency", &json!({"frequencies": [10]})).unwrap();
    let k = 2.0 * std::f64::consts::PI * 10e9 / emcraft_model::units::C0;
    let beta = (k * k - (std::f64::consts::PI / 0.02286f64).powi(2)).sqrt();
    let want = emcraft_solver::Complex64::from_polar(1.0, -beta * 0.03);
    let (s11, s21) = (super::s(&r, 0, "s11"), super::s(&r, 0, "s21"));
    assert!(s11.norm() < 0.01 && (s21 - want).norm() < 0.01, "S11 {s11}, S21 {s21} against {want}");
    // A metal post placed first stays when the interior is drawn round it.
    let mut t = Session::new();
    t.execute("geometry.body", &json!({"shape": "cylinder", "radius": 1, "length": 10.16, "at": [0, 0, 15], "axis": "y", "material": "metal"}))
        .unwrap();
    t.execute("geometry.body", &json!({"shape": "box", "size": [22.86, 10.16, 30], "at": [-11.43, -5.08, 0], "material": "interior"})).unwrap();
    assert_eq!(t.model.features.len(), 1);
    // An interior cylinder off the origin is refused.
    assert!(t.execute("geometry.body", &json!({"shape": "cylinder", "radius": 5, "length": 10, "at": [1, 0, 0], "material": "interior"})).is_err());
}

/// The steps' states come in the bar's order (each at its step's index), the region after the
/// settings whose band sizes it (ADR 0005).
#[test]
fn the_steps_come_in_the_bar_s_order_the_region_after_the_settings() {
    let mut s = Session::new();
    s.execute("geometry.body", &json!({"shape": "sphere", "radius": 5, "material": "metal"})).unwrap();
    let states = crate::workflow::steps(&s);
    assert!(states.iter().enumerate().all(|(k, st)| st.step.index() == k));
    use crate::workflow::Step;
    assert!(Step::Setup.index() < Step::Boundaries.index() && Step::Boundaries.index() < Step::Mesh.index());
    assert_eq!(Step::Boundaries.label(), "Region");
    assert_eq!(Step::of_command("model.region"), Some(Step::Boundaries));
}
