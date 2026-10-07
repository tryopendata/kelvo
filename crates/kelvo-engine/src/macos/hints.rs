//! Device hints from IOKit match notifications (architecture.md infra 9): first-match and
//! terminated notifications for `IOMedia` (disks, Disk module) and `IONetworkInterface`
//! (Network module), delivered on a utility dispatch queue. A burst (a disk with several
//! partitions) becomes several hints the engine coalesces into one re-probe.

use std::ffi::{CStr, c_char, c_int, c_void};

use core_foundation_sys::dictionary::{CFDictionaryRef, CFMutableDictionaryRef};
use kelvo_schema::Module;

use super::dispatch::Queue;
use crate::hints::DeviceHints;
use crate::inbox::Inbox;

type IoObject = u32;
type NotificationPort = *mut c_void;
type MatchingCallback = extern "C" fn(*mut c_void, IoObject);

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IONotificationPortCreate(main_port: u32) -> NotificationPort;
    fn IONotificationPortSetDispatchQueue(port: NotificationPort, queue: *mut c_void);
    fn IONotificationPortDestroy(port: NotificationPort);
    fn IOServiceMatching(name: *const c_char) -> CFMutableDictionaryRef;
    fn IOServiceAddMatchingNotification(
        port: NotificationPort,
        notification_type: *const c_char,
        matching: CFDictionaryRef,
        callback: MatchingCallback,
        refcon: *mut c_void,
        iterator: *mut IoObject,
    ) -> c_int;
    fn IOIteratorNext(iterator: IoObject) -> IoObject;
    fn IOObjectRelease(object: IoObject) -> c_int;
}

const FIRST_MATCH: &CStr = c"IOServiceFirstMatch";
const TERMINATED: &CStr = c"IOServiceTerminate";
const WATCHED: [(&CStr, Module); 2] = [
    (c"IOMedia", Module::Disk),
    (c"IONetworkInterface", Module::Network),
];

struct HintCtx {
    inbox: Inbox,
    module: Module,
}

/// Releases every object in the iterator, which re-arms the notification. Returns how
/// many there were.
fn drain(iterator: IoObject) -> usize {
    let mut n = 0;
    loop {
        // SAFETY: `iterator` is a live notification iterator; each object returned is
        // owned and released here.
        let obj = unsafe { IOIteratorNext(iterator) };
        if obj == 0 {
            return n;
        }
        // SAFETY: as above.
        unsafe { IOObjectRelease(obj) };
        n += 1;
    }
}

extern "C" fn on_match(refcon: *mut c_void, iterator: IoObject) {
    // SAFETY: refcon is a boxed `HintCtx` that outlives the port (see `Watch::drop`).
    let ctx = unsafe { &*(refcon as *const HintCtx) };
    if drain(iterator) > 0 {
        ctx.inbox.hint(&[ctx.module]);
    }
}

struct Watch {
    queue: Queue,
    port: NotificationPort,
    iterators: Vec<IoObject>,
    // Boxed on purpose: IOKit holds raw pointers to each context, so they must not move
    // when the vector grows.
    #[allow(clippy::vec_box)]
    _ctxs: Vec<Box<HintCtx>>,
}

impl Watch {
    fn start(inbox: &Inbox) -> Option<Watch> {
        let queue = Queue::utility(c"com.tryopendata.kelvo.devices")?;
        // SAFETY: kIOMainPortDefault (0) is always valid.
        let port = unsafe { IONotificationPortCreate(0) };
        if port.is_null() {
            return None;
        }
        // SAFETY: valid port and queue.
        unsafe { IONotificationPortSetDispatchQueue(port, queue.raw()) };
        let mut watch = Watch {
            queue,
            port,
            iterators: Vec::new(),
            _ctxs: Vec::new(),
        };
        for (class, module) in WATCHED {
            let ctx = Box::new(HintCtx {
                inbox: inbox.clone(),
                module,
            });
            let refcon: *mut c_void = (&raw const *ctx).cast_mut().cast();
            watch._ctxs.push(ctx);
            for kind in [FIRST_MATCH, TERMINATED] {
                // SAFETY: NUL-terminated class name. The dictionary is consumed by
                // IOServiceAddMatchingNotification below, so it is not released here.
                let matching = unsafe { IOServiceMatching(class.as_ptr()) };
                if matching.is_null() {
                    continue;
                }
                let mut iterator: IoObject = 0;
                // SAFETY: valid port, consumed matching dictionary, refcon kept alive in
                // `_ctxs` until after the port is destroyed.
                let kr = unsafe {
                    IOServiceAddMatchingNotification(
                        port,
                        kind.as_ptr(),
                        matching as CFDictionaryRef,
                        on_match,
                        refcon,
                        &mut iterator,
                    )
                };
                if kr != 0 || iterator == 0 {
                    tracing::warn!(class = ?class, kr, "device notification not armed");
                    continue;
                }
                // The existing devices: draining arms the notification; no hint.
                drain(iterator);
                watch.iterators.push(iterator);
            }
        }
        Some(watch)
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        for it in self.iterators.drain(..) {
            // SAFETY: releasing iterators we own; this cancels their notifications.
            unsafe { IOObjectRelease(it) };
        }
        // SAFETY: destroying the port we created.
        unsafe { IONotificationPortDestroy(self.port) };
        // No callback can be left running after this; then the contexts drop.
        self.queue.drain();
    }
}

/// [`DeviceHints`] for disks and network interfaces from IOKit.
#[derive(Default)]
pub struct IoKitDeviceHints {
    watch: Option<Watch>,
}

// SAFETY: the raw port and iterators are touched only by `start` and `Drop` on the owning
// thread; callbacks only read the boxed contexts, whose `Inbox` is Send + Sync.
unsafe impl Send for IoKitDeviceHints {}

impl IoKitDeviceHints {
    pub fn new() -> Self {
        Self::default()
    }
}

impl DeviceHints for IoKitDeviceHints {
    fn start(&mut self, inbox: Inbox) {
        if self.watch.is_none() {
            self.watch = Watch::start(&inbox);
            if self.watch.is_none() {
                tracing::warn!("IOKit device notifications unavailable; no re-probe hints");
            }
        }
    }
}
