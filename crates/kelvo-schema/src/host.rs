//! Host identity (architecture.md infra 2, a one-way door).
//!
//! Each machine gets a random UUID on first run. [`HostId`] is that UUID everywhere outside
//! SQLite: commands, channels, events, the proto and the frontend's per-host stores. Being
//! local is a flag on [`HostRecord`], not a special ID, and it is the controller's own
//! fact about a host: a peer introduces itself with a [`HostIdentity`], which has no such
//! flag (D-064). Inside SQLite the store interns the UUID to a small integer (`HostRef`)
//! that never leaves the database.

use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A host's persistent identity. Serialized as the hyphenated UUID string.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, specta::Type,
)]
#[serde(transparent)]
pub struct HostId(pub Uuid);

impl fmt::Display for HostId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.hyphenated().fmt(f)
    }
}

/// A known host as the controller sees it: listed by `list_hosts`, stored in the `hosts`
/// table. Never sent to a peer; the proto `Hello` carries a [`HostIdentity`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct HostRecord {
    pub id: HostId,
    /// This machine. At most one stored host is local (a partial unique index in the
    /// store); it is never taken from what a peer says about itself.
    pub is_local: bool,
    /// User-facing name, for example "MacBook Pro".
    pub display_name: String,
    pub info: HostInfo,
}

impl HostRecord {
    /// A record for a host that introduced itself with `identity`, local or not by the
    /// controller's own knowledge.
    pub fn from_identity(identity: HostIdentity, is_local: bool) -> Self {
        Self {
            id: identity.id,
            is_local,
            display_name: identity.display_name,
            info: identity.info,
        }
    }

    /// What this host says about itself to a peer: everything but `is_local`.
    pub fn identity(&self) -> HostIdentity {
        HostIdentity {
            id: self.id,
            display_name: self.display_name.clone(),
            info: self.info.clone(),
        }
    }
}

/// A host as it introduces itself on the wire (proto `Hello`, a one-way door). Whether
/// the receiver treats it as local is the receiver's business, so there is no `is_local`
/// here (D-064).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HostIdentity {
    pub id: HostId,
    /// User-facing name, for example "MacBook Pro".
    pub display_name: String,
    pub info: HostInfo,
}

/// Static facts about a host, read once at startup.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct HostInfo {
    pub os: OsKind,
    pub os_version: String,
    /// Model identifier, for example "Mac15,8".
    pub model: Option<String>,
    /// Marketing chip name, for example "Apple M3 Max".
    pub chip: Option<String>,
    /// False when the sensor map does not know this chip; drives the unknown-chip state.
    pub chip_known: bool,
    /// CPU clusters in the order the collector reports them. Empty when unknown.
    pub cpu_topology: Vec<ClusterInfo>,
    /// Exported to TS as `number`; see [`crate::JS_SAFE_INT_MAX`].
    #[specta(type = crate::JsSafeInt)]
    pub mem_total_bytes: u64,
    /// Millisecond epoch. Exported to TS as `number`; see [`crate::JS_SAFE_INT_MAX`].
    #[specta(type = crate::JsSafeInt)]
    pub boot_time_ms: i64,
    /// The GPU's DVFS operating points in MHz, in the order of `gpu.residency`'s `state`
    /// labels and without the off state; not necessarily ascending, so the top frequency
    /// is the largest entry. Empty when unknown, and from a peer or record written before
    /// D-092.
    #[serde(default)]
    pub gpu_dvfs_mhz: Vec<u32>,
    /// The mount points of the boot volume's APFS container that `disk.*` series are
    /// labelled with, `/` first (`/`, `/System/Volumes/Data`). Empty when unknown, and
    /// from a peer or record written before D-092.
    #[serde(default)]
    pub boot_mounts: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum OsKind {
    MacOs,
    Linux,
    /// An OS this build does not know, from a newer peer (D-040).
    Unknown,
}

impl OsKind {
    /// Every known OS (not `Unknown`).
    pub const ALL: [OsKind; 2] = [OsKind::MacOs, OsKind::Linux];

    /// The snake_case text form, as serialized.
    pub const fn as_str(self) -> &'static str {
        match self {
            OsKind::MacOs => "mac_os",
            OsKind::Linux => "linux",
            OsKind::Unknown => "unknown",
        }
    }
}

