//! `results.track`: electrons sent through a solved eigenmode (docs/adr/0012-particles.md, stage
//! A), pushed by `emcraft-particles` through the mode's fields at a given accelerating voltage,
//! with an optional uniform static magnetic field.
//!
//! **The field.** The mode's phasors Ê, Ĥ at amplitude a: E(t) = Re(a Ê e^{jωt}),
//! B(t) = μ₀ Re(a Ĥ e^{jωt}) (the particles cross vacuum). |a| sets the field's own voltage along
//! the line, V₀ = |∫ E·dl|, to the voltage asked for; its phase is chosen so that a particle
//! entering at phase φ = 0 and keeping its speed v₀ gains the most: with
//! I = ∫ Ê·dl e^{jωs/v₀}, a particle of charge q entering at t = 0 gains
//! W = q Re(a e^{jφ} I), so a = (V₀/|∫ Ê·dl|)·sign(q)·Ī/|I| gives W = |q| V₀ T cos φ, T = |I|/V₀
//! the transit-time factor (T. P. Wangler, *RF Linear Accelerators*, 2nd ed., Wiley 2008, §2.2).
//! A particle whose speed changes on the way gains what it gains: that is what tracking adds.
//!
//! **Where.** The line runs from the entry plane through `from` to the exit plane through `to`
//! (the region's axis along z by default, as `results.cavity`'s). Past the planes there is no
//! field; between them a point outside the mesh is a wall the particle is lost on. Each particle
//! starts on the entry plane, `offset` off the line, at its phase.

use emcraft_model::units::MU0;
use emcraft_model::{Point3, Vec3};
use emcraft_particles::{ELECTRON, End, Line, Pusher, Sample, Settings, Start, V3, gamma, track as push_through, velocity};
use emcraft_solver::Complex64 as C;
use serde_json::{Value, json};

use crate::{EngineError, Params, Result, Session};

/// The most particles one run sends.
const MAX_PARTICLES: usize = 1000;
/// The most steps one run takes, all its particles together.
const MAX_TOTAL_STEPS: usize = 50_000_000;
/// The highest kinetic energy a particle may start with (eV).
const MAX_ENERGY: f64 = 1e13;
/// Intervals of the line integrals that set the field's amplitude and phase.
const INTERVALS: usize = 400;
/// Below this share of the asked voltage per joule the mode has no field along the line.
const NO_FIELD: f64 = 1e-12;

/// The elementary charge (C): energies in and out are in electronvolts.
const EV: f64 = 1.602_176_634e-19;

/// A unit vector across `axis`: x when the axis is along z, else the axis turned into the
/// plane it makes with z.
fn across(axis: Vec3) -> Vec3 {
    let z = Vec3::new(0.0, 0.0, 1.0);
    let n = Vec3::new(axis.y * z.z - axis.z * z.y, axis.z * z.x - axis.x * z.z, axis.x * z.y - axis.y * z.x);
    let side = if n.norm() < 1e-9 { Vec3::new(1.0, 0.0, 0.0) } else { n };
    side.normalized().unwrap_or(Vec3::new(1.0, 0.0, 0.0))
}

fn v3(p: Vec3) -> V3 {
    [p.x, p.y, p.z]
}

fn at(p: Point3) -> V3 {
    [p.x, p.y, p.z]
}

/// The end of a run as the reply says it.
fn end_name(end: End) -> &'static str {
    match end {
        End::Through => "through",
        End::Back => "back",
        End::Lost(_) => "lost",
        End::Steps => "steps",
        End::NotFinite => "not_finite",
    }
}

