//! macOS pieces of the shell: the login item and the Reduce Transparency setting. Every
//! `unsafe` block here has a `SAFETY:` comment. The local host record is
//! `kelvo_engine::host_record`.

use std::ptr::NonNull;

use block2::RcBlock;
use objc2_app_kit::{NSWorkspace, NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification};
use objc2_foundation::NSNotification;
use smappservice_rs::{AppService, ServiceManagementError, ServiceStatus, ServiceType};

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

/// SMAppService launches carry an open-application Apple event, not a CLI flag.
/// https://developer.apple.com/documentation/coreservices/keyaelaunchedasloginitem
pub fn launched_at_login() -> bool {
    let manager = objc2_foundation::NSAppleEventManager::sharedAppleEventManager();
    manager.currentAppleEvent().is_some_and(|event| {
        // SAFETY: selectors/signatures are Foundation's NSAppleEventDescriptor API;
        // AEEventID and AEKeyword are UInt32, and the property is a retained object.
        unsafe {
            let event_id: u32 = objc2::msg_send![&*event, eventID];
            let property: Option<objc2::rc::Retained<objc2_foundation::NSAppleEventDescriptor>> =
                objc2::msg_send![&*event,paramDescriptorForKeyword:u32::from_be_bytes(*b"prdt")];
            event_id == u32::from_be_bytes(*b"oapp")
                && property.is_some_and(|property| {
                    property.enumCodeValue() == u32::from_be_bytes(*b"lgit")
                })
        }
    })
}
