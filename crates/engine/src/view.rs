//! Pictures of results, the same headless and in the desktop app: the electric and magnetic
//! fields on a plane through the waveguide (a component as a colour map, the in-plane vectors as
//! arrows, at one phase of the period) and the S-parameters against frequency with the exact
//! curves of the TE₁₀ theory.
//!
//! The planes, in the guide's frame (x across the broad wall, y across the narrow wall, z along):
//! - `h` (H-plane): z across, x up, at height y — for TE₁₀, E_y (out of the plane) as colour and
//!   the magnetic field's loops (H_z, H_x) as arrows;
//! - `e` (E-plane): z across, y up, at x — E_y in the plane;
//! - `cross` (cross-section): x across, y up, at z — the TE₁₀ pattern of E (vertical) and H.

use emcraft_model::units::decibels;
use emcraft_render::chart::{Chart, Series, Style};
use emcraft_render::plot::{Arrows, FieldPlot, Scalar, ScalarKind};
use emcraft_render::{Axis, Rgb};
use emcraft_solver::Complex64 as C;
use emcraft_theory::{Layer, Te10Line, te10_cutoff};

use crate::{EngineError, Result, Session};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plane {
    H,
    E,
    Cross,
}

impl Plane {
    pub fn parse(s: &str) -> Option<Plane> {
        match s {
            "h" | "h-plane" => Some(Plane::H),
            "e" | "e-plane" => Some(Plane::E),
            "cross" | "cross-section" => Some(Plane::Cross),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Plane::H => "h",
            Plane::E => "e",
            Plane::Cross => "cross",
        }
    }
}

/// What a field picture shows.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldView {
    /// Hz.
    pub frequency: f64,
    pub plane: Plane,
    /// The plane's position along its normal (m); its middle when `None`.
    pub at: Option<f64>,
    /// The colour-mapped quantity: `ex`, `ey`, `ez`, `hx`, `hy`, `hz`, `|e|` or `|h|`.
    pub component: String,
    pub show_e: bool,
    pub show_h: bool,
    /// Phase ωt (rad): the picture shows Re(F e^{jωt}).
    pub phase: f64,
    /// The excited port's index.
    pub excitation: usize,
}

impl FieldView {
    pub fn new(frequency: f64, plane: Plane) -> FieldView {
        FieldView { frequency, plane, at: None, component: "ey".into(), show_e: true, show_h: true, phase: 0.0, excitation: 0 }
    }
}

/// Samples the scalar on this many points along the plane's longer side.
const SCALAR_SAMPLES: usize = 240;
/// Arrows along the longer side.
const ARROWS: usize = 24;
/// A vector field whose in-plane part peaks below this share of its whole peak draws no arrows.
const IN_PLANE: f64 = 0.05;

