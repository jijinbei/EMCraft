//! Ports on the axis of a rotationally symmetric model, solved on its sector.
//!
//! **Rotating modes.** A circular guide's mode of azimuthal order n ≥ 1 comes in two standing
//! polarizations, cos nφ and sin nφ; their combinations e^{∓jnφ} rotate. A sector tied by
//! e^{−j2πm/n_s} ([`emcraft_model::Rotation`]) holds exactly the fields going as e^{−jνφ} with
//! ν ≡ m (mod n_s), so its port on an end cap centred on the axis takes the rotating modes of
//! those orders ν (positive: e^{−j|ν|φ}, negative: e^{+j|ν|φ}): TE_{|ν|p} from
//! H_z ∝ J_|ν|(k_c ρ) e^{−jνφ} (k_c = x′_{|ν|p}/a) and TM_{|ν|p} from E_z ∝ J_|ν|(k_c ρ) e^{−jνφ}
//! (k_c = x_{|ν|p}/a) (D. M. Pozar, *Microwave Engineering*, 4th ed., §3.4; N. Marcuvitz,
//! *Waveguide Handbook*, 1951, §2.3). Built from the standing modes of
//! [`emcraft_fem::modes`]: e_TE,ν = e_c ± j e_s, e_TM,ν = e_c ∓ j e_s (upper sign ν > 0), so
//! ‖e‖² over the whole cap is the two polarizations' sum and over the sector 1/n_s of it.
//!
//! **The port condition** is the Floquet ports' ([`super::floquet`]): the modes are complex, so
//! the coefficients take the conjugate product c = ∫ ē · E dS/‖e‖², the rank-one terms
//! (κ/μr‖e‖²) v v̄ᵀ border the reduced (Hermitian-tied) matrix, and the S-parameters are read
//! from the coefficients normalised to the modes' powers. Each harmonic m gives the S of its
//! own modes: the whole model's S is block-diagonal in them (J.-M. Jin, *The Finite Element
//! Method in Electromagnetics*, 3rd ed., Wiley 2014, cyclic symmetry).

use emcraft_fem::modes::{self, Aperture, Kind, Mode};
use emcraft_model::{Model, Port, PortShape, Rotation};
use num_complex::Complex64 as C;

use super::assembly::Space;
use super::floquet::FloquetTerm;
use super::ports::for_each_port_point;
use super::{Result, invalid, wavenumber};

/// The standing modes searched for the rotating ones (both polarizations of each order).
const CANDIDATES: usize = 64;

/// A port on an end cap centred on the axis of a sector's rotation: its medium, faces, the
/// circle and the rotating modes it takes.
#[derive(Clone, Debug)]
pub struct CyclicPort {
    pub eps_r: f64,
    pub mu_r: f64,
    pub(super) faces: Vec<usize>,
    pub modes: Vec<RotatingMode>,
    /// The sectors in a turn: a mode's norm over the sector is 1/n of the whole cap's.
    sectors: u32,
}

/// A circular guide's mode going as e^{−jνφ}.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RotatingMode {
    /// The standing cos-polarized mode of order |ν| (its sin partner is the same but `odd`).
    pub standing: Mode,
    pub nu: i32,
}

impl RotatingMode {
    /// TE11+ (e^{−jφ}), TE11− (e^{+jφ}), TM01 (ν = 0).
    pub fn name(&self) -> String {
        let kind = if self.standing.kind == Kind::Te { "TE" } else { "TM" };
        let sign = match self.nu.signum() {
            1 => "+",
            -1 => "−",
            _ => "",
        };
        format!("{kind}{}{}{sign}", self.standing.n, self.standing.m)
    }

    /// The transverse field (x, y) at a point, complex.
    pub fn field(&self, x: f64, y: f64) -> [C; 2] {
        let c = self.standing.field(x, y);
        if self.nu == 0 {
            return [C::new(c[0], 0.0), C::new(c[1], 0.0)];
        }
        let odd = match self.standing.aperture {
            Aperture::Circle { r, cx, cy, kc, .. } => Mode { aperture: Aperture::Circle { r, cx, cy, kc, odd: true }, ..self.standing },
            Aperture::Rect { .. } => self.standing,
        };
        let s = odd.field(x, y);
        // TE: e_c + j e_s goes as e^{−jnφ}; TM: e_c − j e_s (the standing modes' signs).
        let j = if (self.standing.kind == Kind::Te) == (self.nu > 0) { 1.0 } else { -1.0 };
        [C::new(c[0], j * s[0]), C::new(c[1], j * s[1])]
    }

    /// ∫ |e|² over the whole cap.
    fn norm(&self) -> f64 {
        if self.nu == 0 { self.standing.norm() } else { 2.0 * self.standing.norm() }
    }
}

