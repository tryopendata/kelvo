//! The libdispatch declarations Kelvo uses, in one place: the NetworkStatistics session's
//! queue here, and the engine's queues and timer through [`Queue`] and [`TimerSource`].
//! libdispatch is part of libSystem, so no extra link is required. Function pointers
//! (`*_f` variants) are used instead of blocks. The declarations are crate-private: code
//! in this crate wraps each call in `unsafe` with a `SAFETY:` comment, and the engine
//! uses the wrappers.

use std::ffi::{CStr, c_char, c_int, c_void};

/// `dispatch_object_t` and friends: opaque, reference counted, thread-safe.
pub(crate) type DispatchObject = *mut c_void;

/// The type behind `DISPATCH_SOURCE_TYPE_*`.
#[repr(C)]
struct SourceType {
    _opaque: [u8; 0],
}

unsafe extern "C" {
    /// `DISPATCH_SOURCE_TYPE_TIMER` is `&_dispatch_source_type_timer`.
    static _dispatch_source_type_timer: SourceType;

    pub(crate) fn dispatch_queue_create(label: *const c_char, attr: *mut c_void) -> DispatchObject;
    fn dispatch_queue_attr_make_with_qos_class(
        attr: *mut c_void,
        qos_class: u32,
        relative_priority: c_int,
    ) -> *mut c_void;
    fn dispatch_source_create(
        kind: *const SourceType,
        handle: usize,
        mask: usize,
        queue: DispatchObject,
    ) -> DispatchObject;
    fn dispatch_source_set_timer(source: DispatchObject, start: u64, interval: u64, leeway: u64);
    fn dispatch_source_set_event_handler_f(
        source: DispatchObject,
        handler: extern "C" fn(*mut c_void),
    );
    fn dispatch_source_cancel(source: DispatchObject);
    fn dispatch_set_context(object: DispatchObject, context: *mut c_void);
    fn dispatch_resume(object: DispatchObject);
    pub(crate) fn dispatch_release(object: DispatchObject);
    fn dispatch_time(when: u64, delta: i64) -> u64;
    pub(crate) fn dispatch_sync_f(
        queue: DispatchObject,
        context: *mut c_void,
        work: extern "C" fn(*mut c_void),
    );
}

/// `DISPATCH_TIME_NOW`.
const TIME_NOW: u64 = 0;

/// `QOS_CLASS_UTILITY`.
const QOS_CLASS_UTILITY: u32 = 0x11;

/// A serial dispatch queue at utility QoS, released on drop.
pub struct Queue(DispatchObject);

// SAFETY: dispatch queues are thread-safe reference-counted objects; every libdispatch
// call on them may be made from any thread.
unsafe impl Send for Queue {}
// SAFETY: as above; `&Queue` only exposes thread-safe libdispatch calls.
unsafe impl Sync for Queue {}

extern "C" fn noop(_: *mut c_void) {}

impl Queue {
    pub fn utility(label: &CStr) -> Option<Queue> {
        // SAFETY: a NULL attribute is DISPATCH_QUEUE_SERIAL; the QoS constant is valid.
        let attr = unsafe {
            dispatch_queue_attr_make_with_qos_class(std::ptr::null_mut(), QOS_CLASS_UTILITY, 0)
        };
        // SAFETY: `label` is NUL-terminated and outlives the call (libdispatch copies it);
        // `attr` came from libdispatch.
        let q = unsafe { dispatch_queue_create(label.as_ptr(), attr) };
        (!q.is_null()).then_some(Queue(q))
    }

    pub(crate) fn raw(&self) -> DispatchObject {
        self.0
    }

    /// Waits until everything already submitted to the queue has run. Used before
    /// freeing a context a handler on this queue might still be reading. Never call it
    /// from the queue itself: it would deadlock.
    pub fn drain(&self) {
        // SAFETY: the queue is valid; `noop` ignores its (null) context.
        unsafe { dispatch_sync_f(self.0, std::ptr::null_mut(), noop) };
    }
}

impl Drop for Queue {
    fn drop(&mut self) {
        // SAFETY: we own one reference from dispatch_queue_create.
        unsafe { dispatch_release(self.0) };
    }
}

/// A running `DispatchSource` timer, released on drop. Cancel it and drain its queue
/// before freeing the handler's context.
pub struct TimerSource(DispatchObject);

// SAFETY: dispatch sources are thread-safe reference-counted objects; cancel and release
// may be called from any thread.
unsafe impl Send for TimerSource {}

impl TimerSource {
    /// A timer on `queue` that calls `handler(context)` first after `delay_ns`, then every
    /// `interval_ns`, with `leeway_ns` for the kernel to coalesce wakeups. `None` when
    /// the source cannot be created.
    ///
    /// # Safety
    ///
    /// `context` must stay valid for `handler` until [`TimerSource::cancel`] has returned
    /// and `queue` has been drained.
    pub unsafe fn start(
        queue: &Queue,
        context: *mut c_void,
        handler: extern "C" fn(*mut c_void),
        delay_ns: i64,
        interval_ns: u64,
        leeway_ns: u64,
    ) -> Option<TimerSource> {
        // SAFETY: the timer source type is a libdispatch constant; the queue is valid.
        let source = unsafe {
            dispatch_source_create(&raw const _dispatch_source_type_timer, 0, 0, queue.raw())
        };
        if source.is_null() {
            return None;
        }
        // SAFETY: `source` is a valid, suspended timer source. The caller keeps `context`
        // valid until the source is cancelled and the queue drained.
        unsafe {
            dispatch_set_context(source, context);
            dispatch_source_set_event_handler_f(source, handler);
            dispatch_source_set_timer(
                source,
                dispatch_time(TIME_NOW, delay_ns),
                interval_ns,
                leeway_ns,
            );
            dispatch_resume(source);
        }
        Some(TimerSource(source))
    }

    /// Stops further handler calls. One already running may still finish; drain the
    /// queue to wait it out.
    pub fn cancel(&self) {
        // SAFETY: `self.0` is a live source.
        unsafe { dispatch_source_cancel(self.0) };
    }
}

impl Drop for TimerSource {
    fn drop(&mut self) {
        // SAFETY: we own the reference from dispatch_source_create, and it was resumed in
        // `start`, so releasing it is allowed.
        unsafe { dispatch_release(self.0) };
    }
}
