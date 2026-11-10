//! The far field of an open problem, a row per direction: a source's directivity, gains and
//! polarization, or a plane wave's radar cross-section, each with the closed form's where it
//! applies; from the frequency domain's solve or the time domain's.

use std::f64::consts::PI;

use emcraft_model::Vec3;
use emcraft_model::units::wavenumber;
use emcraft_solver::Complex64 as C;
use emcraft_solver::radiation::{FarField, OpenResult};
use serde::{Serialize, Serializer};
use serde_json::{Value, json};

use super::{ETA0, lone_dipole, lone_sphere, plane_wave};
use crate::{EngineError, Params, Result, Session};

/// One direction's far field, as `results.far_field` replies it: the angles (degrees), the
/// parts of r·e^{jkr}·E along θ̂ and φ̂ (V) and what it measures.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct FarRow {
    pub theta: f64,
    pub phi: f64,
    #[serde(serialize_with = "phasor")]
    pub e_theta: C,
    #[serde(serialize_with = "phasor")]
    pub e_phi: C,
    #[serde(flatten)]
    pub measure: Measure,
}

/// What a far-field row measures: a plane wave's scattering or a source's radiation.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub(crate) enum Measure {
    Rcs(Rcs),
    Antenna(Antenna),
}

/// A plane wave's radar cross-section: in the display's area, in dB of m², and the Mie
/// series' (display area) for a lone sphere.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct Rcs {
    pub rcs: f64,
    pub rcs_dbsm: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exact_rcs: Option<f64>,
}

/// A source's far field in one direction: its directivity, the gains when it has feeds, the
/// directivity's right- and left-hand circular and Ludwig-3 co- and cross-polar shares (dBi),
/// the polarization ellipse's axial ratio (dB; `None`, replied `null`, for linear) and sense,
/// and a lone dipole's closed form.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct Antenna {
    pub directivity: f64,
    pub directivity_dbi: f64,
    #[serde(flatten)]
    pub gain: Option<Gain>,
    pub directivity_rhcp_dbi: f64,
    pub directivity_lhcp_dbi: f64,
    pub directivity_co_dbi: f64,
    pub directivity_cross_dbi: f64,
    pub axial_ratio_db: Option<f64>,
    pub sense: Sense,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exact_directivity: Option<f64>,
    /// |r e^{jkr} E_θ| of the dipole (V).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exact_far_field: Option<f64>,
}

/// The gain (directivity times the radiation efficiency) and the realized gain (with the feeds'
/// mismatch counted too).
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub(crate) struct Gain {
    pub gain: f64,
    pub gain_dbi: f64,
    pub realized_gain: f64,
    pub realized_gain_dbi: f64,
}

/// The sense the polarization ellipse turns in (IEEE Std 145), or none for linear.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Sense {
    Right,
    Left,
    Linear,
}

fn phasor<S: Serializer>(c: &C, out: S) -> std::result::Result<S::Ok, S::Error> {
    crate::reply::phasor(*c).serialize(out)
}

fn dbi(x: f64) -> f64 {
    10.0 * x.log10()
}

impl Antenna {
    /// Directivity `d` in the direction of `ff`, its feeds' `gains`, and its polarization split
    /// about `co_polar` (radians from x̂).
    fn new(d: f64, ff: &FarField, gains: Option<Gains>, co_polar: f64) -> Antenna {
        let gain = gains.map(|g| Gain {
            gain: d * g.efficiency,
            gain_dbi: dbi(d * g.efficiency),
            realized_gain: d * g.realized,
            realized_gain_dbi: dbi(d * g.realized),
        });
        let total = ff.magnitude_squared();
        let share = |part: f64| if total > 0.0 { d * part / total } else { 0.0 };
        let (right, left) = ff.circular();
        let (co, cross) = ff.ludwig3(co_polar);
        let ar = ff.axial_ratio();
        let sense = match () {
            _ if !ar.is_finite() => Sense::Linear,
            _ if right.norm() > left.norm() => Sense::Right,
            _ => Sense::Left,
        };
        Antenna {
            directivity: d,
            directivity_dbi: dbi(d),
            gain,
            directivity_rhcp_dbi: dbi(share(right.norm_sqr())),
            directivity_lhcp_dbi: dbi(share(left.norm_sqr())),
            directivity_co_dbi: dbi(share(co.norm_sqr())),
            directivity_cross_dbi: dbi(share(cross.norm_sqr())),
            axial_ratio_db: ar.is_finite().then(|| 20.0 * ar.log10()),
            sense,
            exact_directivity: None,
            exact_far_field: None,
        }
    }
}

