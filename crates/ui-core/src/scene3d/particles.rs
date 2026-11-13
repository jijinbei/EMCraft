//! The electrons `results.track` sent last, drawn in the model (docs/adr/0012-particles.md):
//! each trajectory a line through its points, coloured by the energy gained so far (red) or
//! lost (blue) on the diverging map, white for none; the legend saying the largest change.

use emcraft_engine::Session;
use emcraft_render::view3d::{Line, Scene};
use serde_json::Value;

use super::Built;

/// A trajectory's lines are drawn this opaque.
const ALPHA: u8 = 255;

/// A recorded point of the reply: its position (display units), its energy (eV).
fn point(v: &Value) -> Option<([f64; 3], f64)> {
    let a = v.as_array()?;
    let n = |k: usize| a.get(k).and_then(Value::as_f64).filter(|x| x.is_finite());
    Some(([n(0)?, n(1)?, n(2)?], n(4)?))
}

/// The trajectories of the session's last tracking, in metres, while the eigenmode it was run
/// on is still the one solved (its frequency the reply's); nothing otherwise.
///
/// ```
/// use emcraft_ui_core::scene3d::trajectories;
/// let s = emcraft_engine::Session::new();
/// // Nothing tracked: nothing drawn.
/// assert!(trajectories(&s).scene.lines.is_empty());
/// ```
pub fn trajectories(session: &Session) -> Built {
    let Some(reply) = session.views().tracked.as_deref() else { return Built::default() };
    let solved = session.results().eigen.as_ref().and_then(|e| {
        let index = reply.get("mode").and_then(Value::as_u64)?.checked_sub(1)?;
        e.modes.get(usize::try_from(index).ok()?).map(|m| session.show_frequency(m.frequency))
    });
    let current = reply.get("frequency").and_then(Value::as_f64).zip(solved).is_some_and(|(a, b)| (a - b).abs() <= 1e-9 * a.abs().max(1.0));
    if !current {
        return Built::default();
    }
    let unit = session.display().length;
    let start = reply.get("energy").and_then(Value::as_f64).unwrap_or(0.0);
    let runs: Vec<Vec<([f64; 3], f64)>> = reply.get("particles").and_then(Value::as_array).map_or_default(|ps| {
        ps.iter().map(|p| p.get("points").and_then(Value::as_array).map_or_default(|pts| pts.iter().filter_map(point).collect())).collect()
    });
    let most = runs.iter().flatten().map(|(_, e)| (e - start).abs()).fold(0.0, f64::max);
    let scale = if most > 0.0 { 1.0 / most } else { 0.0 };
    let si = |p: [f64; 3]| p.map(|x| unit.to_si(x));
    let lines = runs
        .iter()
        .flat_map(|run| {
            run.windows(2).map(|w| {
                let gained = 0.5 * (w[0].1 + w[1].1) - start;
                Line { a: si(w[0].0), b: si(w[1].0), color: emcraft_render::colormap::diverging(gained * scale), alpha: ALPHA }
            })
        })
        .collect();
    let said = format!("electrons: ±{} keV", crate::number::value(most / 1e3));
    Built { scene: Scene { lines, ..Scene::default() }, legend: vec![(said, emcraft_render::colormap::diverging(1.0))], ..Built::default() }
}
