//! IOReport subscriptions and delta samples (macmon `IOReport`, `IOReportIterator`,
//! `cfio_get_residencies`, `cfio_watts`).
//!
//! IOReport is a private framework (`/usr/lib/libIOReport.dylib`). The signatures below
//! are the ones macmon, asitop and socpowerbud use; Apple does not document them.
//! `IOReportCopyChannelsInGroup` and `IOReportMergeChannels` replace upstream's
//! copy-everything-then-filter: verified on an M3 Max, macOS 27.0.1.

use std::marker::PhantomData;

use core_foundation::array::{
    CFArrayAppendValue, CFArrayCreateMutable, CFArrayGetCount, CFArrayGetValueAtIndex, CFArrayRef,
    kCFTypeArrayCallBacks,
};
use core_foundation::base::{CFType, CFTypeRef, TCFType, kCFAllocatorDefault};
use core_foundation::dictionary::{
    CFDictionaryGetValue, CFDictionaryRef, CFDictionarySetValue, CFMutableDictionaryRef,
};
use core_foundation::string::{CFString, CFStringRef};

use super::cf;

/// Opaque `IOReportSubscriptionRef`. It is a CF object (released with `CFRelease`).
type SubscriptionRef = CFTypeRef;

#[link(name = "IOReport", kind = "dylib")]
unsafe extern "C" {
    fn IOReportCopyChannelsInGroup(
        group: CFStringRef,
        subgroup: CFStringRef,
        a: u64,
        b: u64,
        c: u64,
    ) -> CFMutableDictionaryRef;
    fn IOReportMergeChannels(
        into: CFMutableDictionaryRef,
        from: CFMutableDictionaryRef,
        unused: CFTypeRef,
    );
    fn IOReportCreateSubscription(
        unused: CFTypeRef,
        channels: CFMutableDictionaryRef,
        subscribed: *mut CFMutableDictionaryRef,
        channel_id: u64,
        unused2: CFTypeRef,
    ) -> SubscriptionRef;
    fn IOReportCreateSamples(
        sub: SubscriptionRef,
        channels: CFMutableDictionaryRef,
        unused: CFTypeRef,
    ) -> CFDictionaryRef;
    fn IOReportCreateSamplesDelta(
        prev: CFDictionaryRef,
        cur: CFDictionaryRef,
        unused: CFTypeRef,
    ) -> CFDictionaryRef;
    fn IOReportChannelGetGroup(item: CFDictionaryRef) -> CFStringRef;
    fn IOReportChannelGetChannelName(item: CFDictionaryRef) -> CFStringRef;
    fn IOReportChannelGetUnitLabel(item: CFDictionaryRef) -> CFStringRef;
    fn IOReportSimpleGetIntegerValue(item: CFDictionaryRef, index: i32) -> i64;
    fn IOReportStateGetCount(item: CFDictionaryRef) -> i32;
    fn IOReportStateGetNameForIndex(item: CFDictionaryRef, index: i32) -> CFStringRef;
    fn IOReportStateGetResidency(item: CFDictionaryRef, index: i32) -> i64;
}