/// The closed form a far field is compared with, prepared once for all its directions.
enum Exact {
    /// A sphere a plane wave lights: k, the incidence and polarization, its Mie coefficients.
    Mie {
        k: f64,
        incidence: Vec3,
        polarization: Vec3,
        coefficients: Vec<(C, C)>,
    },
    /// The one dipole in an empty box: moment I·l and axis.
    Dipole {
        moment: f64,
        axis: Vec3,
    },
    /// The one dipole's directivity alone (its axis): the time domain's far field is not scaled
    /// to the moment.
    Directivity {
        axis: Vec3,
    },
    None,
}

impl Exact {
    fn of(s: &Session, frequency: f64) -> Exact {
        let k = wavenumber(frequency);
        if let Some((incidence, polarization)) = plane_wave(s) {
            return match lone_sphere(s) {
                Some((a, Some(er))) => {
                    Exact::Mie { k, incidence, polarization, coefficients: emcraft_theory::radiation::mie_coefficients(k * a, er.sqrt()) }
                }
                Some((a, None)) => Exact::Mie { k, incidence, polarization, coefficients: emcraft_theory::radiation::mie_coefficients_pec(k * a) },
                None => Exact::None,
            };
        }
        lone_dipole(s).map_or(Exact::None, |(moment, axis)| Exact::Dipole { moment, axis })
    }

    /// The Mie series' radar cross-section (m²) toward `dir`; `None` without it.
    fn rcs(&self, dir: Vec3) -> Option<f64> {
        let Exact::Mie { k, incidence, polarization, coefficients } = self else { return None };
        // The scattering angle from the incident direction, and the azimuth from its
        // polarization in the plane across it.
        let cos_t = incidence.dot(dir).clamp(-1.0, 1.0);
        let (pe, ph) = (dir.dot(*polarization), dir.dot(incidence.cross(*polarization)));
        let cphi2 = if pe * pe + ph * ph > 0.0 { pe * pe / (pe * pe + ph * ph) } else { 1.0 };
        let (s1, s2) = emcraft_theory::radiation::amplitudes(coefficients, cos_t.acos());
        Some(4.0 * PI * (s2.norm_sqr() * cphi2 + s1.norm_sqr() * (1.0 - cphi2)) / (k * k))
    }

    /// A lone dipole's directivity and |r e^{jkr} E_θ| (V, where known) toward `dir` at
    /// `frequency` (Hz).
    fn dipole(&self, dir: Vec3, frequency: f64) -> (Option<f64>, Option<f64>) {
        let (axis, moment) = match self {
            Exact::Dipole { moment, axis } => (axis, Some(*moment)),
            Exact::Directivity { axis } => (axis, None),
            Exact::Mie { .. } | Exact::None => return (None, None),
        };
        let cos_a = axis.dot(dir).clamp(-1.0, 1.0);
        let field = moment.map(|il| emcraft_theory::radiation::dipole_far_field(il, frequency, cos_a.acos()));
        (Some(1.5 * (1.0 - cos_a * cos_a)), field)
    }
}

/// What every row of one far field shares: scattering or not, its closed form, the power (W)
/// radiated or scattered, the feeds' gains, the co-polar direction (radians) and the display's
/// length per metre.
struct Reading {
    frequency: f64,
    power: f64,
    scattering: bool,
    exact: Exact,
    gains: Option<Gains>,
    co_polar: f64,
    metre: f64,
}

impl Reading {
    fn of(s: &Session, frequency: f64, power: f64, feeds: Option<Feeds>, co_polar: f64) -> Reading {
        Reading {
            frequency,
            power,
            scattering: plane_wave(s).is_some(),
            exact: Exact::of(s, frequency),
            gains: feeds.map(|f| f.gains(power)),
            co_polar,
            metre: s.display.length.from_si(1.0),
        }
    }

    fn row(&self, ff: &FarField) -> FarRow {
        let [dir, _, _] = Vec3::spherical(ff.theta, ff.phi);
        let measure = if self.scattering {
            let rcs = 4.0 * PI * ff.magnitude_squared();
            Measure::Rcs(Rcs {
                rcs: rcs * self.metre * self.metre,
                rcs_dbsm: dbi(rcs),
                exact_rcs: self.exact.rcs(dir).map(|x| x * self.metre * self.metre),
            })
        } else {
            let d = 4.0 * PI * ff.magnitude_squared() / (2.0 * ETA0 * self.power);
            let (exact_directivity, exact_far_field) = self.exact.dipole(dir, self.frequency);
            Measure::Antenna(Antenna { exact_directivity, exact_far_field, ..Antenna::new(d, ff, self.gains, self.co_polar) })
        };
        FarRow { theta: ff.theta.to_degrees(), phi: ff.phi.to_degrees(), e_theta: ff.e_theta, e_phi: ff.e_phi, measure }
    }
}

