//! Power signals on macOS, from public APIs only.
//!
//! - System sleep and wake: `IORegisterForSystemPower` with its notification port on a
//!   utility dispatch queue. On `kIOMessageSystemWillSleep` the callback sends `WillSleep`
//!   with a [`SleepAck`] and waits (at most [`SLEEP_ACK_TIMEOUT`]) for the engine to flush
//!   and stop before it allows the sleep. No AppKit run loop is needed, so this works in
//!   the `dump` example as well as in the app.
//! - On battery and charging: the `com.apple.system.powersources.source` and
//!   `com.apple.system.powersources` notify(3) keys, checked every poll (shared-memory
//!   reads), and one IOPowerSources snapshot only on a change that can matter: its
//!   providing source type and the internal battery's `Is Charging` (the key
//!   `battery.charging` reads), so `charging` never lags `on_battery` (D-092). The first
//!   key is the source switching. The second fires on any power-source update, every
//!   capacity tick included; it is read only on the adapter, where it covers a battery
//!   that starts or stops charging while the source stays the same. On battery nothing
//!   charges, so those updates cost no snapshot.
//! - Low Power Mode (`NSProcessInfo.isLowPowerModeEnabled`), display sleep
//!   (`CGDisplayIsAsleep`) and screen lock (`CGSessionCopyCurrentDictionary`'s
//!   `CGSSessionScreenIsLocked`) are read at most every [`SLOW_POLL`]. The lock key is
//!   undocumented but long-standing; when it is absent the screen counts as unlocked.

use std::ffi::{CStr, c_char, c_int, c_void};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use core_foundation::array::CFArray;
use core_foundation::base::{CFType, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::CFString;
use core_foundation_sys::array::CFArrayRef;
use core_foundation_sys::base::{CFRelease, CFTypeRef};
use core_foundation_sys::dictionary::CFDictionaryRef;
use core_foundation_sys::string::CFStringRef;
use objc2_foundation::NSProcessInfo;

use super::dispatch::Queue;
use crate::clock;
use crate::inbox::{Inbox, SleepAck};
use crate::power::{PowerEvent, PowerSignals, PowerState};

/// How long the sleep callback waits for the engine before letting the system sleep.
/// macOS itself waits up to 30 s for an answer.
pub const SLEEP_ACK_TIMEOUT: Duration = Duration::from_secs(10);

/// How often the polled states are re-read.
pub const SLOW_POLL: Duration = Duration::from_secs(2);

type IoConnect = u32;
type IoObject = u32;
type NotificationPort = *mut c_void;
type InterestCallback = extern "C" fn(*mut c_void, IoObject, u32, *mut c_void);

// iokit_common_msg(...) = sys_iokit (0xe0000000) | sub_iokit_common (0) | message.
const MSG_CAN_SYSTEM_SLEEP: u32 = 0xe000_0270;
const MSG_SYSTEM_WILL_SLEEP: u32 = 0xe000_0280;
const MSG_SYSTEM_HAS_POWERED_ON: u32 = 0xe000_0300;

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IORegisterForSystemPower(
        refcon: *mut c_void,
        port: *mut NotificationPort,
        callback: InterestCallback,
        notifier: *mut IoObject,
    ) -> IoConnect;
    fn IODeregisterForSystemPower(notifier: *mut IoObject) -> c_int;
    fn IOAllowPowerChange(kernel_port: IoConnect, notification_id: isize) -> c_int;
    fn IONotificationPortSetDispatchQueue(port: NotificationPort, queue: *mut c_void);
    fn IONotificationPortDestroy(port: NotificationPort);
    fn IOServiceClose(connect: IoConnect) -> c_int;
    fn IOPSCopyPowerSourcesInfo() -> CFTypeRef;
    fn IOPSGetProvidingPowerSourceType(snapshot: CFTypeRef) -> CFStringRef;
    fn IOPSCopyPowerSourcesList(snapshot: CFTypeRef) -> CFArrayRef;
    fn IOPSGetPowerSourceDescription(snapshot: CFTypeRef, ps: CFTypeRef) -> CFDictionaryRef;
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGMainDisplayID() -> u32;
    fn CGDisplayIsAsleep(display: u32) -> c_int;
    fn CGSessionCopyCurrentDictionary() -> CFDictionaryRef;
}

unsafe extern "C" {
    fn notify_register_check(name: *const c_char, out_token: *mut c_int) -> u32;
    fn notify_check(token: c_int, check: *mut c_int) -> u32;
    fn notify_cancel(token: c_int) -> u32;
}

const NOTIFY_STATUS_OK: u32 = 0;

struct PowerCtx {
    inbox: Inbox,
    root: AtomicU32,
}

