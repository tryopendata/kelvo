//! First-party IOKit registry helpers for the disk, battery and GPU-process collectors:
//! child walks, registry ids and single properties on [`IoObject`], and typed reads of
//! CF values; and the engine's device-match and system-power notifications behind
//! [`NotificationPort`] and [`SystemPowerRegistration`]. The registry handle itself
//! ([`IoObject`], [`matching_services`]) is the vendored one (`vendor/iokit.rs`), so the
//! IOKit FFI is declared once. Only documented IOKit calls live here; private-API
//! collectors keep their own FFI.

use std::ffi::{CStr, c_char, c_void};

use core_foundation::base::{CFType, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_foundation_sys::base::kCFAllocatorDefault;
use core_foundation_sys::dictionary::CFDictionaryRef;

use super::dispatch::Queue;
use super::vendor::iokit::{
    IOIteratorNext, IOObjectRelease, IORegistryEntryCreateCFProperty, IOServiceClose,
    IOServiceMatching,
};
pub(crate) use super::vendor::iokit::{IoObject, matching_services};

type IoObjectT = u32;
type KernReturn = i32;
type IoConnect = u32;

/// `IONotificationPortRef`.
type PortRef = *mut c_void;

/// `IOServiceMatchingCallback`: `(refcon, iterator)`. Drain the iterator with
/// [`drain_notification`] to re-arm the notification.
pub type MatchingCallback = extern "C" fn(*mut c_void, u32);

/// `IOServiceInterestCallback`: `(refcon, service, message type, message argument)`.
pub type InterestCallback = extern "C" fn(*mut c_void, u32, u32, *mut c_void);

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOIteratorIsValid(iterator: IoObjectT) -> u32;
    fn IORegistryEntryGetChildEntry(
        entry: IoObjectT,
        plane: *const c_char,
        child: *mut IoObjectT,
    ) -> KernReturn;
    fn IORegistryEntryGetChildIterator(
        entry: IoObjectT,
        plane: *const c_char,
        iterator: *mut IoObjectT,
    ) -> KernReturn;
    fn IORegistryEntryGetRegistryEntryID(entry: IoObjectT, id: *mut u64) -> KernReturn;
    // For the engine's device and system-power notifications.
    fn IONotificationPortCreate(main_port: u32) -> PortRef;
    fn IONotificationPortSetDispatchQueue(port: PortRef, queue: *mut c_void);
    fn IONotificationPortDestroy(port: PortRef);
    fn IOServiceAddMatchingNotification(
        port: PortRef,
        notification_type: *const c_char,
        matching: CFDictionaryRef,
        callback: MatchingCallback,
        refcon: *mut c_void,
        iterator: *mut IoObjectT,
    ) -> KernReturn;
    fn IORegisterForSystemPower(
        refcon: *mut c_void,
        port: *mut PortRef,
        callback: InterestCallback,
        notifier: *mut IoObjectT,
    ) -> IoConnect;
    fn IODeregisterForSystemPower(notifier: *mut IoObjectT) -> KernReturn;
    fn IOAllowPowerChange(kernel_port: IoConnect, notification_id: isize) -> KernReturn;
}

const SERVICE_PLANE: &CStr = c"IOService";

/// An immutable CFString kept by a collector for per-tick lookups. `CFString` is not
/// `Send` in core-foundation, but collectors move between threads once (to the engine
/// thread), and Apple documents immutable CF objects as safe to use from any thread.
pub(crate) struct Key(CFString);

// SAFETY: the wrapped CFString is immutable and never mutated after construction;
// immutable CoreFoundation objects are thread-safe (CF "Thread Safety" docs).
unsafe impl Send for Key {}

impl Key {
    pub(crate) fn new(s: &'static str) -> Self {
        Self(CFString::from_static_string(s))
    }
}

impl std::ops::Deref for Key {
    type Target = CFString;
    fn deref(&self) -> &CFString {
        &self.0
    }
}

