//! Device hints from IOKit match notifications (architecture.md infra 9): first-match and
//! terminated notifications for `IOMedia` (disks, Disk module) and `IONetworkInterface`
//! (Network module), delivered on a utility dispatch queue. A burst (a disk with several
//! partitions) becomes several hints the engine coalesces into one re-probe.

use std::ffi::{CStr, c_void};

use kelvo_collect::macos::dispatch::Queue;
use kelvo_collect::macos::iokit::{
    MatchEvent, MatchIterator, NotificationPort, drain_notification,
};
use kelvo_schema::Module;

use crate::hints::DeviceHints;
use crate::inbox::Inbox;

const WATCHED: [(&CStr, Module); 2] = [
    (c"IOMedia", Module::Disk),
    (c"IONetworkInterface", Module::Network),
];

struct HintCtx {
    inbox: Inbox,
    module: Module,
}

extern "C" fn on_match(refcon: *mut c_void, iterator: u32) {
    // SAFETY: refcon is a boxed `HintCtx` that outlives the port (see `Watch`).
    let ctx = unsafe { &*(refcon as *const HintCtx) };
    // SAFETY: IOKit passes the live notification iterator.
    if unsafe { drain_notification(iterator) } > 0 {
        ctx.inbox.hint(&[ctx.module]);
    }
}

/// Fields drop in order: the iterators (cancelling their notifications), then the port
/// (destroyed, its queue drained so no callback is left running), then the contexts.
struct Watch {
    iterators: Vec<MatchIterator>,
    port: NotificationPort,
    // Boxed on purpose: IOKit holds raw pointers to each context, so they must not move
    // when the vector grows.
    #[allow(clippy::vec_box)]
    _ctxs: Vec<Box<HintCtx>>,
}

impl Watch {
    fn start(inbox: &Inbox) -> Option<Watch> {
        let queue = Queue::utility(c"com.tryopendata.kelvo.devices")?;
        let port = NotificationPort::on_queue(queue)?;
        let mut watch = Watch {
            iterators: Vec::new(),
            port,
            _ctxs: Vec::new(),
        };
        for (class, module) in WATCHED {
            let ctx = Box::new(HintCtx {
                inbox: inbox.clone(),
                module,
            });
            let refcon: *mut c_void = (&raw const *ctx).cast_mut().cast();
            watch._ctxs.push(ctx);
            for event in [MatchEvent::FirstMatch, MatchEvent::Terminated] {
                // SAFETY: refcon is kept alive in `_ctxs`, which drops after the port.
                let armed = unsafe { watch.port.add_matching(event, class, on_match, refcon) };
                let iterator = match armed {
                    Ok(iterator) => iterator,
                    Err(None) => continue,
                    Err(Some(kr)) => {
                        tracing::warn!(class = ?class, kr, "device notification not armed");
                        continue;
                    }
                };
                // The existing devices: draining arms the notification; no hint.
                iterator.drain();
                watch.iterators.push(iterator);
            }
        }
        Some(watch)
    }
}

/// [`DeviceHints`] for disks and network interfaces from IOKit.
#[derive(Default)]
pub struct IoKitDeviceHints {
    watch: Option<Watch>,
}

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