/// The polar angles `theta` (degrees; every 10° by default) and the azimuth `phi`.
fn angles(p: &Params) -> Result<(Vec<f64>, f64)> {
    let theta: Vec<f64> = match p.raw("theta") {
        None => (0..=18).map(|i| 10.0 * f64::from(i)).collect(),
        Some(Value::Array(list)) => {
            list.iter().map(|v| p.number_of("theta", v).ok()).collect::<Option<_>>().ok_or_else(|| p.error("theta", "must be numbers"))?
        }
        Some(_) => vec![p.number("theta").required()?],
    };
    if theta.is_empty() || theta.len() > 3601 {
        return Err(p.error("theta", "needs 1 to 3601 angles"));
    }
    Ok((theta, p.number("phi").or(0.0)?))
}

emcraft_macros::named_enum! {
    /// Which solver's far field to read.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum FarSolver {
        /// The frequency domain's (`solve.radiation`).
        Fem = "fem",
        /// The time domain's (`solve.time_domain` on an open model).
        TimeDomain = "time_domain",
    }
}

emcraft_macros::named_serde!(FarSolver);

/// The far field at one frequency in the directions asked: a row per direction.
pub(crate) struct FarRows {
    /// Hz.
    pub frequency: f64,
    /// The power (W) radiated or scattered.
    pub power: f64,
    pub rows: Vec<FarRow>,
}

/// What a far field is read from and how its polarization is split: the solver (the frequency
/// domain's, or the time domain's at the frequency nearest `frequency`, Hz) and the co-polar
/// direction at the pole (radians from x̂ toward ŷ) of Ludwig's third definition.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct FarAsk {
    pub solver: Option<FarSolver>,
    pub frequency: Option<f64>,
    pub co_polar: f64,
}

/// Far-field rows in the directions given (radians), from the frequency-domain solve or, when
/// asked (`solver` "time_domain") or the only one there, the time domain's.
pub(crate) fn far_rows(s: &Session, dirs: &[(f64, f64)], ask: &FarAsk) -> Result<FarRows> {
    let use_td = match ask.solver {
        Some(solver) => solver == FarSolver::TimeDomain,
        None => s.results.radiation.is_none() && s.results.td_radiation.is_some(),
    };
    if use_td { time_domain_far_rows(s, dirs, ask) } else { fem_far_rows(s, dirs, ask) }
}

/// The powers an antenna's gains are counted against: what its feeds deliver to it and what
/// their sources have available (what a matched antenna would take).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Feeds {
    accepted: f64,
    available: f64,
}

/// The ratios of `radiated` power to a [`Feeds`]' powers: radiation efficiency, and the
/// efficiency with the mismatch counted too.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Gains {
    efficiency: f64,
    realized: f64,
}

impl Feeds {
    fn gains(self, radiated: f64) -> Gains {
        let over = |p: f64| if p > 0.0 { radiated / p } else { f64::NAN };
        Gains { efficiency: over(self.accepted), realized: over(self.available) }
    }
}

/// The power incident on a feed that takes `accepted` and reflects `s11`: accepted / (1 − |S11|²);
/// unknown when it reflects (nearly) everything.
fn incident(accepted: f64, s11: C) -> f64 {
    let taken = 1.0 - s11.norm_sqr();
    if taken > 1e-12 { accepted / taken } else { f64::NAN }
}

/// The frequency-domain solve's feeds: its lumped ports (each source's available power
/// |V|²/8R) or its waveguide feed; `None` without either (a plane wave, current elements).
fn fem_feeds(s: &Session, r: &OpenResult) -> Option<Feeds> {
    if !r.ports.is_empty() {
        let accepted = r.ports.iter().map(|p| p.power).sum();
        let available =
            s.model.lumped.iter().map(|lp| (lp.voltage[0] * lp.voltage[0] + lp.voltage[1] * lp.voltage[1]) / (8.0 * lp.impedance.get())).sum();
        return Some(Feeds { accepted, available });
    }
    r.feed.map(|(s11, accepted)| Feeds { accepted, available: incident(accepted, s11) })
}