extern "C" fn on_system_power(refcon: *mut c_void, _service: IoObject, msg: u32, arg: *mut c_void) {
    // SAFETY: refcon is the `PowerCtx` boxed in `SystemPower::register`, freed only after
    // deregistration and a drain of the queue this callback runs on.
    let ctx = unsafe { &*(refcon as *const PowerCtx) };
    let root = ctx.root.load(Ordering::Acquire);
    let id = arg as isize;
    match msg {
        MSG_CAN_SYSTEM_SLEEP => {
            // SAFETY: `root` is the connection IORegisterForSystemPower returned; `id` is
            // the notification id the kernel passed in.
            unsafe { IOAllowPowerChange(root, id) };
        }
        MSG_SYSTEM_WILL_SLEEP => {
            let (ack, rx) = SleepAck::new();
            ctx.inbox
                .power(PowerEvent::WillSleep, clock::now(), Some(ack));
            // Returns early on the ack, or when the engine drops it (gone or done).
            let _ = rx.recv_timeout(SLEEP_ACK_TIMEOUT);
            // SAFETY: as above.
            unsafe { IOAllowPowerChange(root, id) };
        }
        MSG_SYSTEM_HAS_POWERED_ON => {
            ctx.inbox.power(PowerEvent::DidWake, clock::now(), None);
        }
        _ => {}
    }
}

/// The system power registration, torn down on drop.
struct SystemPower {
    queue: Queue,
    port: NotificationPort,
    notifier: IoObject,
    root: IoConnect,
    ctx: Box<PowerCtx>,
}

impl SystemPower {
    fn register(inbox: Inbox) -> Option<SystemPower> {
        let queue = Queue::utility(c"com.tryopendata.kelvo.power")?;
        let ctx = Box::new(PowerCtx {
            inbox,
            root: AtomicU32::new(0),
        });
        let mut port: NotificationPort = std::ptr::null_mut();
        let mut notifier: IoObject = 0;
        // SAFETY: out-pointers are valid; the refcon outlives the registration (see Drop).
        let root = unsafe {
            IORegisterForSystemPower(
                (&raw const *ctx).cast_mut().cast(),
                &mut port,
                on_system_power,
                &mut notifier,
            )
        };
        if root == 0 || port.is_null() {
            tracing::warn!("IORegisterForSystemPower failed; sleep gaps rely on stall detection");
            return None;
        }
        ctx.root.store(root, Ordering::Release);
        // SAFETY: `port` came from the registration; the queue is valid and retained by
        // IOKit while set.
        unsafe { IONotificationPortSetDispatchQueue(port, queue.raw()) };
        Some(SystemPower {
            queue,
            port,
            notifier,
            root,
            ctx,
        })
    }
}

impl Drop for SystemPower {
    fn drop(&mut self) {
        // SAFETY: tearing down the registration made in `register`, in IOKit's documented
        // order. The queue is drained afterwards so no callback can still read `ctx`.
        unsafe {
            IODeregisterForSystemPower(&mut self.notifier);
            IONotificationPortDestroy(self.port);
            IOServiceClose(self.root);
        }
        self.queue.drain();
        let _ = &self.ctx;
    }
}

pub struct MacPowerSignals {
    system: Option<SystemPower>,
    battery_token: Option<c_int>,
    /// Any change to any power source's state (`kIOPSNotifyAnyPowerSource`).
    any_token: Option<c_int>,
    state: PowerState,
    last_slow: Option<Instant>,
}

// SAFETY: the raw IOKit port and notify token are used only from the owning thread (the
// engine) and in Drop; IOKit callbacks touch only the boxed context, which is Sync.
unsafe impl Send for MacPowerSignals {}

impl Default for MacPowerSignals {
    fn default() -> Self {
        Self::new()
    }
}

impl MacPowerSignals {
    pub fn new() -> Self {
        Self {
            system: None,
            battery_token: register(c"com.apple.system.powersources.source"),
            any_token: register(c"com.apple.system.powersources"),
            state: PowerState::default(),
            last_slow: None,
        }
    }

    /// Whether the power source or the charging state may have changed since the last
    /// poll, so a new snapshot is needed.
    fn source_changed(&mut self) -> bool {
        // Both checks run: each clears its own pending flag.
        let source = changed(self.battery_token);
        let any = changed(self.any_token);
        // Any update on battery is a capacity tick: charging stays false until the
        // source switches, which the first key reports.
        source || (any && !self.state.on_battery)
    }
}

/// Registers a notify(3) key for `notify_check`; `None` when registration failed.
fn register(key: &CStr) -> Option<c_int> {
    let mut token: c_int = 0;
    // SAFETY: NUL-terminated key; `token` is a valid out-pointer.
    let rc = unsafe { notify_register_check(key.as_ptr(), &mut token) };
    (rc == NOTIFY_STATUS_OK).then_some(token)
}