pub fn track(s: &mut Session, p: &Params) -> Result<Value> {
    let e = s.results.eigen.as_ref().ok_or(EngineError::NotSolved { what: "eigenmodes", run: "solve.eigenmodes" })?;
    let index = p.integer("mode", 1..=e.modes.len().max(1)).or(1)?;
    let m = e.modes.get(index - 1).ok_or_else(|| p.error("mode", format!("must be 1 to {}", e.modes.len())))?;
    let voltage = p.number("voltage").or(1e6)?;
    if !(voltage.is_finite() && voltage > 0.0 && voltage <= 1e12) {
        return Err(p.error("voltage", "must be above 0 and at most 1e12 V"));
    }
    let energy = p.number("energy").or(1e6)?;
    if !(energy.is_finite() && energy > 0.0 && energy <= MAX_ENERGY) {
        return Err(p.error("energy", format!("must be above 0 and at most {MAX_ENERGY:e} eV")));
    }
    let phases: Vec<f64> = match p.number("phase").optional()? {
        Some(deg) if deg.is_finite() => vec![deg],
        Some(_) => return Err(p.error("phase", "must be a finite number of degrees")),
        None => {
            let n = p.integer("phases", 1..=360).or(12)?;
            (0..n).map(|k| 360.0 * k as f64 / n as f64).collect()
        }
    };
    let offsets: Vec<f64> = match p.lengths("offsets").optional()? {
        Some(list) if !list.is_empty() && list.iter().all(|o| o.is_finite()) => list,
        Some(_) => return Err(p.error("offsets", "must be a non-empty list of finite lengths")),
        None => vec![0.0],
    };
    if phases.len() * offsets.len() > MAX_PARTICLES {
        return Err(p.error("phases", format!("phases × offsets: at most {MAX_PARTICLES} particles")));
    }
    let static_b = match p.vector("magnetic").optional()? {
        Some(b) if b.x.is_finite() && b.y.is_finite() && b.z.is_finite() && b.norm() <= 1e3 => v3(b),
        Some(_) => return Err(p.error("magnetic", "must be [Bx, By, Bz] in teslas, at most 1000 T")),
        None => [0.0; 3],
    };
    let pusher = match p.string("pusher").or("boris")? {
        "boris" => Pusher::Boris,
        "vay" => Pusher::Vay,
        other => return Err(p.error("pusher", format!("must be `boris` or `vay`, got `{}`", crate::text::clip(other, 32)))),
    };
    let per_period = p.integer("steps", 20..=100_000).or(400)?;
    let most_points = p.integer("points", 2..=10_000).or(200)?;
    let (from, to) = crate::results::cavity_line(s, p)?;
    let span: Vec3 = to - from;
    let length = span.norm();
    let axis = span.normalized().ok_or_else(|| p.error("to", "must differ from `from`"))?;

    let driven = e.driven_of(m);
    let (x, f) = (&m.vector, m.frequency);
    let omega = 2.0 * std::f64::consts::PI * f;
    let species = ELECTRON;
    let u0 = species.momentum(energy * EV, v3(axis));
    let v0 = velocity(u0);
    let speed = (v0[0] * v0[0] + v0[1] * v0[1] + v0[2] * v0[2]).sqrt();
    let field_voltage = driven.line_integral(x, f, (from, to), INTERVALS, |_| C::new(1.0, 0.0))?;
    let felt = driven.line_integral(x, f, (from, to), INTERVALS, |z| C::from_polar(1.0, omega * z / speed))?;
    let v_own = field_voltage.norm();
    let energy_scale = driven.stored_energy(x, f)?.max(f64::MIN_POSITIVE).sqrt();
    if !(v_own / energy_scale > NO_FIELD && felt.norm() > 0.0) {
        return Err(EngineError::failed("the mode has no field along the line to accelerate with (a TE mode on the axis?)"));
    }
    let amplitude = C::new(voltage / v_own, 0.0) * species.charge.signum() * felt.conj() / felt.norm();
    let transit = felt.norm() / v_own;

    let dt = 1.0 / (f * per_period as f64);
    let transit_steps = (length / speed / dt).ceil();
    let max_steps = (4.0 * transit_steps + 2.0 * per_period as f64).min(emcraft_particles::MAX_STEPS as f64) as usize;
    if max_steps.saturating_mul(phases.len() * offsets.len()) > MAX_TOTAL_STEPS {
        return Err(EngineError::failed(format!(
            "too many steps: {} particles of up to {max_steps} steps each (at most {MAX_TOTAL_STEPS} in all): fewer phases or offsets, or fewer `steps` per period",
            phases.len() * offsets.len()
        )));
    }
    let every = ((transit_steps as usize) / most_points).max(1);
    let line = Line { origin: at(from), axis: v3(axis), length };
    let side = across(axis);
    let settings = Settings { pusher, dt, max_steps, every };
    let margin = 1e-9 * length;
    let show = |pt: V3| json!([s.show_length(pt[0]), s.show_length(pt[1]), s.show_length(pt[2])]);
    let mut particles = Vec::with_capacity(phases.len() * offsets.len());
    for &offset in &offsets {
        for &phase in &phases {
            let a = amplitude * C::from_polar(1.0, phase.to_radians());
            let fields = |pt: V3, t: f64| {
                let along = line.along(pt);
                if along <= margin || along >= length - margin {
                    return if static_b == [0.0; 3] { Sample::Free } else { Sample::Field([0.0; 3], static_b) };
                }
                let Some((ec, hc)) = driven.fields(x, f, Point3::new(pt[0], pt[1], pt[2])) else { return Sample::Wall };
                let w = a * C::from_polar(1.0, omega * t);
                let re = |c: C| (w * c).re;
                let e = [re(ec.x), re(ec.y), re(ec.z)];
                let b = [MU0 * re(hc.x) + static_b[0], MU0 * re(hc.y) + static_b[1], MU0 * re(hc.z) + static_b[2]];
                Sample::Field(e, b)
            };
            let start = Start { x: at(from + side * offset), u: u0, t: 0.0 };
            let run = push_through(species, start, &fields, line, settings).map_err(|e| EngineError::failed(e.to_string()))?;
            let out = species.kinetic(run.u) / EV;
            let points: Vec<Value> = run
                .points
                .iter()
                .map(|q| json!([s.show_length(q.x[0]), s.show_length(q.x[1]), s.show_length(q.x[2]), q.t * 1e9, species.kinetic(q.u) / EV]))
                .collect();
            let lost = match run.end {
                End::Lost(at) => Some(show(at)),
                _ => None,
            };
            particles.push(json!({
                "phase": phase,
                "offset": s.show_length(offset),
                "end": end_name(run.end),
                "lost_at": lost,
                "energy": out,
                "gain": out - energy,
                "expected": voltage * transit * phase.to_radians().cos(),
                "gamma": gamma(run.u),
                "steps": run.steps,
                "points": points,
            }));
        }
    }
    let reply = json!({
        "mode": index,
        "frequency": s.show_frequency(f),
        "from": show(at(from)),
        "to": show(at(to)),
        "voltage": voltage,
        "transit": transit,
        "energy": energy,
        "time_step": dt * 1e9,
        "pusher": match pusher { Pusher::Boris => "boris", Pusher::Vay => "vay" },
        "particles": particles,
    });
    s.views.track = Some(p.value().clone());
    Ok(reply)
}
