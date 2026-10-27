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
use emcraft_model::{CVec3, Material, Vec3};
use emcraft_render::chart::{Chart, Series, Style};
use emcraft_render::plot::{Arrows, FieldPlot, Scalar, ScalarKind};
use emcraft_render::{Axis, Rgb};
use emcraft_solver::Complex64 as C;
use emcraft_theory::{EPlaneLine, ESection, HPlaneLine, HSection, Layer, Te10Line, te10_cutoff};

use crate::{EngineError, Result, Session};

/// A plane through the guide a field picture shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Plane {
    #[serde(alias = "h-plane")]
    H,
    #[serde(alias = "e-plane")]
    E,
    #[serde(alias = "cross-section")]
    Cross,
}

impl Plane {
    pub fn name(self) -> &'static str {
        match self {
            Plane::H => "h",
            Plane::E => "e",
            Plane::Cross => "cross",
        }
    }
}

/// The quantity a field picture colours: a Cartesian component of E or H, or the magnitude of
/// one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Component {
    Ex,
    #[default]
    Ey,
    Ez,
    Hx,
    Hy,
    Hz,
    /// |E|.
    E,
    /// |H|.
    H,
}

impl Component {
    pub const ALL: [Component; 8] =
        [Component::Ex, Component::Ey, Component::Ez, Component::Hx, Component::Hy, Component::Hz, Component::E, Component::H];

    /// The name commands use: `ex` … `hz`, `|e|`, `|h|`.
    pub const fn name(self) -> &'static str {
        match self {
            Component::Ex => "ex",
            Component::Ey => "ey",
            Component::Ez => "ez",
            Component::Hx => "hx",
            Component::Hy => "hy",
            Component::Hz => "hz",
            Component::E => "|e|",
            Component::H => "|h|",
        }
    }

    /// The name in text: `Ey`, `|H|`.
    pub fn label(self) -> String {
        self.name().replace('e', "E").replace('h', "H")
    }

    /// The name as TeX math, for pictures: `$E_y$`, `$|H|$`.
    pub fn tex(self) -> String {
        match self {
            Component::E => "$|E|$".into(),
            Component::H => "$|H|$".into(),
            c => {
                let n = c.label();
                let (f, axis) = n.split_at(1);
                format!("${f}_{axis}$")
            }
        }
    }

    pub const fn is_magnetic(self) -> bool {
        matches!(self, Component::Hx | Component::Hy | Component::Hz | Component::H)
    }

    pub const fn is_magnitude(self) -> bool {
        matches!(self, Component::E | Component::H)
    }

    /// The value at the phase `rot` (e^{jωt}) and the amplitude over the period, of the fields
    /// E and H at a point.
    fn sample(self, e: &CVec3, h: &CVec3, rot: C) -> (f64, f64) {
        let f = if self.is_magnetic() { h } else { e };
        let axis = match self {
            Component::Ex | Component::Hx => 0,
            Component::Ey | Component::Hy => 1,
            Component::Ez | Component::Hz => 2,
            Component::E | Component::H => return ((*f * rot).re().norm(), f.norm_sqr().sqrt()),
        };
        ((f[axis] * rot).re, f[axis].norm())
    }
}

impl std::fmt::Display for Component {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

impl std::str::FromStr for Component {
    type Err = EngineError;

    /// A component by its name, in either case.
    fn from_str(s: &str) -> Result<Component> {
        let lower = s.to_ascii_lowercase();
        Component::ALL.into_iter().find(|c| c.name() == lower).ok_or_else(|| {
            EngineError::Params(format!("`component` must be ex, ey, ez, hx, hy, hz, |e| or |h|, got `{}`", s.chars().take(20).collect::<String>()))
        })
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
    /// The colour-mapped quantity.
    pub component: Component,
    pub show_e: bool,
    pub show_h: bool,
    /// Phase ωt (rad): the picture shows Re(F e^{jωt}).
    pub phase: f64,
    /// The excited port's index.
    pub excitation: usize,
    /// An eigenmode's index (from `solve.eigenmodes`) instead of a driven solution.
    pub mode: Option<usize>,
}

impl FieldView {
    pub fn new(frequency: f64, plane: Plane) -> FieldView {
        FieldView { frequency, plane, at: None, component: Component::Ey, show_e: true, show_h: true, phase: 0.0, excitation: 0, mode: None }
    }

