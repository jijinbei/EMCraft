//! Checked numbers: "parse, don't validate". A value from outside (a command parameter, a
//! file) is checked once, where it enters, into one of these types; the model stores the type,
//! and everything that reads it (mesher, solvers, renderers) knows the rule holds without
//! checking again.
//!
//! Each type is built only through `new` (or `named`, which says what the value is in its
//! error) and `TryFrom<f64>`, both refusing a value that breaks its rule with
//! [`ModelError::Invalid`]; it reads back as an `f64` through `get()` or `Deref`, and
//! arithmetic on it (`+ − × ÷` with its own type or an `f64`, negation) gives an `f64`. They
//! serialize as plain numbers (`#[serde(try_from = "f64", into = "f64")]`), so files are the
//! same as with `f64` fields, and a value read from a file is checked as one given to `new`.
//!
//! - [`Positive`]: finite, > 0 (a relative permittivity, a pole's Δε, a time);
//! - [`Hertz`]: a frequency (Hz), finite, > 0;
//! - [`Metres`]: a length (m), finite, > 0;
//! - [`Ohms`]: an impedance (Ω), finite, > 0;
//! - [`Conductivity`]: S/m, finite, > 0, at most [`MAX_CONDUCTIVITY`];
//! - [`LossTangent`]: finite, 0 to [`MAX_LOSS_TANGENT`].
//!
//! ```
//! use emcraft_model::{Conductivity, Hertz, Quantity};
//!
//! let f = Hertz::new(9e9)?; // Hz, SI
//! assert_eq!(f.get(), 9e9);
//! assert_eq!(*f * 2.0, 1.8e10); // reads as an f64
//! assert_eq!(1.0 / f, 1.0 / 9e9); // and computes as one
//! let copper = Conductivity::named("wall conductivity", 5.8e7)?; // S/m
//! assert_eq!(f64::from(copper), 5.8e7);
//!
//! // Refused: not finite, not > 0, beyond a cap; the error names the value.
//! assert!(Hertz::new(f64::NAN).is_err() && Hertz::new(0.0).is_err());
//! let e = Conductivity::named("wall conductivity", 1e30).unwrap_err();
//! assert!(e.to_string().starts_with("invalid wall conductivity: must be a finite number > 0"));
//! # Ok::<(), emcraft_model::ModelError>(())
//! ```

use std::fmt;
use std::ops::{Add, AddAssign, Deref, Div, Mul, Neg, Sub};

use serde::{Deserialize, Serialize};

use crate::error::{ModelError, Result, invalid};

/// The largest conductivity (S/m) a model may hold: far beyond any metal's (copper's is
/// 5.8e7), small enough that a wall's surface admittance stays finite.
pub const MAX_CONDUCTIVITY: f64 = 1e20;

/// The largest loss tangent a material may have (dielectric or magnetic): far beyond any
/// dielectric's (a good conductor's σ/ωε is about 10⁸ at 10 GHz), small enough that k² stays
/// finite.
pub const MAX_LOSS_TANGENT: f64 = 1e9;

/// A checked number: what every type of this module shares.
pub trait Quantity: Copy + Into<f64> + TryFrom<f64, Error = ModelError> {
    /// What a value of the type is called when no better name is given ("frequency").
    const WHAT: &'static str;
    /// What a finite value must be, after "must be": "> 0", "from 0 to 1e9".
    const MUST: &'static str;

    /// Whether `v` keeps the rule.
    fn keeps(v: f64) -> bool;

    /// `v` checked, the error naming it `what` ("wall conductivity").
    fn named(what: &str, v: f64) -> Result<Self>;

    /// The number.
    fn get(self) -> f64 {
        self.into()
    }
}

