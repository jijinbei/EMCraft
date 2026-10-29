//! The electric and magnetic fields on a plane through the model: a component as a colour map,
//! the in-plane vectors as arrows, at one phase of the period.
//!
//! The planes, in the guide's frame (x across the broad wall, y across the narrow wall, z along):
//! - `h` (H-plane): z across, x up, at height y — for TE₁₀, E_y (out of the plane) as colour and
//!   the magnetic field's loops (H_z, H_x) as arrows;
//! - `e` (E-plane): z across, y up, at x — E_y in the plane;
//! - `cross` (cross-section): x across, y up, at z — the TE₁₀ pattern of E (vertical) and H.

use emcraft_model::{CVec3, Point3, Vec3};
use emcraft_render::plot::{Arrows, FieldPlot, Scalar, ScalarKind};
use emcraft_render::{Axis, Rgb};
use emcraft_solver::Complex64 as C;

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

    /// The model axes (0 x, 1 y, 2 z) along the picture's horizontal, its vertical and the
    /// plane's normal.
    const fn axes(self) -> [usize; 3] {
        match self {
            Plane::H => [2, 0, 1],
            Plane::E => [2, 1, 0],
            Plane::Cross => [0, 1, 2],
        }
    }

    fn title(self) -> &'static str {
        match self {
            Plane::H => "H",
            Plane::E => "E",
            Plane::Cross => "Cross-section",
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
        Component::ALL
            .into_iter()
            .find(|c| c.name() == lower)
            .ok_or_else(|| EngineError::Params(format!("`component` must be ex, ey, ez, hx, hy, hz, |e| or |h|, got `{}`", crate::text::clip(s, 20))))
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

/// The field a view shows.
struct Solution<'a> {
    driven: &'a emcraft_solver::driven::Driven,
    /// The solution vector.
    field: &'a [C],
    /// Hz.
    frequency: f64,
    /// What the field is, for the picture's subtitle.
    caption: String,
}

/// The field a view shows: an eigenmode, the open problem's field, or the sweep's at the
/// view's frequency and excitation.
fn solution<'a>(session: &'a Session, view: &FieldView) -> Result<Solution<'a>> {
    let results = &session.results;
    if let Some(i) = view.mode {
        let e = results.eigen.as_ref().ok_or_else(|| EngineError::Failed("no eigenmodes: solve first (solve.eigenmodes)".into()))?;
        let m = e.modes.get(i).ok_or_else(|| EngineError::Params(format!("no eigenmode {} (there are {})", i + 1, e.modes.len())))?;
        let q = m.q().map_or(String::new(), |q| format!(", Q = {}", emcraft_render::axes::format(q)));
        let caption = format!("eigenmode {}{q} (arbitrary amplitude), order {}", i + 1, e.driven.order.as_u8());
        return Ok(Solution { driven: &e.driven, field: &m.vector, frequency: m.frequency, caption });
    }
    if results.sweep.is_none()
        && let Some(r) = &results.radiation
    {
        let what = if session.model.sources.iter().any(|s| matches!(s, emcraft_model::Source::PlaneWave { .. })) {
            "the field scattered from a 1 V/m plane wave"
        } else {
            "the field the sources radiate"
        };
        let caption = format!("{what}, order {}", r.driven.order.as_u8());
        return Ok(Solution { driven: &r.driven, field: &r.result.field, frequency: r.result.frequency, caption });
    }
    let sweep = results.sweep()?;
    let r = sweep.at(view.frequency, true).ok_or_else(|| EngineError::Failed("no results".into()))?;
    let x = r.fields.get(view.excitation).ok_or_else(|| EngineError::Params(format!("no excitation {}", view.excitation + 1)))?;
    let entries = sweep.entries();
    let entry = entries.get(view.excitation);
    let port_name = entry.map_or("1", |e| e.name.as_str());
    let mode = match entry.map(|e| (e.terminal, sweep.driven.ports.get(e.port).map(|p| &p.modes))) {
        Some((Some(_), _)) => {
            format!("a 1 W wave on its strip, the other terminals matched to {} Ω", entry.and_then(|e| e.reference).unwrap_or_default())
        }
        Some((None, Some(emcraft_solver::driven::PortModes::Analytic { modes, .. }))) => {
            modes.first().map_or("TE10".to_owned(), |(m, _)| format!("1 V/m {}", m.name()))
        }
        _ => "its first mode".to_owned(),
    };
    let what = if entry.is_some_and(|e| e.terminal.is_some()) { "terminal" } else { "port" };
    let caption = format!("{what} {port_name} excited by {mode}, order {}", sweep.order.as_u8());
    Ok(Solution { driven: &sweep.driven, field: x, frequency: r.frequency, caption })
}