    /// A view of eigenmode `index`.
    pub fn mode(index: usize, plane: Plane) -> FieldView {
        FieldView { mode: Some(index), ..FieldView::new(0.0, plane) }
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
    pub mode: Option<usize>,
    nx: usize,
    ny: usize,
    /// (E, H) on an nx × ny grid over the plane, row 0 at the bottom; `None` outside the mesh.
    grid: Vec<Option<(CVec3, CVec3)>>,
    /// Arrow positions (display units) with E and H there: the arrow lattice's cell centres.
    arrows: Vec<([f64; 2], CVec3, CVec3)>,
    /// The same at the lattice's inner corners, between the centres: where H's arrows go when
    /// E's are drawn too, so the two never sit on one point.
    corners: Vec<([f64; 2], CVec3, CVec3)>,
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
    let emcraft_model::Aabb { min: lo, max: hi } = session.model.bounds().ok_or_else(|| EngineError::failed("no geometry"))?;
    // The field: a driven solution at a frequency, or an eigenmode.
    let (driven, x, frequency, caption) = match view.mode {
        Some(i) => {
            let e = session.results.eigen.as_ref().ok_or_else(|| EngineError::Failed("no eigenmodes: solve first (solve.eigenmodes)".into()))?;
            let m = e.modes.get(i).ok_or_else(|| EngineError::Params(format!("no eigenmode {} (there are {})", i + 1, e.modes.len())))?;
            let q = m.q().map_or(String::new(), |q| format!(", Q = {}", emcraft_render::axes::format(q)));
            (&e.driven, &m.vector, m.frequency, format!("eigenmode {}{q} (arbitrary amplitude), order {}", i + 1, e.driven.order.as_u8()))
        }
        None if session.results.sweep.is_none() && session.results.radiation.is_some() => {
            let r = session.results.radiation.as_ref().ok_or_else(|| EngineError::Failed("no results".into()))?;
            let what = if session.model.sources.iter().any(|s| matches!(s, emcraft_model::Source::PlaneWave { .. })) {
                "the field scattered from a 1 V/m plane wave"
            } else {
                "the field the sources radiate"
            };
            (&r.driven, &r.result.field, r.result.frequency, format!("{what}, order {}", r.driven.order.as_u8()))
        }
        None => {
            let sweep = session.results.sweep()?;
            let r = sweep.at(view.frequency, true).ok_or_else(|| EngineError::Failed("no results".into()))?;
            let x = r.fields.get(view.excitation).ok_or_else(|| EngineError::Params(format!("no excitation {}", view.excitation + 1)))?;
            let port_name = session.model.ports.get(view.excitation).map_or("1", |p| p.name.as_str());
            let mode = match sweep.driven.ports.get(view.excitation).map(|p| &p.modes) {
                Some(emcraft_solver::driven::PortModes::Analytic { modes, .. }) => {
                    modes.first().map_or("TE10".to_owned(), |(m, _)| format!("1 V/m {}", m.name()))
                }
                _ => "its first mode".to_owned(),
            };
            (&*sweep.driven, x, r.frequency, format!("port {port_name} excited by {mode}, order {}", sweep.order.as_u8()))
        }
    };
    let Vec3 { x: a, y: b, z: len } = hi - lo;
    // (horizontal span, vertical span, normal span) and the map from plane to guide; plane
    // coordinates run from the bounding box's low corner, `at` from its low side.
    let (hspan, vspan, normal_span) = match view.plane {
        Plane::H => (len, a, b),
        Plane::E => (len, b, a),
        Plane::Cross => (a, b, len),
    };
    let (h0, v0, n0) = match view.plane {
        Plane::H => (lo.z, lo.x, lo.y),
        Plane::E => (lo.z, lo.y, lo.x),
        Plane::Cross => (lo.x, lo.y, lo.z),
    };
    let at = view.at.unwrap_or(normal_span / 2.0).clamp(0.0, normal_span);
    let point = |u: f64, v: f64| {
        lo + match view.plane {
            Plane::H => Vec3::new(v, at, u),
            Plane::E => Vec3::new(at, v, u),
            Plane::Cross => Vec3::new(u, v, at),
        }
    };
    let longer = hspan.max(vspan);
    let (nx, ny) =
        (((SCALAR_SAMPLES as f64 * hspan / longer).ceil() as usize).max(2), ((SCALAR_SAMPLES as f64 * vspan / longer).ceil() as usize).max(2));
    let grid = (0..ny)
        .flat_map(|j| (0..nx).map(move |i| (hspan * i as f64 / (nx - 1) as f64, vspan * j as f64 / (ny - 1) as f64)))
        .map(|(u, v)| driven.fields(x, frequency, point(u, v)))
        .collect();
    let l = session.display.length;
    let step = longer / ARROWS as f64;
    let (ax, ay) = (((hspan / step).round() as usize).max(2), ((vspan / step).round() as usize).max(2));
    let arrows = (0..ay)
        .flat_map(|j| (0..ax).map(move |i| (hspan * (i as f64 + 0.5) / ax as f64, vspan * (j as f64 + 0.5) / ay as f64)))
        .filter_map(|(u, v)| driven.fields(x, frequency, point(u, v)).map(|(e, h)| ([l.from_si(h0 + u), l.from_si(v0 + v)], e, h)))
        .collect();
    let corners = (1..ay)
        .flat_map(|j| (1..ax).map(move |i| (hspan * i as f64 / ax as f64, vspan * j as f64 / ay as f64)))
        .filter_map(|(u, v)| driven.fields(x, frequency, point(u, v)).map(|(e, h)| ([l.from_si(h0 + u), l.from_si(v0 + v)], e, h)))
        .collect();
    let (hlabel, vlabel, nlabel) = match view.plane {
        Plane::H => ("z", "x", "y"),
        Plane::E => ("z", "y", "x"),
        Plane::Cross => ("x", "y", "z"),
    };
    let mut lines = Vec::new();
    if view.plane != Plane::Cross
        && let Some(w) = session.model.waveguide.as_ref()
    {
        for z in w.interfaces().iter().skip(1).take(w.sections.len().saturating_sub(1)) {
            lines.push([[l.from_si(*z), 0.0], [l.from_si(*z), l.from_si(vspan)]]);
        }
    }
    let lname = l.symbol();
    let fsym = session.display.frequency;
    let title = format!(
        "{} plane, {nlabel} = {} {lname}   f = {} {}",
        match view.plane {
            Plane::H => "H",
            Plane::E => "E",
            Plane::Cross => "Cross-section",
        },
        emcraft_render::axes::format(l.from_si(n0 + at)),
        emcraft_render::axes::format(fsym.from_si(frequency)),
        fsym.symbol()
    );
    Ok(PlaneSamples {
        plane: view.plane,
        frequency,
        at,
        excitation: view.excitation,
        mode: view.mode,
        nx,
        ny,
        grid,
        arrows,
        corners,
        arrow_length: l.from_si(step) * 1.1,
        lines,
        title,
        caption,
        x: Axis::new(&format!("{hlabel} ({lname})"), l.from_si(h0), l.from_si(h0 + hspan)),
        y: Axis::new(&format!("{vlabel} ({lname})"), l.from_si(v0), l.from_si(v0 + vspan)),
    })
}

/// The fields at one sample of a plane: where it is (display units along the picture's axes)
/// and the complex E (V/m) and H (A/m) there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Probe {
    pub at: [f64; 2],
    pub e: CVec3,
    pub h: CVec3,
}

impl Probe {
    /// The value of `component` at phase ωt = `phase` (rad), and its amplitude over the period.
    pub fn value(&self, component: Component, phase: f64) -> (f64, f64) {
        component.sample(&self.e, &self.h, C::from_polar(1.0, phase))
    }
}

impl PlaneSamples {
    /// The picture's horizontal and vertical axes (display units).
    pub fn axes(&self) -> (&Axis, &Axis) {
        (&self.x, &self.y)
    }

