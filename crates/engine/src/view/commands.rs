//! The `view.*` commands: each reads its parameters, builds its picture and writes it as a PNG,
//! in the colours of its `theme` (`light` by default, `dark` or a theme file: [`crate::theme`]).

use emcraft_render::Palette;
use emcraft_render::chart::Chart;
use emcraft_render::plot::FieldPlot;
use serde_json::{Value, json};

use super::field::{FieldView, Plane};
use crate::views::ChartId;
use crate::{EIGEN_MAX, EngineError, Params, Result, Session, args};

/// Writes a picture's PNG bytes (`None` when it could not be encoded) to `path`.
pub(crate) fn write_png(path: &str, png: Option<Vec<u8>>) -> Result<()> {
    if path.trim().is_empty() {
        return Err(EngineError::Params("`path` must not be empty".into()));
    }
    let bytes = png.ok_or_else(|| EngineError::failed("could not encode the picture"))?;
    std::fs::write(path, bytes).map_err(|e| EngineError::cannot_write(path, e))
}

/// Where a chart command writes its picture: `path`, `width` (default 1000) by `height`
/// (default 600) pixels, in the colours of `theme`.
struct ChartFile<'a> {
    path: &'a str,
    width: usize,
    height: usize,
    palette: Palette,
}

impl<'a> ChartFile<'a> {
    /// `path`, then the size and the theme.
    fn read(s: &Session, p: &Params<'a>) -> Result<ChartFile<'a>> {
        let path = p.string("path").required()?;
        ChartFile::sized(s, p, path)
    }

    /// The size and the theme, for a chart to `path`.
    fn sized(s: &Session, p: &Params, path: &'a str) -> Result<ChartFile<'a>> {
        let (width, height) = (p.side("width", s.limits.picture_side, 1000)?, p.side("height", s.limits.picture_side, 600)?);
        Ok(ChartFile { path, width, height, palette: crate::theme::palette_of(p)? })
    }

    /// Draws `chart` in the project's style for chart `id` (`view.chart.style`) and writes it:
    /// the reply.
    fn write(&self, s: &Session, id: ChartId, chart: Chart) -> Result<Value> {
        let chart = crate::views::style_chart(s, id, chart);
        let c = emcraft_render::chart::draw(&chart, self.width, self.height, &self.palette);
        write_png(self.path, c.png())?;
        Ok(json!({"path": self.path, "width": c.w, "height": c.h}))
    }
}

/// Draws `plot` `width` pixels wide and writes it to `path`: the reply.
fn write_plot(path: &str, plot: &FieldPlot, width: usize, palette: &Palette) -> Result<Value> {
    let c = emcraft_render::plot::draw(plot, width, palette);
    write_png(path, c.png())?;
    Ok(json!({"path": path, "width": c.w, "height": c.h}))
}

emcraft_macros::named_enum! {
    /// The fields `arrows` draws.
    #[derive(Clone, Copy, Debug, PartialEq)]
    enum Arrows {
        E = "e",
        H = "h",
    }
}

/// A field view from command parameters.
pub fn field_view(s: &Session, p: &Params) -> Result<FieldView> {
    let plane = p.choice("plane").or(Plane::H)?;
    let mut v = match p.integer("mode", 1..=EIGEN_MAX).optional()? {
        Some(m) => FieldView::mode(m - 1, plane),
        None => FieldView::new(p.frequency("frequency").required_or("or `mode`")?, plane),
    };
    v.at = p.length("at").optional()?;
    if let Some(c) = p.string("component").optional()? {
        v.component = c.parse()?;
    }
    // The arrows of the field coloured unless asked otherwise.
    (v.show_e, v.show_h) = match p.list("arrows", 16).optional()? {
        Some(list) => {
            let fields = list
                .iter()
                .map(|x| {
                    x.as_str()
                        .and_then(Arrows::parse)
                        .ok_or_else(|| p.error("arrows", format!("holds `e` or `h`, got {}", crate::text::clip(&x.to_string(), 40))))
                })
                .collect::<Result<Vec<Arrows>>>()?;
            (fields.contains(&Arrows::E), fields.contains(&Arrows::H))
        }
        None => (!v.component.is_magnetic(), v.component.is_magnetic()),
    };
    v.phase = p.number("phase").or(0.0)?.to_radians();
    v.excitation = args::excitation(s, p)?;
    Ok(v)
}

/// `view.field`.
pub fn field(s: &mut Session, p: &Params) -> Result<Value> {
    let path = p.string("path").required()?;
    let width = p.side("width", s.limits.picture_side, 1100)?;
    let palette = crate::theme::palette_of(p)?;
    let plot = super::field_plot(s, &field_view(s, p)?)?;
    write_plot(path, &plot, width, &palette)
}

/// `view.port_mode`: a port mode's transverse E across the port's face.
pub fn port_mode(s: &mut Session, p: &Params) -> Result<Value> {
    let path = p.string("path").required()?;
    let width = p.side("width", s.limits.picture_side, 900)?;
    let port = p.string("port").required()?;
    let mode = p.integer("mode", 1..=EIGEN_MAX).or(1)?;
    let frequency = args::frequency_or_band_top(s, p, "frequency", args::SET_THE_BAND)?;
    let palette = crate::theme::palette_of(p)?;
    let plot = super::port_mode_plot(s, port, mode - 1, frequency, p.order()?.as_u8())?;
    let mut reply = write_plot(path, &plot, width, &palette)?;
    reply["mode"] = json!(plot.title);
    Ok(reply)
}

/// `view.revolution_field`: the field of the model as a body of revolution on its meridian
/// half-plane for a wave into one of its ports: |E| in colour, (E_r, E_z) as arrows (φ = 0, phase
/// 0), the axis on the left.
pub fn revolution_field(s: &mut Session, p: &Params) -> Result<Value> {
    use emcraft_render::plot::{Arrows, FieldPlot, Scalar, ScalarKind};
    let path = p.string("path").required()?;
    let width = p.side("width", s.limits.picture_side, 600)?;
    let frequency = args::frequency_or_band_top(s, p, "frequency", args::SET_THE_BAND)?;
    let m = p.integer("m", 0..=64).or(1)? as u32;
    let palette = crate::theme::palette_of(p)?;
    let (profile, ports) = crate::solve::revolution_setup(s, p, frequency)?;
    let excitation = match p.string("port").optional()? {
        Some(name) => s.model.ports.iter().position(|x| x.name == name).ok_or_else(|| p.error("port", format!("no port `{name}`")))?,
        None => 0,
    };
    let (r0, r1, z0, z1) = profile.points.iter().fold((f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY), |b, q| {
        (b.0.min(q[0]), b.1.max(q[0]), b.2.min(q[1]), b.3.max(q[1]))
    });
    // Square cells: the longer side 160 points.
    let side = 160.0 / (r1 - r0).max(z1 - z0).max(f64::MIN_POSITIVE);
    let (nx, ny) = (((r1 - r0) * side).round().max(2.0) as usize, ((z1 - z0) * side).round().max(2.0) as usize);
    let f = emcraft_solver::bor::field(&profile, m, &ports, frequency, excitation, [nx, ny])?;
    let l = s.display.length;
    let lu = l.symbol();
    let magnitude: Vec<f64> = f.e.iter().map(|e| e.map_or(f64::NAN, |e| (e[0].norm_sqr() + e[1].norm_sqr() + e[2].norm_sqr()).sqrt())).collect();
    let step = (nx.max(ny) / 20).max(1);
    let mut arrows = Vec::new();
    for j in (step / 2..ny).step_by(step) {
        for i in (step / 2..nx).step_by(step) {
            if let Some(e) = f.e[j * nx + i] {
                let q = [r0 + (r1 - r0) * i as f64 / (nx - 1) as f64, z0 + (z1 - z0) * j as f64 / (ny - 1) as f64];
                arrows.push(([l.from_si(q[0]), l.from_si(q[1])], [e[0].re, e[1].re]));
            }
        }
    }
    let fu = s.display.frequency;
    let name = f.point.modes.get(excitation).cloned().unwrap_or_default();
    let plot = FieldPlot {
        title: format!(
            "Body of revolution, m = {m}, {name} into port {}  f = {:.4} {}",
            s.model.ports.get(excitation).map_or("", |x| x.name.as_str()),
            fu.from_si(frequency),
            fu.symbol()
        ),
        subtitle: "the meridian half-plane (axis on the left): |E| in colour, (E_r, E_z) at φ = 0".into(),
        x: emcraft_render::Axis::new(&format!("r ({lu})"), l.from_si(r0), l.from_si(r1)),
        y: emcraft_render::Axis::new(&format!("z ({lu})"), l.from_si(z0), l.from_si(z1)),
        scalar: Some(Scalar { label: "$|E|$".into(), nx, ny, values: magnitude, kind: ScalarKind::Magnitude, range: None }),
        arrows: vec![Arrows { label: "E (in plane)".into(), pen: emcraft_render::Pen::EArrows, arrows, scale: None }],
        lines: Vec::new(),
        arrow_length: l.from_si((r1 - r0).max(z1 - z0)) / 20.0 * 0.9,
    };
    let mut reply = write_plot(path, &plot, width, &palette)?;
    reply["mode"] = json!(name);
    Ok(reply)
}

/// The most frames an animation may have.
pub const MAX_FRAMES: usize = 120;

/// `view.animation`: one period of the field's phase as an animated PNG, the plane sampled once,
/// a frame at each of `frames` phases evenly spaced over 360°, looping.
pub fn animation(s: &mut Session, p: &Params) -> Result<Value> {
    let path = p.string("path").required()?;
    let width = p.side("width", s.limits.picture_side, 800)?;
    let frames = p.integer("frames", 4..=MAX_FRAMES).or(24)?;
    let period = p.positive("period").check(|t| *t <= 60.0, "at most 60 s").or(2.0)?;
    let palette = crate::theme::palette_of(p)?;
    let mut v = field_view(s, p)?;
    let samples = super::sample_plane(s, &v)?;
    let delay = (period * 1000.0 / frames as f64).round().clamp(1.0, f64::from(u16::MAX)) as u16;
    let mut failed = None;
    let pictures = (0..frames).map_while(|k| {
        v.phase = std::f64::consts::TAU * k as f64 / frames as f64;
        match samples.plot(&v) {
            Ok(plot) => Some(emcraft_render::plot::draw(&plot, width, &palette)),
            Err(e) => {
                failed = Some(e);
                None
            }
        }
    });
    let bytes = emcraft_render::apng(pictures, frames, delay);
    if let Some(e) = failed {
        return Err(e);
    }
    write_png(path, bytes)?;
    Ok(json!({"path": path, "frames": frames, "period": period, "delay_ms": delay}))
}

/// `view.sparameters`.
pub fn sparameters(s: &mut Session, p: &Params) -> Result<Value> {
    let file = ChartFile::read(s, p)?;
    let display = p.pick("display").or(super::SDisplay::Db)?;
    let compare: Vec<u32> = p
        .list("compare", crate::archive::MAX_KEPT * 4)
        .or(&[])?
        .iter()
        .map(|v| {
            v.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or_else(|| p.error("compare", "must list the ids of kept solves (results.history)"))
        })
        .collect::<Result<_>>()?;
    file.write(s, ChartId::SParameters, super::sparameter_chart_compared(s, display, &compare)?)
}

/// `view.tdr`.
pub fn tdr(s: &mut Session, p: &Params) -> Result<Value> {
    let file = ChartFile::read(s, p)?;
    let (t, port, z0) = crate::results::tdr_of(s, p)?;
    file.write(s, ChartId::Tdr, super::tdr_chart(&t, &port, z0))
}

/// `view.dispersion`.
pub fn dispersion(s: &mut Session, p: &Params) -> Result<Value> {
    let file = ChartFile::read(s, p)?;
    file.write(s, ChartId::Dispersion, super::dispersion_chart(s)?)
}

/// `view.study`: the last study's chart.
pub fn study(s: &mut Session, p: &Params) -> Result<Value> {
    let file = ChartFile::read(s, p)?;
    let study = s.results.study.as_ref().ok_or(EngineError::NotSolved { what: "study", run: "study.sweep or study.optimize" })?;
    let at = match p.raw("at") {
        None => None,
        Some(v) => Some(v.as_object().ok_or_else(|| p.error("at", "must map the held variables to their values: {\"b\": 5}"))?),
    };
    let slice = super::study::Slice::named(study, at)?;
    let units = super::study::variable_units(s, study);
    let chart = super::study::chart_with_units(study, p.string("parameter").optional()?, &slice, &units)?;
    file.write(s, ChartId::Study, chart)
}

/// A far-field pattern's settings from command parameters: the cut (`phi`, the default, or
/// `theta`) at its angle (`phi` or `theta`, degrees), the component, the angles drawn, the
/// solver and frequency.
pub fn pattern_view(p: &Params) -> Result<super::PatternView> {
    let cut = p.pick("cut").or(super::Cut::Phi)?;
    let angle = match cut {
        super::Cut::Phi => p.number("phi").or(0.0)?,
        super::Cut::Theta => p.number("theta").check(|t| (0.0..=180.0).contains(t), "0 to 180").or(90.0)?,
    };
    Ok(super::PatternView {
        cut,
        angle,
        component: p.pick("component").or(super::FarComponent::Total)?,
        points: p.integer("points", 13..=721).or(91)?,
        solver: p.choice("solver").optional()?,
        frequency: p.frequency("frequency").optional()?,
    })
}

/// `view.pattern`: the far-field pattern.
pub fn pattern(s: &mut Session, p: &Params) -> Result<Value> {
    let path = p.string("path").required()?;
    let chart = super::pattern_chart_in(s, &pattern_view(p)?)?;
    ChartFile::sized(s, p, path)?.write(s, ChartId::Pattern, chart)
}