/// A view's plane in the model. Plane coordinates (u along the picture's horizontal, v its
/// vertical) run from the bounding box's low corner, `at` from its low side along the normal.
struct Frame {
    lo: Point3,
    /// The model axes along the picture's horizontal, its vertical and the normal.
    axes: [usize; 3],
    hspan: f64,
    vspan: f64,
    /// The low corner's coordinates along the three axes.
    origin: [f64; 3],
    /// The plane's position along its normal from the low side (m).
    at: f64,
}

impl Frame {
    fn new(session: &Session, view: &FieldView) -> Result<Frame> {
        let emcraft_model::Aabb { min: lo, max: hi } = session.model.bounds().ok_or_else(|| EngineError::failed("no geometry"))?;
        let axes @ [hk, vk, nk] = view.plane.axes();
        let (size, low) = ((hi - lo).to_array(), lo.to_array());
        let normal_span = size[nk];
        let at = view.at.unwrap_or(normal_span / 2.0).clamp(0.0, normal_span);
        Ok(Frame { lo, axes, hspan: size[hk], vspan: size[vk], origin: [low[hk], low[vk], low[nk]], at })
    }

    /// The model point at plane coordinates (u, v).
    fn point(&self, u: f64, v: f64) -> Point3 {
        let [hk, vk, nk] = self.axes;
        let mut c = [0.0; 3];
        (c[hk], c[vk], c[nk]) = (u, v, self.at);
        self.lo + Vec3::new(c[0], c[1], c[2])
    }

    fn longer(&self) -> f64 {
        self.hspan.max(self.vspan)
    }
}

/// Samples the fields of a view's plane (its component, arrows and phase are not used).
pub fn sample_plane(session: &Session, view: &FieldView) -> Result<PlaneSamples> {
    let frame = Frame::new(session, view)?;
    let Solution { driven, field: x, frequency, caption } = solution(session, view)?;
    let fields = |(u, v): (f64, f64)| driven.fields(x, frequency, frame.point(u, v));
    let (hspan, vspan, longer) = (frame.hspan, frame.vspan, frame.longer());
    let [h0, v0, n0] = frame.origin;
    let [hk, vk, nk] = frame.axes;
    let (nx, ny) =
        (((SCALAR_SAMPLES as f64 * hspan / longer).ceil() as usize).max(2), ((SCALAR_SAMPLES as f64 * vspan / longer).ceil() as usize).max(2));
    let grid =
        (0..ny).flat_map(|j| (0..nx).map(move |i| (hspan * i as f64 / (nx - 1) as f64, vspan * j as f64 / (ny - 1) as f64))).map(fields).collect();
    let l = session.display.length;
    let step = longer / ARROWS as f64;
    let (ax, ay) = (((hspan / step).round() as usize).max(2), ((vspan / step).round() as usize).max(2));
    let sample = |(u, v): (f64, f64)| fields((u, v)).map(|(e, h)| ([l.from_si(h0 + u), l.from_si(v0 + v)], e, h));
    let arrows = (0..ay)
        .flat_map(|j| (0..ax).map(move |i| (hspan * (i as f64 + 0.5) / ax as f64, vspan * (j as f64 + 0.5) / ay as f64)))
        .filter_map(sample)
        .collect();
    let corners =
        (1..ay).flat_map(|j| (1..ax).map(move |i| (hspan * i as f64 / ax as f64, vspan * j as f64 / ay as f64))).filter_map(sample).collect();
    let lname = l.symbol();
    let fsym = session.display.frequency;
    let title = format!(
        "{} plane, {} = {} {lname}   f = {} {}",
        view.plane.title(),
        emcraft_model::Axis::name_of(nk),
        emcraft_render::axes::format(l.from_si(n0 + frame.at)),
        emcraft_render::axes::format(fsym.from_si(frequency)),
        fsym.symbol()
    );
    Ok(PlaneSamples {
        plane: view.plane,
        frequency,
        at: frame.at,
        excitation: view.excitation,
        mode: view.mode,
        nx,
        ny,
        grid,
        arrows,
        corners,
        arrow_length: l.from_si(step) * 1.1,
        lines: section_lines(session, view.plane, vspan),
        title,
        caption,
        x: Axis::new(&format!("{} ({lname})", emcraft_model::Axis::name_of(hk)), l.from_si(h0), l.from_si(h0 + hspan)),
        y: Axis::new(&format!("{} ({lname})", emcraft_model::Axis::name_of(vk)), l.from_si(v0), l.from_si(v0 + vspan)),
    })
}

