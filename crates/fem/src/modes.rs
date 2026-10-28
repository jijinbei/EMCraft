//! The modes of waveguide ports with perfectly conducting walls — rectangular (x ∈ [x0, x0 + a],
//! y ∈ [y0, y0 + b]) and circular (radius r about (cx, cy)) — for port boundary conditions:
//! transverse electric field patterns, cut-off wavenumbers, propagation constants and the modal
//! factor κ that relates n̂ × ∇ × E to the transverse field of a mode leaving the port (D. M.
//! Pozar, *Microwave Engineering*, 4th ed., Wiley 2012, §3.3–3.4; J.-M. Jin, *The Finite Element
//! Method in Electromagnetics*, 3rd ed., Wiley 2014, §8.5).
//!
//! Each mode has a potential ψ with ∇²ψ + k_c²ψ = 0: in the rectangle ψ = cos(mπx/a) cos(nπy/b)
//! for TE and sin sin for TM, k_c = √((mπ/a)² + (nπ/b)²); in the circle ψ = J_n(k_c ρ) cos(nφ)
//! (or sin(nφ), the other polarization) with k_c = p'_nm/r (TE, the m-th zero of J_n') or
//! p_nm/r (TM, of J_n). Then
//! - TE: e = −(ẑ × ∇ψ)/k_c, so the rectangle's TE₁₀ is e = ŷ sin(πx/a);
//! - TM: e = −∇ψ/k_c.
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

/// A port's cross-section.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Aperture {
    /// Broad wall a (x) and narrow wall b (y), m, the corner at (x0, y0).
    Rect { a: f64, b: f64, x0: f64, y0: f64 },
    /// Radius r about (cx, cy), m; `kc` the mode's cut-off, `odd` the sin(nφ) polarization.
    Circle { r: f64, cx: f64, cy: f64, kc: f64, odd: bool },
}

/// One waveguide mode: `m`, `n` are the rectangle's indices along x and y, or the circle's
/// azimuthal order n and radial index m (named TE_nm as usual).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mode {
    pub kind: Kind,
    pub m: u32,
    pub n: u32,
    pub aperture: Aperture,
}

/// J_n(x)/x, finite at x = 0.
fn j_over_x(n: usize, x: f64) -> f64 {
    if x.abs() < 1e-8 {
        return if n == 1 { 0.5 } else { 0.0 };
    }
    crate::bessel::j(n, x) / x
}

impl Mode {
    pub fn te10(a: f64, b: f64) -> Mode {
        Mode { kind: Kind::Te, m: 1, n: 0, aperture: Aperture::Rect { a, b, x0: 0.0, y0: 0.0 } }
    }

    /// The cut-off wavenumber k_c (rad/m).
    pub fn cutoff(&self) -> f64 {
        match self.aperture {
            Aperture::Rect { a, b, .. } => {
                let (kx, ky) = (self.m as f64 * PI / a, self.n as f64 * PI / b);
                (kx * kx + ky * ky).sqrt()
            }
            Aperture::Circle { kc, .. } => kc,
        }
    }

    pub fn name(&self) -> String {
        let kind = if self.kind == Kind::Te { "TE" } else { "TM" };
        match self.aperture {
            Aperture::Rect { .. } => format!("{kind}{}{}", self.m, self.n),
            Aperture::Circle { odd, .. } => format!(
                "{kind}{}{}{}",
                self.n,
                self.m,
                if self.n == 0 {
                    ""
                } else if odd {
                    "s"
                } else {
                    "c"
                }
            ),
        }
    }

    /// The same mode on an aperture placed at (x0, y0): the rectangle's corner or the circle's
    /// centre.
    pub fn at(self, x0: f64, y0: f64) -> Mode {
        let aperture = match self.aperture {
            Aperture::Rect { a, b, .. } => Aperture::Rect { a, b, x0, y0 },
            Aperture::Circle { r, kc, odd, .. } => Aperture::Circle { r, cx: x0, cy: y0, kc, odd },
        };
        Mode { aperture, ..self }
    }

