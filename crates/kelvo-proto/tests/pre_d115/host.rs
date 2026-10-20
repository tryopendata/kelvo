//! `kelvo-schema/src/host.rs` at 2b0b7cb: the wire types of `Hello.host`.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HostId(pub Uuid);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HostIdentity {
    pub id: HostId,
    pub display_name: String,
    pub info: HostInfo,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HostInfo {
    pub os: OsKind,
    pub os_version: String,
    pub model: Option<String>,
    pub chip: Option<String>,
    pub chip_known: bool,
    pub cpu_topology: Vec<ClusterInfo>,
    pub mem_total_bytes: u64,
    pub boot_time_ms: i64,
    #[serde(default)]
    pub gpu_dvfs_mhz: Vec<u32>,
    #[serde(default)]
    pub boot_mounts: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OsKind {
    MacOs,
    Linux,
    Unknown,
}

impl OsKind {
    pub const ALL: [OsKind; 2] = [OsKind::MacOs, OsKind::Linux];

    pub const fn as_str(self) -> &'static str {
        match self {
            OsKind::MacOs => "mac_os",
            OsKind::Linux => "linux",
            OsKind::Unknown => "unknown",
        }
    }
}

crate::pre_d115::compat::text_enum_deserialize!(OsKind);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClusterInfo {
    pub name: String,
    pub kind: CoreKind,
    pub cores: Vec<String>,
    pub dvfs_mhz: Vec<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CoreKind {
    Performance,
    Efficiency,
    Unknown,
}

impl CoreKind {
    pub const ALL: [CoreKind; 2] = [CoreKind::Performance, CoreKind::Efficiency];

    pub const fn as_str(self) -> &'static str {
        match self {
            CoreKind::Performance => "performance",
            CoreKind::Efficiency => "efficiency",
            CoreKind::Unknown => "unknown",
        }
    }
}

crate::pre_d115::compat::text_enum_deserialize!(CoreKind);
