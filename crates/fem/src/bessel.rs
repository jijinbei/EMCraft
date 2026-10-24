//! Bessel functions of integer order and the zeros of functions, for the modes of circular
//! waveguide ports and the resonances of round cavities.
//!
//! J_n(x) by Miller's backward recurrence normalised with J₀ + 2ΣJ_{2k} = 1 (M. Abramowitz,
//! I. A. Stegun, *Handbook of Mathematical Functions*, 1964, §9.12; the algorithm of J. C. P.
//! Miller in the British Association Mathematical Tables, 1952); J_n' = (J_{n−1} − J_{n+1})/2.
//! Zeros are bracketed on a fine grid and bisected to the last bit.

/// J_0 … J_{n+1} at x > 0.
fn bessel_all(n: usize, x: f64) -> Vec<f64> {
    let top = 2 * ((n as f64 + x).ceil() as usize + 20);
    let top = top + top % 2;
    let mut j = vec![0.0; top + 2];
    j[top + 1] = 0.0;
    j[top] = 1e-30;
    for k in (1..=top).rev() {
        j[k - 1] = 2.0 * k as f64 / x * j[k] - j[k + 1];
        // Rescale against overflow.
        if j[k - 1].abs() > 1e250 {
            for v in j.iter_mut().skip(k - 1) {
                *v *= 1e-250;
            }
        }
    }
    let norm = j[0] + 2.0 * j.iter().skip(2).step_by(2).sum::<f64>();
    j.truncate(n + 2);
    j.iter().map(|v| v / norm).collect()
}

/// J_n(x).
pub fn j(n: usize, x: f64) -> f64 {
    if x == 0.0 {
        return if n == 0 { 1.0 } else { 0.0 };
    }
    bessel_all(n, x.abs())[n] * if x < 0.0 && n % 2 == 1 { -1.0 } else { 1.0 }
}

/// J_n'(x).
pub fn jp(n: usize, x: f64) -> f64 {
    if n == 0 {
        return -j(1, x);
    }
    0.5 * (j(n - 1, x) - j(n + 1, x))
}

/// The first `count` positive zeros of `f` in (0, x_max].
pub fn zeros(f: impl Fn(f64) -> f64, count: usize, x_max: f64) -> Vec<f64> {
    let mut out = Vec::new();
    let step = 1e-3;
    let mut a = 1e-6;
    let mut fa = f(a);
    while out.len() < count && a < x_max {
        let b = a + step;
        let fb = f(b);
        if fa == 0.0 || fa * fb < 0.0 {
            let (mut lo, mut hi, mut flo) = (a, b, fa);
            for _ in 0..200 {
                let mid = 0.5 * (lo + hi);
                if mid <= lo || mid >= hi {
                    break;
                }
                let fm = f(mid);
                if (fm < 0.0) == (flo < 0.0) {
                    lo = mid;
                    flo = fm;
                } else {
                    hi = mid;
                }
            }
            out.push(0.5 * (lo + hi));
        }
        a = b;
        fa = fb;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabulated_zeros_and_values() {
        // Abramowitz & Stegun, Table 9.5; J₀(1) = 0.7651976865579666.
        assert!((zeros(|x| j(0, x), 1, 20.0)[0] - 2.404825557695773).abs() < 1e-12);
        assert!((zeros(|x| jp(1, x), 1, 20.0)[0] - 1.841183781340659).abs() < 1e-12);
        assert!((j(0, 1.0) - 0.7651976865579666).abs() < 1e-14);
        assert!((j(1, -2.0) + j(1, 2.0)).abs() < 1e-16 && j(2, 0.0) == 0.0);
    }
}
