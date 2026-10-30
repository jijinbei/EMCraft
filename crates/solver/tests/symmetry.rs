//! Symmetry planes: a model solved on its half (or quarter) below a magnetic or an electric wall
//! against the whole model. The discrete identity first — the whole mesh made of the half and its
//! mirror image gives the half's S to rounding — then the half against the whole model meshed on
//! its own and against mode matching, electric walls (TE20 as the first mode), the numerical
//! cross-section solver's free rim and the images of the fields.

use std::time::Instant;

use emcraft_fem::Order;
use emcraft_model::units::C0;
use emcraft_model::{Axis, Boundary, BoundaryFace, End, Mesh, ModeSolver, Model, Point3, Port, Section, Symmetry, SymmetryKind, Waveguide};
use emcraft_solver::Driven;
use emcraft_theory::{HPlaneLine, HSection};

const A: f64 = 0.02286;
const B: f64 = 0.01016;

/// A 2 mm thick centred inductive iris, 12 mm wide, 15 mm from each port.
fn iris(modes: u32, planes: &[Symmetry], opts: &emcraft_mesh::Options) -> Model {
    let mut m = Model::new();
    let sec = |l: f64, w: Option<f64>| Section { length: l, material: "vacuum".into(), width: w, ..Default::default() };
    m.set_waveguide(Waveguide {
        width: A,
        height: B,
        sections: vec![sec(0.015, None), sec(0.002, Some(0.012)), sec(0.015, None)],
        layers: Vec::new(),
        strips: Vec::new(),
        vias: Vec::new(),
    })
    .unwrap();
    m.add_port(Port::new("1", End::Start).with_modes(modes)).unwrap();
    m.add_port(Port::new("2", End::End).with_modes(modes)).unwrap();
    m.set_symmetry(planes.to_vec()).unwrap();
    m.mesh = Some(emcraft_mesh::waveguide_with(&m, opts).unwrap().into());
    m
}

fn magnetic_x() -> Symmetry {
    Symmetry { axis: Axis::X, at: A / 2.0, kind: SymmetryKind::Magnetic }
}

/// The half mesh and its mirror image about x = at, joined on the plane (whose faces go).
fn mirrored(half: &Mesh, at: f64) -> Mesh {
    let mut nodes = half.nodes.clone();
    let image: Vec<u32> = half
        .nodes
        .iter()
        .enumerate()
        .map(|(i, p)| {
            if (p.x - at).abs() <= 1e-12 {
                i as u32
            } else {
                nodes.push(Point3::new(2.0 * at - p.x, p.y, p.z));
                (nodes.len() - 1) as u32
            }
        })
        .collect();
    let nt = half.tets.len() as u32;
    let mut tets = half.tets.clone();
    // A reflection turns the orientation over: two corners swap.
    tets.extend(half.tets.iter().map(|t| [image[t[0] as usize], image[t[1] as usize], image[t[3] as usize], image[t[2] as usize]]));
    let mut tet_material = half.tet_material.clone();
    tet_material.extend_from_slice(&half.tet_material);
    let mut boundary = Vec::new();
    for f in half.boundary.iter().filter(|f| !matches!(f.tag, Boundary::Symmetry(_))) {
        boundary.push(*f);
        boundary.push(BoundaryFace { nodes: f.nodes.map(|n| image[n as usize]), tet: f.tet + nt, tag: f.tag });
    }
    let mut mesh = Mesh { nodes, tets, tet_material, boundary, grid: None, buckets: None, curved: Vec::new() };
    mesh.buckets = Some(emcraft_mesh::solid::buckets(&mesh));
    mesh
}

fn reference() -> HPlaneLine {
    let s = |length: f64, x0: f64, width: f64| HSection { length, eps_r: 1.0, mu_r: 1.0, x0, width, loss_tangent: 0.0 };
    HPlaneLine { sections: vec![s(0.015, 0.0, A), s(0.002, (A - 0.012) / 2.0, 0.012), s(0.015, 0.0, A)] }
}

