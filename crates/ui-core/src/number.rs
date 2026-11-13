//! How the window writes a number, in one place: by what it is, not by each view's own format —
//! a frequency to four decimals in the display unit (a tenth of a kHz in GHz), a length to three,
//! a level in decibels to one, a value of any size to four significant figures, a field's text
//! with no more decimals than it needs. A value that is not a finite number is a dash.
//!
//! ```
//! use emcraft_ui_core::number;
//! assert_eq!(number::frequency(9.0), "9.0000");
//! assert_eq!(number::length(5.08), "5.080");
//! assert_eq!(number::decibels(-20.04), "-20.0");
//! assert_eq!(number::value(5.8e7), "5.80e7");
//! assert_eq!(number::value(1.685), "1.685");
//! assert_eq!(number::trimmed(2.50, 4), "2.5");
//! assert_eq!(number::frequency(f64::NAN), "–");
//! ```

/// What a value that is not a finite number shows.
pub const NONE: &str = "–";

/// A frequency in the display unit: four decimals.
pub fn frequency(v: f64) -> String {
    fixed(v, 4)
}

/// A length in the display unit: three decimals.
pub fn length(v: f64) -> String {
    fixed(v, 3)
}

/// A level in decibels: one decimal.
pub fn decibels(v: f64) -> String {
    fixed(v, 1)
}

/// A value of any size: four significant figures, in powers of ten outside 0.001 to 99999
/// ([`emcraft_render::axes::format`], what the charts' ticks say).
pub fn value(v: f64) -> String {
    if v.is_finite() { emcraft_render::axes::format(v) } else { NONE.to_owned() }
}

/// `v` to `decimals` decimals, the trailing zeros (and a bare point) dropped: what a field shows
/// of a number it holds.
pub fn trimmed(v: f64, decimals: usize) -> String {
    if !v.is_finite() {
        return NONE.to_owned();
    }
    let s = format!("{v:.decimals$}");
    let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_owned() } else { s };
    // No negative zero.
    if s == "-0" { "0".to_owned() } else { s }
}

/// `v` to `decimals` decimals.
fn fixed(v: f64, decimals: usize) -> String {
    if v.is_finite() { format!("{v:.decimals$}") } else { NONE.to_owned() }
}