/// The field plot of a view.
pub fn field_plot(session: &Session, view: &FieldView) -> Result<FieldPlot> {
    let sweep = session.sweep.as_ref().ok_or_else(|| EngineError::Failed("no results: solve first (solve.frequency)".into()))?;
    let w = session.model.waveguide.as_ref().ok_or_else(|| EngineError::Failed("no waveguide".into()))?;
    let r = sweep.at(view.frequency, true).ok_or_else(|| EngineError::Failed("no results".into()))?;
    let x = r.fields.get(view.excitation).ok_or_else(|| EngineError::Params(format!("no excitation {}", view.excitation + 1)))?;
    let (a, b, len) = (w.width, w.height, w.length());
    // (horizontal span, vertical span, default position) and the map from plane to guide.
    let (hspan, vspan, normal_span) = match view.plane {
        Plane::H => (len, a, b),
        Plane::E => (len, b, a),
        Plane::Cross => (a, b, len),
    };
    let at = view.at.unwrap_or(normal_span / 2.0).clamp(0.0, normal_span);
    let point = |u: f64, v: f64| match view.plane {
        Plane::H => [v, at, u],
        Plane::E => [at, v, u],
        Plane::Cross => [u, v, at],
    };
    // In-plane components of a vector: (horizontal, vertical).
    let in_plane = |f: [C; 3]| match view.plane {
        Plane::H => (f[2], f[0]),
        Plane::E => (f[2], f[1]),
        Plane::Cross => (f[0], f[1]),
    };
    let rot = C::from_polar(1.0, view.phase);
    let re = |c: C| (c * rot).re;
    let comp: Box<dyn Fn(&[C; 3], &[C; 3]) -> (f64, f64)> = match view.component.as_str() {
        c @ ("ex" | "ey" | "ez" | "hx" | "hy" | "hz") => {
            let k = match &c[1..] {
                "x" => 0,
                "y" => 1,
                _ => 2,
            };
            let magnetic = c.starts_with('h');
            Box::new(move |e: &[C; 3], h: &[C; 3]| {
                let v = if magnetic { h[k] } else { e[k] };
                (re(v), v.norm())
            })
        }
        "|e|" => Box::new(|e: &[C; 3], _: &[C; 3]| {
            let m = e.iter().map(|c| re(*c).powi(2)).sum::<f64>().sqrt();
            (m, e.iter().map(|c| c.norm_sqr()).sum::<f64>().sqrt())
        }),
        "|h|" => Box::new(|_: &[C; 3], h: &[C; 3]| {
            let m = h.iter().map(|c| re(*c).powi(2)).sum::<f64>().sqrt();
            (m, h.iter().map(|c| c.norm_sqr()).sum::<f64>().sqrt())
        }),
        other => return Err(EngineError::Params(format!("`component` must be ex, ey, ez, hx, hy, hz, |e| or |h|, got `{}`", other.chars().take(20).collect::<String>()))),
    };
    let magnitude = view.component.starts_with('|');

    let longer = hspan.max(vspan);
    let (nx, ny) = (((SCALAR_SAMPLES as f64 * hspan / longer).ceil() as usize).max(2), ((SCALAR_SAMPLES as f64 * vspan / longer).ceil() as usize).max(2));
    let mut values = Vec::with_capacity(nx * ny);
    let mut peak: f64 = 0.0;
    let driven = &sweep.driven;
    for j in 0..ny {
        for i in 0..nx {
            let (u, v) = (hspan * i as f64 / (nx - 1) as f64, vspan * j as f64 / (ny - 1) as f64);
            match driven.fields(x, r.frequency, point(u, v)) {
                Some((e, h)) => {
                    let (val, amp) = comp(&e, &h);
                    peak = peak.max(amp);
                    values.push(val);
                }
                None => values.push(f64::NAN),
            }
        }
    }
    let l = session.display.length;
    let lname = l.symbol();
    let mut arrow_sets = Vec::new();
    let step = longer / ARROWS as f64;
    let (ax, ay) = (((hspan / step).round() as usize).max(2), ((vspan / step).round() as usize).max(2));
    let mut e_arrows = Vec::new();
    let mut h_arrows = Vec::new();
    let (mut e_peak, mut h_peak): (f64, f64) = (0.0, 0.0);
    let (mut e_total, mut h_total): (f64, f64) = (0.0, 0.0);
    let norm = |f: &[C; 3]| f.iter().map(|c| c.norm_sqr()).sum::<f64>().sqrt();
    for j in 0..ay {
        for i in 0..ax {
            let (u, v) = (hspan * (i as f64 + 0.5) / ax as f64, vspan * (j as f64 + 0.5) / ay as f64);
            let Some((e, h)) = driven.fields(x, r.frequency, point(u, v)) else { continue };
            let pos = [l.from_si(u), l.from_si(v)];
            let (eu, ev) = in_plane(e);
            let (hu, hv) = in_plane(h);
            e_total = e_total.max(norm(&e));
            h_total = h_total.max(norm(&h));
            e_peak = e_peak.max((eu.norm_sqr() + ev.norm_sqr()).sqrt());
            h_peak = h_peak.max((hu.norm_sqr() + hv.norm_sqr()).sqrt());
            e_arrows.push((pos, [re(eu), re(ev)]));
            h_arrows.push((pos, [re(hu), re(hv)]));
        }
    }
    // Arrows scaled by the peak over the period, so an animation keeps one scale. A field
    // with (next to) nothing in the plane — E in the H-plane for TE10, whose in-plane part is
    // discretisation error — draws no arrows.
    if view.show_e && e_peak > IN_PLANE * e_total {
        arrow_sets.push(Arrows { label: "E (in plane)".into(), color: Rgb(20, 20, 20), arrows: e_arrows, scale: Some(e_peak) });
    }
    if view.show_h && h_peak > IN_PLANE * h_total {
        arrow_sets.push(Arrows { label: "H (in plane)".into(), color: Rgb(0, 120, 60), arrows: h_arrows, scale: Some(h_peak) });
    }

    let (hlabel, vlabel, nlabel) = match view.plane {
        Plane::H => ("z", "x", "y"),
        Plane::E => ("z", "y", "x"),
        Plane::Cross => ("x", "y", "z"),
    };
    let mut lines = Vec::new();
    if view.plane != Plane::Cross {
        for z in w.interfaces().iter().skip(1).take(w.sections.len().saturating_sub(1)) {
            lines.push([[l.from_si(*z), 0.0], [l.from_si(*z), l.from_si(vspan)]]);
        }
    }
    let unit = if view.component.contains('h') { "A/m" } else { "V/m" };
    let fsym = session.display.frequency;
    let port_name = session.model.ports.get(view.excitation).map_or("1", |p| p.name.as_str());
    Ok(FieldPlot {
        title: format!(
            "{} plane, {} = {} {lname}   f = {} {}",
            match view.plane {
                Plane::H => "H",
                Plane::E => "E",
                Plane::Cross => "Cross-section",
            },
            nlabel,
            emcraft_render::axes::format(l.from_si(at)),
            emcraft_render::axes::format(fsym.from_si(r.frequency)),
            fsym.symbol()
        ),
        subtitle: format!(
            "Re(F e^jωt) at ωt = {} deg, port {port_name} excited by 1 V/m TE10, order {}",
            emcraft_render::axes::format(view.phase.to_degrees()),
            sweep.order.as_u8()
        ),
        x: Axis::new(&format!("{hlabel} ({lname})"), 0.0, l.from_si(hspan)),
        y: Axis::new(&format!("{vlabel} ({lname})"), 0.0, l.from_si(vspan)),
        scalar: Some(Scalar {
            label: format!("{} ({unit})", view.component.replace('e', "E").replace('h', "H")),
            nx,
            ny,
            values,
            kind: if magnitude { ScalarKind::Magnitude } else { ScalarKind::Signed },
            range: Some(peak),
        }),
        arrows: arrow_sets,
        lines,
        arrow_length: l.from_si(step) * 1.1,
    })
}

