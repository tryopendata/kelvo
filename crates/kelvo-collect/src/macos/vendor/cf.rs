//! CoreFoundation helpers (macmon `cfdict_get_val`, `from_cfstr`, `cfnum`).
//!
//! Upstream built CFStrings with `CFStringCreateWithBytesNoCopy` because
//! `CFString::new` "creates broken objects if string len > 9" on an older
//! core-foundation release. core-foundation 0.10's `CFString::new` copies the bytes and
//! works for the long IOReport group names used here ("CPU Complex Performance States").

use core_foundation::base::{CFType, TCFType};
use core_foundation::data::CFData;
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::{CFString, CFStringRef};

/// A CoreFoundation dictionary with string keys and values of any type, the shape of
/// IORegistry property tables and IOReport sample dictionaries.
pub(crate) type Dict = CFDictionary<CFString, CFType>;

/// The value for `key`, retained.
pub(crate) fn get(dict: &Dict, key: &str) -> Option<CFType> {
    dict.find(CFString::new(key)).map(|v| v.clone())
}

/// The value for `key` as an integer, if it is a CFNumber.
pub(crate) fn get_i64(dict: &Dict, key: &str) -> Option<i64> {
    get(dict, key)?.downcast::<CFNumber>()?.to_i64()
}

/// The value for `key` as bytes, if it is CFData.
pub(crate) fn get_bytes(dict: &Dict, key: &str) -> Option<Vec<u8>> {
    get(dict, key)?
        .downcast::<CFData>()
        .map(|d| d.bytes().to_vec())
}

/// Copies a borrowed CFString into a Rust string. `None` for a null reference.
///
/// # Safety
///
/// `s` is null or a valid CFString that stays alive for the duration of the call.
pub(crate) unsafe fn string(s: CFStringRef) -> Option<String> {
    if s.is_null() {
        return None;
    }
    // SAFETY: non-null and valid per the caller's contract; the get rule retains it for
    // the wrapper's lifetime.
    Some(unsafe { CFString::wrap_under_get_rule(s) }.to_string())
}
