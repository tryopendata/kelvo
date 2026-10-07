//! macOS implementations of the engine's platform seams. All of them use public APIs
//! (libdispatch, IOKit power management and match notifications, notify(3), CoreGraphics
//! session state, NSProcessInfo); FFI is confined to this module, each `unsafe` block
//! with a `SAFETY:` comment.

mod dispatch;
mod hints;
mod power;
mod ticker;

pub use hints::IoKitDeviceHints;
pub use power::MacPowerSignals;
pub use ticker::GcdTicker;

/// Host facts the collectors read, for the shell's `HostInfo` (D-092).
pub use kelvo_collect::macos::disk::boot_mounts;
pub use kelvo_collect::macos::gpu_dvfs_mhz;
/// The sysctl reads behind the shell's host record (`string`, `int`, `boot_time`).
pub use kelvo_collect::macos::sysctl;

/// Moves the calling thread to the utility QoS class (architecture.md, Engine).
pub fn set_current_thread_utility_qos() {
    // SAFETY: plain libc call on the current thread with a valid QoS class constant.
    let rc =
        unsafe { libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_UTILITY, 0) };
    if rc != 0 {
        tracing::warn!(rc, "cannot set utility QoS on the engine thread");
    }
}
