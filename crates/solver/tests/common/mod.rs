//! Models of the verification cases, built from their descriptions.
#![allow(dead_code)]

use emcraft_model::{End, Material, Model, Port, Section, Waveguide};
use emcraft_theory::{Layer, Te10Line};

/// WR-90: a = 22.86 mm, b = 10.16 mm.
pub const A: f64 = 0.02286;
pub const B: f64 = 0.01016;

/// A WR-90 guide with sections (length m, εr), meshed at `size`, ports at both ends.
pub fn guide(sections: &[(f64, f64)], size: f64, modes: u32) -> Model {
    let mut m = Model::new();
    let mut names = Vec::new();
    for (i, (_, eps)) in sections.iter().enumerate() {
        let name = if *eps == 1.0 { "vacuum".to_owned() } else { format!("fill{i}") };
        if m.material(&name).is_err() {
            m.add_material(Material::new(&name, *eps, 1.0).unwrap()).unwrap();
        }
        names.push(name);
    }
    let sections = sections.iter().zip(&names).map(|((l, _), n)| Section::new(*l, n)).collect();
    m.set_waveguide(Waveguide { width: A, height: B, sections }).unwrap();
    m.add_port(Port { name: "1".into(), end: End::Start, modes, face: None }).unwrap();
    m.add_port(Port { name: "2".into(), end: End::End, modes, face: None }).unwrap();
    m.mesh = Some(emcraft_mesh::waveguide(&m, size).unwrap());
    m
}

pub fn line(sections: &[(f64, f64)]) -> Te10Line {
    Te10Line { a: A, layers: sections.iter().map(|(l, e)| Layer::lossless(*l, *e, 1.0)).collect() }
}

/// The high-pass filter: PTFE-like fill (εr = 2) feeding a 20 mm empty section below its cut-off.
pub const FILTER: [(f64, f64); 3] = [(0.015, 2.0), (0.02, 1.0), (0.015, 2.0)];
