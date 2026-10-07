//! Minimal safe wrappers over the public IOKit registry API, shared by the disk and
//! battery collectors. Only documented IOKit calls live here; private-API collectors
//! keep their own FFI.

use std::ffi::{CStr, c_char};

use core_foundation::base::{CFType, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_foundation_sys::base::{CFAllocatorRef, CFTypeRef, kCFAllocatorDefault};
use core_foundation_sys::dictionary::{CFDictionaryRef, CFMutableDictionaryRef};
use core_foundation_sys::string::CFStringRef;

type IoObjectT = u32;
type KernReturn = i32;

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOServiceMatching(name: *const c_char) -> CFMutableDictionaryRef;
    fn IOServiceGetMatchingServices(
        main_port: u32,
        matching: CFDictionaryRef,
        existing: *mut IoObjectT,
    ) -> KernReturn;
    fn IOIteratorNext(iterator: IoObjectT) -> IoObjectT;
    fn IOIteratorIsValid(iterator: IoObjectT) -> u32;
    fn IOObjectRelease(object: IoObjectT) -> KernReturn;
    fn IORegistryEntryCreateCFProperty(
        entry: IoObjectT,
        key: CFStringRef,
        allocator: CFAllocatorRef,
        options: u32,
    ) -> CFTypeRef;
    fn IORegistryEntryCreateCFProperties(
        entry: IoObjectT,
        properties: *mut CFMutableDictionaryRef,
        allocator: CFAllocatorRef,
        options: u32,
    ) -> KernReturn;
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
}

/// `kIOMainPortDefault`.
const MAIN_PORT_DEFAULT: u32 = 0;
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

/// An owned IOKit object reference, released on drop.
pub(crate) struct IoObject(IoObjectT);

impl Drop for IoObject {
    fn drop(&mut self) {
        // SAFETY: we own one reference to a non-zero object.
        unsafe {
            IOObjectRelease(self.0);
        }
    }
}

impl IoObject {
    /// Every registered service of `class`.
    pub(crate) fn services(class: &CStr) -> Vec<IoObject> {
        let mut out = Vec::new();
        // SAFETY: `class` is NUL-terminated. The returned dictionary is consumed by
        // IOServiceGetMatchingServices below, so we do not release it.
        let matching = unsafe { IOServiceMatching(class.as_ptr()) };
        if matching.is_null() {
            return out;
        }
        let mut iter: IoObjectT = 0;
        crate::calls::count(crate::calls::Api::IoKit);
        // SAFETY: `matching` is a valid dictionary (consumed); `iter` is an out-pointer.
        let kr = unsafe {
            IOServiceGetMatchingServices(MAIN_PORT_DEFAULT, matching as CFDictionaryRef, &mut iter)
        };
        if kr != 0 || iter == 0 {
            return out;
        }
        let iter = IoObject(iter);
        loop {
            // SAFETY: `iter` is a valid iterator we own; each returned object is owned.
            let obj = unsafe { IOIteratorNext(iter.0) };
            if obj == 0 {
                break;
            }
            out.push(IoObject(obj));
        }
        out
    }

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

    /// All registry properties as one dictionary.
    pub(crate) fn properties(&self) -> Option<CFDictionary<CFString, CFType>> {
        let mut dict: CFMutableDictionaryRef = std::ptr::null_mut();
        crate::calls::count(crate::calls::Api::IoKit);
        // SAFETY: `self.0` is valid; `dict` is an out-pointer that receives an owned
        // dictionary on success.
        let kr =
            unsafe { IORegistryEntryCreateCFProperties(self.0, &mut dict, kCFAllocatorDefault, 0) };
        if kr != 0 || dict.is_null() {
            return None;
        }
        // SAFETY: owned (Create rule) dictionary with CFString keys.
        Some(unsafe { CFDictionary::wrap_under_create_rule(dict as CFDictionaryRef) })
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
    let service = IoObject::services(c"IOPlatformExpertDevice")
        .into_iter()
        .next()?;
    as_string(&service.property(&CFString::from_static_string("IOPlatformUUID"))?)
}
