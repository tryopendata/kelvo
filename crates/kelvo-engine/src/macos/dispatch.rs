//! The few libdispatch calls the engine needs: a utility-QoS serial queue, and the timer
//! calls the ticker makes. The declarations are collect's (`kelvo_collect::macos::dispatch`),
//! shared with the NetworkStatistics collector.

use std::ffi::{CStr, c_void};

pub(super) use kelvo_collect::macos::dispatch::{
    _dispatch_source_type_timer, DispatchObject, dispatch_release, dispatch_resume,
    dispatch_set_context, dispatch_source_cancel, dispatch_source_create,
    dispatch_source_set_event_handler_f, dispatch_source_set_timer, dispatch_time,
};
use kelvo_collect::macos::dispatch::{
    dispatch_queue_attr_make_with_qos_class, dispatch_queue_create, dispatch_sync_f,
};

/// `DISPATCH_TIME_NOW`.
pub(super) const TIME_NOW: u64 = 0;

/// `QOS_CLASS_UTILITY`.
const QOS_CLASS_UTILITY: u32 = 0x11;

/// A serial dispatch queue at utility QoS, released on drop.
pub(super) struct Queue(DispatchObject);

// SAFETY: dispatch queues are thread-safe reference-counted objects; every libdispatch
// call on them may be made from any thread.
unsafe impl Send for Queue {}
// SAFETY: as above; `&Queue` only exposes thread-safe libdispatch calls.
unsafe impl Sync for Queue {}

extern "C" fn noop(_: *mut c_void) {}

impl Queue {
    pub(super) fn utility(label: &CStr) -> Option<Queue> {
        // SAFETY: a NULL attribute is DISPATCH_QUEUE_SERIAL; the QoS constant is valid.
        let attr = unsafe {
            dispatch_queue_attr_make_with_qos_class(std::ptr::null_mut(), QOS_CLASS_UTILITY, 0)
        };
        // SAFETY: `label` is NUL-terminated and outlives the call (libdispatch copies it);
        // `attr` came from libdispatch.
        let q = unsafe { dispatch_queue_create(label.as_ptr(), attr) };
        (!q.is_null()).then_some(Queue(q))
    }

    pub(super) fn raw(&self) -> DispatchObject {
        self.0
    }

    /// Waits until everything already submitted to the queue has run. Used before
    /// freeing a context a handler on this queue might still be reading.
    pub(super) fn drain(&self) {
        // SAFETY: the queue is valid; `noop` ignores its (null) context. Never called from
        // the queue itself, so it cannot deadlock.
        unsafe { dispatch_sync_f(self.0, std::ptr::null_mut(), noop) };
    }
}

impl Drop for Queue {
    fn drop(&mut self) {
        // SAFETY: we own one reference from dispatch_queue_create.
        unsafe { dispatch_release(self.0) };
    }
}
