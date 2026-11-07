//! Colour maps for scalar fields: a diverging map for signed values (a field component at one
//! phase) and a sequential one for magnitudes, both our own control points interpolated
//! linearly in sRGB; and [`ColourScale`], a map with its range (and a decibel scale for a
//! magnitude), which the 3D view's shader, its colour bar and a probe's reading all take.
//!
//! The diverging map runs from a deep blue through a light grey to a deep red: its ends are dark
//! and saturated enough to stand off both a white and a dark background, its middle a grey
//! a shade below white, so a field near zero still reads as the plane's own and not as the page.

use crate::Rgb;

/// The diverging map's stops, −1 to 1.
const DIVERGING: [Rgb; 9] = [
    Rgb(24, 52, 118),
    Rgb(38, 92, 168),
    Rgb(80, 142, 204),
    Rgb(158, 193, 228),
    Rgb(232, 232, 232),
    Rgb(240, 178, 148),
    Rgb(218, 108, 78),
    Rgb(176, 42, 44),
    Rgb(108, 14, 30),
];

/// The sequential map's stops, 0 to 1: dark to bright.
const SEQUENTIAL: [Rgb; 9] = [
    Rgb(18, 14, 56),
    Rgb(52, 22, 98),
    Rgb(98, 30, 120),
    Rgb(146, 44, 116),
    Rgb(192, 64, 92),
    Rgb(226, 98, 64),
    Rgb(246, 144, 46),
    Rgb(250, 196, 72),
    Rgb(252, 240, 140),
];

/// How many stops a map has (the shader's array).
pub const STOPS: usize = 9;

fn sample(stops: &[Rgb; STOPS], t: f64) -> Rgb {
    let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.5 };
    let x = t * (STOPS - 1) as f64;
    let i = (x.floor() as usize).min(STOPS - 2);
    stops[i].mix(stops[i + 1], x - i as f64)
}

/// A signed value in [−1, 1] (clamped): blue through grey to red.
pub fn diverging(v: f64) -> Rgb {
    sample(&DIVERGING, 0.5 * (v + 1.0))
}

/// A magnitude in [0, 1] (clamped): dark to bright.
pub fn sequential(v: f64) -> Rgb {
    sample(&SEQUENTIAL, v)
}

/// The lowest a decibel scale may reach (dB below its peak).
pub const MIN_DB: f64 = -120.0;

/// A colour map with its range: signed values from −peak to peak (diverging), or magnitudes
/// from 0 to peak (sequential), linearly or in decibels down to `db` below the peak.
///
/// ```
/// use emcraft_render::colormap::ColourScale;
/// let s = ColourScale::signed(2.0);
/// assert_eq!((s.position(-2.0), s.position(0.0), s.position(2.0)), (0.0, 0.5, 1.0));
/// let db = ColourScale::magnitude(10.0).in_db(-40.0);
/// assert_eq!((db.position(10.0), db.position(0.1), db.position(1e-9)), (1.0, 0.0, 0.0));
/// assert!((db.position(1.0) - 0.5).abs() < 1e-12); // −20 dB: half way to −40
/// // The ticks of a colour bar: positions and the values they mark.
/// assert_eq!(s.ticks(), vec![(0.0, -2.0), (0.5, 0.0), (1.0, 2.0)]);
/// // A hostile range draws the middle, never a NaN.
/// assert_eq!(ColourScale::signed(f64::NAN).position(1.0), 0.5);
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColourScale {
    pub signed: bool,
    /// The value drawn at full colour (> 0).
    pub peak: f64,
    /// A magnitude's decibel scale: how far below the peak the map starts (dB, < 0).
    pub db: Option<f64>,
}

impl ColourScale {
    /// Signed values from −`peak` to `peak`.
    pub fn signed(peak: f64) -> ColourScale {
        ColourScale { signed: true, peak, db: None }
    }

    /// Magnitudes from 0 to `peak`.
    pub fn magnitude(peak: f64) -> ColourScale {
        ColourScale { signed: false, peak, db: None }
    }

    /// A magnitude's scale in decibels, from `floor` dB (clamped to [`MIN_DB`], −1) to the
    /// peak; a signed scale stays as it is.
    pub fn in_db(self, floor: f64) -> ColourScale {
        let floor = if floor.is_finite() { floor.clamp(MIN_DB, -1.0) } else { -40.0 };
        ColourScale { db: (!self.signed).then_some(floor), ..self }
    }

    /// The peak, or 1 when it is not a positive finite number.
    fn peak(&self) -> f64 {
        if self.peak > 0.0 && self.peak.is_finite() { self.peak } else { 1.0 }
    }

    /// Where `v` lies on the map, 0 to 1 (clamped; the middle for a NaN).
    pub fn position(&self, v: f64) -> f64 {
        if !v.is_finite() || !(self.peak > 0.0 && self.peak.is_finite()) {
            return if self.signed { 0.5 } else { 0.0 };
        }
        let x = v / self.peak();
        let t = match (self.signed, self.db) {
            (true, _) => 0.5 * (x + 1.0),
            (false, None) => x,
            (false, Some(floor)) => {
                let db = 20.0 * x.abs().max(1e-300).log10();
                1.0 - db / floor
            }
        };
        t.clamp(0.0, 1.0)
    }