/// Defines a checked number type `$name`: its rule `$keeps`, its name and its cap in errors.
macro_rules! quantity {
    ($(#[$doc:meta])* $name:ident, $what:literal, $must:expr, |$v:ident| $keeps:expr) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Serialize, Deserialize)]
        #[serde(try_from = "f64", into = "f64")]
        pub struct $name(f64);

        impl $name {
            /// `v` checked, the error naming it by the type.
            pub fn new(v: f64) -> Result<$name> {
                <$name as Quantity>::named($what, v)
            }

            /// The number.
            pub fn get(self) -> f64 {
                self.0
            }
        }

        impl Quantity for $name {
            const WHAT: &'static str = $what;
            const MUST: &'static str = $must;

            fn keeps($v: f64) -> bool {
                $v.is_finite() && $keeps
            }

            fn named(what: &str, v: f64) -> Result<$name> {
                if <$name as Quantity>::keeps(v) {
                    Ok($name(v))
                } else {
                    Err(invalid(what, format!("must be a finite number {}, got {v}", $must)))
                }
            }
        }

        impl TryFrom<f64> for $name {
            type Error = ModelError;

            fn try_from(v: f64) -> Result<$name> {
                $name::new(v)
            }
        }

        impl From<$name> for f64 {
            fn from(q: $name) -> f64 {
                q.0
            }
        }

        impl Deref for $name {
            type Target = f64;

            fn deref(&self) -> &f64 {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }

        impl fmt::LowerExp for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::LowerExp::fmt(&self.0, f)
            }
        }

        arithmetic!($name, Add add, Sub sub, Mul mul, Div div);

        impl AddAssign<$name> for f64 {
            fn add_assign(&mut self, o: $name) {
                *self += o.0;
            }
        }

        impl Neg for $name {
            type Output = f64;

            fn neg(self) -> f64 {
                -self.0
            }
        }
    };
}

/// Arithmetic on a checked number, with another of its type or an `f64` on either side: a
/// plain `f64` (what it gives is not checked; checking it again is the reader's choice).
macro_rules! arithmetic {
    ($name:ident, $($op:ident $f:ident),*) => {$(
        impl $op for $name {
            type Output = f64;

            fn $f(self, o: $name) -> f64 {
                $op::$f(self.0, o.0)
            }
        }

        impl $op<f64> for $name {
            type Output = f64;

            fn $f(self, o: f64) -> f64 {
                $op::$f(self.0, o)
            }
        }

        impl $op<$name> for f64 {
            type Output = f64;

            fn $f(self, o: $name) -> f64 {
                $op::$f(self, o.0)
            }
        }

        impl $op<f64> for &$name {
            type Output = f64;

            fn $f(self, o: f64) -> f64 {
                $op::$f(self.0, o)
            }
        }

        impl $op<&$name> for f64 {
            type Output = f64;

            fn $f(self, o: &$name) -> f64 {
                $op::$f(self, o.0)
            }
        }
    )*};
}

quantity! {
    /// A finite number > 0, of any unit (a relative permittivity, a pole's Δε, a time in s).
    ///
    /// ```
    /// use emcraft_model::Positive;
    ///
    /// assert_eq!(Positive::new(2.2)?.get(), 2.2);
    /// assert!(Positive::new(0.0).is_err() && Positive::new(f64::INFINITY).is_err());
    /// # Ok::<(), emcraft_model::ModelError>(())
    /// ```
    Positive, "value", "> 0", |v| v > 0.0
}

quantity! {
    /// A frequency (Hz): finite, > 0.
    ///
    /// ```
    /// use emcraft_model::Hertz;
    ///
    /// assert_eq!(Hertz::new(9e9)?.get(), 9e9); // 9 GHz, in Hz
    /// assert!(Hertz::new(-1.0).is_err() && Hertz::new(f64::NAN).is_err());
    /// # Ok::<(), emcraft_model::ModelError>(())
    /// ```
    Hertz, "frequency", "> 0", |v| v > 0.0
}

quantity! {
    /// A length (m): finite, > 0.
    ///
    /// ```
    /// use emcraft_model::Metres;
    ///
    /// assert_eq!(Metres::new(1e-3)?.get(), 1e-3); // 1 mm, in m
    /// assert!(Metres::new(0.0).is_err() && Metres::new(f64::NEG_INFINITY).is_err());
    /// # Ok::<(), emcraft_model::ModelError>(())
    /// ```
    Metres, "length", "> 0", |v| v > 0.0
}

quantity! {
    /// An impedance (Ω): finite, > 0 (a lumped port's, a port's reference).
    ///
    /// ```
    /// use emcraft_model::Ohms;
    ///
    /// assert_eq!(Ohms::new(50.0)?.get(), 50.0);
    /// assert!(Ohms::new(0.0).is_err() && Ohms::new(f64::NAN).is_err());
    /// # Ok::<(), emcraft_model::ModelError>(())
    /// ```
    Ohms, "impedance", "> 0", |v| v > 0.0
}

