//! Enums whose variants each have a text id (and a label): views, ribbon tabs, glyphs, the
//! fixed handles, the solvers. [`named_enum!`] writes the enum from one list of its variants,
//! with `ALL` (in the list's order), `id()`, `label()` (the id unless given), `parse()` and
//! `Display` (the id). The ids are checked distinct when the crate compiles, so `parse`
//! finds each variant by its own id.
//!
//! ```ignore
//! named_enum! {
//!     /// A view.
//!     #[derive(Clone, Copy, Debug, PartialEq, Eq)]
//!     pub enum View {
//!         Model = "model" => "Model 3D",
//!         Field = "field" => "Field",
//!     }
//! }
//! ```

/// Whether two texts are the same, in a constant.
pub(crate) const fn same(a: &str, b: &str) -> bool {
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
pub(crate) const fn has_twins(ids: &[&str]) -> bool {
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
/// "Label"`), see the [module](self).
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
                    $($name::$variant => $crate::named::named_enum!(@label $id $($label)?)),+
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
        const _: () = assert!(!$crate::named::has_twins(&[$($id),+]), concat!("two variants of ", stringify!($name), " share an id"));
    };
    (@label $id:literal $label:literal) => {
        $label
    };
    (@label $id:literal) => {
        $id
    };
}

pub(crate) use named_enum;

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
        assert_eq!((Fruit::Apple.id(), Fruit::Apple.label(), Fruit::Pear.label()), ("apple", "An apple", "pear"));
        assert_eq!((Fruit::parse("pear"), Fruit::parse("Pear")), (Some(Fruit::Pear), None));
        assert_eq!(Fruit::Pear.to_string(), "pear");
        for f in Fruit::ALL {
            assert_eq!(Fruit::parse(f.id()), Some(f));
        }
        assert!(has_twins(&["a", "b", "a"]) && !has_twins(&["a", "b", "ab"]) && !has_twins(&[]));
        assert!(same("tab:MODEL", "tab:MODEL") && !same("tab:MODEL", "tab:MODEl"));
    }
}
