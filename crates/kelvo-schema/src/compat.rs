//! Forward compatibility for enums that cross the wire or are stored as text (D-040).
//!
//! A newer peer or a newer build's database can carry an enum value this build has never
//! seen. Failing the whole message (or row) on it breaks the "mismatched versions
//! interoperate" rule, so every such enum has an `Unknown` variant that unseen values
//! decode to.
//!
//! `#[serde(other)]` would do this for plain string enums, but specta-serde refuses to
//! export it on an externally tagged enum, so the schema's string enums implement
//! `Deserialize` through [`text_enum_deserialize!`] instead. `Serialize` stays derived
//! with `rename_all = "snake_case"`; a test keeps each enum's `as_str` equal to its
//! serialized form.

use std::fmt;
use std::marker::PhantomData;

use serde::Deserializer;
use serde::de::{self, Visitor};

/// A unit-only enum with a text form and an `Unknown` fallback.
pub(crate) trait TextEnum: Copy + 'static {
    /// Every value except `Unknown`.
    const KNOWN: &'static [Self];
    const UNKNOWN: Self;
    fn text(self) -> &'static str;
}

pub(crate) fn deserialize_text<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: TextEnum,
{
    struct TextVisitor<T>(PhantomData<T>);

    impl<T: TextEnum> Visitor<'_> for TextVisitor<T> {
        type Value = T;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a snake_case string")
        }

        fn visit_str<E: de::Error>(self, v: &str) -> Result<T, E> {
            Ok(T::KNOWN
                .iter()
                .copied()
                .find(|k| k.text() == v)
                .unwrap_or(T::UNKNOWN))
        }
    }

    deserializer.deserialize_str(TextVisitor(PhantomData))
}

/// Implements [`TextEnum`] and `Deserialize` for `$ty` from its `ALL` list (known values)
/// and `as_str`.
macro_rules! text_enum_deserialize {
    ($ty:ty) => {
        impl $crate::compat::TextEnum for $ty {
            const KNOWN: &'static [Self] = &<$ty>::ALL;
            const UNKNOWN: Self = <$ty>::Unknown;
            fn text(self) -> &'static str {
                self.as_str()
            }
        }

        impl<'de> serde::Deserialize<'de> for $ty {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                $crate::compat::deserialize_text(d)
            }
        }
    };
}

pub(crate) use text_enum_deserialize;
