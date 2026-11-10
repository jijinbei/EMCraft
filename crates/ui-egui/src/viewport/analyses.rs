//! The analyses read from a sweep (`results.material`, `.noise`, `.active`, `.breakdown`,
//! `.thermal`): a button for each, opening its dialog, and the last one's chart — or, for a
//! temperature, its numbers.

use super::*;
use crate::args;

/// Each analysis' button: its command, label and what it tells.
const BUTTONS: [(&str, crate::lang::Text, crate::lang::Text); 5] = [
    ("results.material", t!("Material from S"), t!("εr and μr of a sample filling the line (Nicolson–Ross–Weir)")),
    ("results.noise", t!("Noise figure"), t!("The noise figure of the passive two-port at its temperature")),
    ("results.active", t!("Active reflection"), t!("Each element's reflection as the array scans")),
    ("results.breakdown", t!("Power handling"), t!("The largest field for 1 W and the power at breakdown")),
    ("results.thermal", t!("Temperature"), t!("The temperature the losses raise, the walls a heat sink")),
];

pub(super) struct Analyses;

impl ChartView for Analyses {
    type Key = SharedChart;
    const HANDLE: Fixed = Fixed::ChartAnalyses;
    const CHART: emcraft_engine::views::ChartId = emcraft_engine::views::ChartId::Analyses;
    const HEIGHT: Height = Height::new(0.75, 260.0, 700.0);
    const BELOW: f32 = 60.0;

    fn controls(app: &EmcraftApp, _st: &mut Parts, ui: &mut egui::Ui, out: &mut Actions) -> Option<Self::Key> {
        let shown = app.replies.analysis.as_ref();
        ui.horizontal_wrapped(|ui| {
            for (id, label, tip) in BUTTONS {
                let on = shown.is_some_and(|a| a.command == id);
                let b = ui.named_enabled(!app.busy(), egui::Button::selectable(on, tr(label)), Handle::Analysis(id)).hint(tip);
                if b.clicked() {
                    out.push(Action::OpenDialog { id: id.to_owned(), values: None });
                }
            }
        });
        let a = shown?;
        let unit = app.session.display().frequency.symbol();
        emcraft_engine::view::analyses::chart(&a.command, &a.reply, unit).map(|_| SharedChart::Analysis(a.run))
    }

    fn chart(app: &EmcraftApp, _key: &Self::Key) -> Result<emcraft_render::chart::Chart, String> {
        let a = app.replies.analysis.as_ref().ok_or("no analysis yet")?;
        emcraft_engine::view::analyses::chart(&a.command, &a.reply, app.session.display().frequency.symbol())
            .ok_or_else(|| "nothing to chart".to_owned())
    }

    fn below(app: &EmcraftApp, _st: &mut Parts, ui: &mut egui::Ui, _chart: &egui::Response, _size: egui::Vec2, _out: &mut Actions) {
        if let Some(text) = app.replies.analysis.as_ref().and_then(|a| summary(app, a)) {
            ui.text(Style::Body, Raw(&text));
        }
    }
}

/// One line on the last analysis' numbers: a temperature's hottest point and heat, a power
/// handling's least power and where the field peaks.
fn summary(app: &EmcraftApp, a: &crate::state::Analysis) -> Option<String> {
    let r = &a.reply;
    let lu = app.session.display().length.symbol();
    let at = |v: &serde_json::Value| {
        v.as_array().map(|p| p.iter().filter_map(serde_json::Value::as_f64).map(|x| format!("{x:.2}")).collect::<Vec<_>>().join(", "))
    };
    match a.command.as_str() {
        "results.thermal" => Some(crate::lang::trf(
            t!("Hottest {temperature} °C at ({at}) {unit}; {heat} W deposited for {power} W in"),
            args!(
                temperature = format!("{:.1}", r["max_temperature"].as_f64()?),
                at = at(&r["at"])?,
                unit = lu,
                heat = format!("{:.3}", r["heat"].as_f64()?),
                power = r["power"].as_f64()?
            ),
        )),
        "results.breakdown" => {
            let least = r["points"].as_array()?.iter().filter_map(|p| Some((p["max_power"].as_f64()?, p))).min_by(|x, y| x.0.total_cmp(&y.0))?;
            Some(crate::lang::trf(
                t!("Breaks down first at {power} kW ({frequency}); the field peaks at ({at}) {unit}"),
                args!(power = format!("{:.1}", least.0 * 1e-3), frequency = least.1["frequency"].as_f64()?, at = at(&least.1["at"])?, unit = lu),
            ))
        }
        _ => None,
    }
}