const CHANNELS_KEY: &str = "IOReportChannels";

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub(crate) enum IoReportError {
    #[error("IOReport has no channel group {0:?}")]
    NoGroup(&'static str),
    #[error("no IOReport channel passed the filter")]
    NoChannels,
    #[error("IOReportCreateSubscription returned null")]
    Subscribe,
}

/// One `(group, subgroup)` to subscribe to. `subgroup: None` takes the whole group.
#[derive(Clone, Copy, Debug)]
pub(crate) struct GroupSpec {
    pub group: &'static str,
    pub subgroup: Option<&'static str>,
}

/// A live subscription plus the previous raw sample deltas are taken against.
pub(crate) struct Subscription {
    sub: CFType,
    /// The channel dictionary the subscription was created from; `IOReportCreateSamples`
    /// takes it on every call.
    channels: CFType,
    prev: Option<(CFType, u64)>,
    /// `"IOReportChannels"`, kept so sampling does not build the string every tick.
    channels_key: CFString,
}

// SAFETY: the CF objects are only touched through `&mut self` (one thread at a time), and
// CoreFoundation retain/release is thread-safe. IOReport does not tie a subscription to
// the thread that created it.
unsafe impl Send for Subscription {}

impl Subscription {
    /// Subscribes to `groups`, keeping only channels for which `keep(group, channel)` is
    /// true.
    pub(crate) fn new(
        groups: &[GroupSpec],
        keep: impl Fn(&str, &str) -> bool,
    ) -> Result<Self, IoReportError> {
        let mut merged: Option<CFType> = None;
        for spec in groups {
            let group = CFString::new(spec.group);
            let subgroup = spec.subgroup.map(CFString::new);
            let subgroup_ref = subgroup
                .as_ref()
                .map_or(std::ptr::null(), |s| s.as_concrete_TypeRef());
            // SAFETY: both strings are valid CFStrings (or null for "any subgroup"); the
            // result is null or a mutable dictionary we own (copy rule).
            let found = unsafe {
                IOReportCopyChannelsInGroup(group.as_concrete_TypeRef(), subgroup_ref, 0, 0, 0)
            };
            if found.is_null() {
                return Err(IoReportError::NoGroup(spec.group));
            }
            // SAFETY: non-null, owned under the copy rule; CFType releases it on drop.
            let found = unsafe { CFType::wrap_under_create_rule(found.cast_const().cast()) };
            match &merged {
                None => merged = Some(found),
                Some(into) => {
                    // SAFETY: both are live mutable channel dictionaries from
                    // IOReportCopyChannelsInGroup; the call appends `found`'s channels to
                    // `into` and keeps no reference to `found`.
                    unsafe {
                        IOReportMergeChannels(
                            into.as_CFTypeRef().cast_mut().cast(),
                            found.as_CFTypeRef().cast_mut().cast(),
                            std::ptr::null(),
                        );
                    }
                }
            }
        }
        let channels = merged.ok_or(IoReportError::NoChannels)?;
        filter_channels(&channels, &keep)?;

        let mut subscribed: CFMutableDictionaryRef = std::ptr::null_mut();
        // SAFETY: `channels` is a live mutable channel dictionary; `subscribed` is a valid
        // out pointer. Both results are owned by us (create rule) when non-null.
        let sub = unsafe {
            IOReportCreateSubscription(
                std::ptr::null(),
                channels.as_CFTypeRef().cast_mut().cast(),
                &mut subscribed,
                0,
                std::ptr::null(),
            )
        };
        if !subscribed.is_null() {
            // SAFETY: owned under the create rule; we do not use it, so release it now.
            drop(unsafe { CFType::wrap_under_create_rule(subscribed.cast_const().cast()) });
        }
        if sub.is_null() {
            return Err(IoReportError::Subscribe);
        }
        // SAFETY: non-null subscription under the create rule.
        let sub = unsafe { CFType::wrap_under_create_rule(sub) };
        Ok(Self {
            sub,
            channels,
            prev: None,
            channels_key: CFString::new(CHANNELS_KEY),
        })
    }

    fn raw_sample(&self) -> Option<CFType> {
        crate::calls::count(crate::calls::Api::IoReport);
        // SAFETY: `sub` and `channels` are the live pair the subscription was created
        // with; the result is null or a dictionary we own.
        let s = unsafe {
            IOReportCreateSamples(
                self.sub.as_CFTypeRef(),
                self.channels.as_CFTypeRef().cast_mut().cast(),
                std::ptr::null(),
            )
        };
        if s.is_null() {
            return None;
        }
        // SAFETY: non-null, create rule.
        Some(unsafe { CFType::wrap_under_create_rule(s.cast()) })
    }

    /// Takes a sample at `now_ns` (a monotonic clock that advances during sleep) and
    /// returns the delta against the previous one. The first call only stores the
    /// baseline and returns `None`, as does a failed sample (which also drops the
    /// baseline, so the next delta never spans a failure).
    pub(crate) fn delta(&mut self, now_ns: u64) -> Option<Delta> {
        let Some(cur) = self.raw_sample() else {
            self.prev = None;
            return None;
        };
        let prev = self.prev.replace((cur.clone(), now_ns));
        let (prev, prev_ns) = prev?;
        // SAFETY: both samples are live dictionaries from IOReportCreateSamples on the
        // same subscription; the result is null or owned by us.
        let d = unsafe {
            IOReportCreateSamplesDelta(
                prev.as_CFTypeRef().cast(),
                cur.as_CFTypeRef().cast(),
                std::ptr::null(),
            )
        };
        if d.is_null() {
            return None;
        }
        // SAFETY: non-null, create rule.
        let dict = unsafe { CFType::wrap_under_create_rule(d.cast()) };
        Delta::new(dict, &self.channels_key, now_ns.saturating_sub(prev_ns))
    }

    /// Forgets the baseline, so the next [`Subscription::delta`] starts over.
    pub(crate) fn reset(&mut self) {
        self.prev = None;
    }
}

/// Replaces the channel array in `channels` with the subset `keep` accepts.
fn filter_channels(
    channels: &CFType,
    keep: &impl Fn(&str, &str) -> bool,
) -> Result<(), IoReportError> {
    let dict = channels.as_CFTypeRef().cast_mut() as CFMutableDictionaryRef;
    let key = CFString::new(CHANNELS_KEY);
    // SAFETY: `dict` is a live channel dictionary; the value is borrowed (get rule) and
    // stays alive until we replace it below.
    let array = unsafe { CFDictionaryGetValue(dict, key.as_concrete_TypeRef().cast()) };
    if array.is_null() {
        return Err(IoReportError::NoChannels);
    }
    let array = array as CFArrayRef;
    // SAFETY: `array` is the live CFArray of channel dictionaries.
    let count = unsafe { CFArrayGetCount(array) };
    // SAFETY: creates an empty mutable array we own.
    let kept = unsafe { CFArrayCreateMutable(kCFAllocatorDefault, count, &kCFTypeArrayCallBacks) };
    if kept.is_null() {
        return Err(IoReportError::NoChannels);
    }
    // SAFETY: owned under the create rule.
    let kept_owner = unsafe { CFType::wrap_under_create_rule(kept.cast_const().cast()) };
    let mut n = 0;
    for i in 0..count {
        // SAFETY: `i < count`; items are channel dictionaries borrowed from `array`.
        let item = unsafe { CFArrayGetValueAtIndex(array, i) } as CFDictionaryRef;
        if item.is_null() {
            continue;
        }
        // SAFETY: `item` is a live channel dictionary.
        let (group, name) = unsafe {
            (
                cf::string(IOReportChannelGetGroup(item)).unwrap_or_default(),
                cf::string(IOReportChannelGetChannelName(item)).unwrap_or_default(),
            )
        };
        if keep(&group, &name) {
            // SAFETY: `kept` is our mutable array with CFType callbacks, which retain.
            unsafe { CFArrayAppendValue(kept, item.cast()) };
            n += 1;
        }
    }
    if n == 0 {
        return Err(IoReportError::NoChannels);
    }
    // SAFETY: `dict` is mutable (from IOReportCopyChannelsInGroup); the dictionary
    // retains `kept` and releases the old array.
    unsafe {
        CFDictionarySetValue(
            dict,
            key.as_concrete_TypeRef().cast(),
            kept.cast_const().cast(),
        )
    };
    drop(kept_owner);
    Ok(())
}

/// A delta sample: the change in every subscribed channel over `elapsed_ns`.
pub(crate) struct Delta {
    /// Owns the sample; `items` borrows from it.
    _dict: CFType,
    items: CFArrayRef,
    len: usize,
    pub elapsed_ns: u64,
}

impl Delta {
    fn new(dict: CFType, key: &CFString, elapsed_ns: u64) -> Option<Self> {
        // SAFETY: `dict` is a live sample dictionary; the array is borrowed and lives as
        // long as `dict`, which `Delta` owns.
        let items = unsafe {
            CFDictionaryGetValue(dict.as_CFTypeRef().cast(), key.as_concrete_TypeRef().cast())
        };
        if items.is_null() {
            return None;
        }
        let items = items as CFArrayRef;
        // SAFETY: `items` is a live CFArray.
        let len = usize::try_from(unsafe { CFArrayGetCount(items) }).ok()?;
        Some(Self {
            _dict: dict,
            items,
            len,
            elapsed_ns,
        })
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn channel(&self, i: usize) -> Option<Channel<'_>> {
        if i >= self.len {
            return None;
        }
        // SAFETY: `i < len`, and the array outlives the returned borrow (tied to `self`).
        let item = unsafe { CFArrayGetValueAtIndex(self.items, i as isize) } as CFDictionaryRef;
        (!item.is_null()).then_some(Channel {
            item,
            _delta: PhantomData,
        })
    }
}

/// One channel of a [`Delta`].
#[derive(Clone, Copy)]
pub(crate) struct Channel<'a> {
    item: CFDictionaryRef,
    _delta: PhantomData<&'a Delta>,
}

