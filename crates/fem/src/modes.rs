//! The modes of a rectangular waveguide (x ∈ [0, a], y ∈ [0, b], perfectly conducting walls),
//! for port boundary conditions: transverse electric field patterns, cut-off wavenumbers,
//! propagation constants and the modal factor κ that relates n̂ × ∇ × E to the transverse field
//! of a mode leaving the port (D. M. Pozar, *Microwave Engineering*, 4th ed., Wiley 2012, §3.3;
//! J.-M. Jin, *The Finite Element Method in Electromagnetics*, 3rd ed., Wiley 2014, §8.5).
//!
//! With ψ = cos(mπx/a) cos(nπy/b) for TE and ψ = sin(mπx/a) sin(nπy/b) for TM and the cut-off
//! wavenumber k_c = √((mπ/a)² + (nπ/b)²):
//! - TE_mn: e = −(ẑ × ∇ψ)/k_c, so TE₁₀ is e = ŷ sin(πx/a);
//! - TM_mn: e = −∇ψ/k_c.
//!
//! A mode travels as e^{−γs} along the outward normal s, with γ = √(k_c² − k²) for an
//! evanescent mode and γ = j√(k² − k_c²) for a propagating one (time dependence e^{jωt}). For
//! it, n̂ × ∇ × E_t = κ E_t with κ = γ (TE) or κ = −k²/γ (TM), k² = ω²με.

use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Te,
    Tm,
}

/// One waveguide mode.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mode {
    pub kind: Kind,
    pub m: u32,
    pub n: u32,
    /// Broad wall a (x) and narrow wall b (y), m.
    pub a: f64,
    pub b: f64,
    /// The aperture's corner (m): the mode lives on [x0, x0 + a] × [y0, y0 + b].
    pub x0: f64,
    pub y0: f64,
}

impl Mode {
    pub fn te10(a: f64, b: f64) -> Mode {
        Mode { kind: Kind::Te, m: 1, n: 0, a, b, x0: 0.0, y0: 0.0 }
    }

    /// The cut-off wavenumber k_c (rad/m).
    pub fn cutoff(&self) -> f64 {
        let (kx, ky) = (self.m as f64 * PI / self.a, self.n as f64 * PI / self.b);
        (kx * kx + ky * ky).sqrt()
    }

    pub fn name(&self) -> String {
        format!("{}{}{}", if self.kind == Kind::Te { "TE" } else { "TM" }, self.m, self.n)
    }

    /// The same mode on an aperture whose corner is at (x0, y0).
    pub fn at(self, x0: f64, y0: f64) -> Mode {
        Mode { x0, y0, ..self }
    }

    /// The transverse field e(x, y) (global x, y; the aperture from its corner (x0, y0)).
    pub fn field(&self, x: f64, y: f64) -> [f64; 2] {
        let (x, y) = (x - self.x0, y - self.y0);
        let (kx, ky) = (self.m as f64 * PI / self.a, self.n as f64 * PI / self.b);
        let kc = self.cutoff();
        let (sx, cx) = (kx * x).sin_cos();
        let (sy, cy) = (ky * y).sin_cos();
        match self.kind {
            // ψ = cos cos: ∇ψ = (−kx sx cy, −ky cx sy); −ẑ × ∇ψ = (∂ψ/∂y, −∂ψ/∂x).
            Kind::Te => [-ky * cx * sy / kc, kx * sx * cy / kc],
            // ψ = sin sin: −∇ψ = (−kx cx sy, −ky sx cy).
            Kind::Tm => [-kx * cx * sy / kc, -ky * sx * cy / kc],
        }
    }

    /// ∫ |e|² over the cross-section.
    pub fn norm(&self) -> f64 {
        match self.kind {
            Kind::Te => {
                let f = |k: u32, l: f64| if k == 0 { l } else { 0.5 * l };
                f(self.m, self.a) * f(self.n, self.b)
            }
            Kind::Tm => 0.25 * self.a * self.b,
        }
    }

    /// The propagation constant γ for a wavenumber k: `(re, im)`, γ = re + j·im.
    pub fn gamma(&self, k: f64) -> (f64, f64) {
        let kc = self.cutoff();
        let d = kc * kc - k * k;
        if d >= 0.0 { (d.sqrt(), 0.0) } else { (0.0, (-d).sqrt()) }
    }

    /// κ with n̂ × ∇ × E_t = κ E_t for this mode leaving the port: `(re, im)`. `None` at cut-off,
    /// where the port condition is singular.
    pub fn kappa(&self, k: f64) -> Option<(f64, f64)> {
        let (gr, gi) = self.gamma(k);
        let g2 = gr * gr + gi * gi;
        if !(g2 > 1e-20 * k * k) {
            return None;
        }
        match self.kind {
            Kind::Te => Some((gr, gi)),
            // −k²/γ = −k² γ* / |γ|².
            Kind::Tm => Some((-k * k * gr / g2, k * k * gi / g2)),
        }
    }
}

