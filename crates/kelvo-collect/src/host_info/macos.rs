//! macOS host facts from a handful of public sysctls and the SystemConfiguration
//! computer name. Every `unsafe` block here has a `SAFETY:` comment.

use std::ffi::{CStr, CString, c_void};

use core_foundation::base::TCFType;
use core_foundation::string::{CFString, CFStringRef};
use kelvo_schema::{ClusterInfo, CoreKind, HostId, HostInfo, HostRecord, OsKind};

use crate::macos::{disk::boot_mounts, gpu_dvfs_mhz, sysctl};

/// A string sysctl, trimmed.
fn sysctl_string(name: &CStr) -> Option<String> {
    sysctl::string(name).map(|s| s.trim().to_owned())
}

#[link(name = "SystemConfiguration", kind = "framework")]
unsafe extern "C" {
    fn SCDynamicStoreCopyComputerName(store: *const c_void, encoding: *mut u32) -> CFStringRef;
}

/// The name in System Settings > General > Sharing ("Sam's MacBook Pro").
fn computer_name() -> Option<String> {
    // SAFETY: a null store means the default session; a null encoding pointer is allowed.
    // The function follows the Create rule, so the returned string is owned here.
    let s = unsafe { SCDynamicStoreCopyComputerName(std::ptr::null(), std::ptr::null_mut()) };
    if s.is_null() {
        return None;
    }
    // SAFETY: `s` is a non-null CFString we own (Create rule).
    Some(unsafe { CFString::wrap_under_create_rule(s) }.to_string())
}

/// One cluster per performance level (`hw.perflevelN`), with core labels in the same form
/// the CPU collector uses (`P0`, `E0`). DVFS tables come from IOReport and are not read
/// here.
fn cpu_topology() -> Vec<ClusterInfo> {
    let levels = sysctl::int(c"hw.nperflevels").unwrap_or(0);
    let mut out = Vec::new();
    for i in (0..levels).rev() {
        let (Ok(name_key), Ok(count_key)) = (
            CString::new(format!("hw.perflevel{i}.name")),
            CString::new(format!("hw.perflevel{i}.logicalcpu")),
        ) else {
            continue;
        };
        let (Some(name), Some(count)) = (sysctl_string(&name_key), sysctl::int(&count_key)) else {
            continue;
        };
        let (prefix, kind) = match name.as_str() {
            "Performance" => ("P", CoreKind::Performance),
            "Efficiency" => ("E", CoreKind::Efficiency),
            _ => ("C", CoreKind::Unknown),
        };
        out.push(ClusterInfo {
            name,
            kind,
            cores: (0..count.max(0)).map(|c| format!("{prefix}{c}")).collect(),
            dvfs_mhz: Vec::new(),
        });
    }
    out
}

/// The local host's record. `chip_known` starts as "is this an Apple chip" and is
/// corrected from the first capabilities (a sensors module reporting `unknown_chip`).
pub fn host_record(id: HostId) -> HostRecord {
    let chip = sysctl_string(c"machdep.cpu.brand_string");
    let boot_time_ms = sysctl::boot_time()
        .map(|tv| tv.tv_sec * 1000 + i64::from(tv.tv_usec) / 1000)
        .unwrap_or(0);
    let model = sysctl_string(c"hw.model");
    HostRecord {
        id,
        is_local: true,
        display_name: computer_name()
            .or_else(|| model.clone())
            .unwrap_or_else(|| "This Mac".into()),
        info: HostInfo {
            os: OsKind::MacOs,
            os_version: sysctl_string(c"kern.osproductversion").unwrap_or_default(),
            model,
            chip_known: chip.as_deref().is_some_and(|c| c.starts_with("Apple M")),
            chip,
            cpu_topology: cpu_topology(),
            mem_total_bytes: sysctl::int(c"hw.memsize")
                .and_then(|b| u64::try_from(b).ok())
                .unwrap_or(0),
            boot_time_ms,
            gpu_dvfs_mhz: gpu_dvfs_mhz(),
            boot_mounts: boot_mounts(),
        },
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    #[test]
    fn local_record_reads_this_mac() {
        let r = host_record(HostId(Uuid::nil()));
        assert!(r.is_local);
        assert!(!r.display_name.is_empty());
        assert!(!r.info.os_version.is_empty());
        assert!(r.info.mem_total_bytes > 0);
        assert!(
            r.info.boot_time_ms > 1_000_000_000_000,
            "{}",
            r.info.boot_time_ms
        );
        let cores: usize = r.info.cpu_topology.iter().map(|c| c.cores.len()).sum();
        if cfg!(target_arch = "aarch64") {
            assert!(cores > 0, "{:?}", r.info.cpu_topology);
            // A virtual machine (hosted CI runners) has no GPU frequency table.
            if !is_virtual_machine() {
                assert!(!r.info.gpu_dvfs_mhz.is_empty());
            }
        }
        assert_eq!(r.info.boot_mounts.first().map(String::as_str), Some("/"));
    }

    fn is_virtual_machine() -> bool {
        sysctl::int(c"kern.hv_vmm_present") == Some(1)
    }
}
