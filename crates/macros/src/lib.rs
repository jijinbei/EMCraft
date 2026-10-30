//! EMCraft's declarative macros, shared by its crates. No workspace dependencies: any crate may
//! use them (docs/architecture.md).
//!
//! [`named_enum!`] writes an enum whose variants each have a text id (and a label) — views,
//! ribbon tabs, workflow steps, backends, the names commands give — from one list of its
//! variants, with `ALL` (in the list's order), `COUNT`, `id()`, `label()` (the id unless
//! given), `parse()` and `Display` (the id). The ids are checked distinct when the crate
//! compiles, so `parse` finds each variant by its own id. The enum must be `Copy`.
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

/// Writes an enum from its variants and their ids (`Variant = "id"`, or `Variant = "id" =>
/// "Label"`), see the [crate docs](crate).
#[macro_export]
macro_rules! named_enum {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $( $(#[$vmeta:meta])* $variant:ident = $id:literal $(=> $label:literal)? ),+ $(,)?
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

            /// The one with id `s`.
            #[allow(dead_code)]
            pub fn parse(s: &str) -> Option<$name> {
                $name::ALL.into_iter().find(|v| v.id() == s)
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(self.id())
            }
        }

        // Every id is its variant's own: a repeated one stops the build.
        const _: () = assert!(!$crate::has_twins(&[$($id),+]), concat!("two variants of ", stringify!($name), " share an id"));
    };
    (@label $id:literal $label:literal) => {
        $label
    };
    (@label $id:literal) => {
        $id
    };
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
        assert!(has_twins(&["a", "b", "a"]) && !has_twins(&["a", "b", "ab"]) && !has_twins(&[]));
        assert!(same("tab:MODEL", "tab:MODEL") && !same("tab:MODEL", "tab:MODEl"));
    }
}
