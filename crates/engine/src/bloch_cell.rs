//! `solve.bloch`: a periodic guide's Bloch waves from one cell solved in 3D — a helically
//! corrugated guide, say, which no body of revolution holds.
//!
//! **The cell.** The model between its two waveguide ports, one on each end (the same guide on
//! both, one period apart along z). At each frequency its generalized scattering matrix between
//! every mode of both ports ([`emcraft_solver::Driven::generalized`]) gives the transfer matrix
//! T = [[S₂₁ − S₂₂S₁₂⁻¹S₁₁, S₂₂S₁₂⁻¹], [−S₁₂⁻¹S₁₁, S₁₂⁻¹]] on the waves (a₁, b₁) at the near end,
//! and its eigenvalues λ = e^{−γp} are the Bloch waves (R. E. Collin, *Foundations for Microwave
//! Engineering*, 2nd ed., McGraw-Hill 1992, §9.2; the body of revolution's
//! [`emcraft_solver::bor::bloch_lambda`]). The waves going +z are those decaying (|λ| < 1) and,
//! on the unit circle (within 2·10⁻³: a 3D cell's S is unitary to a few parts in 10⁴), those
//! carrying power +z at the near end (Σ P_k (|a_k|² − |b_k|²) > 0, P_k a unit amplitude's power).
//!
//! **One basis at both ends.** Each port's modes are solved on its own face, so a pair of
//! degenerate modes (a round guide's two TE11, a trefoil's) comes out in any basis, and any mode
//! with either sign. The far port's modes are written in the near port's: their transverse
//! fields sampled at the same points of the two faces, e₂ⱼ = Σᵢ Rᵢⱼ e₁ᵢ by least squares, and S
//! taken to that basis (S₂₁ → R S₂₁, S₁₂ → S₁₂ R⁻¹, S₂₂ → R S₂₂ R⁻¹). A far port whose modes
//! the near one's do not span (other guides) is refused.
//!
//! **The waves followed.** At the first frequency the waves going +z, the propagating ones by
//! ascending β, then the decaying ones by ascending α, each named by its strongest mode at the
//! near end; at the next each is the one whose
//! near-end amplitudes are most nearly parallel to its own before. β = −arg λ / p up to a
//! multiple of 2π/p: at the first frequency the branch nearest its strongest mode's own β, then
//! the one nearest its last.

use std::f64::consts::PI;

use emcraft_solver::Complex64 as C;
use serde_json::{Value, json};

use crate::{EngineError, Params, Result, Session};

/// The most waves a solve follows.
pub const MAX_WAVES: usize = 16;

/// Points along each side of the faces' sampling grid.
const SAMPLES: usize = 40;

/// How far the far port's modes may stray from the near port's span (relative, least squares).
const SPAN_TOLERANCE: f64 = 0.05;

fn failed(why: impl Into<String>) -> EngineError {
    EngineError::failed(format!("Bloch wave: {}", why.into()))
}

/// n × n products and inverses, row-major.
fn mul(a: &[C], b: &[C], n: usize) -> Vec<C> {
    (0..n * n).map(|ij| (0..n).map(|k| a[(ij / n) * n + k] * b[k * n + ij % n]).sum()).collect()
}

fn inverse(a: &[C], n: usize, what: &str) -> Result<Vec<C>> {
    let lu = emcraft_solver::dense::Lu::new(a.to_vec(), n).ok_or_else(|| failed(format!("{what} is singular")))?;
    let mut inv = vec![C::new(0.0, 0.0); n * n];
    for j in 0..n {
        let e: Vec<C> = (0..n).map(|i| C::new(if i == j { 1.0 } else { 0.0 }, 0.0)).collect();
        for (i, x) in lu.solve(&e).into_iter().enumerate() {
            inv[i * n + j] = x;
        }
    }
    Ok(inv)
}

