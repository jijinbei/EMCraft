//! EMCraft's declarative macros, shared by its crates. No workspace dependencies: any crate may
//! use them (docs/architecture.md).
//!
//! [`named_enum!`] writes an enum whose variants each have a text id (and a label) — views,
//! ribbon tabs, units, axes, workflow steps, backends, the choices command parameters take —
//! from one list of its variants, with `ALL` (in the list's order), `COUNT`, `IDS`, `id()`,
//! `label()` (the id unless given), `parse()` (the id or an alias, `"h" | "h-plane"`),
//! `Display` (the id) and [`Named`], what generic code knows of one. The ids and aliases are
//! checked distinct when the crate compiles, so `parse` finds each variant by its own. The
//! enum must be `Copy`. [`named_serde!`] gives it serde by the same ids, so a name is written
//! once for the command table, the parameter reader and the files.
//!
//! ```
//! emcraft_macros::named_enum! {
//!     /// A view.
//!     #[derive(Clone, Copy, Debug, PartialEq, Eq)]
//!     pub enum View {
//!         Model = "model" => "Model 3D",
//!         Field = "field" => "Field",
//!         Mesh = "mesh",
//!     }
//! }
//!
//! assert_eq!(View::ALL, [View::Model, View::Field, View::Mesh]);
//! assert_eq!((View::Model.id(), View::Model.label(), View::Mesh.label()), ("model", "Model 3D", "mesh"));
//! assert_eq!((View::parse("field"), View::parse("Field")), (Some(View::Field), None));
//! assert_eq!(View::Field.to_string(), "field");
//! ```
//!
//! Two variants with one id stop the build:
//!
//! ```compile_fail
//! emcraft_macros::named_enum! {
//!     #[derive(Clone, Copy)]
//!     enum Twins {
//!         A = "a",
//!         B = "a",
//!     }
//! }
//! ```

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

/// Whether two texts are the same, in a constant.
pub const fn same(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// Whether any two of `ids` are the same, in a constant.
pub const fn has_twins(ids: &[&str]) -> bool {
    let mut i = 0;
    while i < ids.len() {
        let mut j = i + 1;
        while j < ids.len() {
            if same(ids[i], ids[j]) {
                return true;
            }
            j += 1;
        }
        i += 1;
    }
    false
}

/// An enum named by text, as [`named_enum!`] writes it: what generic code (a command's
/// parameter reader, its reference) knows of one.
pub trait Named: Copy + 'static {
    /// Every one, in the order they are listed.
    const ALL: &'static [Self];
    /// Their ids, in the same order.
    const IDS: &'static [&'static str];

    /// Its id.
    fn id(self) -> &'static str;

    /// Its label (its id unless given).
    fn label(self) -> &'static str;

    /// The one with id (or alias) `s`.
    fn parse(s: &str) -> Option<Self>;

    /// The ids as a sentence lists them: "a, b or c".
    fn listed() -> String {
        match Self::IDS.split_last() {
            None => String::new(),
            Some((last, [])) => (*last).to_owned(),
            Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
        }
    }
}

