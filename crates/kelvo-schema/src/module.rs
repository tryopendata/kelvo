//! [`Module`]: the UI and settings modules every metric is filed under.

use serde::{Deserialize, Serialize};

/// A UI and settings module. Every metric belongs to exactly one.
///
/// Serialized in snake_case (`"cpu"`), the same text the store writes to `gaps.module`.
/// The UI shows `Power` and `Sensors` as one "Power & Sensors" module; see
/// [`crate::settings`] for how settings treat the pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, specta::Type)]
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
    /// cgroup v2 groups on a Linux host: services, containers and user sessions
    /// (`cgroup.*`, D-116). A data and capabilities module only: it has no settings
    /// entry, no menu-bar item and no module switch, so it is not in
    /// [`crate::settings::SETTINGS_MODULES`] and settings code never offers it. A Mac
    /// never produces it; its local Docker page has its own model (D-103).
    Containers,
    /// A module this build does not know, sent by a newer peer or written by a newer
    /// build (D-040). Never produced locally and not in [`Module::ALL`]. Consumers skip
    /// it: capabilities and settings drop it on decode, the store skips its gaps.
    Unknown,
}

impl Module {
    /// Every known module (not `Unknown`), for decoding and the store's text form. Code
    /// that builds settings or menu-bar UI iterates
    /// [`crate::settings::SETTINGS_MODULES`] instead, which leaves out `Sensors` and
    /// `Containers`.
    pub const ALL: [Module; 9] = [
        Module::Cpu,
        Module::Gpu,
        Module::Memory,
        Module::Power,
        Module::Sensors,
        Module::Network,
        Module::Disk,
        Module::Battery,
        Module::Containers,
    ];

    /// The snake_case text form, as stored in `gaps.module`.
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
            Module::Containers => "containers",
            Module::Unknown => "unknown",
        }
    }

    /// Inverse of [`Module::as_str`] for the known modules. `None` for anything else,
    /// including `"unknown"`.
    pub fn parse(s: &str) -> Option<Module> {
        Module::ALL.into_iter().find(|m| m.as_str() == s)
    }
}

crate::compat::text_enum_deserialize!(Module);

/// `deserialize_with` for a map keyed by [`Module`]: entries for modules this build does
/// not know are dropped, so a newer peer's extra module neither fails the decode nor
/// shows up as an `"unknown"` key (D-040).
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unseen_module_decodes_as_unknown() {
        let mut buf = Vec::new();
        ciborium::into_writer(&"npu", &mut buf).unwrap();
        assert_eq!(
            ciborium::from_reader::<Module, _>(buf.as_slice()).unwrap(),
            Module::Unknown
        );
        assert_eq!(Module::parse("unknown"), None);
        assert!(!Module::ALL.contains(&Module::Unknown));
    }

    #[test]
    fn module_text_round_trip() {
        for m in Module::ALL {
            assert_eq!(Module::parse(m.as_str()), Some(m));
            assert_eq!(
                serde_json::to_string(&m).unwrap(),
                format!("\"{}\"", m.as_str())
            );
        }
    }
}
