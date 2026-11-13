//! The field view's side as every window shows it: the power that reaches each port at the
//! field's frequency, the project's probes with the field coloured there, the fields at a point
//! (the pointer's, the pinned one's), and the parameters its picture and animation are saved with
//! (`view.field`, `view.animation`).

use emcraft_engine::Session;
use emcraft_engine::view::{Component, FieldView, Probe};
use serde_json::{Value, json};

use super::Table;
use crate::lang::{args, tr, trf};

/// Frames a saved animation takes for a period.
const FRAMES: usize = 24;

/// The power the S entries carry at the field's frequency (`emcraft_engine::power::at`'s
/// reply) as a table — each port with its S, its share of the incident power and of what
/// leaves — and what is not accounted for (loss, radiation, error).
pub fn power(report: &Value) -> (Table, String) {
    let percent = |v: Option<f64>| v.map_or_else(|| "—".to_owned(), |v| format!("{:.1}%", 100.0 * v));
    let rows = report["outputs"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|o| {
            vec![
                format!("{} · {}", o["port"].as_str().unwrap_or(""), o["s"].as_str().unwrap_or("")),
                percent(o["incident_fraction"].as_f64()),
                percent(o["branch_fraction"].as_f64()),
            ]
        })
        .collect();
    let headers = [t!("Port"), t!("Incident power"), t!("Output share")].map(|h| tr(h).to_owned()).to_vec();
    let rest = trf(t!("Unaccounted: {value}%"), args!(value = format!("{:.2}", 100.0 * report["unaccounted"].as_f64().unwrap_or(0.0))));
    (Table { caption: Some(tr(t!("Power in the S entries; the rest is loss, radiation or error.")).to_owned()), headers, rows, marks: None }, rest)
}

/// The quantity a component's magnitude is read as, and its unit: |E| in V/m, |H| in A/m.
fn magnitude(component: Component) -> (&'static str, &'static str) {
    if component.is_magnetic() { ("|H|", "A/m") } else { ("|E|", "V/m") }
}

/// A probe's line: `◆ name  |E| 12.3 V/m` (– where the field is not there).
pub fn probe_line(name: &str, values: Option<(emcraft_model::CVec3, emcraft_model::CVec3)>, component: Component) -> String {
    let (quantity, unit) = magnitude(component);
    let norm = |v: &emcraft_model::CVec3| v.iter().map(|c| c.norm_sqr()).sum::<f64>().sqrt();
    let value = values.map_or_else(
        || "–".to_owned(),
        |(e, h)| format!("{quantity} {} {unit}", emcraft_render::axes::format(norm(if component.is_magnetic() { &h } else { &e }))),
    );
    format!("◆ {name}  {value}")
}

/// The fields at `p` (plane coordinates in display units `lu`, the plane's axes named `axes`):
/// where, the component coloured now and its amplitude, and with `full` each component of E and
/// H as a phasor.
pub fn readout(p: &Probe, axes: (&str, &str), lu: &str, component: Component, phase: f64, full: bool) -> Vec<String> {
    let fmt = emcraft_render::axes::format;
    let unit = magnitude(component).1;
    let (now, amp) = p.value(component, phase);
    let place = format!("{} {} {lu}, {} {} {lu}", axes.0, fmt(p.at[0]), axes.1, fmt(p.at[1]));
    let value = trf(
        t!("{component} = {now} {unit} (amplitude {amplitude} {unit})"),
        args!(component = component.label(), now = fmt(now), unit = unit, amplitude = fmt(amp)),
    );
    let phasor = |c: emcraft_model::Complex64, u: &str| {
        if c.norm() == 0.0 { format!("0 {u}") } else { format!("{} {u} ∠ {}°", fmt(c.norm()), fmt(c.arg().to_degrees().round())) }
    };
    let parts = full.then(|| {
        [("E", p.e, "V/m"), ("H", p.h, "A/m")]
            .into_iter()
            .flat_map(|(f, v, u)| [("x", v.x), ("y", v.y), ("z", v.z)].map(|(a, c)| format!("{f}{a} {}", phasor(c, u))))
            .collect::<Vec<_>>()
    });
    [place, value].into_iter().chain(parts.into_iter().flatten()).collect()
}

/// The parameters the field view is saved with as a picture (`view.field`) at `path`, the plane
/// `at` (display units from the model's low side) where it is placed; an animation of a period
/// (`view.animation`) with `frames`.
///
/// ```
/// use emcraft_engine::view::{FieldView, Plane};
/// let s = emcraft_engine::Session::new();
/// let v = FieldView { show_e: true, show_h: false, ..FieldView::new(8.25e9, Plane::H) };
/// let p = emcraft_ui_core::results::fields::save_params(&v, Some(12.3456789), &s, "field.png", false);
/// assert_eq!((p["path"].as_str(), p["arrows"].clone(), p["at"].as_f64()), (Some("field.png"), serde_json::json!(["e"]), Some(12.3457)));
/// assert!(emcraft_ui_core::results::fields::save_params(&v, None, &s, "a.png", true)["frames"].as_u64().is_some());
/// ```
pub fn save_params(v: &FieldView, at: Option<f64>, s: &Session, path: &str, animation: bool) -> Value {
    let entries = s.sparameter_entries();
    let arrows: Vec<&str> = [(v.show_e, "e"), (v.show_h, "h")].into_iter().filter(|(on, _)| *on).map(|(_, a)| a).collect();
    let mut p = json!({"path": path, "plane": v.plane.id(), "component": v.component.id(), "arrows": arrows});
    match v.mode {
        Some(m) => p["mode"] = json!(m + 1),
        None => p["frequency"] = json!(s.display().frequency.from_si(v.frequency)),
    }
    if animation {
        p["frames"] = json!(FRAMES);
    } else {
        p["phase"] = json!((v.phase.to_degrees() * 10.0).round() / 10.0);
    }
    if let Some(at) = at.filter(|a| a.is_finite()) {
        p["at"] = json!((at * 1e4).round() / 1e4);
    }
    if let Some(e) = entries.get(v.excitation).filter(|_| entries.len() > 1) {
        p["excitation"] = json!(e.name);
    }
    if let Some(c) = v.clamp {
        p["clamp"] = json!(c);
    }
    p
}
