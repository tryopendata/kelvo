//! Stand-ins for platforms the app shell does not ship on.

use kelvo_schema::{HostId, HostInfo, HostRecord, OsKind};

pub fn local_host_record(id: HostId) -> HostRecord {
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
            boot_mounts: Vec::new(),
        },
    }
}

pub fn set_launch_at_login(_enabled: bool) -> Result<(), String> {
    Ok(())
}

pub fn launch_at_login_registered() -> Option<bool> {
    None
}

pub fn reduce_transparency() -> bool {
    false
}

pub fn observe_reduce_transparency(_on_change: impl Fn(bool) + 'static) {}
