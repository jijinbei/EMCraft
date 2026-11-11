//! Colours in OKLCH — the lightness, chroma and hue of Björn Ottosson's Oklab ("A perceptual
//! color space for image processing", 2020, bottosson.github.io/posts/oklab; its matrices as
//! published there): equal steps look equal, so a colour's hue can change while its lightness,
//! and with it its contrast against what it sits on, stays.
//!
//! ```
//! use emcraft_render::Rgb;
//! use emcraft_render::oklch::Oklch;
//! let blue = Rgb(38, 120, 218).oklch();
//! assert!((blue.l - 0.577).abs() < 1e-3 && (blue.h - 256.0).abs() < 1.0);
//! // The same lightness and chroma in another hue, and back.
//! let warm = Oklch { h: 73.0, ..blue }.rgb();
//! assert!((warm.oklch().l - blue.l).abs() < 0.02);
//! assert_eq!(blue.rgb(), Rgb(38, 120, 218));
//! // Out of range (not finite, or outside sRGB) is clipped, never a panic.
//! assert_eq!(Oklch { l: f64::NAN, c: 9.0, h: 0.0 }.rgb(), Rgb(0, 0, 0));
//! ```

use crate::Rgb;

/// A colour's lightness (0 black … 1 white), chroma (0 grey, about 0.37 at most in sRGB) and hue
/// (degrees).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Oklch {
    pub l: f64,
    pub c: f64,
    pub h: f64,
}

/// An sRGB channel (0…255) as linear light.
pub fn linear(v: u8) -> f64 {
    let v = f64::from(v) / 255.0;
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

/// Linear light as an sRGB channel, clipped to 0…255 (not a number: 0).
fn encoded(v: f64) -> u8 {
    let v = if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
    let s = if v <= 0.003_130_8 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0).round().clamp(0.0, 255.0) as u8
}

impl Rgb {
    /// The colour in OKLCH.
    pub fn oklch(self) -> Oklch {
        let [r, g, b] = [linear(self.0), linear(self.1), linear(self.2)];
        let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
        let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
        let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
        let lightness = 0.210_454_255_3 * l + 0.793_617_785_0 * m - 0.004_072_046_8 * s;
        let a = 1.977_998_495_1 * l - 2.428_592_205_0 * m + 0.450_593_709_9 * s;
        let bb = 0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766_0 * s;
        Oklch { l: lightness, c: a.hypot(bb), h: bb.atan2(a).to_degrees().rem_euclid(360.0) }
    }
}

impl Oklch {
    /// The colour in sRGB, each channel clipped to its range.
    pub fn rgb(self) -> Rgb {
        let (a, b) = (self.c * self.h.to_radians().cos(), self.c * self.h.to_radians().sin());
        let l = (self.l + 0.396_337_777_4 * a + 0.215_803_757_3 * b).powi(3);
        let m = (self.l - 0.105_561_345_8 * a - 0.063_854_172_8 * b).powi(3);
        let s = (self.l - 0.089_484_177_5 * a - 1.291_485_548_0 * b).powi(3);
        Rgb(
            encoded(4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s),
            encoded(-1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s),
            encoded(-0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701_0 * s),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every sRGB colour on a grid comes back as itself (to a step of rounding), and the
    /// published anchors hold: white's lightness 1 and chroma 0, black's lightness 0.
    #[test]
    fn colours_round_trip_and_the_anchors_hold() {
        let w = Rgb::WHITE.oklch();
        assert!((w.l - 1.0).abs() < 1e-4 && w.c < 1e-4, "{w:?}");
        assert!(Rgb(0, 0, 0).oklch().l.abs() < 1e-9);
        for (r, g, b) in
            (0..=255u8).step_by(15).flat_map(|r| (0..=255u8).step_by(15).flat_map(move |g| (0..=255u8).step_by(15).map(move |b| (r, g, b))))
        {
            let back = Rgb(r, g, b).oklch().rgb();
            let near = |x: u8, y: u8| x.abs_diff(y) <= 1;
            assert!(near(back.0, r) && near(back.1, g) && near(back.2, b), "{:?} → {back:?}", Rgb(r, g, b));
        }
    }
}