impl Channel<'_> {
    pub(crate) fn group(&self) -> String {
        // SAFETY: `item` is a live channel dictionary for the borrow's lifetime.
        unsafe { cf::string(IOReportChannelGetGroup(self.item)) }.unwrap_or_default()
    }

    pub(crate) fn name(&self) -> String {
        // SAFETY: as above.
        unsafe { cf::string(IOReportChannelGetChannelName(self.item)) }.unwrap_or_default()
    }

    /// The unit label, trimmed ("mJ", "nJ", "24Mticks").
    pub(crate) fn unit(&self) -> String {
        // SAFETY: as above.
        unsafe { cf::string(IOReportChannelGetUnitLabel(self.item)) }
            .unwrap_or_default()
            .trim()
            .to_owned()
    }

    /// A simple channel's value (energy counters).
    pub(crate) fn integer(&self) -> i64 {
        // SAFETY: as above; index 0 is the only value of a simple channel.
        unsafe { IOReportSimpleGetIntegerValue(self.item, 0) }
    }

    /// Number of states of a state channel (0 for other channel kinds).
    pub(crate) fn state_count(&self) -> usize {
        // SAFETY: as above.
        usize::try_from(unsafe { IOReportStateGetCount(self.item) }).unwrap_or(0)
    }

    pub(crate) fn state_name(&self, i: usize) -> String {
        let Ok(i) = i32::try_from(i) else {
            return String::new();
        };
        // SAFETY: as above; out-of-range indexes return null.
        unsafe { cf::string(IOReportStateGetNameForIndex(self.item, i)) }.unwrap_or_default()
    }

    /// Residency of state `i` over the delta, in the channel's unit (ticks).
    pub(crate) fn residency(&self, i: usize) -> i64 {
        let Ok(i) = i32::try_from(i) else {
            return 0;
        };
        // SAFETY: as above.
        unsafe { IOReportStateGetResidency(self.item, i) }
    }
}

/// Joules from an energy counter delta in `unit` ("mJ", "uJ", "nJ"). `None` for any
/// other unit.
pub(crate) fn joules(value: i64, unit: &str) -> Option<f64> {
    let scale = match unit {
        "mJ" => 1e-3,
        "uJ" => 1e-6,
        "nJ" => 1e-9,
        _ => return None,
    };
    Some(value as f64 * scale)
}

/// Monotonic nanoseconds that keep advancing during sleep (`CLOCK_MONOTONIC` on Darwin is
/// `mach_continuous_time`). Used to time IOReport deltas.
pub(crate) fn now_ns() -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `ts` is a valid out pointer; CLOCK_MONOTONIC always exists on Darwin.
    let rc = unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    if rc != 0 {
        return 0;
    }
    u64::try_from(ts.tv_sec)
        .unwrap_or(0)
        .saturating_mul(1_000_000_000)
        .saturating_add(u64::try_from(ts.tv_nsec).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn energy_units() {
        assert_eq!(joules(1500, "mJ"), Some(1.5));
        assert_eq!(joules(2_000_000, "uJ"), Some(2.0));
        assert_eq!(joules(3_000_000_000, "nJ"), Some(3.0));
        assert_eq!(joules(1, "W"), None);
    }
}