    /// The fields at the sample nearest `point` (display units along the axes); `None` off the
    /// plane or where the plane leaves the mesh (inside a conductor).
    pub fn probe(&self, point: [f64; 2]) -> Option<Probe> {
        if self.nx < 2 || self.ny < 2 || !point.iter().all(|v| v.is_finite()) {
            return None;
        }
        let ((xa, xb), (ya, yb)) = (self.x.span(), self.y.span());
        let (u, v) = ((point[0] - xa) / (xb - xa), (point[1] - ya) / (yb - ya));
        if !((0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v)) {
            return None;
        }
        let i = (u * (self.nx - 1) as f64).round() as usize;
        let j = (v * (self.ny - 1) as f64).round() as usize;
        let (e, h) = (*self.grid.get(j * self.nx + i)?)?;
        let at = [xa + (xb - xa) * i as f64 / (self.nx - 1) as f64, ya + (yb - ya) * j as f64 / (self.ny - 1) as f64];
        Some(Probe { at, e, h })
    }

    /// Whether these samples serve a view (same frequency, plane, position and excitation).
    pub fn serves(&self, view: &FieldView, at: f64) -> bool {
        self.plane == view.plane
            && self.mode == view.mode
            && self.excitation == view.excitation
            && (view.mode.is_some() || (self.frequency - view.frequency).abs() <= 1e-9 * self.frequency)
            && (self.at - at).abs() <= 1e-12
    }

