//! Capabilities (architecture.md infra 9). They change at runtime: an eGPU is unplugged, a
//! disk is ejected. The engine re-probes and emits `CapabilitiesChanged` with a higher
//! `revision`.

use std::collections::BTreeMap;

use serde::de::{self, IgnoredAny, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use crate::catalog::{Module, deserialize_known_modules};

/// What a host can measure right now, per module.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct Capabilities {
    /// Entries for modules this build does not know are dropped on decode (D-040).
    #[serde(deserialize_with = "deserialize_known_modules")]
    #[specta(type = BTreeMap<Module, ModuleCap>)]
    pub modules: BTreeMap<Module, ModuleCap>,
    /// Increases on every change, so a receiver can drop a stale update. Exported to TS
    /// as `number`; see [`crate::JS_SAFE_INT_MAX`].
    #[specta(type = crate::JsSafeInt)]
    pub revision: u64,
    /// Process rows can carry network rates (`net_rx_bps`, `net_tx_bps`) while a view asks
    /// for them: the in-process NetworkStatistics API loaded and the build may use it
    /// (D-081). When false the UI hides the network columns. Defaults to false when a
    /// sender does not know it.
    #[serde(default)]
    pub process_network: bool,
    /// Process rows can carry GPU time (`gpu_pct`) while a view asks for it: the GPU's
    /// IORegistry user clients report per-process `accumulatedGPUTime` and the build may
    /// read them (D-085). When false the UI hides the GPU columns. Defaults to false.
    #[serde(default)]
    pub process_gpu: bool,
}

impl Capabilities {
    /// The state of `module`. A module missing from the map is [`ModuleCap::NotPresent`].
    pub fn module(&self, module: Module) -> &ModuleCap {
        self.modules.get(&module).unwrap_or(&ModuleCap::NotPresent)
    }
}

/// The UI maps these to its three states: data, "not available" and unknown chip.
///
/// Deserialization is hand-written so that a state added by a newer peer, with or without
/// content, decodes as [`ModuleCap::Unknown`] instead of failing the whole message
/// (`#[serde(other)]` only covers content-free values; D-038, D-040).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ModuleCap {
    /// The module produces `series` series.
    Available { series: u32 },
    /// A collector exists for the module but cannot run.
    Unsupported(UnsupportedReason),
    /// The host does not have this hardware (no battery on a Mac mini).
    NotPresent,
    /// A state this build does not know (D-040). Never produced locally; the UI shows it
    /// like "not available".
    Unknown,
}

impl<'de> Deserialize<'de> for ModuleCap {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(ModuleCapVisitor)
    }
}

/// Reads serde's externally tagged form: a bare string for unit variants
/// (`"not_present"`), a one-entry map for the others (`{"available": {"series": 16}}`).
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum UnsupportedReason {
    /// The sensor map does not know this chip yet ("Share sensor dump").
    UnknownChip,
    /// The build cannot hold the entitlement the collector needs ("not available in this
    /// edition").
    MissingEntitlement,
    /// The API is there but reports no such hardware.
    NoHardware,
    /// A reason this build does not know (D-040). The UI shows a generic
    /// "not available".
    Unknown,
}

impl UnsupportedReason {
    /// Every known reason (not `Unknown`).
    pub const ALL: [UnsupportedReason; 3] = [
        UnsupportedReason::UnknownChip,
        UnsupportedReason::MissingEntitlement,
        UnsupportedReason::NoHardware,
    ];

    /// The snake_case text form, as serialized.
    pub const fn as_str(self) -> &'static str {
        match self {
            UnsupportedReason::UnknownChip => "unknown_chip",
            UnsupportedReason::MissingEntitlement => "missing_entitlement",
            UnsupportedReason::NoHardware => "no_hardware",
            UnsupportedReason::Unknown => "unknown",
        }
    }
}

crate::compat::text_enum_deserialize!(UnsupportedReason);

/// What a collector needs to run. These are capability requirements, not codesign
/// entitlements; an `appstore` build allows only [`Entitlement::None`].
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum Entitlement {
    /// Sandbox-safe public APIs.
    None,
    /// Private IOReport framework.
    IoReport,
    /// AppleSMC IOKit user client.
    SmcUserClient,
    /// IOHIDEventSystemClient thermal sensors.
    HidSensors,
    /// Private NetworkStatistics framework.
    NetworkStatistics,
    /// Per-process GPU time from the IORegistry.
    IoRegistryGpuClients,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Capabilities {
        let mut modules = BTreeMap::new();
        modules.insert(Module::Cpu, ModuleCap::Available { series: 16 });
        modules.insert(
            Module::Sensors,
            ModuleCap::Unsupported(UnsupportedReason::UnknownChip),
        );
        modules.insert(Module::Battery, ModuleCap::NotPresent);
        Capabilities {
            modules,
            revision: 3,
            process_network: false,
            process_gpu: false,
        }
    }

    #[test]
    fn json_shape() {
        assert_eq!(
            serde_json::to_string(&sample()).unwrap(),
            r#"{"modules":{"cpu":{"available":{"series":16}},"sensors":{"unsupported":"unknown_chip"},"battery":"not_present"},"revision":3,"process_network":false,"process_gpu":false}"#
        );
    }

    #[test]
    fn round_trips_cbor() {
        let c = sample();
        let mut buf = Vec::new();
        ciborium::into_writer(&c, &mut buf).unwrap();
        assert_eq!(
            ciborium::from_reader::<Capabilities, _>(buf.as_slice()).unwrap(),
            c
        );
    }

    #[test]
    fn missing_module_is_not_present() {
        assert_eq!(sample().module(Module::Gpu), &ModuleCap::NotPresent);
    }
}