/// Writes an enum from its variants and their ids (`Variant = "id"`, `Variant = "id" | "alias"`,
/// `Variant = "id" => "Label"`), see the [crate docs](crate).
#[macro_export]
macro_rules! named_enum {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $( $(#[$vmeta:meta])* $variant:ident = $id:literal $(| $alias:literal)* $(=> $label:literal)? ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        $vis enum $name {
            $( $(#[$vmeta])* $variant ),+
        }

        impl $name {
            /// How many there are.
            pub const COUNT: usize = [$($id),+].len();

            /// Every one, in the order they are listed.
            pub const ALL: [$name; $name::COUNT] = [$($name::$variant),+];

            /// Their ids, in the same order.
            pub const IDS: [&'static str; $name::COUNT] = [$($id),+];

            /// Its id.
            pub const fn id(self) -> &'static str {
                match self {
                    $($name::$variant => $id),+
                }
            }

            /// Its label.
            #[allow(dead_code)]
            pub const fn label(self) -> &'static str {
                match self {
                    $($name::$variant => $crate::named_enum!(@label $id $($label)?)),+
                }
            }

            /// The one with id (or alias) `s`.
            #[allow(dead_code)]
            pub fn parse(s: &str) -> Option<$name> {
                match s {
                    $($id $(| $alias)* => Some($name::$variant),)+
                    _ => None,
                }
            }
        }

        impl $crate::Named for $name {
            const ALL: &'static [Self] = &$name::ALL;
            const IDS: &'static [&'static str] = &$name::IDS;

            fn id(self) -> &'static str {
                $name::id(self)
            }

            fn label(self) -> &'static str {
                $name::label(self)
            }

            fn parse(s: &str) -> Option<Self> {
                $name::parse(s)
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(self.id())
            }
        }

        // Every id and alias is its variant's own: a repeated one stops the build.
        const _: () = assert!(!$crate::has_twins(&[$($id $(, $alias)*),+]), concat!("two variants of ", stringify!($name), " share an id"));
    };
    (@label $id:literal $label:literal) => {
        $label
    };
    (@label $id:literal) => {
        $id
    };
}

/// Serde for enums written by [`named_enum!`]: each is written as its id and read from its id
/// or an alias; anything else is refused with the ids it could have been. The crate using it
/// depends on `serde`.
///
/// ```
/// emcraft_macros::named_enum! {
///     #[derive(Clone, Copy, Debug, PartialEq, Eq)]
///     enum Plane {
///         H = "h" | "h-plane",
///         E = "e" | "e-plane",
///     }
/// }
/// emcraft_macros::named_serde!(Plane);
///
/// assert_eq!(serde_json::to_string(&Plane::E).ok(), Some("\"e\"".to_owned()));
/// assert_eq!(serde_json::from_str::<Plane>("\"h-plane\"").ok(), Some(Plane::H));
/// let refused = serde_json::from_str::<Plane>("\"x\"").err().map(|e| e.to_string());
/// assert_eq!(refused.as_deref(), Some("unknown variant `x`, expected `h` or `e` at line 1 column 3"));
/// ```
#[macro_export]
macro_rules! named_serde {
    ($($name:ty),+ $(,)?) => {$(
        impl ::serde::Serialize for $name {
            fn serialize<S: ::serde::Serializer>(&self, s: S) -> ::std::result::Result<S::Ok, S::Error> {
                s.serialize_str(<$name>::id(*self))
            }
        }

        impl<'de> ::serde::Deserialize<'de> for $name {
            fn deserialize<D: ::serde::Deserializer<'de>>(d: D) -> ::std::result::Result<$name, D::Error> {
                struct Ids;

                impl ::serde::de::Visitor<'_> for Ids {
                    type Value = $name;

                    fn expecting(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                        write!(f, "one of {}", <$name as $crate::Named>::listed())
                    }

                    fn visit_str<E: ::serde::de::Error>(self, s: &str) -> ::std::result::Result<$name, E> {
                        <$name>::parse(s).ok_or_else(|| E::unknown_variant(s, &<$name>::IDS))
                    }
                }

                d.deserialize_str(Ids)
            }
        }
    )+};
}

#[cfg(test)]
mod tests {
    use super::*;

    named_enum! {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        enum Fruit {
            Apple = "apple" => "An apple",
            Pear = "pear",
        }
    }

    #[test]
    fn a_named_enum_reads_and_writes_its_ids() {
        assert_eq!(Fruit::ALL, [Fruit::Apple, Fruit::Pear]);
        assert_eq!(Fruit::COUNT, 2);
        for f in Fruit::ALL {
            assert_eq!(Fruit::parse(f.id()), Some(f));
        }
        assert_eq!((<Fruit as Named>::IDS, Fruit::listed()), (&["apple", "pear"][..], "apple or pear".to_owned()));
        assert!(has_twins(&["a", "b", "a"]) && !has_twins(&["a", "b", "ab"]) && !has_twins(&[]));
        assert!(same("tab:MODEL", "tab:MODEL") && !same("tab:MODEL", "tab:MODEl"));
    }
}