    /// The plot of a view's component and arrows at its phase.
    pub fn plot(&self, view: &FieldView) -> Result<FieldPlot> {
        let rot = C::from_polar(1.0, view.phase);
        let re = |c: C| (c * rot).re;
        let norm = |f: &CVec3| f.norm_sqr().sqrt();
        let comp = view.component;
        let mut peak: f64 = 0.0;
        let values = self
            .grid
            .iter()
            .map(|g| match g.as_ref().map(|(e, h)| comp.sample(e, h, rot)) {
                Some((v, amp)) => {
                    peak = peak.max(amp);
                    v
                }
                None => f64::NAN,
            })
            .collect();
        // In-plane components of a vector: (horizontal, vertical).
        let in_plane = |f: &CVec3| match self.plane {
            Plane::H => (f.z, f.x),
            Plane::E => (f.z, f.y),
            Plane::Cross => (f.x, f.y),
        };
        let mut sets = Vec::new();
        for (show, magnetic, label, color) in
            [(view.show_e, false, "E (in plane)", Rgb(20, 20, 20)), (view.show_h, true, "H (in plane)", Rgb(0, 120, 60))]
        {
            if !show {
                continue;
            }
            let (mut in_peak, mut total): (f64, f64) = (0.0, 0.0);
            // H between E's arrows when both show.
            let lattice = if magnetic && view.show_e && !self.corners.is_empty() { &self.corners } else { &self.arrows };
            let mut list = Vec::with_capacity(lattice.len());
            for (pos, e, h) in lattice {
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
        let unit = if comp.is_magnetic() { "A/m" } else { "V/m" };
        Ok(FieldPlot {
            title: self.title.clone(),
            subtitle: format!(
                "Re($F\\,e^{{j\\omega t}}$) at $\\omega t$ = {}°, {}",
                emcraft_render::axes::format(view.phase.to_degrees()),
                self.caption
            ),
            x: self.x.clone(),
            y: self.y.clone(),
            scalar: Some(Scalar {
                label: format!("{} ({unit})", comp.tex()),
                nx: self.nx,
                ny: self.ny,
                values,
                kind: if comp.is_magnitude() { ScalarKind::Magnitude } else { ScalarKind::Signed },
                range: Some(peak),
            }),
            arrows: sets,
            lines: self.lines.clone(),
            arrow_length: self.arrow_length,
        })
    }
}

/// A TE₁₀ line's section `length` long of material `m` at `frequency` (Hz): its permittivity
/// εr(ω) = ε′ − jε″ as εr (1 − j tan δ), tan δ = ε″/ε′.
fn layer_at(length: f64, m: &Material, frequency: f64) -> Layer {
    if !m.is_dispersive() {
        return Layer { length, eps_r: m.eps_r, mu_r: m.mu_r, loss_tangent: m.loss_tangent };
    }
    let e = m.eps_at(frequency);
    Layer { length, eps_r: e.re, mu_r: m.mu_r, loss_tangent: -e.im / e.re }
}

/// A uniform guide's sections (length, material) and its width: its exact TE₁₀ line at each
/// frequency.
#[derive(Clone, Debug)]
pub struct ExactLine {
    pub a: f64,
    pub sections: Vec<(f64, Material)>,
}

impl ExactLine {
    /// The line at `frequency` (Hz), dispersive materials evaluated there.
    pub fn at(&self, frequency: f64) -> Te10Line {
        Te10Line { a: self.a, layers: self.sections.iter().map(|(l, m)| layer_at(*l, m, frequency)).collect() }
    }
}

/// The exact TE₁₀ line of a waveguide of uniform cross-section.
pub fn te10_line(session: &Session) -> Result<ExactLine> {
    let w = session.model.waveguide.as_ref().ok_or_else(|| EngineError::Failed("no waveguide".into()))?;
    if !w.uniform() {
        return Err(EngineError::Failed(
            "not supported yet: exact fields of a guide whose aperture changes (the S-parameters are compared with mode matching)".into(),
        ));
    }
    let mut sections = Vec::with_capacity(w.sections.len());
    for s in &w.sections {
        let (_, m) = session.model.material(&s.material)?;
        sections.push((s.length, m.clone()));
    }
    Ok(ExactLine { a: w.width, sections })
}

/// Mode matching takes constant permittivities: refuses a dispersive material.
fn constant(m: &Material) -> Result<&Material> {
    if m.is_dispersive() {
        return Err(EngineError::Failed(format!(
            "not supported yet: a mode-matching reference for the dispersive material `{}` (only uniform guides have one)",
            m.name
        )));
    }
    Ok(m)
}

/// x'₁₁, the first zero of J₁' (Abramowitz & Stegun, Table 9.5): the circular guide's TE11
/// cut-off is x'₁₁/r.
const X11_PRIME: f64 = 1.841_183_781_340_659;

/// TE_m0 modes in the widest section when the reference is mode matching: converged to about
/// 10⁻⁵ in S for irises and steps of a few millimetres.
pub const MODE_MATCHING_MODES: usize = 160;

/// What a model's S-parameters are compared with: the exact TE₁₀ cascade when every section
/// has the full cross-section, mode matching when the aperture changes (H-plane steps, irises).
#[derive(Clone, Debug)]
pub enum Reference {
    Exact(ExactLine),
    ModeMatching(HPlaneLine),
    EPlane(EPlaneLine),
    /// A uniform guide `length` long whose first mode has cut-off wavenumber `cutoff` (0 for
    /// TEM) in a fill of εr μr = `index2`: S11 = 0, S21 = e^{−jβL}.
    Uniform {
        length: f64,
        cutoff: f64,
        index2: f64,
    },
}

impl Reference {
    pub fn label(&self) -> &'static str {
        match self {
            Reference::Exact(_) | Reference::Uniform { .. } => "exact",
            Reference::ModeMatching(_) | Reference::EPlane(_) => "mode matching",
        }
    }

    /// (S11, S21), power waves, with `modes` TE_m0 modes for mode matching.
    pub fn s(&self, frequency: f64, modes: usize) -> Result<(C, C)> {
        match self {
            Reference::Exact(l) => Ok(l.at(frequency).solve(frequency)?.s_power()),
            Reference::ModeMatching(l) => Ok(l.solve(frequency, modes)?),
            Reference::EPlane(l) => Ok(l.solve(frequency, modes)?),
            Reference::Uniform { length, cutoff, index2 } => {
                let k = 2.0 * std::f64::consts::PI * frequency / emcraft_model::units::C0;
                let b2 = k * k * index2 - cutoff * cutoff;
                if !(b2 > 0.0) {
                    return Err(EngineError::Failed(format!("{frequency} Hz is below the guide's cut-off")));
                }
                Ok((C::new(0.0, 0.0), C::from_polar(1.0, -b2.sqrt() * length)))
            }
        }
    }
}

pub fn reference(session: &Session) -> Result<Reference> {
    if session.model.waveguide.is_none()
        && let Some(solid) = session.model.solid.as_ref()
    {
        // A box between two ports is an empty guide.
        let (_, m) = session.model.material(&solid.material)?;
        let plain = session.model.inclusions.is_empty() && m.loss_tangent == 0.0 && !m.is_dispersive();
        let index2 = m.eps_r * m.mu_r;
        return match (&solid.shape, &session.model.features[..]) {
            (emcraft_model::Shape::Box { size }, []) if session.model.inclusions.is_empty() => {
                Ok(Reference::Exact(ExactLine { a: size[0], sections: vec![(size[2], m.clone())] }))
            }
            // An empty circular guide: TE11, cut off at x'₁₁/r (the first zero of J₁').
            (emcraft_model::Shape::Cylinder { radius, length }, []) if plain => {
                Ok(Reference::Uniform { length: *length, cutoff: X11_PRIME / radius, index2 })
            }
            // A coaxial line: a cylinder through the whole length on the axis, subtracted: TEM.
            (emcraft_model::Shape::Cylinder { radius, length }, [f])
                if plain
                    && f.combine == emcraft_model::Combine::Subtract
                    && f.axis == emcraft_model::Axis::Z
                    && f.at.x == 0.0
                    && f.at.y == 0.0
                    && f.at.z <= 0.0
                    && matches!(f.shape, emcraft_model::Shape::Cylinder { radius: r, length: l } if r < *radius && f.at.z + l >= *length) =>
            {
                Ok(Reference::Uniform { length: *length, cutoff: 0.0, index2 })
            }
            _ => Err(EngineError::Failed("not supported yet: a reference for this body's ports".into())),
        };
    }
    let w = session.model.waveguide.as_ref().ok_or_else(|| EngineError::Failed("no waveguide".into()))?;
    if !w.plain() {
        return Err(EngineError::Failed("not supported yet: a reference for a layered guide or one with strips".into()));
    }
    if w.uniform() {
        return Ok(Reference::Exact(te10_line(session)?));
    }
    if w.full_width() {
        let mut sections = Vec::with_capacity(w.sections.len());
        for (i, s) in w.sections.iter().enumerate() {
            let m = constant(session.model.material(&s.material)?.1)?;
            let emcraft_model::Span { start: y0, width: height } = w.aperture_y(i).unwrap_or(w.across_y());
            sections.push(ESection { length: s.length, eps_r: m.eps_r, mu_r: m.mu_r, loss_tangent: m.loss_tangent, y0, height });
        }
        return Ok(Reference::EPlane(EPlaneLine { a: w.width, sections }));
    }
    if !w.full_height() {
        return Err(EngineError::Failed("not supported yet: a reference for guides whose width and height both change".into()));
    }
    let mut sections = Vec::with_capacity(w.sections.len());
    for (i, s) in w.sections.iter().enumerate() {
        let m = constant(session.model.material(&s.material)?.1)?;
        let emcraft_model::Span { start: x0, width } = w.aperture(i).unwrap_or(w.across_x());
        sections.push(HSection { length: s.length, eps_r: m.eps_r, mu_r: m.mu_r, x0, width, loss_tangent: m.loss_tangent });
    }
    Ok(Reference::ModeMatching(HPlaneLine { sections }))
}

/// |S11| and |S21| in dB against frequency: the solved points and the reference curves.
pub fn sparameter_chart(session: &Session) -> Result<Chart> {
    let sweep = session.results.sweep.as_ref();
    let td = session.results.time_domain.as_ref();
    let fu = session.display.frequency;
    let (f0, f1) = match (sweep, td) {
        (Some(sw), _) if !sw.results.is_empty() => (sw.results[0].frequency, sw.results[sw.results.len() - 1].frequency),
        (_, Some(t)) if !t.frequencies.is_empty() => (t.frequencies[0], t.frequencies[t.frequencies.len() - 1]),
        _ => return Err(EngineError::Failed("no results: solve first (solve.frequency or solve.time_domain)".into())),
    };
    let pad = if f1 > f0 { 0.0 } else { 0.05 * f0 };
    let (f0, f1) = (f0 - pad, f1 + pad);
    let db = |c: C| decibels(c.norm()).max(-80.0);
    let mut series = Vec::new();
    let mut marks = Vec::new();
    if let Ok(reference) = reference(session) {
        // The curve: fewer modes than the comparison takes (mode matching is dense linear algebra
        // per frequency); 40 keep the curve within 10⁻² dB.
        let (n, modes) = match reference {
            Reference::Exact(_) | Reference::Uniform { .. } => (400, 1),
            Reference::ModeMatching(_) | Reference::EPlane(_) => (160, 40),
        };
        let (mut s11, mut s21) = (Vec::new(), Vec::new());
        for k in 0..=n {
            let f = f0 + (f1 - f0) * k as f64 / n as f64;
            if let Ok((r, t)) = reference.s(f, modes) {
                s11.push([fu.from_si(f), db(r)]);
                s21.push([fu.from_si(f), db(t)]);
            }
        }
        let label = reference.label();
        series.push(Series { label: format!("$|S_{{21}}|$ {label}"), color: Rgb(30, 90, 200), style: Style::Line, points: s21 });
        series.push(Series { label: format!("$|S_{{11}}|$ {label}"), color: Rgb(200, 60, 40), style: Style::Line, points: s11 });
    }
    if let Some(w) = session.model.waveguide.as_ref() {
        let lu = session.display.length;
        let mut seen: Vec<f64> = Vec::new();
        for (i, sec) in w.sections.iter().enumerate() {
            let (Ok((_, m)), Some(emcraft_model::Span { width, .. })) = (session.model.material(&sec.material), w.aperture(i)) else { continue };
            let fc = te10_cutoff(width, m.eps_r, m.mu_r);
            if !seen.iter().any(|x| (x - fc).abs() < 1e-6 * fc) {
                seen.push(fc);
                let name = if (width - w.width).abs() <= 1e-9 * w.width {
                    format!("$f_c$ {}", sec.material)
                } else {
                    format!("$f_c$ {} {}", emcraft_render::axes::format(lu.from_si(width)), lu.symbol())
                };
                marks.push((fu.from_si(fc), name));
            }
        }
    }
    if let Some(sweep) = sweep {
        let pts = |p: usize, q: usize| sweep.results.iter().filter_map(|r| r.s.get(p, q).map(|s| [fu.from_si(r.frequency), db(s)])).collect();
        series.push(Series {
            label: format!("$|S_{{21}}|$ FEM (order {})", sweep.order.as_u8()),
            color: Rgb(20, 40, 120),
            style: Style::Markers,
            points: pts(1, 0),
        });
        series.push(Series { label: "$|S_{11}|$ FEM".into(), color: Rgb(140, 30, 20), style: Style::Markers, points: pts(0, 0) });
    }
    if let Some(td) = td {
        let pts = |v: &[C]| td.frequencies.iter().zip(v).map(|(f, s)| [fu.from_si(*f), db(*s)]).collect();
        series.push(Series { label: "$|S_{21}|$ time domain".into(), color: Rgb(40, 150, 90), style: Style::Line, points: pts(&td.s21) });
        series.push(Series { label: "$|S_{11}|$ time domain".into(), color: Rgb(210, 140, 30), style: Style::Line, points: pts(&td.s11) });
    }
    let lowest = series.iter().flat_map(|s| s.points.iter().map(|p| p[1])).fold(0.0f64, f64::min);
    let ymin = ((lowest / 10.0).floor() * 10.0).max(-80.0);
    Ok(Chart {
        title: "S-parameters".into(),
        x: Axis::new(&format!("frequency ({})", fu.symbol()), fu.from_si(f0), fu.from_si(f1)),
        y: Axis::new("$|S|$ (dB)", ymin.min(-10.0), 2.0),
        series,
        marks,
    })
}
