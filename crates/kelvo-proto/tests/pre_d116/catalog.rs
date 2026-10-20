//! `kelvo-schema/src/catalog.rs` at 2b0b7cb: `Module` and the known-module map decoder.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Module {
    Cpu,
    Gpu,
    Memory,
    Power,
    Sensors,
    Network,
    Disk,
    Battery,
    Unknown,
}

impl Module {
    pub const ALL: [Module; 8] = [
        Module::Cpu,
        Module::Gpu,
        Module::Memory,
        Module::Power,
        Module::Sensors,
        Module::Network,
        Module::Disk,
        Module::Battery,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Module::Cpu => "cpu",
            Module::Gpu => "gpu",
            Module::Memory => "memory",
            Module::Power => "power",
            Module::Sensors => "sensors",
            Module::Network => "network",
            Module::Disk => "disk",
            Module::Battery => "battery",
            Module::Unknown => "unknown",
        }
    }
}

crate::pre_d116::compat::text_enum_deserialize!(Module);

pub(crate) fn deserialize_known_modules<'de, D, V>(
    deserializer: D,
) -> Result<std::collections::BTreeMap<Module, V>, D::Error>
where
    D: serde::Deserializer<'de>,
    V: Deserialize<'de>,
{
    let mut map = std::collections::BTreeMap::<Module, V>::deserialize(deserializer)?;
    map.remove(&Module::Unknown);
    Ok(map)
}