/// R with e₂ⱼ = Σᵢ Rᵢⱼ e₁ᵢ at the points where both are sampled (least squares, the normal
/// equations), refused when the far modes stray from the near ones' span.
fn basis(near: &[Vec<[f64; 2]>], far: &[Vec<[f64; 2]>]) -> Result<Vec<C>> {
    let n = near.len();
    if far.len() != n || n == 0 {
        return Err(failed("the cell's two ends need as many modes"));
    }
    let points = near[0].len();
    let both: Vec<usize> =
        (0..points).filter(|&k| near.iter().chain(far).all(|m| m.get(k).is_some_and(|e| e[0].is_finite() && e[1].is_finite()))).collect();
    if both.len() < n {
        return Err(failed("the two ends' cross-sections do not overlap"));
    }
    let comp = |m: &Vec<[f64; 2]>| -> Vec<f64> { both.iter().flat_map(|&k| m[k]).collect() };
    let e1: Vec<Vec<f64>> = near.iter().map(comp).collect();
    let e2: Vec<Vec<f64>> = far.iter().map(comp).collect();
    let dot = |a: &[f64], b: &[f64]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f64>();
    let gram: Vec<C> = (0..n * n).map(|ij| C::new(dot(&e1[ij / n], &e1[ij % n]), 0.0)).collect();
    let inv = inverse(&gram, n, "the near modes' Gram matrix")?;
    let rhs: Vec<C> = (0..n * n).map(|ij| C::new(dot(&e1[ij / n], &e2[ij % n]), 0.0)).collect();
    let r = mul(&inv, &rhs, n);
    for j in 0..n {
        let norm = dot(&e2[j], &e2[j]).sqrt();
        let fit: Vec<f64> = (0..e2[j].len()).map(|k| (0..n).map(|i| r[i * n + j].re * e1[i][k]).sum::<f64>()).collect();
        let miss = fit.iter().zip(&e2[j]).map(|(a, b)| (a - b) * (a - b)).sum::<f64>().sqrt();
        if !(miss <= SPAN_TOLERANCE * norm) {
            return Err(failed(format!(
                "the far end's mode {} is not among the near end's ({:.0} % off): the cell's ends must be the same guide, with as many modes (whole degenerate pairs)",
                j + 1,
                100.0 * miss / norm.max(f64::MIN_POSITIVE)
            )));
        }
    }
    Ok(r)
}

/// How near the unit circle a wave's λ counts as propagating (|ln |λ|| below it): a 3D cell's
/// S is unitary to a few parts in 10⁴.
const UNIT: f64 = 2e-3;

/// One frequency's waves going +z: λ and the near-end amplitudes a₁ of each. `power` is what a
/// unit amplitude of each near-end mode carries, which tells a propagating wave's way.
fn forward_waves(s: &[C], n: usize, r: &[C], power: &[f64]) -> Result<Vec<(C, Vec<C>)>> {
    let w = 2 * n;
    let block = |r0: usize, c0: usize| -> Vec<C> { (0..n * n).map(|ij| s[(r0 + ij / n) * w + c0 + ij % n]).collect() };
    let (s11, s12, s21, s22) = (block(0, 0), block(0, n), block(n, 0), block(n, n));
    let r_inv = inverse(r, n, "the far modes' basis")?;
    let (s12, s21, s22) = (mul(&s12, &r_inv, n), mul(r, &s21, n), mul(&mul(r, &s22, n), &r_inv, n));
    let inv = inverse(&s12, n, "S₁₂ (the cell transmits nothing)")?;
    let inv_s11 = mul(&inv, &s11, n);
    let s22_inv = mul(&s22, &inv, n);
    let s22_inv_s11 = mul(&s22_inv, &s11, n);
    let mut t = vec![C::new(0.0, 0.0); w * w];
    for i in 0..n {
        for j in 0..n {
            t[i * w + j] = s21[i * n + j] - s22_inv_s11[i * n + j];
            t[i * w + n + j] = s22_inv[i * n + j];
            t[(n + i) * w + j] = -inv_s11[i * n + j];
            t[(n + i) * w + n + j] = inv[i * n + j];
        }
    }
    let pairs = emcraft_solver::zdense::eigen(&t, w).ok_or_else(|| failed("the transfer matrix's eigenproblem did not converge"))?;
    let unit = |l: &C| l.norm().ln().abs() < UNIT;
    // The power it carries +z at the near end: in less what goes out.
    let forward = |v: &[C]| (0..n).map(|k| power.get(k).copied().unwrap_or(0.0) * (v[k].norm_sqr() - v[n + k].norm_sqr())).sum::<f64>() > 0.0;
    let mut out: Vec<(C, Vec<C>)> =
        pairs.into_iter().filter(|(l, v)| if unit(l) { forward(v) } else { l.norm() < 1.0 }).map(|(l, v)| (l, v[..n].to_vec())).collect();
    out.sort_by(|a, b| b.0.norm().total_cmp(&a.0.norm()));
    Ok(out)
}

/// |⟨a, b⟩| / (|a| |b|).
fn parallel(a: &[C], b: &[C]) -> f64 {
    let d: C = a.iter().zip(b).map(|(x, y)| x.conj() * y).sum();
    let na = a.iter().map(C::norm_sqr).sum::<f64>().sqrt();
    let nb = b.iter().map(C::norm_sqr).sum::<f64>().sqrt();
    d.norm() / (na * nb).max(f64::MIN_POSITIVE)
}