    /// The transverse field e(x, y) (global x, y).
    pub fn field(&self, x: f64, y: f64) -> [f64; 2] {
        let kc = self.cutoff();
        // ∇ψ, then TE: −ẑ × ∇ψ = (∂ψ/∂y, −∂ψ/∂x); TM: −∇ψ.
        let grad = match self.aperture {
            Aperture::Rect { a, b, x0, y0 } => {
                let (x, y) = (x - x0, y - y0);
                let (kx, ky) = (self.m as f64 * PI / a, self.n as f64 * PI / b);
                let (sx, cx) = (kx * x).sin_cos();
                let (sy, cy) = (ky * y).sin_cos();
                match self.kind {
                    Kind::Te => [-kx * sx * cy, -ky * cx * sy],
                    Kind::Tm => [kx * cx * sy, ky * sx * cy],
                }
            }
            Aperture::Circle { cx, cy, odd, .. } => {
                let (x, y) = (x - cx, y - cy);
                let rho = (x * x + y * y).sqrt();
                let phi = y.atan2(x);
                let n = self.n as usize;
                let (s, c) = (self.n as f64 * phi).sin_cos();
                // The even polarization is −J_n cos(nφ) for TE (its field along +y at the
                // centre, like the rectangle's TE₁₀) and J_n cos(nφ) for TM; the odd one J_n sin(nφ).
                let (ang, dang, sign) = if odd { (s, c, 1.0) } else { (c, -s, if self.kind == Kind::Te { -1.0 } else { 1.0 }) };
                let x_ = kc * rho;
                // ∂ψ/∂ρ and (1/ρ) ∂ψ/∂φ.
                let dr = sign * kc * crate::bessel::jp(n, x_) * ang;
                let dphi = sign * self.n as f64 * kc * j_over_x(n, x_) * dang;
                let (sp, cp) = if rho > 0.0 { (y / rho, x / rho) } else { (0.0, 1.0) };
                // At the centre only n = 1 has a gradient, along φ = 0 (cos) or φ = π/2 (sin).
                if rho == 0.0 {
                    let g = if n == 1 { sign * kc * 0.5 } else { 0.0 };
                    if odd { [0.0, g] } else { [g, 0.0] }
                } else {
                    [dr * cp - dphi * sp, dr * sp + dphi * cp]
                }
            }
        };
        match self.kind {
            Kind::Te => [grad[1] / kc, -grad[0] / kc],
            Kind::Tm => [-grad[0] / kc, -grad[1] / kc],
        }
    }