/// The exact TE₁₀ line of the model's waveguide.
pub fn te10_line(session: &Session) -> Result<Te10Line> {
    let w = session.model.waveguide.as_ref().ok_or_else(|| EngineError::Failed("no waveguide".into()))?;
    let mut layers = Vec::with_capacity(w.sections.len());
    for s in &w.sections {
        let (_, m) = session.model.material(&s.material)?;
        layers.push(Layer { length: s.length, eps_r: m.eps_r, mu_r: m.mu_r });
    }
    Ok(Te10Line { a: w.width, layers })
}

/// |S11| and |S21| in dB against frequency: the solved points and the exact TE₁₀ curves.
pub fn sparameter_chart(session: &Session) -> Result<Chart> {
    let sweep = session.sweep.as_ref().ok_or_else(|| EngineError::Failed("no results: solve first (solve.frequency)".into()))?;
    let fu = session.display.frequency;
    let (f0, f1) = match (sweep.results.first(), sweep.results.last()) {
        (Some(a), Some(b)) => (a.frequency, b.frequency),
        _ => return Err(EngineError::Failed("no results".into())),
    };
    let pad = if f1 > f0 { 0.0 } else { 0.05 * f0 };
    let (f0, f1) = (f0 - pad, f1 + pad);
    let db = |c: C| decibels(c.norm()).max(-80.0);
    let mut series = Vec::new();
    let mut marks = Vec::new();
    if let Ok(line) = te10_line(session) {
        let n = 400;
        let (mut s11, mut s21) = (Vec::new(), Vec::new());
        for k in 0..=n {
            let f = f0 + (f1 - f0) * k as f64 / n as f64;
            if let Ok(s) = line.solve(f) {
                let (r, t) = s.s_power();
                s11.push([fu.from_si(f), db(r)]);
                s21.push([fu.from_si(f), db(t)]);
            }
        }
        series.push(Series { label: "|S21| exact".into(), color: Rgb(30, 90, 200), style: Style::Line, points: s21 });
        series.push(Series { label: "|S11| exact".into(), color: Rgb(200, 60, 40), style: Style::Line, points: s11 });
        let mut seen: Vec<f64> = Vec::new();
        for (layer, name) in line.layers.iter().zip(session.model.waveguide.iter().flat_map(|w| w.sections.iter().map(|s| s.material.clone()))) {
            let fc = te10_cutoff(line.a, layer.eps_r, layer.mu_r);
            if !seen.iter().any(|x| (x - fc).abs() < 1e-6 * fc) {
                seen.push(fc);
                marks.push((fu.from_si(fc), format!("fc {name}")));
            }
        }
    }
    let pts = |p: usize, q: usize| sweep.results.iter().filter_map(|r| r.s.get(p).and_then(|row| row.get(q)).map(|s| [fu.from_si(r.frequency), db(*s)])).collect();
    series.push(Series { label: format!("|S21| FEM (order {})", sweep.order.as_u8()), color: Rgb(20, 40, 120), style: Style::Markers, points: pts(1, 0) });
    series.push(Series { label: "|S11| FEM".into(), color: Rgb(140, 30, 20), style: Style::Markers, points: pts(0, 0) });
    let lowest = series.iter().flat_map(|s| s.points.iter().map(|p| p[1])).fold(0.0f64, f64::min);
    let ymin = ((lowest / 10.0).floor() * 10.0).max(-80.0);
    Ok(Chart {
        title: "S-parameters".into(),
        x: Axis::new(&format!("frequency ({})", fu.symbol()), fu.from_si(f0), fu.from_si(f1)),
        y: Axis::new("|S| (dB)", ymin.min(-10.0), 2.0),
        series,
        marks,
    })
}