impl IoObject {
    /// The first child in the IOService plane.
    pub(crate) fn first_child(&self) -> Option<IoObject> {
        let mut child: IoObjectT = 0;
        crate::calls::count(crate::calls::Api::IoKit);
        // SAFETY: `self.0` is valid; `child` is an out-pointer. The child is returned
        // retained, so we own it.
        let kr =
            unsafe { IORegistryEntryGetChildEntry(self.0, SERVICE_PLANE.as_ptr(), &mut child) };
        (kr == 0 && child != 0).then_some(IoObject(child))
    }

    /// Every child in the IOService plane, registered or not (a GPU's user clients are
    /// children that `IOServiceGetMatchingServices` does not return). Yields owned objects
    /// one at a time, so walking them does not allocate. `None` when the iterator could
    /// not be created.
    pub(crate) fn children(&self) -> Option<Children> {
        let mut iter: IoObjectT = 0;
        crate::calls::count(crate::calls::Api::IoKit);
        // SAFETY: `self.0` is valid; `iter` is an out-pointer that receives an owned
        // iterator on success.
        let kr =
            unsafe { IORegistryEntryGetChildIterator(self.0, SERVICE_PLANE.as_ptr(), &mut iter) };
        (kr == 0 && iter != 0).then_some(Children(IoObject(iter)))
    }

    /// The entry's registry-wide id, stable for its lifetime and never reused while the
    /// system runs.
    pub(crate) fn registry_id(&self) -> Option<u64> {
        let mut id = 0u64;
        crate::calls::count(crate::calls::Api::IoKit);
        // SAFETY: `self.0` is valid; `id` is an out-pointer.
        let kr = unsafe { IORegistryEntryGetRegistryEntryID(self.0, &mut id) };
        (kr == 0).then_some(id)
    }

    /// One registry property. Callers that read every tick keep `key` around rather
    /// than building a CFString per call.
    pub(crate) fn property(&self, key: &CFString) -> Option<CFType> {
        crate::calls::count(crate::calls::Api::IoKit);
        // SAFETY: `self.0` is valid and `key` lives across the call. The result follows
        // the Create rule.
        let r = unsafe {
            IORegistryEntryCreateCFProperty(
                self.0,
                key.as_concrete_TypeRef(),
                kCFAllocatorDefault,
                0,
            )
        };
        // SAFETY: non-null result owned by us (Create rule).
        (!r.is_null()).then(|| unsafe { CFType::wrap_under_create_rule(r) })
    }
}

/// The children of a registry entry, from [`IoObject::children`].
pub(crate) struct Children(IoObject);

impl Children {
    /// False when the registry changed under the walk (IOKit then ends the iteration
    /// early, so some children were not seen). Ask after the walk.
    pub(crate) fn is_valid(&self) -> bool {
        crate::calls::count(crate::calls::Api::IoKit);
        // SAFETY: `self.0` is a valid iterator we own.
        unsafe { IOIteratorIsValid(self.0.0) != 0 }
    }
}

impl Iterator for Children {
    type Item = IoObject;

    fn next(&mut self) -> Option<IoObject> {
        // SAFETY: `self.0` is a valid iterator we own; each returned object is owned.
        let obj = unsafe { IOIteratorNext(self.0.0) };
        (obj != 0).then_some(IoObject(obj))
    }
}

/// An IOKit notification port delivering on a dispatch queue (the engine's device hints).
/// Drop destroys the port, then drains the queue, so no callback is still running once it
/// is gone.
pub struct NotificationPort {
    port: PortRef,
    queue: Queue,
}

// SAFETY: the port is only created, armed and destroyed by its owner; IOKit delivers
// callbacks on `queue`, which is thread-safe.
unsafe impl Send for NotificationPort {}

/// What a matching notification fires on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchEvent {
    /// `kIOFirstMatchNotification`: a matching service appeared.
    FirstMatch,
    /// `kIOTerminatedNotification`: a matching service went away.
    Terminated,
}

impl MatchEvent {
    fn name(self) -> &'static CStr {
        match self {
            MatchEvent::FirstMatch => c"IOServiceFirstMatch",
            MatchEvent::Terminated => c"IOServiceTerminate",
        }
    }
}