quantity! {
    /// A conductivity (S/m): finite, > 0 and at most [`MAX_CONDUCTIVITY`].
    ///
    /// ```
    /// use emcraft_model::{Conductivity, MAX_CONDUCTIVITY};
    ///
    /// assert_eq!(Conductivity::new(5.8e7)?.get(), 5.8e7); // copper, S/m
    /// assert!(Conductivity::new(0.0).is_err() && Conductivity::new(2.0 * MAX_CONDUCTIVITY).is_err());
    /// # Ok::<(), emcraft_model::ModelError>(())
    /// ```
    Conductivity, "conductivity", "> 0, at most 1e20 S/m", |v| v > 0.0 && v <= MAX_CONDUCTIVITY
}

quantity! {
    /// A loss tangent tan δ (dielectric or magnetic): finite, 0 to [`MAX_LOSS_TANGENT`].
    ///
    /// ```
    /// use emcraft_model::LossTangent;
    ///
    /// assert_eq!(LossTangent::new(0.01)?.get(), 0.01);
    /// assert_eq!(LossTangent::default().get(), 0.0); // lossless
    /// assert!(LossTangent::new(-0.1).is_err() && LossTangent::new(1e300).is_err());
    /// # Ok::<(), emcraft_model::ModelError>(())
    /// ```
    LossTangent, "loss tangent", "from 0 to 1e9", |v| (0.0..=MAX_LOSS_TANGENT).contains(&v)
}

impl Positive {
    /// One (vacuum's εr and μr).
    pub const ONE: Positive = Positive(1.0);
}

impl Ohms {
    /// 50 Ω, the usual reference impedance.
    pub const FIFTY: Ohms = Ohms(50.0);
}

impl LossTangent {
    /// No loss.
    pub const ZERO: LossTangent = LossTangent(0.0);

    /// Whether there is no loss.
    pub fn is_zero(&self) -> bool {
        self.0 == 0.0
    }
}

impl Default for LossTangent {
    /// No loss.
    fn default() -> LossTangent {
        LossTangent::ZERO
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every hostile number each type refuses, with the values at its edges it keeps.
    #[test]
    fn each_type_keeps_its_rule_and_refuses_the_rest() {
        let hostile = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, -0.0, 0.0, -1e-300];
        fn refuses<T: Quantity + fmt::Debug>(bad: &[f64]) {
            for &v in bad {
                let e = T::named("thing", v).unwrap_err();
                assert!(matches!(&e, ModelError::Invalid { what, .. } if what == "thing"), "{v}: {e}");
                assert!(T::try_from(v).is_err(), "{v}");
            }
        }
        refuses::<Positive>(&hostile);
        refuses::<Hertz>(&hostile);
        refuses::<Metres>(&hostile);
        refuses::<Ohms>(&hostile);
        refuses::<Conductivity>(&[&hostile[..], &[1e21, f64::MAX]].concat());
        refuses::<LossTangent>(&[f64::NAN, f64::INFINITY, -1.0, -1e-300, 1e10, 1e300]);
        assert_eq!(Positive::new(f64::MIN_POSITIVE).map(Positive::get), Ok(f64::MIN_POSITIVE));
        assert_eq!(Conductivity::new(MAX_CONDUCTIVITY).map(Conductivity::get), Ok(MAX_CONDUCTIVITY));
        assert_eq!(LossTangent::new(0.0), Ok(LossTangent::ZERO));
        assert_eq!(LossTangent::new(MAX_LOSS_TANGENT).map(LossTangent::get), Ok(MAX_LOSS_TANGENT));
        assert_eq!(Hertz::new(-1.0).unwrap_err().to_string(), "invalid frequency: must be a finite number > 0, got -1");
    }

    #[test]
    fn they_read_and_write_as_plain_numbers() {
        let f = Hertz::new(9e9).unwrap();
        assert_eq!(serde_json::to_string(&f).unwrap(), "9000000000.0");
        assert_eq!(serde_json::from_str::<Hertz>("9000000000.0").unwrap(), f);
        assert!(serde_json::from_str::<Hertz>("-1.0").is_err(), "a file is checked as `new` checks");
        assert_eq!(format!("{f} {:.1e}", Conductivity::new(5.8e7).unwrap()), "9000000000 5.8e7");
    }
}