/// Whether the key behind `token` was posted since the last check. Without a token every
/// poll counts as a change.
fn changed(token: Option<c_int>) -> bool {
    let Some(token) = token else {
        return true;
    };
    let mut check: c_int = 0;
    // SAFETY: `token` is a registered notify token; `check` is a valid out-pointer.
    let rc = unsafe { notify_check(token, &mut check) };
    rc != NOTIFY_STATUS_OK || check != 0
}

impl Drop for MacPowerSignals {
    fn drop(&mut self) {
        for token in [self.battery_token.take(), self.any_token.take()]
            .into_iter()
            .flatten()
        {
            // SAFETY: cancels a token registered in `new`.
            unsafe { notify_cancel(token) };
        }
    }
}

/// `(on_battery, charging)` from one IOPowerSources snapshot. Without a snapshot or a
/// battery, both are false: the Mac counts as on its adapter.
fn power_source() -> (bool, bool) {
    // SAFETY: Create rule: we own the snapshot and release it below.
    let info = unsafe { IOPSCopyPowerSourcesInfo() };
    if info.is_null() {
        return (false, false);
    }
    // SAFETY: `info` is a valid snapshot; the returned string follows the Get rule and is
    // only used while `info` is alive.
    let kind = unsafe { IOPSGetProvidingPowerSourceType(info) };
    let battery = if kind.is_null() {
        false
    } else {
        // SAFETY: non-null CFString borrowed under the Get rule.
        let s = unsafe { CFString::wrap_under_get_rule(kind) };
        s == CFString::from_static_string("Battery Power")
    };
    let charging = internal_battery_charging(info);
    // SAFETY: releasing the snapshot we own.
    unsafe { CFRelease(info) };
    (battery, charging)
}

/// `Is Charging` of the internal battery in `info`, false when there is none.
fn internal_battery_charging(info: CFTypeRef) -> bool {
    // SAFETY: `info` is a live snapshot; Copy rule, so a non-null list is owned here.
    let list = unsafe { IOPSCopyPowerSourcesList(info) };
    if list.is_null() {
        return false;
    }
    // SAFETY: non-null CFArray we own (Copy rule).
    let list: CFArray<CFType> = unsafe { CFArray::wrap_under_create_rule(list) };
    for ps in list.iter() {
        // SAFETY: `ps` comes from this snapshot's list; Get rule, valid while `info` is.
        let desc = unsafe { IOPSGetPowerSourceDescription(info, ps.as_CFTypeRef()) };
        if desc.is_null() {
            continue;
        }
        // SAFETY: non-null description dictionary with CFString keys, borrowed (Get rule).
        let desc: CFDictionary<CFString, CFType> =
            unsafe { CFDictionary::wrap_under_get_rule(desc) };
        let internal = desc
            .find(CFString::from_static_string("Type"))
            .and_then(|v| v.downcast::<CFString>())
            .is_some_and(|t| t == CFString::from_static_string("InternalBattery"));
        if internal {
            return desc
                .find(CFString::from_static_string("Is Charging"))
                .and_then(|v| v.downcast::<CFBoolean>())
                .is_some_and(bool::from);
        }
    }
    false
}

fn display_asleep() -> bool {
    // SAFETY: plain CoreGraphics queries with no pointers.
    unsafe { CGDisplayIsAsleep(CGMainDisplayID()) != 0 }
}

fn screen_locked() -> bool {
    // SAFETY: Create rule; NULL outside a GUI session (ssh), which counts as unlocked.
    let dict = unsafe { CGSessionCopyCurrentDictionary() };
    if dict.is_null() {
        return false;
    }
    // SAFETY: non-null dictionary we own, with CFString keys.
    let dict: CFDictionary<CFString, CFType> =
        unsafe { CFDictionary::wrap_under_create_rule(dict) };
    dict.find(CFString::from_static_string("CGSSessionScreenIsLocked"))
        .and_then(|v| v.downcast::<CFBoolean>())
        .is_some_and(bool::from)
}

impl PowerSignals for MacPowerSignals {
    fn start(&mut self, inbox: Inbox) {
        if self.system.is_none() {
            self.system = SystemPower::register(inbox);
        }
    }

    fn poll(&mut self) -> PowerState {
        if self.source_changed() {
            (self.state.on_battery, self.state.charging) = power_source();
        }
        let due = self.last_slow.is_none_or(|t| t.elapsed() >= SLOW_POLL);
        if due {
            self.last_slow = Some(Instant::now());
            self.state.low_power_mode = NSProcessInfo::processInfo().isLowPowerModeEnabled();
            self.state.display_asleep = display_asleep();
            self.state.screen_locked = screen_locked();
        }
        self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads the real power state once.
    #[test]
    #[ignore = "reads live macOS power state; run by hand with --ignored --nocapture"]
    fn live_power_state() {
        let mut p = MacPowerSignals::new();
        let s = p.poll();
        println!("{s:?}");
        let s2 = p.poll();
        assert_eq!(s, s2);
    }
}