#[test]
fn half_model_is_the_whole_mirrored_model_to_rounding() {
    let opts = emcraft_mesh::Options { size_y: Some(B), ..emcraft_mesh::Options::new(0.004) };
    // Three modes even about x = a/2 on the half (TE10, TE11, TM11) are the even ones of the
    // whole guide's five lowest (TE10, TE20, TE01, TE11, TM11); the odd ones are not excited.
    let half = iris(3, &[magnetic_x()], &opts);
    let mut whole = iris(5, &[], &opts);
    whole.mesh = Some(mirrored(half.mesh.as_ref().unwrap(), A / 2.0).into());
    let (dh, dw) = (Driven::new(&half, Order::Two).unwrap(), Driven::new(&whole, Order::Two).unwrap());
    for f in [9e9, 10e9, 11e9] {
        let (rh, rw) = (dh.solve(f).unwrap(), dw.solve(f).unwrap());
        for (p, q) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let d = (rh.s[p][q] - rw.s[p][q]).norm();
            assert!(d < 1e-10, "{f}: S{}{} {} against {} ({d:e})", p + 1, q + 1, rh.s[p][q], rw.s[p][q]);
        }
        // The power a unit wave carries into the half is half the whole's.
        let (ph, pw) = (dh.incident_power(0, f).unwrap(), dw.incident_power(0, f).unwrap());
        assert!((2.0 * ph / pw - 1.0).abs() < 1e-12);
    }
    // The field beyond the plane is the image of the one inside: E_y even, E_x odd, H_x even.
    let r = dh.solve(10e9).unwrap();
    let rw = dw.solve(10e9).unwrap();
    for p in [Point3::new(0.017, 0.004, 0.01), Point3::new(0.02, 0.007, 0.024), Point3::new(0.004, 0.002, 0.003)] {
        let (eh, hh) = dh.fields(&r.fields[0], 10e9, p).unwrap();
        let (ew, hw) = dw.fields(&rw.fields[0], 10e9, p).unwrap();
        let scale = ew.norm_sqr().sqrt();
        assert!((eh - ew).norm_sqr().sqrt() < 1e-8 * scale, "E at {p:?}: {eh:?} against {ew:?}");
        assert!((hh - hw).norm_sqr().sqrt() < 1e-8 * hw.norm_sqr().sqrt(), "H at {p:?}");
    }
}

#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn half_and_quarter_iris_match_the_whole_and_mode_matching() {
    // Measured (order 2, 4 mm, three frequencies): whole 16 676 unknowns in 3.9 s, half 8 520 in
    // 1.2 s, quarter 3 616 in 0.27 s; |S − S_mm| 6.3e-3, 6.5e-3, 6.5e-3.
    // Two layers across y, so the quarter's one is the same lines.
    let opts = emcraft_mesh::Options { size_y: Some(B / 2.0), ..emcraft_mesh::Options::new(0.004) };
    let freqs = [9e9, 10e9, 11e9];
    let line = reference();
    let electric_y = Symmetry { axis: Axis::Y, at: B / 2.0, kind: SymmetryKind::Electric };
    let mut report = Vec::new();
    let mut s21 = Vec::new();
    for (name, planes) in [("whole", vec![]), ("half", vec![magnetic_x()]), ("quarter", vec![magnetic_x(), electric_y])] {
        let m = iris(4, &planes, &opts);
        let t = Instant::now();
        let d = Driven::new(&m, Order::Two).unwrap();
        let mut worst: f64 = 0.0;
        let mut s = Vec::new();
        for f in freqs {
            let r = d.solve(f).unwrap();
            let (s11, s21) = line.solve(f, 160).unwrap();
            worst = worst.max((r.s[0][0] - s11).norm()).max((r.s[1][0] - s21).norm());
            // Lossless and reciprocal on any part.
            let p = r.s[0][0].norm_sqr() + r.s[1][0].norm_sqr();
            assert!((p - 1.0).abs() < 1e-9, "{name} {f}: {p}");
            assert!((r.s[0][1] - r.s[1][0]).norm() < 1e-9);
            s.push(r.s[1][0]);
        }
        let seconds = t.elapsed().as_secs_f64();
        report.push(format!("{name}: {} unknowns, {seconds:.2} s, worst |S − S_mm| {worst:.2e}", d.dofs.unknowns));
        // The whole model's measured error is 6.6e-3 (discontinuities.rs); its parts mesh the
        // same lines.
        assert!(worst < 1.5e-2, "{name}: {worst:e}");
        s21.push((d.dofs.unknowns, s));
    }
    eprintln!("{}", report.join("\n"));
    let (whole, half, quarter) = (&s21[0], &s21[1], &s21[2]);
    // About half and a quarter of the unknowns (the planes' own unknowns are kept or shared).
    assert!(half.0 * 2 < whole.0 * 11 / 10 && half.0 * 2 > whole.0 * 9 / 10, "{} {}", half.0, whole.0);
    assert!(quarter.0 * 4 < whole.0 * 12 / 10, "{} {}", quarter.0, whole.0);
    // Their S differ from the whole's only by their meshes' slightly different lines.
    for k in 0..freqs.len() {
        assert!((half.1[k] - whole.1[k]).norm() < 5e-3, "{} {}", half.1[k], whole.1[k]);
        assert!((quarter.1[k] - half.1[k]).norm() < 5e-3, "{} {}", quarter.1[k], half.1[k]);
    }
}

