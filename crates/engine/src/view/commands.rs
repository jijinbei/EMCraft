//! The `view.*` commands: each reads its parameters, builds its picture and writes it as a PNG.

use emcraft_render::chart::Chart;
use emcraft_render::plot::FieldPlot;
use serde_json::{Value, json};

use super::field::{FieldView, Plane};
use crate::{EIGEN_MAX, EngineError, Params, Result, Session, args};

/// Writes a picture's PNG bytes (`None` when it could not be encoded) to `path`.
pub(crate) fn write_png(path: &str, png: Option<Vec<u8>>) -> Result<()> {
    if path.trim().is_empty() {
        return Err(EngineError::Params("`path` must not be empty".into()));
    }
    let bytes = png.ok_or_else(|| EngineError::failed("could not encode the picture"))?;
    std::fs::write(path, bytes).map_err(|e| EngineError::Failed(format!("cannot write {path}: {e}")))
}

/// Where a chart command writes its picture: `path`, `width` (default 1000) by `height`
/// (default 600) pixels.
struct ChartFile<'a> {
    path: &'a str,
    width: usize,
    height: usize,
}

impl<'a> ChartFile<'a> {
    /// `path`, then the size.
    fn read(s: &Session, p: &Params<'a>) -> Result<ChartFile<'a>> {
        let path = p.string("path").required()?;
        ChartFile::sized(s, p, path)
    }

    /// The size, for a chart to `path`.
    fn sized(s: &Session, p: &Params, path: &'a str) -> Result<ChartFile<'a>> {
        let (width, height) = (p.side("width", s.limits.picture_side, 1000)?, p.side("height", s.limits.picture_side, 600)?);
        Ok(ChartFile { path, width, height })
    }

    /// Draws `chart` and writes it: the reply.
    fn write(&self, chart: &Chart) -> Result<Value> {
        let c = emcraft_render::chart::draw(chart, self.width, self.height);
        write_png(self.path, c.png())?;
        Ok(json!({"path": self.path, "width": c.w, "height": c.h}))
    }
}

/// Draws `plot` `width` pixels wide and writes it to `path`: the reply.
fn write_plot(path: &str, plot: &FieldPlot, width: usize) -> Result<Value> {
    let c = emcraft_render::plot::draw(plot, width);
    write_png(path, c.png())?;
    Ok(json!({"path": path, "width": c.w, "height": c.h}))
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
        Some(list) => (list.iter().any(|x| x.as_str() == Some("e")), list.iter().any(|x| x.as_str() == Some("h"))),
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
    let plot = super::field_plot(s, &field_view(s, p)?)?;
    write_plot(path, &plot, width)
}

/// `view.port_mode`: a port mode's transverse E across the port's face.
pub fn port_mode(s: &mut Session, p: &Params) -> Result<Value> {
    let path = p.string("path").required()?;
    let width = p.side("width", s.limits.picture_side, 900)?;
    let port = p.string("port").required()?;
    let mode = p.integer("mode", 1..=EIGEN_MAX).or(1)?;
    let frequency = args::frequency_or_band_top(s, p, "frequency", args::SET_THE_BAND)?;
    let plot = super::port_mode_plot(s, port, mode - 1, frequency, p.order()?.as_u8())?;
    let mut reply = write_plot(path, &plot, width)?;
    reply["mode"] = json!(plot.title);
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
    let mut v = field_view(s, p)?;
    let samples = super::sample_plane(s, &v)?;
    let delay = (period * 1000.0 / frames as f64).round().clamp(1.0, f64::from(u16::MAX)) as u16;
    let mut failed = None;
    let pictures = (0..frames).map_while(|k| {
        v.phase = std::f64::consts::TAU * k as f64 / frames as f64;
        match samples.plot(&v) {
            Ok(plot) => Some(emcraft_render::plot::draw(&plot, width)),
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
    file.write(&super::sparameter_chart(s)?)
}

/// `view.tdr`.
pub fn tdr(s: &mut Session, p: &Params) -> Result<Value> {
    let file = ChartFile::read(s, p)?;
    let (t, port, z0) = crate::results::tdr_of(s, p)?;
    file.write(&super::tdr_chart(&t, &port, z0))
}

/// `view.dispersion`.
pub fn dispersion(s: &mut Session, p: &Params) -> Result<Value> {
    let file = ChartFile::read(s, p)?;
    file.write(&super::dispersion_chart(s)?)
}

/// `view.study`: the last study's chart.
pub fn study(s: &mut Session, p: &Params) -> Result<Value> {
    let file = ChartFile::read(s, p)?;
    let study = s.results.study.as_ref().ok_or_else(|| EngineError::failed("no study: run study.sweep or study.optimize first"))?;
    let at = match p.raw("at") {
        None => None,
        Some(v) => Some(v.as_object().ok_or_else(|| p.error("at", "must map the held variables to their values: {\"b\": 5}"))?),
    };
    let slice = super::study::Slice::named(study, at)?;
    file.write(&super::study::chart(study, p.string("parameter").optional()?, &slice)?)
}

/// `view.pattern`: the far-field pattern.
pub fn pattern(s: &mut Session, p: &Params) -> Result<Value> {
    let path = p.string("path").required()?;
    let phi = p.number("phi").or(0.0)?;
    let n = p.integer("points", 13..=721).or(91)?;
    let chart = super::pattern_chart_of(s, phi, n, p.choice("solver").optional()?, p.frequency("frequency").optional()?)?;
    ChartFile::sized(s, p, path)?.write(&chart)
}