    /// ∫ |e|² over the cross-section (= ∫ ψ² for both kinds, by Green's identity with ψ or
    /// ∂ψ/∂n zero on the wall).
    pub fn norm(&self) -> f64 {
        match self.aperture {
            Aperture::Rect { a, b, .. } => match self.kind {
                Kind::Te => {
                    let f = |k: u32, l: f64| if k == 0 { l } else { 0.5 * l };
                    f(self.m, a) * f(self.n, b)
                }
                Kind::Tm => 0.25 * a * b,
            },
            Aperture::Circle { r, kc, .. } => {
                // ∫₀^r J_n(kρ)² ρ dρ = r²/2 [J_n'(kr)² + (1 − n²/(kr)²) J_n(kr)²] (Abramowitz &
                // Stegun 11.4.5), times ∫ cos² or sin² of nφ: π, or 2π for n = 0.
                let n = self.n as usize;
                let x = kc * r;
                let radial = 0.5 * r * r * (crate::bessel::jp(n, x).powi(2) + (1.0 - (n * n) as f64 / (x * x)) * crate::bessel::j(n, x).powi(2));
                radial * if n == 0 { 2.0 * PI } else { PI }
            }
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

impl Mode {
    /// γ for a complex k² = ω²μ_cε_c (a lossy or dispersive filling): γ = √(k_c² − k²) on the
    /// branch Re γ ≥ 0 (decaying along the outward normal), so γ = α + jβ with α > 0 once the
    /// medium absorbs.
    pub fn gamma_lossy(&self, k2: num_complex::Complex64) -> num_complex::Complex64 {
        let g = (self.cutoff().powi(2) - k2).sqrt();
        if g.re < 0.0 || (g.re == 0.0 && g.im < 0.0) { -g } else { g }
    }

    /// κ for a complex k²: γ (TE) or −k²/γ (TM). `None` at cut-off, where γ = 0.
    pub fn kappa_lossy(&self, k2: num_complex::Complex64) -> Option<num_complex::Complex64> {
        let g = self.gamma_lossy(k2);
        if !(g.norm_sqr() > 1e-20 * k2.norm()) {
            return None;
        }
        Some(match self.kind {
            Kind::Te => g,
            Kind::Tm => -k2 / g,
        })
    }
}

fn order(p: &Mode, q: &Mode) -> std::cmp::Ordering {
    p.cutoff().total_cmp(&q.cutoff()).then((p.kind == Kind::Tm).cmp(&(q.kind == Kind::Tm))).then(p.m.cmp(&q.m)).then(p.n.cmp(&q.n))
}

/// The `count` modes of an a × b guide with the lowest cut-offs (TE before TM at equal
/// cut-off, then by m and n).
pub fn lowest(a: f64, b: f64, count: usize) -> Vec<Mode> {
    let count = count.min(256);
    let mut all = Vec::new();
    // Enough indices to hold the `count` lowest of each family.
    let span = (count as u32 + 1).min(64);
    let aperture = Aperture::Rect { a, b, x0: 0.0, y0: 0.0 };
    for m in 0..=span {
        for n in 0..=span {
            if m + n == 0 {
                continue;
            }
            all.push(Mode { kind: Kind::Te, m, n, aperture });
            if m > 0 && n > 0 {
                all.push(Mode { kind: Kind::Tm, m, n, aperture });
            }
        }
    }
    all.sort_by(order);
    all.truncate(count);
    all
}

/// The `count` modes of a circular guide of radius r with the lowest cut-offs, both
/// polarizations of each n ≥ 1 (the even one first), centred at the origin.
pub fn lowest_circular(r: f64, count: usize) -> Vec<Mode> {
    let count = count.min(64);
    // All zeros below x_max; widened until it holds `count` modes.
    let mut x_max = 6.0;
    loop {
        let mut all = Vec::new();
        for n in 0..(x_max as u32 + 1) {
            let nu = n as usize;
            for (kind, z) in [
                (Kind::Te, crate::bessel::zeros(|x| crate::bessel::jp(nu, x), 64, x_max)),
                (Kind::Tm, crate::bessel::zeros(|x| crate::bessel::j(nu, x), 64, x_max)),
            ] {
                for (m, x) in z.into_iter().enumerate() {
                    for odd in if n == 0 { &[false][..] } else { &[false, true][..] } {
                        all.push(Mode { kind, m: m as u32 + 1, n, aperture: Aperture::Circle { r, cx: 0.0, cy: 0.0, kc: x / r, odd: *odd } });
                    }
                }
            }
        }
        if all.len() >= count || x_max > 200.0 {
            all.sort_by(|p, q| {
                let odd = |m: &Mode| matches!(m.aperture, Aperture::Circle { odd: true, .. });
                order(p, q).then(odd(p).cmp(&odd(q)))
            });
            all.truncate(count);
            return all;
        }
        x_max *= 2.0;
    }
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
        let tm = Mode { kind: Kind::Tm, m: 1, n: 1, aperture: Aperture::Rect { a: 0.02, b: 0.01, x0: 0.0, y0: 0.0 } };
        let k = 2.0 * tm.cutoff();
        let (kr, ki) = tm.kappa(k).unwrap();
        // −k²/(jβ) = j k²/β.
        let beta = (k * k - tm.cutoff().powi(2)).sqrt();
        assert!(kr.abs() < 1e-9 && (ki - k * k / beta).abs() < 1e-6 * ki);
    }

    #[test]
    fn lossy_propagation_constants_decay_and_reduce_to_the_lossless_ones() {
        use num_complex::Complex64 as C;
        let m = Mode::te10(0.02, 0.01);
        let kc = m.cutoff();
        // Lossless k² gives the real constants.
        let g = m.gamma_lossy(C::new(4.0 * kc * kc, 0.0));
        assert!(g.re == 0.0 && (g.im - 3f64.sqrt() * kc).abs() < 1e-9);
        let g = m.gamma_lossy(C::new(0.25 * kc * kc, 0.0));
        assert!(g.im == 0.0 && (g.re - 0.75f64.sqrt() * kc).abs() < 1e-9);
        // A lossy medium, k² = k′²(1 − j tan δ): α > 0, β > 0, and γ² = k_c² − k².
        let k2 = C::new(4.0 * kc * kc, -0.04 * kc * kc);
        let g = m.gamma_lossy(k2);
        assert!(g.re > 0.0 && g.im > 0.0 && (g * g - (kc * kc - k2)).norm() < 1e-9 * kc * kc);
        // α ≈ k″²/(2β) to first order.
        assert!((g.re - 0.02 * kc * kc / (3f64.sqrt() * kc)).abs() < 1e-3 * g.re);
        assert_eq!(m.kappa_lossy(k2), Some(g));
        assert!(m.kappa_lossy(C::new(kc * kc, 0.0)).is_none());
        let tm = Mode { kind: Kind::Tm, m: 1, n: 1, aperture: Aperture::Rect { a: 0.02, b: 0.01, x0: 0.0, y0: 0.0 } };
        let k2 = C::new(4.0 * tm.cutoff().powi(2), -0.1);
        let kappa = tm.kappa_lossy(k2).unwrap();
        assert!((kappa + k2 / tm.gamma_lossy(k2)).norm() < 1e-12 * kappa.norm());
    }

    #[test]
    fn circular_modes_are_orthogonal_with_their_norms_and_meet_the_wall() {
        let r = 0.01;
        let modes = lowest_circular(r, 9);
        let names: Vec<String> = modes.iter().map(Mode::name).collect();
        assert_eq!(names, ["TE11c", "TE11s", "TM01", "TE21c", "TE21s", "TE01", "TM11c", "TM11s", "TE31c"]);
        assert!((modes[0].cutoff() * r - 1.841183781340659).abs() < 1e-12);
        // The dominant mode points along +y at the centre (like the rectangle's TE₁₀).
        let e = modes[0].field(0.0, 0.0);
        assert!(e[0].abs() < 1e-15 && e[1] > 0.0);
        let e = modes[0].field(1e-12, 0.0);
        assert!(e[0].abs() < 1e-9 && e[1] > 0.0);
        // Gauss in ρ and the trapezoid rule (exact for trigonometric polynomials) in φ.
        let (x, w) = crate::quadrature::gauss_legendre(30);
        let nphi = 64;
        for p in &modes {
            for q in &modes {
                let mut s = 0.0;
                for (xi, wi) in x.iter().zip(&w) {
                    let rho = xi * r;
                    for k in 0..nphi {
                        let phi = 2.0 * PI * k as f64 / nphi as f64;
                        let (ep, eq) = (p.field(rho * phi.cos(), rho * phi.sin()), q.field(rho * phi.cos(), rho * phi.sin()));
                        s += wi * r * rho * 2.0 * PI / nphi as f64 * (ep[0] * eq[0] + ep[1] * eq[1]);
                    }
                }
                let expect = if p == q { p.norm() } else { 0.0 };
                assert!((s - expect).abs() < 1e-10 * r * r, "{} {}: {s} {expect}", p.name(), q.name());
            }
            // Tangential field zero on the wall.
            for phi in [0.3, 1.7, 4.0] {
                let e = p.field(r * f64::cos(phi), r * f64::sin(phi));
                assert!((-e[0] * phi.sin() + e[1] * phi.cos()).abs() < 1e-12, "{}", p.name());
            }
        }
        let shifted = modes[0].at(0.003, -0.002);
        assert_eq!(shifted.field(0.004, -0.001), modes[0].field(0.001, 0.001));
    }
}