/// An empty WR-90 guide, 30 mm long.
fn empty(planes: &[Symmetry], solver: ModeSolver) -> Model {
    let mut m = Model::new();
    m.set_waveguide(Waveguide {
        width: A,
        height: B,
        sections: vec![Section::new(0.03, "vacuum")],
        layers: Vec::new(),
        strips: Vec::new(),
        vias: Vec::new(),
    })
    .unwrap();
    m.add_port(Port::new("1", End::Start).with_modes(2).with_mode_solver(solver)).unwrap();
    m.add_port(Port::new("2", End::End).with_modes(2).with_mode_solver(solver)).unwrap();
    m.set_symmetry(planes.to_vec()).unwrap();
    m.mesh = Some(emcraft_mesh::waveguide(&m, 0.002).unwrap().into());
    m
}

#[test]
fn an_electric_wall_keeps_te20_and_a_numeric_port_frees_its_rim() {
    // An electric wall at x = a/2 leaves TE20 the first mode (cut-off c/a = 13.1 GHz).
    let m = empty(&[Symmetry { axis: Axis::X, at: A / 2.0, kind: SymmetryKind::Electric }], ModeSolver::Auto);
    let d = Driven::new(&m, Order::Two).unwrap();
    // Below TE20's cut-off the wave decays from port to port: S21 = e^{−αL} in pseudo-waves.
    let below = d.solve(12e9).unwrap();
    let k = 2.0 * std::f64::consts::PI * 12e9 / C0;
    let alpha = ((2.0 * std::f64::consts::PI / A).powi(2) - k * k).sqrt();
    assert!((below.s[1][0].norm() - (-alpha * 0.03).exp()).abs() < 2e-3 && below.s[0][0].norm() < 1e-2, "{:?}", below.s);
    let f = 16e9;
    let k = 2.0 * std::f64::consts::PI * f / C0;
    let beta = (k * k - (2.0 * std::f64::consts::PI / A).powi(2)).sqrt();
    let r = d.solve(f).unwrap();
    let exact = num_complex::Complex64::from_polar(1.0, -beta * 0.03);
    // TE20 has half TE10's wavelength across x: 4.3e-3 at 3 mm, within 2e-3 at 2 mm.
    assert!((r.s[1][0] - exact).norm() < 2e-3, "{} against {exact}", r.s[1][0]);
    let names = emcraft_solver::driven::port_modes(&m, Order::Two, f).unwrap();
    assert_eq!(names[0].1[0].name, "TE20");

    // The cross-section solver on the half section, its rim on the plane free: TE10.
    let m = empty(&[magnetic_x()], ModeSolver::Numeric);
    let d = Driven::new(&m, Order::Two).unwrap();
    let f = 10e9;
    let k = 2.0 * std::f64::consts::PI * f / C0;
    let beta = (k * k - (std::f64::consts::PI / A).powi(2)).sqrt();
    let r = d.solve(f).unwrap();
    let exact = num_complex::Complex64::from_polar(1.0, -beta * 0.03);
    assert!((r.s[1][0] - exact).norm() < 2e-3, "{} against {exact}", r.s[1][0]);
    let modes = emcraft_solver::driven::port_modes(&m, Order::Two, f).unwrap();
    assert!((modes[0].1[0].beta2 / (beta * beta) - 1.0).abs() < 1e-4, "{} {}", modes[0].1[0].beta2, beta * beta);
}

