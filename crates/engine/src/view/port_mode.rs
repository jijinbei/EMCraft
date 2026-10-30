//! A port mode's shape: its transverse E across the port's face.

use emcraft_model::units::decibels;
use emcraft_render::plot::{Arrows, FieldPlot, Scalar, ScalarKind};
use emcraft_render::{Axis, Pen};

use crate::{EngineError, Result, Session};

/// Grid points along the longer side of a port-mode picture.
const MODE_SIDE: usize = 120;

/// Arrows along the longer side of a port-mode picture.
const MODE_ARROWS: usize = 18;

/// Mode `mode` (from 0) of the port named `port` at `frequency` (Hz): |e_t| in colour and e_t as
/// arrows across the port's face, conductors grey. A mode's amplitude is arbitrary: the picture
/// shows its shape.
pub fn port_mode_plot(session: &Session, port: &str, mode: usize, frequency: f64, order: u8) -> Result<FieldPlot> {
    let order = emcraft_fem::Order::from_u8(order).ok_or_else(|| EngineError::Params("`order` must be 1 or 2".into()))?;
    let (k, p) = session.model.port(port)?;
    let pat = emcraft_solver::driven::port_mode_pattern(&session.model, order, frequency, k, mode, MODE_SIDE)?;
    let l = session.display.length;
    let fu = session.display.frequency;
    let lu = l.symbol();
    let magnitude: Vec<f64> = pat.e.iter().map(|e| e[0].hypot(e[1])).collect();
    let (nx, ny) = (pat.nx, pat.ny);
    let at = |i: usize, j: usize| {
        [pat.x.0 + (pat.x.1 - pat.x.0) * i as f64 / (nx - 1).max(1) as f64, pat.y.0 + (pat.y.1 - pat.y.0) * j as f64 / (ny - 1).max(1) as f64]
    };
    // Arrows on a coarser grid of the same points.
    let step = (nx.max(ny) / MODE_ARROWS).max(1);
    let mut arrows = Vec::new();
    for j in (step / 2..ny).step_by(step) {
        for i in (step / 2..nx).step_by(step) {
            if let Some(e) = pat.e.get(j * nx + i).filter(|e| e[0].is_finite() && e[1].is_finite()) {
                let q = at(i, j);
                arrows.push(([l.from_si(q[0]), l.from_si(q[1])], *e));
            }
        }
    }
    let spacing = l.from_si((pat.x.1 - pat.x.0).max(pat.y.1 - pat.y.0)) / MODE_ARROWS as f64;
    let state = if pat.beta2 > 0.0 {
        format!("propagates, β = {:.4} rad/{lu}", pat.beta2.sqrt() * l.to_si(1.0))
    } else {
        format!("cut off, {:.3} dB/{lu}", decibels((-(-pat.beta2).sqrt() * l.to_si(1.0)).exp()).abs())
    };
    // The face's own axes: x and y on a z face, y and z on an x face, x and z on a y face.
    let (u, v, _) = p.frame().axes();
    let (u, v) = (emcraft_model::Axis::name_of(u), emcraft_model::Axis::name_of(v));
    Ok(FieldPlot {
        title: format!("Port {}, {}  f = {:.4} {}", p.name, pat.name, fu.from_si(frequency), fu.symbol()),
        subtitle: format!("transverse E of the mode (its shape: the amplitude is arbitrary); {state}"),
        x: Axis::new(&format!("{u} ({lu})"), l.from_si(pat.x.0), l.from_si(pat.x.1)),
        y: Axis::new(&format!("{v} ({lu})"), l.from_si(pat.y.0), l.from_si(pat.y.1)),
        scalar: Some(Scalar { label: "$|E_t|$".into(), nx, ny, values: magnitude, kind: ScalarKind::Magnitude, range: None }),
        arrows: vec![Arrows { label: "E (in plane)".into(), pen: Pen::EArrows, arrows, scale: None }],
        lines: Vec::new(),
        arrow_length: spacing * 0.9,
    })
}