/// The `count` modes of an a × b guide with the lowest cut-offs (TE before TM at equal
/// cut-off, then by m and n).
pub fn lowest(a: f64, b: f64, count: usize) -> Vec<Mode> {
    let count = count.min(256);
    let mut all = Vec::new();
    // Enough indices to hold the `count` lowest of each family.
    let span = (count as u32 + 1).min(64);
    for m in 0..=span {
        for n in 0..=span {
            if m + n == 0 {
                continue;
            }
            all.push(Mode { kind: Kind::Te, m, n, a, b, x0: 0.0, y0: 0.0 });
            if m > 0 && n > 0 {
                all.push(Mode { kind: Kind::Tm, m, n, a, b, x0: 0.0, y0: 0.0 });
            }
        }
    }
    all.sort_by(|p, q| {
        p.cutoff().total_cmp(&q.cutoff()).then((p.kind == Kind::Tm).cmp(&(q.kind == Kind::Tm))).then(p.m.cmp(&q.m)).then(p.n.cmp(&q.n))
    });
    all.truncate(count);
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn te10_is_the_dominant_mode() {
        let (a, b) = (0.02286, 0.01016);
        let modes = lowest(a, b, 5);
        let names: Vec<String> = modes.iter().map(Mode::name).collect();
        assert_eq!(names, ["TE10", "TE20", "TE01", "TE11", "TM11"]);
        let te10 = modes[0];
        assert!((te10.cutoff() - PI / a).abs() < 1e-12);
        let e = te10.field(a / 2.0, 0.3 * b);
        assert!(e[0].abs() < 1e-15 && (e[1] - 1.0).abs() < 1e-15);
        let shifted = te10.at(0.005, 0.0);
        assert!((shifted.field(0.005 + a / 2.0, 0.1 * b)[1] - 1.0).abs() < 1e-15);
    }

    #[test]
    fn modes_are_orthogonal_with_their_norms() {
        let (a, b) = (0.02, 0.012);
        let modes = lowest(a, b, 8);
        let (x, w) = crate::quadrature::gauss_legendre(24);
        for p in &modes {
            for q in &modes {
                let mut s = 0.0;
                for (xi, wi) in x.iter().zip(&w) {
                    for (yi, wj) in x.iter().zip(&w) {
                        let (ep, eq) = (p.field(xi * a, yi * b), q.field(xi * a, yi * b));
                        s += wi * wj * a * b * (ep[0] * eq[0] + ep[1] * eq[1]);
                    }
                }
                let expect = if p == q { p.norm() } else { 0.0 };
                assert!((s - expect).abs() < 1e-12 * a * b, "{} {}: {s} {expect}", p.name(), q.name());
            }
        }
    }

    #[test]
    fn tangential_field_vanishes_on_the_walls() {
        let (a, b) = (0.02, 0.012);
        for m in lowest(a, b, 10) {
            for t in [0.1, 0.5, 0.77] {
                // E_x on y = 0, b and E_y on x = 0, a.
                assert!(m.field(t * a, 0.0)[0].abs() < 1e-12 && m.field(t * a, b)[0].abs() < 1e-12, "{}", m.name());
                assert!(m.field(0.0, t * b)[1].abs() < 1e-12 && m.field(a, t * b)[1].abs() < 1e-12, "{}", m.name());
            }
        }
    }

    #[test]
    fn propagation_constants() {
        let m = Mode::te10(0.02, 0.01);
        let kc = m.cutoff();
        let (gr, gi) = m.gamma(2.0 * kc);
        assert!(gr == 0.0 && (gi - 3f64.sqrt() * kc).abs() < 1e-9);
        let (gr, gi) = m.gamma(0.5 * kc);
        assert!(gi == 0.0 && (gr - 0.75f64.sqrt() * kc).abs() < 1e-9);
        assert!(m.kappa(kc).is_none());
        let tm = Mode { kind: Kind::Tm, m: 1, n: 1, a: 0.02, b: 0.01, x0: 0.0, y0: 0.0 };
        let k = 2.0 * tm.cutoff();
        let (kr, ki) = tm.kappa(k).unwrap();
        // −k²/(jβ) = j k²/β.
        let beta = (k * k - tm.cutoff().powi(2)).sqrt();
        assert!(kr.abs() < 1e-9 && (ki - k * k / beta).abs() < 1e-6 * ki);
    }
}
