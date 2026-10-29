//! Choices: the string parameters that pick one of a few named values. Each is a type with
//! [`Choice`], whose names the command table lists (`ParamSpec::one_of(T::NAMES)`) and the
//! commands read ([`crate::Params::pick`], or serde through [`crate::Params::choice`]), so a
//! choice's names are written once for the table and the reader.
//!
//! [`choices!`] implements it from the variants and their names, in the order the command
//! reference lists them; a `match` it writes makes the compiler refuse a variant left out. The
//! types that also deserialize are checked to read their names as serde does
//! (`tests::reads_as_serde`, from `tests::session`).
//!
//! ```
//! use emcraft_engine::Choice;
//! use emcraft_model::units::Frequency;
//!
//! assert_eq!(Frequency::named("MHz"), Some(Frequency::Megahertz));
//! assert_eq!(Frequency::Gigahertz.name(), "GHz");
//! assert_eq!(Frequency::listed(), "GHz, MHz or Hz");
//! ```

use emcraft_model::units::{Frequency, Length};
use emcraft_model::{Axis, Combine, End, ModeSolver, OpenShape, Polarization, SymmetryKind, Terminals};

/// A parameter's value chosen by name.
pub trait Choice: Copy + PartialEq + 'static {
    /// Every value, in the order of [`Choice::NAMES`].
    const ALL: &'static [Self];
    /// The names, as parameters give them.
    const NAMES: &'static [&'static str];

    /// The value's name.
    fn name(self) -> &'static str {
        Self::ALL.iter().position(|v| *v == self).and_then(|i| Self::NAMES.get(i)).copied().unwrap_or("?")
    }

    /// The value named `name`.
    fn named(name: &str) -> Option<Self> {
        Self::NAMES.iter().position(|n| *n == name).and_then(|i| Self::ALL.get(i)).copied()
    }

    /// The names as a sentence lists them: "a, b or c".
    fn listed() -> String {
        match Self::NAMES.split_last() {
            None => String::new(),
            Some((last, [])) => (*last).to_owned(),
            Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
        }
    }
}

/// Implements [`Choice`] for each type from its variants and their names:
/// `choices! { End { End::Start => "start", End::End => "end" } }`.
macro_rules! choices {
    ($($ty:ty { $($variant:path => $name:literal),+ $(,)? })*) => {$(
        impl $crate::choice::Choice for $ty {
            const ALL: &'static [Self] = &[$($variant),+];
            const NAMES: &'static [&'static str] = &[$($name),+];
        }

        // Every variant is listed: a new one fails this match.
        const _: fn($ty) = |v| match v {
            $($variant => {}),+
        };
    )*};
}

pub(crate) use choices;

choices! {
    Length { Length::Millimetre => "mm", Length::Metre => "m" }
    Frequency { Frequency::Gigahertz => "GHz", Frequency::Megahertz => "MHz", Frequency::Hertz => "Hz" }
    Axis { Axis::X => "x", Axis::Y => "y", Axis::Z => "z" }
    Combine { Combine::Add => "add", Combine::Subtract => "subtract", Combine::Intersect => "intersect" }
    End { End::Start => "start", End::End => "end" }
    ModeSolver { ModeSolver::Auto => "auto", ModeSolver::Numeric => "numeric" }
    Terminals { Terminals::Auto => "auto", Terminals::Modal => "modal" }
    SymmetryKind { SymmetryKind::Electric => "electric", SymmetryKind::Magnetic => "magnetic" }
    Polarization { Polarization::Te => "te", Polarization::Tm => "tm" }
    OpenShape { OpenShape::Box => "box", OpenShape::Sphere => "sphere" }
    emcraft_gpu::Precision { emcraft_gpu::Precision::Single => "single", emcraft_gpu::Precision::Double => "double" }
}

#[cfg(test)]
pub(crate) mod tests {
    use serde::de::DeserializeOwned;
    use serde_json::json;

    use super::*;

    /// `T`'s names read as serde reads them, each to its own value.
    pub(crate) fn reads_as_serde<T: Choice + DeserializeOwned + std::fmt::Debug>() {
        assert_eq!(T::ALL.len(), T::NAMES.len());
        for (v, n) in T::ALL.iter().zip(T::NAMES) {
            assert_eq!(v.name(), *n);
            assert_eq!(T::named(n), Some(*v));
            assert_eq!(serde_json::from_value::<T>(json!(n)).ok(), Some(*v), "{n}");
        }
    }

    #[test]
    fn names_list_and_read_back() {
        assert_eq!(End::listed(), "start or end");
        assert_eq!(Combine::listed(), "add, subtract or intersect");
        assert_eq!(End::named("middle"), None);
        assert_eq!(Choice::name(emcraft_gpu::Precision::Double), "double");
    }
}