impl NotificationPort {
    /// A port on the main port that delivers on `queue`. `None` when IOKit refuses.
    pub fn on_queue(queue: Queue) -> Option<NotificationPort> {
        // SAFETY: kIOMainPortDefault (0) is always valid.
        let port = unsafe { IONotificationPortCreate(0) };
        if port.is_null() {
            return None;
        }
        // SAFETY: valid port and queue; IOKit retains the queue while it is set.
        unsafe { IONotificationPortSetDispatchQueue(port, queue.raw()) };
        Some(NotificationPort { port, queue })
    }

    /// Arms `callback(refcon, iterator)` for `event` on services of IOService `class`.
    /// The returned iterator holds the services already there; drain it to arm the
    /// notification. `Err(None)` when the matching dictionary could not be built;
    /// `Err(Some(kr))` with IOKit's return code when the notification was not armed.
    ///
    /// # Safety
    ///
    /// `refcon` must stay valid for `callback` until this port is dropped.
    pub unsafe fn add_matching(
        &self,
        event: MatchEvent,
        class: &CStr,
        callback: MatchingCallback,
        refcon: *mut c_void,
    ) -> Result<MatchIterator, Option<i32>> {
        // SAFETY: NUL-terminated class name. The dictionary is consumed by
        // IOServiceAddMatchingNotification below, so it is not released here.
        let matching = unsafe { IOServiceMatching(class.as_ptr()) };
        if matching.is_null() {
            return Err(None);
        }
        let mut iterator: IoObjectT = 0;
        // SAFETY: valid port, consumed matching dictionary; the caller keeps `refcon`
        // alive until the port is destroyed.
        let kr = unsafe {
            IOServiceAddMatchingNotification(
                self.port,
                event.name().as_ptr(),
                matching.cast_const(),
                callback,
                refcon,
                &mut iterator,
            )
        };
        if kr != 0 || iterator == 0 {
            return Err(Some(kr));
        }
        Ok(MatchIterator(iterator))
    }
}

impl Drop for NotificationPort {
    fn drop(&mut self) {
        // SAFETY: destroying the port we created.
        unsafe { IONotificationPortDestroy(self.port) };
        // No callback can be left running after this.
        self.queue.drain();
    }
}

/// A matching notification's iterator. Dropping it cancels the notification.
pub struct MatchIterator(IoObjectT);

impl MatchIterator {
    /// Releases every service in the iterator, which re-arms the notification. Returns
    /// how many there were.
    pub fn drain(&self) -> usize {
        // SAFETY: `self.0` is a live notification iterator we own.
        unsafe { drain_notification(self.0) }
    }
}

impl Drop for MatchIterator {
    fn drop(&mut self) {
        // SAFETY: releasing the iterator we own; this cancels its notification.
        unsafe { IOObjectRelease(self.0) };
    }
}

/// [`MatchIterator::drain`] for the iterator a [`MatchingCallback`] receives.
///
/// # Safety
///
/// `iterator` must be a live notification iterator, such as the one passed to the
/// callback.
pub unsafe fn drain_notification(iterator: u32) -> usize {
    let mut n = 0;
    loop {
        // SAFETY: the caller passes a live iterator; each object returned is owned and
        // released here.
        let obj = unsafe { IOIteratorNext(iterator) };
        if obj == 0 {
            return n;
        }
        // SAFETY: as above.
        unsafe { IOObjectRelease(obj) };
        n += 1;
    }
}

/// The system sleep and wake registration (`IORegisterForSystemPower`), its port
/// delivering on a dispatch queue. Drop deregisters, destroys the port and closes the
/// connection in IOKit's documented order, then drains the queue, so no callback is still
/// running once it is gone.
pub struct SystemPowerRegistration {
    port: PortRef,
    notifier: IoObjectT,
    root: IoConnect,
    queue: Queue,
}

// SAFETY: the registration is only made and torn down by its owner; IOKit delivers
// callbacks on `queue`, which is thread-safe.
unsafe impl Send for SystemPowerRegistration {}

