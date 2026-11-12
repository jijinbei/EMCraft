//! The analyses read from a sweep (`results.material`, `.noise`, `.active`, `.breakdown`,
//! `.thermal`): each one's button (its command, label and what it tells) and a line on the last
//! one's numbers; its chart is the engine's (`emcraft_engine::view::analyses::chart`).

use crate::args;

/// Each analysis' button: its command, label and what it tells.
pub const BUTTONS: [(&str, crate::lang::Text, crate::lang::Text); 5] = [
    ("results.material", t!("Material from S"), t!("εr and μr of a sample filling the line (Nicolson–Ross–Weir)")),
    ("results.noise", t!("Noise figure"), t!("The noise figure of the passive two-port at its temperature")),
    ("results.active", t!("Active reflection"), t!("Each element's reflection as the array scans")),
    ("results.breakdown", t!("Power handling"), t!("The largest field for 1 W and the power at breakdown")),
    ("results.thermal", t!("Temperature"), t!("The temperature the losses raise, the walls a heat sink")),
];

/// One line on the last analysis' numbers: a temperature's hottest point and heat, a power
/// handling's least power and where the field peaks.
pub fn summary(session: &emcraft_engine::Session, a: &crate::replies::Analysis) -> Option<String> {
    let r = &a.reply;
    let lu = session.display().length.symbol();
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