/// The azimuth φ (degrees, 0 to 345 by 15) of the strongest direction of the last open solve's
/// far field (directivity, or radar cross-section for a plane wave), over polar angles 5° to
/// 175° by 10°: where a θ-varying cut shows the main lobe (a patch's broadside, off its plane).
pub fn strongest_azimuth(s: &Session) -> Option<f64> {
    let dirs: Vec<(f64, f64)> =
        (0..24).flat_map(|j| (0..18).map(move |i| (f64::from(5 + 10 * i).to_radians(), f64::from(15 * j).to_radians()))).collect();
    let far = far_rows(s, &dirs, &FarAsk::default()).ok()?;
    let strength = |row: &FarRow| match &row.measure {
        Measure::Antenna(a) => a.directivity,
        Measure::Rcs(r) => r.rcs,
    };
    let best = far.rows.iter().zip(&dirs).map(|(row, d)| (strength(row), d.1)).filter(|b| b.0.is_finite()).max_by(|a, b| a.0.total_cmp(&b.0))?;
    Some(best.1.to_degrees().round())
}

/// The frequency-domain solve's far field.
fn fem_far_rows(s: &Session, dirs: &[(f64, f64)], ask: &FarAsk) -> Result<FarRows> {
    let r = s.results.radiation.clone().ok_or(EngineError::NotSolved { what: "open problem", run: "solve.radiation" })?;
    let f = r.result.frequency;
    let reading = Reading::of(s, f, r.power, fem_feeds(s, &r.result), ask.co_polar);
    let rows = emcraft_solver::radiation::far_fields(&r.huygens, dirs).iter().map(|ff| reading.row(ff)).collect();
    Ok(FarRows { frequency: f, power: r.power, rows })
}

/// The time domain's far field at its frequency nearest `frequency` (its first without one).
fn time_domain_far_rows(s: &Session, dirs: &[(f64, f64)], ask: &FarAsk) -> Result<FarRows> {
    let r = s.results.td_radiation.clone().ok_or(EngineError::NotSolved { what: "time-domain open problem", run: "solve.time_domain" })?;
    let fi = ask.frequency.and_then(|f| crate::numbers::nearest(&r.frequencies, f)).unwrap_or(0);
    let f = r.frequencies.get(fi).copied().ok_or_else(|| EngineError::failed("no frequencies"))?;
    let power = r.radiated.get(fi).copied().unwrap_or(f64::NAN);
    let mut reading = Reading::of(s, f, power, time_domain_feeds(s, &r, fi), ask.co_polar);
    if let Exact::Dipole { axis, .. } = reading.exact {
        reading.exact = Exact::Directivity { axis };
    }
    let rows = dirs
        .iter()
        .map(|&(theta, phi)| {
            let (e_theta, e_phi) = r.far_field(fi, theta, phi).unwrap_or_default();
            reading.row(&FarField { theta, phi, e_theta, e_phi })
        })
        .collect();
    Ok(FarRows { frequency: f, power, rows })
}

/// The time domain's lumped ports at frequency `fi`: the power each delivers and, from its
/// reflection against its impedance, the power its source has available.
fn time_domain_feeds(s: &Session, r: &emcraft_fdtd::open::OpenRun, fi: usize) -> Option<Feeds> {
    if r.ports.is_empty() {
        return None;
    }
    let mut feeds = Feeds { accepted: 0.0, available: 0.0 };
    for (pr, lp) in r.ports.iter().zip(&s.model.lumped) {
        let (Some(z), Some(power)) = (pr.impedance.get(fi), pr.power.get(fi)) else { continue };
        let z0 = lp.impedance.get();
        feeds.accepted += power;
        feeds.available += incident(*power, (*z - z0) / (*z + z0));
    }
    Some(feeds)
}

/// The far-field request's `co_polar` (degrees from x̂ toward ŷ; default 0), in radians.
pub(crate) fn co_polar(p: &Params) -> Result<f64> {
    Ok(p.number("co_polar").check(|a| (-360.0..=360.0).contains(a), "−360 to 360 degrees").or(0.0)?.to_radians())
}

pub fn results_far_field(s: &mut Session, p: &Params) -> Result<Value> {
    let (theta, phi) = angles(p)?;
    let dirs: Vec<(f64, f64)> = theta.iter().map(|t| (t.to_radians(), phi.to_radians())).collect();
    let ask = FarAsk { solver: p.choice("solver").optional()?, frequency: p.frequency("frequency").optional()?, co_polar: co_polar(p)? };
    let far = far_rows(s, &dirs, &ask)?;
    Ok(json!({"frequency": s.show_frequency(far.frequency), "power": far.power, "points": far.rows}))
}