#[test]
fn a_half_stripline_has_the_whole_lines_impedance() {
    // A stripline cut through its strip by a magnetic wall: the half carries half the power and
    // half the current, so its impedance by power and current is twice the whole's; the solver
    // reports the whole's (Cohn's 2.4 mm strip in εr = 2.2, b = 3 mm: within 0.3 % at this mesh).
    let (b, w, er, length) = (0.003, 0.0024, 2.2, 0.001);
    let a = w + 6.0 * b;
    let line = |planes: &[Symmetry]| {
        let mut m = Model::new();
        m.add_material(emcraft_model::Material::new("fill", er, 1.0).unwrap()).unwrap();
        let strips = vec![emcraft_model::Strip { y: b / 2.0, x0: (a - w) / 2.0, width: w, z0: 0.0, z1: length }];
        m.set_waveguide(Waveguide {
            width: a,
            height: b,
            sections: vec![Section::new(length, "fill")],
            layers: Vec::new(),
            strips,
            vias: Vec::new(),
        })
        .unwrap();
        m.add_port(Port::new("1", End::Start).with_modes(1)).unwrap();
        m.add_port(Port::new("2", End::End).with_modes(1)).unwrap();
        m.set_symmetry(planes.to_vec()).unwrap();
        m.mesh = Some(emcraft_mesh::waveguide(&m, 0.001).unwrap().into());
        Driven::new(&m, Order::Two).unwrap().solve(2e9).unwrap()
    };
    let (whole, half) = (line(&[]), line(&[Symmetry { axis: Axis::X, at: a / 2.0, kind: SymmetryKind::Magnetic }]));
    let (zw, zh) = (whole.impedances[0].unwrap(), half.impedances[0].unwrap());
    let cohn = emcraft_theory::stripline::z0(w, b, er);
    assert!((zh / zw - 1.0).abs() < 2e-3, "{zh} {zw}");
    assert!((zh / cohn - 1.0).abs() < 5e-3, "{zh} {cohn}");
    assert!((half.s[1][0] - whole.s[1][0]).norm() < 1e-3, "{} {}", half.s[1][0], whole.s[1][0]);
}

#[test]
fn a_stripline_halved_through_its_strip_plane_keeps_the_whole_lines_impedance() {
    // The stripline's TEM is even about the strip's plane y = b/2 (tangential E even, normal E
    // odd): a magnetic wall there keeps it, the strip a conductor on the wall. Power and current
    // both halve, so the reported impedance is the whole's; S is the whole's.
    let (b, w, er, length) = (0.003, 0.0024, 2.2, 0.002);
    let a = w + 6.0 * b;
    let line = |planes: &[Symmetry]| {
        let mut m = Model::new();
        m.add_material(emcraft_model::Material::new("fill", er, 1.0).unwrap()).unwrap();
        let strips = vec![emcraft_model::Strip { y: b / 2.0, x0: (a - w) / 2.0, width: w, z0: 0.0, z1: length }];
        m.set_waveguide(Waveguide {
            width: a,
            height: b,
            sections: vec![Section::new(length, "fill")],
            layers: Vec::new(),
            strips,
            vias: Vec::new(),
        })
        .unwrap();
        m.add_port(Port::new("1", End::Start).with_modes(1)).unwrap();
        m.add_port(Port::new("2", End::End).with_modes(1)).unwrap();
        m.set_symmetry(planes.to_vec()).unwrap();
        m.mesh = Some(emcraft_mesh::waveguide(&m, 0.001).unwrap().into());
        Driven::new(&m, Order::Two).unwrap().solve(2e9).unwrap()
    };
    let (whole, half) = (line(&[]), line(&[Symmetry { axis: Axis::Y, at: b / 2.0, kind: SymmetryKind::Magnetic }]));
    let (zw, zh) = (whole.impedances[0].unwrap(), half.impedances[0].unwrap());
    let cohn = emcraft_theory::stripline::z0(w, b, er);
    assert!((zh / zw - 1.0).abs() < 2e-3, "{zh} {zw}");
    assert!((zh / cohn - 1.0).abs() < 5e-3, "{zh} {cohn}");
    assert!((half.s[1][0] - whole.s[1][0]).norm() < 1e-3, "{} {}", half.s[1][0], whole.s[1][0]);
}
