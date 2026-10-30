//! The 3D view's legend for an overlay: the materials, the ports and sources, or the
//! boundaries.

use emcraft_engine::Session;
use emcraft_model::{Axis, End};

use super::style::{material_rgb, sc, symmetry_rgb};
use super::{Built, Overlay};

pub(super) fn legend(session: &Session, overlay: Overlay, out: &mut Built) {
    let m = session.model();
    let l = session.display().length;
    match overlay {
        Overlay::Materials => {
            for (i, mat) in m.materials.iter().enumerate() {
                out.legend.push((format!("{} (εr {})", mat.name, mat.eps_r), material_rgb(m, i)));
            }
        }
        Overlay::Ports => {
            for p in &m.ports {
                out.legend.push((format!("port {} ({})", p.name, if p.end == End::Start { "start" } else { "end" }), sc().port));
            }
            for p in &m.lumped {
                out.legend.push((format!("lumped port {}", p.name), sc().port));
            }
            if !m.sources.is_empty() {
                out.legend.push((format!("{} source(s)", m.sources.len()), sc().source));
            }
        }
        Overlay::Boundaries => {
            for s in &m.symmetry {
                let wall = match s.kind {
                    emcraft_model::SymmetryKind::Electric => "electric wall",
                    emcraft_model::SymmetryKind::Magnetic => "magnetic wall",
                };
                let axis = if s.axis == Axis::X { "x" } else { "y" };
                out.legend.push((format!("symmetry: {wall}, {axis} = {} {}", l.from_si(s.at), l.symbol()), symmetry_rgb(s.kind)));
            }
            if let Some(p) = &m.periodic {
                let axes: Vec<&str> = p.axes().into_iter().map(|d| ["x", "y", "z"][d.min(2)]).collect();
                let angles = if p.x || p.y { format!(", θ {:.1}°, φ {:.1}°", p.theta.to_degrees(), p.phi.to_degrees()) } else { String::new() };
                out.legend.push((format!("periodic: {}{angles}", axes.join(", ")), sc().periodic));
            }
            let walls = match m.wall_conductivity {
                Some(s) => format!("walls σ = {s:.3e} S/m"),
                None => "walls: perfect conductor".into(),
            };
            out.legend.push((walls, sc().metal));
            if let Some(o) = &m.open {
                out.legend.push((format!("open boundary: sc().pml {} {}", l.from_si(o.thickness), l.symbol()), sc().pml));
            }
        }
        Overlay::Plain | Overlay::Mesh | Overlay::Field => {}
    }
}