/// The `count` lowest rotating modes of a circle of radius `r` about (cx, cy) whose order
/// ν ≡ m (mod n): by cut-off, TE before TM, then |ν|, then ν > 0 first.
///
/// ```
/// use emcraft_solver::driven::cyclic::rotating_modes;
///
/// let names = |m| rotating_modes(0.01, [0.0, 0.0], 4, m, 3).iter().map(|md| md.name()).collect::<Vec<_>>();
/// assert_eq!(names(0), ["TM01", "TE01", "TE41+"]); // ν = 0, ±4, …
/// assert_eq!(names(1), ["TE11+", "TM11+", "TE31−"]);
/// assert_eq!(names(3), ["TE11−", "TM11−", "TE31+"]);
/// ```
pub fn rotating_modes(r: f64, centre: [f64; 2], n: u32, m: u32, count: usize) -> Vec<RotatingMode> {
    let n = i64::from(n.max(1));
    let fits = |nu: i64| (nu - i64::from(m)).rem_euclid(n) == 0;
    let mut out: Vec<RotatingMode> = Vec::new();
    for standing in modes::lowest_circular(r, CANDIDATES) {
        if matches!(standing.aperture, Aperture::Circle { odd: true, .. }) {
            continue;
        }
        let order = i64::from(standing.n);
        let standing = standing.at(centre[0], centre[1]);
        let signs: &[i64] = if order == 0 { &[1] } else { &[1, -1] };
        out.extend(signs.iter().map(|s| s * order).filter(|&nu| fits(nu)).map(|nu| RotatingMode { standing, nu: nu as i32 }));
    }
    // `lowest_circular` is by cut-off, TE first, then the radial index: keep it, |ν| next.
    out.sort_by(|a, b| {
        a.standing
            .cutoff()
            .total_cmp(&b.standing.cutoff())
            .then((a.standing.kind == Kind::Tm).cmp(&(b.standing.kind == Kind::Tm)))
            .then(a.nu.unsigned_abs().cmp(&b.nu.unsigned_abs()))
            .then(b.nu.cmp(&a.nu))
    });
    out.truncate(count);
    out
}

/// Port `p` of a model solved on a sector of `rot`, on its end cap `faces`.
pub(super) fn prepare(model: &Model, rot: &Rotation, p: &Port, faces: &[usize]) -> Result<CyclicPort> {
    let what = || format!("port `{}`", p.name);
    if p.face.is_some() {
        return Err(invalid(what(), "not supported yet: on a sector, a port on a face across the axis other than an end of the solid"));
    }
    if p.deembed != 0.0 || p.reference.is_some() {
        return Err(invalid(what(), "not supported yet: de-embedding or a reference impedance on a sector's port"));
    }
    if p.mode_solver == emcraft_model::ModeSolver::Numeric {
        return Err(invalid(what(), "not supported yet: the cross-section solver on a sector (its modes are a circle's closed forms)"));
    }
    let PortShape::Circle { cx, cy, r } = model.port_shape_of(p)? else {
        return Err(invalid(what(), "not supported yet: on a sector, a port must be a circle centred on the axis (a cylinder's end)"));
    };
    let tol = 1e-9 * r;
    if (cx - rot.centre[0]).abs() > tol || (cy - rot.centre[1]).abs() > tol {
        return Err(invalid(what(), "its circle is not centred on the rotation's axis"));
    }
    let m = model.port_material(p.end)?;
    if m.is_lossy() {
        return Err(invalid(what(), "not supported yet: a sector's port in a lossy or dispersive medium"));
    }
    let modes = rotating_modes(r, rot.centre, rot.n, rot.m, p.modes as usize);
    if modes.is_empty() {
        return Err(invalid(what(), format!("no mode of the circle has harmonic {} of {}", rot.m, rot.n)));
    }
    Ok(CyclicPort { eps_r: m.eps_r.get(), mu_r: m.mu_r.get(), faces: faces.to_vec(), modes, sectors: rot.n })
}

impl CyclicPort {
    /// The modes' terms at `frequency`, on the full unknowns.
    pub(super) fn terms(&self, space: &Space, frequency: f64) -> Result<Vec<FloquetTerm>> {
        let k = wavenumber(frequency) * (self.eps_r * self.mu_r).sqrt();
        let mut acc: Vec<std::collections::BTreeMap<usize, C>> = vec![Default::default(); self.modes.len()];
        for_each_port_point(*space, &self.faces, emcraft_model::Frame::new(emcraft_model::Axis::Z), |x, wa, traced| {
            for (md, map) in self.modes.iter().zip(acc.iter_mut()) {
                let [ex, ey] = md.field(x.x, x.y);
                for &(u, v) in traced {
                    *map.entry(u).or_default() += (ex * v.x + ey * v.y) * wa;
                }
            }
        })?;
        self.modes
            .iter()
            .zip(acc)
            .map(|(md, v)| {
                let (re, im) =
                    md.standing.kappa(k).ok_or_else(|| invalid("frequency", format!("{frequency} Hz is at the cut-off of mode {}", md.name())))?;
                let kappa = C::new(re, im) / self.mu_r;
                let norm = md.norm() / f64::from(self.sectors);
                let propagating = md.standing.cutoff() < k;
                Ok(FloquetTerm {
                    name: md.name(),
                    v: v.into_iter().collect(),
                    factor: kappa / norm,
                    rhs: 2.0 * kappa,
                    norm,
                    proxy: if propagating { norm * kappa.im } else { 0.0 },
                    propagating,
                })
            })
            .collect()
    }
}
