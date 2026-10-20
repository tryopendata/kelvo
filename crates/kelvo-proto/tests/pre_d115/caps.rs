//! `kelvo-schema/src/caps.rs` at 2b0b7cb: `Capabilities` and its decoders.

use std::collections::BTreeMap;

use serde::de::{self, IgnoredAny, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use super::catalog::{Module, deserialize_known_modules};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Capabilities {
    #[serde(deserialize_with = "deserialize_known_modules")]
    pub modules: BTreeMap<Module, ModuleCap>,
    pub revision: u64,
    #[serde(default)]
    pub process_network: bool,
    #[serde(default)]
    pub process_gpu: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModuleCap {
    Available { series: u32 },
    Unsupported(UnsupportedReason),
    NotPresent,
    Unknown,
}

impl<'de> Deserialize<'de> for ModuleCap {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(ModuleCapVisitor)
    }
}

struct ModuleCapVisitor;

impl<'de> Visitor<'de> for ModuleCapVisitor {
    type Value = ModuleCap;

    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a module capability")
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<ModuleCap, E> {
        Ok(match v {
            "not_present" => ModuleCap::NotPresent,
            _ => ModuleCap::Unknown,
        })
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<ModuleCap, A::Error> {
        #[derive(Deserialize)]
        struct Available {
            series: u32,
        }
        let Some(tag) = map.next_key::<String>()? else {
            return Err(de::Error::invalid_length(0, &self));
        };
        let cap = match tag.as_str() {
            "available" => ModuleCap::Available {
                series: map.next_value::<Available>()?.series,
            },
            "unsupported" => ModuleCap::Unsupported(map.next_value()?),
            "not_present" => {
                map.next_value::<IgnoredAny>()?;
                ModuleCap::NotPresent
            }
            _ => {
                map.next_value::<IgnoredAny>()?;
                ModuleCap::Unknown
            }
        };
        if map.next_key::<IgnoredAny>()?.is_some() {
            return Err(de::Error::invalid_length(2, &self));
        }
        Ok(cap)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UnsupportedReason {
    UnknownChip,
    MissingEntitlement,
    NoHardware,
    Unknown,
}

impl UnsupportedReason {
    pub const ALL: [UnsupportedReason; 3] = [
        UnsupportedReason::UnknownChip,
        UnsupportedReason::MissingEntitlement,
        UnsupportedReason::NoHardware,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            UnsupportedReason::UnknownChip => "unknown_chip",
            UnsupportedReason::MissingEntitlement => "missing_entitlement",
            UnsupportedReason::NoHardware => "no_hardware",
            UnsupportedReason::Unknown => "unknown",
        }
    }
}

crate::pre_d115::compat::text_enum_deserialize!(UnsupportedReason);