crate::compat::text_enum_deserialize!(OsKind);

/// One CPU cluster and its DVFS state table.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct ClusterInfo {
    /// The `cluster` label value used by `cpu.cluster.*` series, for example "P0" or "E0".
    pub name: String,
    pub kind: CoreKind,
    /// The `core` label values of the cores in this cluster, for example `["P0", "P1"]`.
    pub cores: Vec<String>,
    /// DVFS operating points in MHz, ascending. The last entry is the cluster's maximum
    /// frequency (the CPU page's "max 4.51" ring label). These are also the `state` label
    /// values of `cpu.cluster.residency` besides `idle`.
    pub dvfs_mhz: Vec<u32>,
}

impl ClusterInfo {
    /// Highest DVFS frequency in Hz, the same unit as `cpu.cluster.freq`.
    pub fn max_freq_hz(&self) -> Option<f64> {
        self.dvfs_mhz.iter().max().map(|&mhz| f64::from(mhz) * 1e6)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum CoreKind {
    Performance,
    Efficiency,
    /// A core type this build does not know, from a newer peer (D-040).
    Unknown,
}

impl CoreKind {
    /// Every known core kind (not `Unknown`).
    pub const ALL: [CoreKind; 2] = [CoreKind::Performance, CoreKind::Efficiency];

    /// The snake_case text form, as serialized.
    pub const fn as_str(self) -> &'static str {
        match self {
            CoreKind::Performance => "performance",
            CoreKind::Efficiency => "efficiency",
            CoreKind::Unknown => "unknown",
        }
    }
}

crate::compat::text_enum_deserialize!(CoreKind);

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn sample_record() -> HostRecord {
        HostRecord {
            id: HostId(Uuid::from_u128(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef)),
            is_local: true,
            display_name: "MacBook Pro".into(),
            info: HostInfo {
                os: OsKind::MacOs,
                os_version: "27.0".into(),
                model: Some("Mac16,6".into()),
                chip: Some("Apple M4 Pro".into()),
                chip_known: true,
                cpu_topology: vec![ClusterInfo {
                    name: "P0".into(),
                    kind: CoreKind::Performance,
                    cores: vec!["P0".into(), "P1".into()],
                    dvfs_mhz: vec![1260, 2424, 4512],
                }],
                mem_total_bytes: 24 << 30,
                boot_time_ms: 1_790_000_000_000,
                gpu_dvfs_mhz: vec![338, 618, 1380],
                boot_mounts: vec!["/".into(), "/System/Volumes/Data".into()],
            },
        }
    }

    #[test]
    fn host_id_serializes_as_uuid_string() {
        let id = sample_record().id;
        assert_eq!(
            serde_json::to_string(&id).unwrap(),
            "\"01234567-89ab-cdef-0123-456789abcdef\""
        );
        assert_eq!(id.to_string(), "01234567-89ab-cdef-0123-456789abcdef");
    }

    #[test]
    fn record_round_trips_json_and_cbor() {
        let r = sample_record();
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(serde_json::from_str::<HostRecord>(&json).unwrap(), r);
        let mut buf = Vec::new();
        ciborium::into_writer(&r, &mut buf).unwrap();
        assert_eq!(
            ciborium::from_reader::<HostRecord, _>(buf.as_slice()).unwrap(),
            r
        );
    }

    #[test]
    fn identity_drops_is_local_and_round_trips() {
        let r = sample_record();
        let id = r.identity();
        let mut buf = Vec::new();
        ciborium::into_writer(&id, &mut buf).unwrap();
        let v: ciborium::Value = ciborium::from_reader(buf.as_slice()).unwrap();
        let keys: Vec<String> = v
            .as_map()
            .unwrap()
            .iter()
            .map(|(k, _)| k.as_text().unwrap().to_string())
            .collect();
        assert_eq!(
            keys,
            ["id", "display_name", "info"],
            "no is_local on the wire"
        );
        assert_eq!(HostRecord::from_identity(id.clone(), true), r);
        assert!(!HostRecord::from_identity(id, false).is_local);
    }

    #[test]
    fn max_freq_from_dvfs_table() {
        let c = &sample_record().info.cpu_topology[0];
        assert_eq!(c.max_freq_hz(), Some(4_512_000_000.0));
    }
}
