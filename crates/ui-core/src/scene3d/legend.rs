//! The 3D view's legend for an overlay: the materials, the ports and sources, or the
//! boundaries.

use emcraft_engine::Session;
use emcraft_model::End;

use super::style::{Colours, material_rgb, symmetry_rgb};
use emcraft_render::Rgb;

use super::Overlay;
use crate::lang::{args, plural, tr, trf};

pub(super) fn legend(colours: &Colours, session: &Session, overlay: Overlay) -> Vec<(String, Rgb)> {
    let mut out = Vec::new();
    let m = session.model();
    let l = session.display().length;
    match overlay {
        Overlay::Materials => {
            for (i, mat) in m.materials.iter().enumerate().filter(|(_, m)| !session.views().hidden_materials.contains(&m.name)) {
                out.push((format!("{} (εr {})", mat.name, mat.eps_r.get()), material_rgb(colours, m, i)));
            }
        }
        Overlay::Ports => {
            // Ports named as the browser and the Excitation step name them.
            // The driven port in the input's colour, the others in the outputs'.
            let driven = session.driven_port();
            for (i, p) in m.ports.iter().enumerate() {
                let template = match (p.face.is_some(), p.end) {
                    (true, _) => t!("Port {name} ({axis} = {at} {unit})"),
                    (false, End::Start) => t!("Port {name} (start)"),
                    (false, End::End) => t!("Port {name} (end)"),
                };
                let colour = if driven == Some(i) { colours.scene.port_input } else { colours.scene.port_output };
                let l = session.display().length;
                let (axis, at) = crate::browser::face_place(p, l);
                out.push((trf(template, args!(name = p.name, axis = axis, at = at, unit = l.symbol())), colour));
            }
            for p in &m.lumped {
                out.push((trf(t!("Lumped port {name}"), args!(name = p.name)), colours.scene.port));
            }
            if !m.sources.is_empty() {
                let n = m.sources.len();
                out.push((plural(n, t!("{n} source"), t!("{n} sources"), args!(n = n)), colours.scene.source));
            }
        }
        Overlay::Boundaries => {
            for s in &m.domain.symmetry {
                let wall = tr(match s.kind {
                    emcraft_model::SymmetryKind::Electric => t!("electric wall"),
                    emcraft_model::SymmetryKind::Magnetic => t!("magnetic wall"),
                });
                let axis = s.axis.id();
                let at = format!("{axis} = {} {}", l.from_si(s.at), l.symbol());
                out.push((trf(t!("symmetry: {wall}, {at}"), args!(wall = wall, at = at)), symmetry_rgb(colours, s.kind)));
            }
            if let Some(p) = &m.domain.periodic {
                let axes: Vec<&str> = p.axes().into_iter().map(|d| ["x", "y", "z"][d.min(2)]).collect();
                let angles = if p.x || p.y { format!(", θ {:.1}°, φ {:.1}°", p.theta.to_degrees(), p.phi.to_degrees()) } else { String::new() };
                out.push((trf(t!("periodic boundaries: {cell}"), args!(cell = format!("{}{angles}", axes.join(", ")))), colours.scene.periodic));
            }
            if let Some(r) = &m.domain.rotation {
                let group = format!("C{}", r.n);
                out.push((trf(t!("rotation: {group}, harmonic {m}"), args!(group = group, m = r.m)), colours.scene.periodic));
            }
            let walls = match m.domain.wall_conductivity {
                Some(s) => trf(t!("walls σ = {sigma} S/m"), args!(sigma = crate::number::value(s.get()))),
                None => tr(t!("walls: perfect conductor")).into(),
            };
            out.push((walls, colours.scene.metal));
            if let Some(o) = &m.domain.open {
                let thickness = format!("{} {}", l.from_si(o.thickness.get()), l.symbol());
                out.push((trf(t!("open boundary: PML {thickness}"), args!(thickness = thickness)), colours.scene.pml));
            }
        }
        Overlay::Plain | Overlay::Mesh | Overlay::Field | Overlay::Board => {}
    }
    out
}
