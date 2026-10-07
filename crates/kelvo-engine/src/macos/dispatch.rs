//! The few libdispatch calls the engine needs. libdispatch is part of libSystem, so no
//! extra link is required. Function pointers (`*_f` variants) are used instead of blocks.

use std::ffi::{CStr, c_char, c_int, c_void};

/// `dispatch_object_t` and friends: opaque, reference counted, thread-safe.
pub(super) type DispatchObject = *mut c_void;

/// `DISPATCH_TIME_NOW`.
pub(super) const TIME_NOW: u64 = 0;

/// `QOS_CLASS_UTILITY`.
const QOS_CLASS_UTILITY: u32 = 0x11;

#[repr(C)]
pub(super) struct SourceType {
    _opaque: [u8; 0],
}

unsafe extern "C" {
    /// `DISPATCH_SOURCE_TYPE_TIMER` is `&_dispatch_source_type_timer`.
    pub(super) static _dispatch_source_type_timer: SourceType;

    fn dispatch_queue_create(label: *const c_char, attr: *mut c_void) -> DispatchObject;
    fn dispatch_queue_attr_make_with_qos_class(
        attr: *mut c_void,
        qos_class: u32,
        relative_priority: c_int,
    ) -> *mut c_void;
    pub(super) fn dispatch_source_create(
        kind: *const SourceType,
        handle: usize,
        mask: usize,
        queue: DispatchObject,
    ) -> DispatchObject;
    pub(super) fn dispatch_source_set_timer(
        source: DispatchObject,
        start: u64,
        interval: u64,
        leeway: u64,
    );
    pub(super) fn dispatch_source_set_event_handler_f(
        source: DispatchObject,
        handler: extern "C" fn(*mut c_void),
    );
    pub(super) fn dispatch_source_cancel(source: DispatchObject);
    pub(super) fn dispatch_set_context(object: DispatchObject, context: *mut c_void);
    pub(super) fn dispatch_resume(object: DispatchObject);
    pub(super) fn dispatch_release(object: DispatchObject);
    pub(super) fn dispatch_time(when: u64, delta: i64) -> u64;
    fn dispatch_sync_f(
        queue: DispatchObject,
        context: *mut c_void,
        work: extern "C" fn(*mut c_void),
    );
}

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
