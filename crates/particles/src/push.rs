//! One time step of a charged particle's momentum in E and B: the relativistic Boris push and
//! Vay's.
//!
//! The momentum is carried as u = γv (m/s), at the half steps between the positions (leapfrog):
//! [`push`] takes u at t − Δt/2 to t + Δt/2 with the fields at t.
//!
//! **Boris** (J. P. Boris, Proc. 4th Conf. Num. Sim. Plasmas, 1970; C. K. Birdsall, A. B.
//! Langdon, *Plasma Physics via Computer Simulation*, 1991, §15-4): half the electric kick,
//! a rotation about B by the angle the magnetic force turns u through in Δt at the γ after
//! the kick, the other half kick. The rotation keeps |u|, so a magnetic field alone does no
//! work, to rounding.
//!
//! **Vay** (J.-L. Vay, *Simulation of beams or plasmas crossing at relativistic velocity*, Phys.
//! Plasmas 15:056701, 2008, eqs. 9–13): the magnetic force taken at the new velocity's γ,
//! solved in closed form, so that fields whose forces cancel (E = −v×B, a relativistic beam's
//! own fields in its frame of motion) leave the particle on its line at any γ, which Boris
//! does not.

use emcraft_model::units::C0;

/// A vector in metres, metres per second, volts per metre or teslas, by context.
pub type V3 = [f64; 3];

pub(crate) fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub(crate) fn scale(a: V3, k: f64) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}

pub(crate) fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub(crate) fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// A kind of particle: its charge (C) and rest mass (kg).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Species {
    pub charge: f64,
    pub mass: f64,
}

/// The electron (CODATA 2018: the elementary charge exact, mₑ).
pub const ELECTRON: Species = Species { charge: -1.602_176_634e-19, mass: 9.109_383_701_5e-31 };

impl Species {
    /// Its rest energy mc² (J).
    pub fn rest_energy(self) -> f64 {
        self.mass * C0 * C0
    }

    /// The momentum u = γv (m/s) of kinetic energy `kinetic` (J) along unit vector `along`.
    ///
    /// ```
    /// use emcraft_particles::{ELECTRON, gamma};
    /// let mev = 1e6 * 1.602_176_634e-19;
    /// let u = ELECTRON.momentum(mev, [0.0, 0.0, 1.0]);
    /// // γ = 1 + T/(mc²): 1 MeV on 0.511 MeV of rest energy.
    /// assert!((gamma(u) - (1.0 + mev / ELECTRON.rest_energy())).abs() < 1e-12);
    /// ```
    pub fn momentum(self, kinetic: f64, along: V3) -> V3 {
        let g = 1.0 + kinetic / self.rest_energy();
        let speed = C0 * (g * g - 1.0).max(0.0).sqrt();
        let n = dot(along, along).sqrt();
        if n > 0.0 { scale(along, speed / n) } else { [0.0; 3] }
    }

    /// The kinetic energy (γ − 1)mc² (J) of momentum `u` (m/s), accurate for small speeds too.
    pub fn kinetic(self, u: V3) -> f64 {
        let u2 = dot(u, u) / (C0 * C0);
        // γ − 1 = u²/(γ + 1): no cancellation at small u.
        self.rest_energy() * u2 / (gamma(u) + 1.0)
    }
}

/// γ of momentum `u` = γv (m/s).
pub fn gamma(u: V3) -> f64 {
    (1.0 + dot(u, u) / (C0 * C0)).sqrt()
}

/// The velocity (m/s) of momentum `u`.
pub fn velocity(u: V3) -> V3 {
    scale(u, 1.0 / gamma(u))
}

/// The step's scheme.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Pusher {
    #[default]
    Boris,
    Vay,
}

/// `u` (m/s) at t − Δt/2 taken to t + Δt/2 by E (V/m) and B (T) at t, for particles of
/// `species`.
///
/// ```
/// use emcraft_particles::{ELECTRON, Pusher, gamma, push};
/// // A magnetic field alone turns u without changing |u|.
/// let u = [1e8, 0.0, 0.0];
/// let turned = push(Pusher::Boris, ELECTRON, u, [0.0; 3], [0.0, 0.0, 0.1], 1e-12);
/// assert!((gamma(turned) - gamma(u)).abs() < 1e-15 && turned[1] != 0.0);
/// ```
pub fn push(pusher: Pusher, species: Species, u: V3, e: V3, b: V3, dt: f64) -> V3 {
    let k = species.charge * dt / species.mass;
    match pusher {
        Pusher::Boris => {
            let minus = add(u, scale(e, 0.5 * k));
            let t = scale(b, 0.5 * k / gamma(minus));
            let prime = add(minus, cross(minus, t));
            let s = scale(t, 2.0 / (1.0 + dot(t, t)));
            add(add(minus, cross(prime, s)), scale(e, 0.5 * k))
        }
        Pusher::Vay => {
            let v = velocity(u);
            let prime = add(u, scale(add(e, scale(cross(v, b), 0.5)), k));
            let tau = scale(b, 0.5 * k);
            let tau2 = dot(tau, tau);
            let ustar = dot(prime, tau) / C0;
            let sigma = gamma(prime).powi(2) - tau2;
            let g = (0.5 * (sigma + (sigma * sigma + 4.0 * (tau2 + ustar * ustar)).sqrt())).sqrt();
            let t = scale(tau, 1.0 / g);
            let s = 1.0 / (1.0 + dot(t, t));
            scale(add(add(prime, scale(t, dot(prime, t))), cross(prime, t)), s)
        }
    }
}
