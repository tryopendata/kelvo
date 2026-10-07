//! The libdispatch declarations Kelvo uses, in one place: the NetworkStatistics session's
//! queue here, and the engine's queues and timer (`kelvo-engine/src/macos/dispatch.rs`).
//! libdispatch is part of libSystem, so no extra link is required. Function pointers
//! (`*_f` variants) are used instead of blocks. Callers wrap each call in `unsafe` with
//! a `SAFETY:` comment.

use std::ffi::{c_char, c_int, c_void};

/// `dispatch_object_t` and friends: opaque, reference counted, thread-safe.
pub type DispatchObject = *mut c_void;

/// The type behind `DISPATCH_SOURCE_TYPE_*`.
#[repr(C)]
pub struct SourceType {
    _opaque: [u8; 0],
}

unsafe extern "C" {
    /// `DISPATCH_SOURCE_TYPE_TIMER` is `&_dispatch_source_type_timer`.
    pub static _dispatch_source_type_timer: SourceType;

    pub fn dispatch_queue_create(label: *const c_char, attr: *mut c_void) -> DispatchObject;
    pub fn dispatch_queue_attr_make_with_qos_class(
        attr: *mut c_void,
        qos_class: u32,
        relative_priority: c_int,
    ) -> *mut c_void;
    pub fn dispatch_source_create(
        kind: *const SourceType,
        handle: usize,
        mask: usize,
        queue: DispatchObject,
    ) -> DispatchObject;
    pub fn dispatch_source_set_timer(
        source: DispatchObject,
        start: u64,
        interval: u64,
        leeway: u64,
    );
    pub fn dispatch_source_set_event_handler_f(
        source: DispatchObject,
        handler: extern "C" fn(*mut c_void),
    );
    pub fn dispatch_source_cancel(source: DispatchObject);
    pub fn dispatch_set_context(object: DispatchObject, context: *mut c_void);
    pub fn dispatch_resume(object: DispatchObject);
    pub fn dispatch_release(object: DispatchObject);
    pub fn dispatch_time(when: u64, delta: i64) -> u64;
    pub fn dispatch_sync_f(
        queue: DispatchObject,
        context: *mut c_void,
        work: extern "C" fn(*mut c_void),
    );
}
