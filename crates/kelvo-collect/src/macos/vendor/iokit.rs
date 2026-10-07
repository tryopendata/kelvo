//! IOKit registry access (macmon `IOServiceIterator`, `cfio_get_props`).
//!
//! Public, documented IOKit calls. They live with the vendored code because the SMC
//! connection and the SoC tables are found through them.

use std::ffi::{CStr, c_char};

use core_foundation::base::{CFAllocatorRef, CFTypeRef, TCFType, kCFAllocatorDefault};
use core_foundation::dictionary::{CFDictionaryRef, CFMutableDictionaryRef};
use core_foundation::string::{CFString, CFStringRef};

use super::cf::Dict;

/// `kIOMainPortDefault`.
const MAIN_PORT_DEFAULT: u32 = 0;

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    pub(crate) fn IOServiceMatching(name: *const c_char) -> CFMutableDictionaryRef;
    fn IOServiceGetMatchingServices(
        main_port: u32,
        matching: CFDictionaryRef,
        existing: *mut u32,
    ) -> i32;
    pub(crate) fn IOIteratorNext(iterator: u32) -> u32;
    fn IORegistryEntryGetName(entry: u32, name: *mut c_char) -> i32;
    fn IORegistryEntryCreateCFProperties(
        entry: u32,
        properties: *mut CFMutableDictionaryRef,
        allocator: CFAllocatorRef,
        options: u32,
    ) -> i32;
    pub(crate) fn IORegistryEntryCreateCFProperty(
        entry: u32,
        key: CFStringRef,
        allocator: CFAllocatorRef,
        options: u32,
    ) -> CFTypeRef;
    pub(crate) fn IOObjectRelease(object: u32) -> i32;
    pub(crate) fn IOServiceClose(connect: u32) -> i32;
}

/// An owned `io_object_t`, released on drop.
pub(crate) struct IoObject(pub(in crate::macos) u32);

impl IoObject {
    pub(crate) fn raw(&self) -> u32 {
        self.0
    }

    /// The registry entry's name (`IORegistryEntryGetName`).
    pub(crate) fn name(&self) -> Option<String> {
        // io_name_t is 128 bytes.
        let mut buf = [0 as c_char; 128];
        // SAFETY: `self.0` is a live registry entry we own; `buf` is the 128-byte
        // io_name_t the call writes a NUL-terminated string into.
        let rc = unsafe { IORegistryEntryGetName(self.0, buf.as_mut_ptr()) };
        if rc != 0 {
            return None;
        }
        // SAFETY: on success the kernel wrote a NUL-terminated string within `buf`.
        let name = unsafe { CStr::from_ptr(buf.as_ptr()) };
        Some(name.to_string_lossy().into_owned())
    }

    /// All properties of the entry (`IORegistryEntryCreateCFProperties`).
    pub(crate) fn properties(&self) -> Option<Dict> {
        let mut props: CFMutableDictionaryRef = std::ptr::null_mut();
        crate::calls::count(crate::calls::Api::IoKit);
        // SAFETY: `self.0` is a live registry entry; `props` is a valid out pointer that
        // receives a dictionary we own (create rule) on success.
        let rc = unsafe {
            IORegistryEntryCreateCFProperties(self.0, &mut props, kCFAllocatorDefault, 0)
        };
        if rc != 0 || props.is_null() {
            return None;
        }
        // SAFETY: non-null dictionary returned under the create rule; the wrapper takes
        // over that reference and releases it on drop.
        Some(unsafe { Dict::wrap_under_create_rule(props.cast_const()) })
    }

    /// One property of the entry (`IORegistryEntryCreateCFProperty`), if it is a
    /// dictionary. Cheaper than [`IoObject::properties`] when only one key is needed.
    pub(crate) fn dict_property(&self, key: &str) -> Option<Dict> {
        let key = CFString::new(key);
        crate::calls::count(crate::calls::Api::IoKit);
        // SAFETY: `self.0` is a live registry entry and `key` a valid CFString; the
        // result is null or an object we own (create rule).
        let value = unsafe {
            IORegistryEntryCreateCFProperty(
                self.0,
                key.as_concrete_TypeRef(),
                kCFAllocatorDefault,
                0,
            )
        };
        if value.is_null() {
            return None;
        }
        // SAFETY: non-null object under the create rule; CFType takes ownership.
        let value = unsafe { core_foundation::base::CFType::wrap_under_create_rule(value) };
        if !value.instance_of::<Dict>() {
            return None;
        }
        // SAFETY: type checked above; the get rule adds a reference for the new wrapper,
        // `value` drops its own.
        Some(unsafe { Dict::wrap_under_get_rule(value.as_CFTypeRef().cast()) })
    }
}

impl Drop for IoObject {
    fn drop(&mut self) {
        // SAFETY: we own exactly one reference to this io_object_t.
        unsafe {
            IOObjectRelease(self.0);
        }
    }
}

/// Every registry entry matching an IOService class name, such as `AppleARMIODevice` or
/// `IOAccelerator`.
pub(crate) fn matching_services(class: &CStr) -> Vec<IoObject> {
    let mut iter: u32 = 0;
    crate::calls::count(crate::calls::Api::IoKit);
    // SAFETY: `class` is NUL-terminated. IOServiceMatching returns a dictionary that
    // IOServiceGetMatchingServices consumes (one reference), so it is not released here.
    let rc = unsafe {
        let matching = IOServiceMatching(class.as_ptr());
        if matching.is_null() {
            return Vec::new();
        }
        IOServiceGetMatchingServices(MAIN_PORT_DEFAULT, matching.cast_const(), &mut iter)
    };
    if rc != 0 || iter == 0 {
        return Vec::new();
    }
    let iter = IoObject(iter);
    let mut out = Vec::new();
    loop {
        // SAFETY: `iter` is a live iterator we own; each returned entry is owned by us.
        let next = unsafe { IOIteratorNext(iter.raw()) };
        if next == 0 {
            break;
        }
        out.push(IoObject(next));
    }
    out
}
