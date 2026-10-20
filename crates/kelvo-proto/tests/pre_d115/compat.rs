//! `kelvo-schema/src/compat.rs` at 2b0b7cb: the text-enum `Unknown` fallback (D-040).

use std::fmt;
use std::marker::PhantomData;

use serde::Deserializer;
use serde::de::{self, Visitor};

pub(crate) trait TextEnum: Copy + 'static {
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

macro_rules! text_enum_deserialize {
    ($ty:ty) => {
        impl $crate::pre_d115::compat::TextEnum for $ty {
            const KNOWN: &'static [Self] = &<$ty>::ALL;
            const UNKNOWN: Self = <$ty>::Unknown;
            fn text(self) -> &'static str {
                self.as_str()
            }
        }

        impl<'de> serde::Deserialize<'de> for $ty {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                $crate::pre_d115::compat::deserialize_text(d)
            }
        }
    };
}

pub(crate) use text_enum_deserialize;