/// `solve.bloch`.
pub fn solve(s: &mut Session, p: &Params) -> Result<Value> {
    let freqs = crate::solve::frequencies(s, p)?;
    let order = p.order()?;
    let count = p.integer("waves", 1..=MAX_WAVES).or(4)?;
    let model = &s.model;
    if model.ports.len() != 2 || !model.lumped.is_empty() {
        return Err(failed("the cell needs its two waveguide ports, one on each end, and no lumped ports"));
    }
    if model.ports[0].modes != model.ports[1].modes {
        return Err(failed("the cell's two ports need as many modes"));
    }
    let mesh = model.mesh.as_ref().ok_or_else(|| EngineError::failed("no mesh: mesh.generate"))?;
    // The period: the cell's length along z; the faces' span across it.
    let (lo, hi) = mesh.nodes.iter().fold(([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]), |(l, h), q| {
        ([l[0].min(q.x), l[1].min(q.y), l[2].min(q.z)], [h[0].max(q.x), h[1].max(q.y), h[2].max(q.z)])
    });
    let period = hi[2] - lo[2];
    if !(period > 0.0 && period.is_finite()) {
        return Err(failed("the cell has no length along z"));
    }
    let points: Vec<(f64, f64)> = (0..SAMPLES)
        .flat_map(|j| (0..SAMPLES).map(move |i| (i, j)))
        .map(|(i, j)| {
            let u = (i as f64 + 0.5) / SAMPLES as f64;
            let v = (j as f64 + 0.5) / SAMPLES as f64;
            (lo[0] + u * (hi[0] - lo[0]), lo[1] + v * (hi[1] - lo[1]))
        })
        .collect();
    let driven = emcraft_solver::Driven::new(model, order)?;
    let turn = 2.0 * PI / period;
    // Each followed wave: its name, its last a₁ and β, its γ so far.
    struct Followed {
        name: String,
        a: Vec<C>,
        beta: f64,
        gamma: Vec<C>,
    }
    let mut followed: Vec<Followed> = Vec::new();
    let mut unknowns = 0;
    let mut n = 0;
    for (k, &f) in freqs.iter().enumerate() {
        crate::progress::report(k, freqs.len(), "frequency")?;
        let g = driven.generalized(f)?;
        n = g.modes.iter().filter(|(port, _)| *port == 0).count();
        if g.modes.len() != 2 * n || g.modes.iter().take(n).any(|(port, _)| *port != 0) {
            return Err(failed("the cell's two ports need as many modes"));
        }
        let near = emcraft_solver::driven::port_mode_fields(model, order, f, 0, &points)?;
        let far = emcraft_solver::driven::port_mode_fields(model, order, f, 1, &points)?;
        let r: Vec<C> = basis(&near, &far)?;
        let waves = forward_waves(&g.s, n, &r, &g.power)?;
        let beta_of = |l: C, guess: f64| {
            let raw = -l.arg() / period;
            raw + turn * ((guess - raw) / turn).round()
        };
        if followed.is_empty() {
            // The propagating waves by β, then the decaying ones by α: an order that holds
            // when two waves share |λ| = 1.
            let mut first: Vec<(C, &Vec<C>, usize, f64)> = waves
                .iter()
                .map(|(l, a)| {
                    let strongest = (0..n).max_by(|&i, &j| a[i].norm().total_cmp(&a[j].norm())).unwrap_or(0);
                    let own = g.gamma.get(strongest).map_or(0.0, |c| c.im);
                    (*l, a, strongest, beta_of(*l, own))
                })
                .collect();
            let key = |w: &(C, &Vec<C>, usize, f64)| if w.0.norm().ln().abs() < UNIT { (0, w.3) } else { (1, -w.0.norm().ln()) };
            first.sort_by(|x, y| key(x).0.cmp(&key(y).0).then(key(x).1.total_cmp(&key(y).1)));
            for (l, a, strongest, beta) in first.into_iter().take(count) {
                followed.push(Followed {
                    name: format!("wave {} (mode {})", followed.len() + 1, strongest + 1),
                    a: a.clone(),
                    beta,
                    gamma: vec![C::new(-l.norm().ln() / period, beta)],
                });
            }
        } else {
            let mut taken = vec![false; waves.len()];
            for w in &mut followed {
                let best =
                    (0..waves.len()).filter(|&j| !taken[j]).max_by(|&i, &j| parallel(&w.a, &waves[i].1).total_cmp(&parallel(&w.a, &waves[j].1)));
                let Some(j) = best else { return Err(failed("a wave followed has no match at the next frequency")) };
                taken[j] = true;
                let (l, a) = &waves[j];
                w.beta = beta_of(*l, w.beta);
                w.a.clone_from(a);
                w.gamma.push(C::new(-l.norm().ln() / period, w.beta));
            }
        }
        unknowns = driven.dofs.unknowns;
    }
    let reply_waves: Vec<Value> = followed
        .iter()
        .map(|w| {
            let points: Vec<Value> = freqs
                .iter()
                .zip(&w.gamma)
                .map(|(&f, g)| json!({"frequency": s.show_frequency(f), "alpha": g.re, "beta": g.im, "phase": (g.im * period).to_degrees()}))
                .collect();
            json!({"name": w.name, "points": points})
        })
        .collect();
    let waves = followed.into_iter().map(|w| crate::BlochWave { name: w.name, gamma: w.gamma }).collect();
    s.results.bloch = Some(std::sync::Arc::new(crate::Bloch { period, frequencies: freqs, waves }));
    Ok(json!({"period": s.show_length(period), "modes": n, "unknowns": unknowns, "waves": reply_waves}))
}