    /// The colour of `v`.
    pub fn colour(&self, v: f64) -> Rgb {
        self.at(self.position(v))
    }

    /// The colour at position `t` (0 to 1) along the map.
    pub fn at(&self, t: f64) -> Rgb {
        if self.signed { sample(&DIVERGING, t) } else { sample(&SEQUENTIAL, t) }
    }

    /// The map's stops, evenly from 0 to 1 (the shader's).
    pub fn stops(&self) -> [Rgb; STOPS] {
        if self.signed { DIVERGING } else { SEQUENTIAL }
    }

    /// The value at position `t`: the inverse of [`ColourScale::position`].
    pub fn value(&self, t: f64) -> f64 {
        let p = self.peak();
        match (self.signed, self.db) {
            (true, _) => (2.0 * t - 1.0) * p,
            (false, None) => t * p,
            (false, Some(floor)) => p * 10f64.powf((1.0 - t) * floor / 20.0),
        }
    }

    /// A colour bar's ticks: (position, value) — the ends and zero of a signed scale, the
    /// ends and middle of a magnitude's, every 10 dB of a decibel one (or every 20 below −60).
    pub fn ticks(&self) -> Vec<(f64, f64)> {
        match self.db {
            Some(floor) if !self.signed => {
                let step = if floor < -60.0 { 20.0 } else { 10.0 };
                let n = (-floor / step).floor() as usize;
                (0..=n).rev().map(|k| 1.0 - k as f64 * step / -floor).map(|t| (t, self.value(t))).collect()
            }
            _ => [0.0, 0.5, 1.0].into_iter().map(|t| (t, self.value(t))).collect(),
        }
    }

    /// A tick's text: the value, or its decibels below the peak (the unit, dB, said once by
    /// the bar's title).
    pub fn tick_text(&self, t: f64) -> String {
        match self.db {
            Some(floor) if !self.signed => crate::axes::format((1.0 - t) * floor),
            _ => crate::axes::format(self.value(t)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ends_middle_and_hostile_values() {
        assert_eq!(diverging(-1.0), DIVERGING[0]);
        assert_eq!(diverging(0.0), DIVERGING[4]);
        assert_eq!(diverging(7.0), DIVERGING[8]);
        assert_eq!(sequential(0.0), SEQUENTIAL[0]);
        assert_eq!(sequential(1.0), SEQUENTIAL[8]);
        assert_eq!(diverging(f64::NAN), DIVERGING[4]);
    }

    /// The map's ends stand off both backgrounds and its middle off white: the lightness
    /// contrast (WCAG's ratio of relative luminances) of the deep ends against a white page is
    /// at least 4.5, against a dark one (#1b2433) the bright magnitude end at least 4.5, and
    /// the middle grey is not the page's white.
    #[test]
    fn the_maps_stand_off_light_and_dark_backgrounds() {
        let lum = |c: Rgb| {
            let ch = |v: u8| {
                let x = f64::from(v) / 255.0;
                if x <= 0.04045 { x / 12.92 } else { ((x + 0.055) / 1.055).powf(2.4) }
            };
            0.2126 * ch(c.0) + 0.7152 * ch(c.1) + 0.0722 * ch(c.2)
        };
        let ratio = |a: Rgb, b: Rgb| {
            let (x, y) = (lum(a), lum(b));
            (x.max(y) + 0.05) / (x.min(y) + 0.05)
        };
        let (white, dark) = (Rgb(255, 255, 255), Rgb(27, 36, 51));
        assert!(ratio(DIVERGING[0], white) >= 4.5 && ratio(DIVERGING[8], white) >= 4.5);
        assert!(ratio(SEQUENTIAL[8], dark) >= 4.5);
        assert_ne!(DIVERGING[4], white);
        // Every step darker or lighter in order: the map reads one way from each end.
        let l: Vec<f64> = DIVERGING.iter().map(|c| lum(*c)).collect();
        assert!(l[..5].windows(2).all(|w| w[0] < w[1]) && l[4..].windows(2).all(|w| w[0] > w[1]));
        let l: Vec<f64> = SEQUENTIAL.iter().map(|c| lum(*c)).collect();
        assert!(l.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn a_db_scale_marks_every_ten_db() {
        let s = ColourScale::magnitude(5.0).in_db(-40.0);
        let ticks = s.ticks();
        assert_eq!(ticks.len(), 5);
        assert_eq!(ticks.first().map(|t| t.0), Some(0.0));
        assert_eq!(ticks.last().map(|t| t.0), Some(1.0));
        assert_eq!(s.tick_text(0.0), "-40");
        assert_eq!(s.tick_text(1.0), "0");
        // A signed scale has no decibels.
        assert_eq!(ColourScale::signed(1.0).in_db(-40.0).db, None);
    }
}