/// The waveguide's section boundaries across a plane along it, `vspan` (m) high, in display
/// units; none across the guide.
fn section_lines(session: &Session, plane: Plane, vspan: f64) -> Vec<[[f64; 2]; 2]> {
    let l = session.display.length;
    match session.model.waveguide.as_ref() {
        Some(w) if plane != Plane::Cross => w
            .interfaces()
            .iter()
            .skip(1)
            .take(w.sections.len().saturating_sub(1))
            .map(|z| [[l.from_si(*z), 0.0], [l.from_si(*z), l.from_si(vspan)]])
            .collect(),
        _ => Vec::new(),
    }
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
        let comp = view.component;
        let mut amplitudes = Vec::with_capacity(self.grid.len());
        let values = self
            .grid
            .iter()
            .map(|g| match g.as_ref().map(|(e, h)| comp.sample(e, h, rot)) {
                Some((v, amp)) => {
                    amplitudes.push(amp);
                    v
                }
                None => f64::NAN,
            })
            .collect();
        let peak = robust_peak(amplitudes);
        let mut sets = Vec::new();
        if view.show_e {
            sets.extend(self.arrow_set(false, &self.arrows, rot));
        }
        if view.show_h {
            // H between E's arrows when both show.
            let lattice = if view.show_e && !self.corners.is_empty() { &self.corners } else { &self.arrows };
            sets.extend(self.arrow_set(true, lattice, rot));
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

    /// The in-plane arrows of E (or H, `magnetic`) on `lattice` at the phase `rot`; none when
    /// the field has (next to) nothing in the plane.
    fn arrow_set(&self, magnetic: bool, lattice: &[([f64; 2], CVec3, CVec3)], rot: C) -> Option<Arrows> {
        let (label, color) = if magnetic { ("H (in plane)", Rgb(0, 120, 60)) } else { ("E (in plane)", Rgb(20, 20, 20)) };
        // In-plane components of a vector: (horizontal, vertical).
        let in_plane = |f: &CVec3| match self.plane {
            Plane::H => (f.z, f.x),
            Plane::E => (f.z, f.y),
            Plane::Cross => (f.x, f.y),
        };
        let re = |c: C| (c * rot).re;
        let mut totals = Vec::new();
        let mut list = Vec::with_capacity(lattice.len());
        let mut sizes = Vec::with_capacity(lattice.len());
        for (pos, e, h) in lattice {
            let f = if magnetic { h } else { e };
            let (u, v) = in_plane(f);
            totals.push(f.norm_sqr().sqrt());
            sizes.push((u.norm_sqr() + v.norm_sqr()).sqrt());
            list.push((*pos, [re(u), re(v)]));
        }
        let (in_peak, total) = (robust_peak(sizes), robust_peak(totals));
        // Scaled by the peak over the period, so an animation keeps one scale. A field with
        // (next to) nothing in the plane — E in the H-plane for TE10, whose in-plane part is
        // discretisation error — draws no arrows.
        (in_peak > IN_PLANE * total).then(|| Arrows { label: label.into(), color, arrows: list, scale: Some(in_peak) })
    }
}

/// How far the largest value may stand above the 90th percentile before it no longer sets the
/// scale.
const SPIKE: f64 = 4.0;

/// The value a picture's colours and arrows are scaled to: the largest, unless it is a spike —
/// a source's near field, falling as 1/r³ round a dipole or a gap, far above everything else —
/// when the 90th percentile is (the larger values saturate), so the rest of the plane shows.
pub fn robust_peak(mut values: Vec<f64>) -> f64 {
    values.retain(|v| v.is_finite());
    let peak = values.iter().copied().fold(0.0f64, f64::max);
    if values.len() < 50 {
        return peak;
    }
    values.sort_by(f64::total_cmp);
    let q = values[(values.len() - 1) * 90 / 100];
    if q > 0.0 && peak > SPIKE * q { q } else { peak }
}
