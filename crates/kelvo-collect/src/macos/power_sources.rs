//! IOPowerSources snapshots (public IOKit power-source API), shared by the battery
//! collector and the engine's power signals so the walk over the sources is written once.

use core_foundation::array::CFArray;
use core_foundation::base::{CFType, TCFType};
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::CFString;
use core_foundation_sys::array::CFArrayRef;
use core_foundation_sys::base::CFTypeRef;
use core_foundation_sys::dictionary::CFDictionaryRef;
use core_foundation_sys::string::CFStringRef;

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOPSCopyPowerSourcesInfo() -> CFTypeRef;
    fn IOPSGetProvidingPowerSourceType(snapshot: CFTypeRef) -> CFStringRef;
    fn IOPSCopyPowerSourcesList(blob: CFTypeRef) -> CFArrayRef;
    fn IOPSGetPowerSourceDescription(blob: CFTypeRef, ps: CFTypeRef) -> CFDictionaryRef;
}

/// One power source's description dictionary.
pub type Description = CFDictionary<CFString, CFType>;

/// One `IOPSCopyPowerSourcesInfo` snapshot, released on drop.
pub struct Snapshot(CFType);

impl Snapshot {
    /// Takes a snapshot. Not call-counted: the engine's power signals take it outside
    /// the collectors' per-tick call ceilings (D-062). Collectors use
    /// [`Snapshot::counted`].
    pub fn take() -> Option<Snapshot> {
        // SAFETY: Copy rule; we own the blob (or get null).
        let blob = unsafe { IOPSCopyPowerSourcesInfo() };
        if blob.is_null() {
            return None;
        }
        // SAFETY: non-null owned CF object.
        Some(Snapshot(unsafe { CFType::wrap_under_create_rule(blob) }))
    }

    /// [`Snapshot::take`], counted as one IOKit call.
    pub(crate) fn counted() -> Option<Snapshot> {
        crate::calls::count(crate::calls::Api::IoKit);
        Self::take()
    }

    /// Whether the providing power source is the battery ("Battery Power").
    pub fn on_battery(&self) -> bool {
        // SAFETY: `self.0` is a valid snapshot; the returned string follows the Get rule
        // and is only used while the snapshot is alive.
        let kind = unsafe { IOPSGetProvidingPowerSourceType(self.0.as_CFTypeRef()) };
        if kind.is_null() {
            return false;
        }
        // SAFETY: non-null CFString borrowed under the Get rule.
        let s = unsafe { CFString::wrap_under_get_rule(kind) };
        s == CFString::from_static_string("Battery Power")
    }

    /// The internal battery's description (`Type` is `InternalBattery`), if there is one.
    pub fn internal_battery(&self) -> Option<Description> {
        let blob = self.0.as_CFTypeRef();
        // SAFETY: `blob` is the power-sources blob; Copy rule for the list.
        let list = unsafe { IOPSCopyPowerSourcesList(blob) };
        if list.is_null() {
            return None;
        }
        // SAFETY: non-null owned CFArray.
        let list: CFArray<CFType> = unsafe { CFArray::wrap_under_create_rule(list) };
        let k_type = CFString::from_static_string("Type");
        for ps in list.iter() {
            // SAFETY: `ps` comes from the list for this blob; Get rule, so we retain it
            // with wrap_under_get_rule before the snapshot is released.
            let desc = unsafe { IOPSGetPowerSourceDescription(blob, ps.as_CFTypeRef()) };
            if desc.is_null() {
                continue;
            }
            // SAFETY: non-null borrowed dictionary with string keys.
            let desc: Description = unsafe { CFDictionary::wrap_under_get_rule(desc) };
            let internal = desc
                .find(&k_type)
                .and_then(|v| v.downcast::<CFString>())
                .is_some_and(|t| t == CFString::from_static_string("InternalBattery"));
            if internal {
                return Some(desc);
            }
        }
        None
    }
}
