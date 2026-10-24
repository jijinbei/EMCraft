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

/// The complex E and H sampled on a view's plane: the expensive part of a picture, done once
/// per frequency, plane, position and excitation. [`PlaneSamples::plot`] then draws any
/// component at any phase cheaply (an animation over the period).
#[derive(Clone, Debug)]
pub struct PlaneSamples {
    pub plane: Plane,
    pub frequency: f64,
    /// The plane's position along its normal (m).
    pub at: f64,
    pub excitation: usize,
    nx: usize,
    ny: usize,
    /// (E, H) on an nx × ny grid over the plane, row 0 at the bottom; `None` outside the mesh.
    grid: Vec<Option<([C; 3], [C; 3])>>,
    /// Arrow positions (display units) with E and H there.
    arrows: Vec<([f64; 2], [C; 3], [C; 3])>,
    arrow_length: f64,
    lines: Vec<[[f64; 2]; 2]>,
    title: String,
    caption: String,
    x: Axis,
    y: Axis,
}

/// The field plot of a view.
pub fn field_plot(session: &Session, view: &FieldView) -> Result<FieldPlot> {
    sample_plane(session, view)?.plot(view)
}

/// Samples the fields of a view's plane (its component, arrows and phase are not used).
pub fn sample_plane(session: &Session, view: &FieldView) -> Result<PlaneSamples> {
    let sweep = session.sweep.as_ref().ok_or_else(|| EngineError::Failed("no results: solve first (solve.frequency)".into()))?;
    let w = session.model.waveguide.as_ref().ok_or_else(|| EngineError::Failed("no waveguide".into()))?;
    let r = sweep.at(view.frequency, true).ok_or_else(|| EngineError::Failed("no results".into()))?;
    let x = r.fields.get(view.excitation).ok_or_else(|| EngineError::Params(format!("no excitation {}", view.excitation + 1)))?;
    let (a, b, len) = (w.width, w.height, w.length());
    // (horizontal span, vertical span, normal span) and the map from plane to guide.
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
    let longer = hspan.max(vspan);
    let (nx, ny) = (((SCALAR_SAMPLES as f64 * hspan / longer).ceil() as usize).max(2), ((SCALAR_SAMPLES as f64 * vspan / longer).ceil() as usize).max(2));
    let driven = &sweep.driven;
    let mut grid = Vec::with_capacity(nx * ny);
    for j in 0..ny {
        for i in 0..nx {
            let (u, v) = (hspan * i as f64 / (nx - 1) as f64, vspan * j as f64 / (ny - 1) as f64);
            grid.push(driven.fields(x, r.frequency, point(u, v)));
        }
    }
    let l = session.display.length;
    let step = longer / ARROWS as f64;
    let (ax, ay) = (((hspan / step).round() as usize).max(2), ((vspan / step).round() as usize).max(2));
    let mut arrows = Vec::with_capacity(ax * ay);
    for j in 0..ay {
        for i in 0..ax {
            let (u, v) = (hspan * (i as f64 + 0.5) / ax as f64, vspan * (j as f64 + 0.5) / ay as f64);
            if let Some((e, h)) = driven.fields(x, r.frequency, point(u, v)) {
                arrows.push(([l.from_si(u), l.from_si(v)], e, h));
            }
        }
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
    let lname = l.symbol();
    let fsym = session.display.frequency;
    let port_name = session.model.ports.get(view.excitation).map_or("1", |p| p.name.as_str());
    let title = format!(
        "{} plane, {nlabel} = {} {lname}   f = {} {}",
        match view.plane {
            Plane::H => "H",
            Plane::E => "E",
            Plane::Cross => "Cross-section",
        },
        emcraft_render::axes::format(l.from_si(at)),
        emcraft_render::axes::format(fsym.from_si(r.frequency)),
        fsym.symbol()
    );
    let caption = format!("port {port_name} excited by 1 V/m TE10, order {}", sweep.order.as_u8());
    Ok(PlaneSamples {
        plane: view.plane,
        frequency: r.frequency,
        at,
        excitation: view.excitation,
        nx,
        ny,
        grid,
        arrows,
        arrow_length: l.from_si(step) * 1.1,
        lines,
        title,
        caption,
        x: Axis::new(&format!("{hlabel} ({lname})"), 0.0, l.from_si(hspan)),
        y: Axis::new(&format!("{vlabel} ({lname})"), 0.0, l.from_si(vspan)),
    })
}

impl PlaneSamples {
    /// Whether these samples serve a view (same frequency, plane, position and excitation).
    pub fn serves(&self, view: &FieldView, at: f64) -> bool {
        self.plane == view.plane && self.excitation == view.excitation && (self.frequency - view.frequency).abs() <= 1e-9 * self.frequency && (self.at - at).abs() <= 1e-12
    }

    /// The plot of a view's component and arrows at its phase.
    pub fn plot(&self, view: &FieldView) -> Result<FieldPlot> {
        let rot = C::from_polar(1.0, view.phase);
        let re = |c: C| (c * rot).re;
        let norm = |f: &[C; 3]| f.iter().map(|c| c.norm_sqr()).sum::<f64>().sqrt();
        let comp = view.component.as_str();
        // (value at this phase, amplitude over the period)
        let value = |e: &[C; 3], h: &[C; 3]| -> Option<(f64, f64)> {
            match comp {
                "ex" | "ey" | "ez" | "hx" | "hy" | "hz" => {
                    let k = match &comp[1..] {
                        "x" => 0,
                        "y" => 1,
                        _ => 2,
                    };
                    let v = if comp.starts_with('h') { h[k] } else { e[k] };
                    Some((re(v), v.norm()))
                }
                "|e|" => Some((e.iter().map(|c| re(*c).powi(2)).sum::<f64>().sqrt(), norm(e))),
                "|h|" => Some((h.iter().map(|c| re(*c).powi(2)).sum::<f64>().sqrt(), norm(h))),
                _ => None,
            }
        };
        if value(&[C::new(0.0, 0.0); 3], &[C::new(0.0, 0.0); 3]).is_none() {
            return Err(EngineError::Params(format!(
                "`component` must be ex, ey, ez, hx, hy, hz, |e| or |h|, got `{}`",
                comp.chars().take(20).collect::<String>()
            )));
        }
        let mut peak: f64 = 0.0;
        let values = self
            .grid
            .iter()
            .map(|g| match g.as_ref().and_then(|(e, h)| value(e, h)) {
                Some((v, amp)) => {
                    peak = peak.max(amp);
                    v
                }
                None => f64::NAN,
            })
            .collect();
        // In-plane components of a vector: (horizontal, vertical).
        let in_plane = |f: &[C; 3]| match self.plane {
            Plane::H => (f[2], f[0]),
            Plane::E => (f[2], f[1]),
            Plane::Cross => (f[0], f[1]),
        };
        let mut sets = Vec::new();
        for (show, magnetic, label, color) in [(view.show_e, false, "E (in plane)", Rgb(20, 20, 20)), (view.show_h, true, "H (in plane)", Rgb(0, 120, 60))] {
            if !show {
                continue;
            }
            let (mut in_peak, mut total): (f64, f64) = (0.0, 0.0);
            let mut list = Vec::with_capacity(self.arrows.len());
            for (pos, e, h) in &self.arrows {
                let f = if magnetic { h } else { e };
                let (u, v) = in_plane(f);
                total = total.max(norm(f));
                in_peak = in_peak.max((u.norm_sqr() + v.norm_sqr()).sqrt());
                list.push((*pos, [re(u), re(v)]));
            }
            // Scaled by the peak over the period, so an animation keeps one scale. A field with
            // (next to) nothing in the plane — E in the H-plane for TE10, whose in-plane part is
            // discretisation error — draws no arrows.
            if in_peak > IN_PLANE * total {
                sets.push(Arrows { label: label.into(), color, arrows: list, scale: Some(in_peak) });
            }
        }
        let unit = if comp.contains('h') { "A/m" } else { "V/m" };
        Ok(FieldPlot {
            title: self.title.clone(),
            subtitle: format!("Re(F e^jωt) at ωt = {} deg, {}", emcraft_render::axes::format(view.phase.to_degrees()), self.caption),
            x: self.x.clone(),
            y: self.y.clone(),
            scalar: Some(Scalar {
                label: format!("{} ({unit})", comp.replace('e', "E").replace('h', "H")),
                nx: self.nx,
                ny: self.ny,
                values,
                kind: if comp.starts_with('|') { ScalarKind::Magnitude } else { ScalarKind::Signed },
                range: Some(peak),
            }),
            arrows: sets,
            lines: self.lines.clone(),
            arrow_length: self.arrow_length,
        })
    }
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
