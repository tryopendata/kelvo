//! Linux host facts. A stand-in until the agent milestone reads os-release, uname,
//! the device tree and cpufreq; it also covers other non-macOS targets.

use kelvo_schema::{HostId, HostInfo, HostRecord, OsKind};

pub fn host_record(id: HostId) -> HostRecord {
    HostRecord {
        id,
        is_local: true,
        display_name: "This computer".into(),
        info: HostInfo {
            os: if cfg!(target_os = "linux") {
                OsKind::Linux
            } else {
                OsKind::Unknown
            },
            os_version: String::new(),
            model: None,
            chip: None,
            chip_known: false,
            cpu_topology: Vec::new(),
            mem_total_bytes: 0,
            boot_time_ms: 0,
            gpu_dvfs_mhz: Vec::new(),
            kernel: None,
            boot_mounts: Vec::new(),
        },
    }
}