impl SystemPowerRegistration {
    /// Registers `callback(refcon, service, message, argument)` for system power
    /// messages, to be delivered on `queue` once [`SystemPowerRegistration::deliver`] is
    /// called. `None` when IOKit refuses.
    ///
    /// # Safety
    ///
    /// `refcon` must stay valid for `callback` until this registration is dropped.
    pub unsafe fn register(
        queue: Queue,
        refcon: *mut c_void,
        callback: InterestCallback,
    ) -> Option<SystemPowerRegistration> {
        let mut port: PortRef = std::ptr::null_mut();
        let mut notifier: IoObjectT = 0;
        // SAFETY: out-pointers are valid; the caller keeps `refcon` alive (see Drop).
        let root = unsafe { IORegisterForSystemPower(refcon, &mut port, callback, &mut notifier) };
        if root == 0 || port.is_null() {
            return None;
        }
        Some(SystemPowerRegistration {
            port,
            notifier,
            root,
            queue,
        })
    }

    /// The root power-domain connection, for [`allow_power_change`].
    pub fn root(&self) -> u32 {
        self.root
    }

    /// Starts delivering messages on the queue. Callbacks can run from here on, so
    /// anything they read through the refcon is set up first.
    pub fn deliver(&self) {
        // SAFETY: `self.port` came from the registration; the queue is valid and retained
        // by IOKit while set.
        unsafe { IONotificationPortSetDispatchQueue(self.port, self.queue.raw()) };
    }
}

impl Drop for SystemPowerRegistration {
    fn drop(&mut self) {
        // SAFETY: tearing down the registration made in `register`, in IOKit's documented
        // order. The queue is drained afterwards so no callback can still read the refcon.
        unsafe {
            IODeregisterForSystemPower(&mut self.notifier);
            IONotificationPortDestroy(self.port);
            IOServiceClose(self.root);
        }
        self.queue.drain();
    }
}

/// Answers a `kIOMessageCanSystemSleep` or `kIOMessageSystemWillSleep` message:
/// `IOAllowPowerChange` on the registration's [`SystemPowerRegistration::root`] with the
/// message's notification id.
pub fn allow_power_change(root: u32, notification_id: isize) {
    // SAFETY: plain IOKit call with no pointers; an unknown connection or id is refused
    // with an error code.
    unsafe { IOAllowPowerChange(root, notification_id) };
}

/// Reads a number as i64 (integers or floats, truncated).
pub(crate) fn as_i64(v: &CFType) -> Option<i64> {
    let n = v.downcast::<CFNumber>()?;
    n.to_i64().or_else(|| n.to_f64().map(|f| f as i64))
}

pub(crate) fn as_f64(v: &CFType) -> Option<f64> {
    let n = v.downcast::<CFNumber>()?;
    n.to_f64().or_else(|| n.to_i64().map(|i| i as f64))
}

pub(crate) fn as_bool(v: &CFType) -> Option<bool> {
    if let Some(b) = v.downcast::<CFBoolean>() {
        return Some(b.into());
    }
    as_i64(v).map(|i| i != 0)
}

pub(crate) fn as_string(v: &CFType) -> Option<String> {
    v.downcast::<CFString>().map(|s| s.to_string())
}

/// Looks up `key` in a CF dictionary with string keys.
pub(crate) fn get(dict: &CFDictionary<CFString, CFType>, key: &CFString) -> Option<CFType> {
    dict.find(key).map(|v| v.clone())
}

/// Views an untyped CF value as a dictionary with string keys.
pub(crate) fn dict_of(v: &CFType) -> Option<CFDictionary<CFString, CFType>> {
    let d = v.downcast::<CFDictionary>()?;
    // SAFETY: re-wrapping the same retained dictionary with typed keys/values; the get
    // rule retains it again, so both wrappers release independently.
    Some(unsafe { CFDictionary::wrap_under_get_rule(d.as_concrete_TypeRef()) })
}

/// `IOPlatformUUID`: the hardware UUID System Information shows (public IOKit). It
/// differs on every Mac, so it does not follow a disk clone or Migration Assistant, which
/// is what the host id's machine binding needs (D-071). Not yet checked in a sandboxed
/// (`appstore`) build; `None` there just skips the binding.
pub fn platform_uuid() -> Option<String> {
    let service = matching_services(c"IOPlatformExpertDevice")
        .into_iter()
        .next()?;
    as_string(&service.property(&CFString::from_static_string("IOPlatformUUID"))?)
}
