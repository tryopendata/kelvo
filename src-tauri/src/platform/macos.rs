//! macOS pieces of the shell: the local host record, the login item, and the Reduce
//! Transparency setting. Every `unsafe` block here has a `SAFETY:` comment.
//!
//! The host record reads a handful of public sysctls. It arguably belongs in
//! `kelvo-collect`, which reads the OS; it lives here until that crate grows a host-info
//! API, because the shell may not depend on `kelvo-collect` (`.claude/rules/rust.md`).

use std::ffi::{CStr, CString, c_void};
use std::ptr::NonNull;

use block2::RcBlock;
use core_foundation::base::TCFType;
use core_foundation::string::{CFString, CFStringRef};
use kelvo_schema::{ClusterInfo, CoreKind, HostId, HostInfo, HostRecord, OsKind};
use objc2_app_kit::{NSWorkspace, NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification};
use objc2_foundation::NSNotification;
use smappservice_rs::{AppService, ServiceManagementError, ServiceStatus, ServiceType};

fn sysctl_string(name: &CStr) -> Option<String> {
    let mut len: libc::size_t = 0;
    // SAFETY: `name` is NUL-terminated; a null buffer asks only for the length.
    let rc = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 || len == 0 {
        return None;
    }
    let mut buf = vec![0u8; len];
    // SAFETY: `buf` has `len` writable bytes and `len` tells sysctl so.
    let rc = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            buf.as_mut_ptr().cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return None;
    }
    buf.truncate(len);
    CStr::from_bytes_until_nul(&buf)
        .ok()
        .map(|s| s.to_string_lossy().trim().to_owned())
}

/// Reads a fixed-size sysctl value of type `T`.
fn sysctl_value<T: Copy + Default>(name: &CStr) -> Option<T> {
    let mut out = T::default();
    let mut len = std::mem::size_of::<T>();
    // SAFETY: `out` is a valid `T` with `len` bytes; sysctl writes at most `len` bytes and
    // reports how many. A short write is rejected below. `T` is a plain integer or C
    // struct, valid for any bit pattern.
    let rc = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            (&raw mut out).cast::<c_void>(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    (rc == 0 && len == std::mem::size_of::<T>()).then_some(out)
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
    let levels = sysctl_value::<i32>(c"hw.nperflevels").unwrap_or(0);
    let mut out = Vec::new();
    for i in (0..levels).rev() {
        let (Ok(name_key), Ok(count_key)) = (
            CString::new(format!("hw.perflevel{i}.name")),
            CString::new(format!("hw.perflevel{i}.logicalcpu")),
        ) else {
            continue;
        };
        let (Some(name), Some(count)) = (sysctl_string(&name_key), sysctl_value::<i32>(&count_key))
        else {
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
pub fn local_host_record(id: HostId) -> HostRecord {
    let chip = sysctl_string(c"machdep.cpu.brand_string");
    let boot_time_ms = sysctl_value::<libc::timeval>(c"kern.boottime")
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
            mem_total_bytes: sysctl_value::<u64>(c"hw.memsize").unwrap_or(0),
            boot_time_ms,
            gpu_dvfs_mhz: kelvo_engine::macos::gpu_dvfs_mhz(),
            boot_mounts: kelvo_engine::macos::boot_mounts(),
        },
    }
}

/// Whether this process runs from an app bundle. `SMAppService.mainApp` registers the
/// bundle; for a bare `cargo run` binary there is nothing to register.
fn in_app_bundle() -> bool {
    std::env::current_exe()
        .ok()
        .is_some_and(|p| p.to_string_lossy().contains(".app/Contents/MacOS/"))
}

/// Registers or unregisters Kelvo as a login item. Outside an app bundle (development) it
/// does nothing and says so in the log.
pub fn set_launch_at_login(enabled: bool) -> Result<(), String> {
    if !in_app_bundle() {
        tracing::info!(
            enabled,
            "launch at login not applied: not running from an app bundle"
        );
        return Ok(());
    }
    let service = AppService::new(ServiceType::MainApp);
    let res = if enabled {
        service.register()
    } else {
        service.unregister()
    };
    match res {
        Ok(())
        | Err(ServiceManagementError::AlreadyRegistered)
        | Err(ServiceManagementError::JobNotFound) => {
            let status = service.status();
            tracing::info!(enabled, %status, "login item updated");
            if enabled && status == ServiceStatus::RequiresApproval {
                tracing::warn!("login item needs approval in System Settings > Login Items");
            }
            Ok(())
        }
        Err(e) => Err(e.to_string()),
    }
}

/// Whether the login item is currently registered (enabled or awaiting approval). `None`
/// outside an app bundle.
pub fn launch_at_login_registered() -> Option<bool> {
    in_app_bundle().then(|| {
        matches!(
            AppService::new(ServiceType::MainApp).status(),
            ServiceStatus::Enabled | ServiceStatus::RequiresApproval
        )
    })
}

pub fn reduce_transparency() -> bool {
    NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceTransparency()
}

/// Calls `on_change` with the new Reduce Transparency value whenever the accessibility
/// display options change. The observer lives for the rest of the process.
pub fn observe_reduce_transparency(on_change: impl Fn(bool) + 'static) {
    let workspace = NSWorkspace::sharedWorkspace();
    let center = workspace.notificationCenter();
    let block = RcBlock::new(move |_note: NonNull<NSNotification>| {
        on_change(reduce_transparency());
    });
    // SAFETY: the notification name is a valid AppKit constant; no object filter and no
    // queue mean the block runs synchronously on the posting thread (AppKit posts this on
    // the main thread). The block captures only `on_change`, which needs no particular
    // thread.
    let token = unsafe {
        center.addObserverForName_object_queue_usingBlock(
            Some(NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification),
            None,
            None,
            &block,
        )
    };
    // The observer is app-lifetime; the center holds the block until the token is
    // removed, which never happens.
    std::mem::forget(token);
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    #[test]
    fn local_record_reads_this_mac() {
        let r = local_host_record(HostId(Uuid::nil()));
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
        let mut vm: libc::c_int = 0;
        let mut len = std::mem::size_of::<libc::c_int>();
        // SAFETY: a NUL-terminated name and an int-sized output buffer we own.
        let rc = unsafe {
            libc::sysctlbyname(
                c"kern.hv_vmm_present".as_ptr(),
                (&raw mut vm).cast(),
                &raw mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        rc == 0 && vm == 1
    }
}
