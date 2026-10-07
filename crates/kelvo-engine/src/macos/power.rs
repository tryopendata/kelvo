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

use core_foundation::base::{CFType, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::CFString;
use core_foundation_sys::dictionary::CFDictionaryRef;
use kelvo_collect::macos::dispatch::Queue;
use kelvo_collect::macos::iokit::{SystemPowerRegistration, allow_power_change};
use kelvo_collect::macos::power_sources::Snapshot;
use objc2_foundation::NSProcessInfo;

use crate::clock;
use crate::inbox::{Inbox, SleepAck};
use crate::power::{PowerEvent, PowerSignals, PowerState};

/// How long the sleep callback waits for the engine before letting the system sleep.
/// macOS itself waits up to 30 s for an answer.
pub const SLEEP_ACK_TIMEOUT: Duration = Duration::from_secs(10);

/// How often the polled states are re-read.
pub const SLOW_POLL: Duration = Duration::from_secs(2);

// iokit_common_msg(...) = sys_iokit (0xe0000000) | sub_iokit_common (0) | message.
const MSG_CAN_SYSTEM_SLEEP: u32 = 0xe000_0270;
const MSG_SYSTEM_WILL_SLEEP: u32 = 0xe000_0280;
const MSG_SYSTEM_HAS_POWERED_ON: u32 = 0xe000_0300;

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

extern "C" fn on_system_power(refcon: *mut c_void, _service: u32, msg: u32, arg: *mut c_void) {
    // SAFETY: refcon is the `PowerCtx` boxed in `SystemPower::register`, freed only after
    // the registration is torn down and the queue this callback runs on drained.
    let ctx = unsafe { &*(refcon as *const PowerCtx) };
    // The connection IORegisterForSystemPower returned; `id` is the notification id the
    // kernel passed in.
    let root = ctx.root.load(Ordering::Acquire);
    let id = arg as isize;
    match msg {
        MSG_CAN_SYSTEM_SLEEP => allow_power_change(root, id),
        MSG_SYSTEM_WILL_SLEEP => {
            let (ack, rx) = SleepAck::new();
            ctx.inbox
                .power(PowerEvent::WillSleep, clock::now(), Some(ack));
            // Returns early on the ack, or when the engine drops it (gone or done).
            let _ = rx.recv_timeout(SLEEP_ACK_TIMEOUT);
            allow_power_change(root, id);
        }
        MSG_SYSTEM_HAS_POWERED_ON => {
            ctx.inbox.power(PowerEvent::DidWake, clock::now(), None);
        }
        _ => {}
    }
}

/// The system power registration. Fields drop in order: the registration (torn down, its
/// queue drained so no callback can still read `ctx`), then the context.
struct SystemPower {
    // Do not reorder: drop order is the teardown order.
    _registration: SystemPowerRegistration,
    _ctx: Box<PowerCtx>,
}

impl SystemPower {
    fn register(inbox: Inbox) -> Option<SystemPower> {
        let queue = Queue::utility(c"com.tryopendata.kelvo.power")?;
        let ctx = Box::new(PowerCtx {
            inbox,
            root: AtomicU32::new(0),
        });
        // SAFETY: the refcon is kept in `_ctx`, which drops after the registration.
        let registration = unsafe {
            SystemPowerRegistration::register(
                queue,
                (&raw const *ctx).cast_mut().cast(),
                on_system_power,
            )
        };
        let Some(registration) = registration else {
            tracing::warn!("IORegisterForSystemPower failed; sleep gaps rely on stall detection");
            return None;
        };
        // Stored before any message can arrive: delivery starts below.
        ctx.root.store(registration.root(), Ordering::Release);
        registration.deliver();
        Some(SystemPower {
            _registration: registration,
            _ctx: ctx,
        })
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
    // Uncounted, as before: the engine's own reads are outside the collectors' call
    // ceilings (D-062).
    let Some(snapshot) = Snapshot::take() else {
        return (false, false);
    };
    let charging = snapshot.internal_battery().is_some_and(|desc| {
        desc.find(CFString::from_static_string("Is Charging"))
            .and_then(|v| v.downcast::<CFBoolean>())
            .is_some_and(bool::from)
    });
    (snapshot.on_battery(), charging)
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
    use std::sync::mpsc;

    use super::*;

    /// Registers for real system power messages and drops the registration: teardown
    /// (deregister, queue drain, context) must return rather than hang.
    #[test]
    fn system_power_start_and_drop() {
        let (inbox, _rx) = Inbox::new();
        let (done_tx, done_rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut signals = MacPowerSignals::new();
            signals.start(inbox);
            let started = signals.system.is_some();
            drop(signals);
            let _ = done_tx.send(started);
        });
        let started = done_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("dropping the power signals returns");
        assert!(started, "IORegisterForSystemPower succeeded");
    }

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
