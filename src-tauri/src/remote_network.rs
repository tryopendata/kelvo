//! Event-driven route and sleep notifications. No endpoint polling or wake assertion.
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    NetworkChanged,
    WillSleep,
    DidWake,
}

pub type Callback = Arc<dyn Fn(Event) + Send + Sync>;

#[cfg(target_os = "macos")]
pub use macos::Monitor;

/// Start from Tauri setup (the main thread). The owner must drop the monitor before
/// shutting down connection supervisors. Network bursts are debounced for two seconds.
#[cfg(target_os = "macos")]
pub fn start(_app: &tauri::AppHandle, callback: Callback) -> anyhow::Result<Monitor> {
    macos::start(callback)
}

#[cfg(not(target_os = "macos"))]
pub struct Monitor;
#[cfg(not(target_os = "macos"))]
pub fn start(_app: &tauri::AppHandle, _callback: Callback) -> anyhow::Result<Monitor> {
    Ok(Monitor)
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use block2::RcBlock;
    use core_foundation::array::{CFArray, CFArrayRef};
    use core_foundation::base::{CFAllocatorRef, CFIndex, CFRelease, TCFType};
    use core_foundation::string::{CFString, CFStringRef};
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_app_kit::{
        NSWorkspace, NSWorkspaceDidWakeNotification, NSWorkspaceWillSleepNotification,
    };
    use objc2_foundation::NSNotification;
    use std::ffi::{c_char, c_void};
    use std::ptr::{self, NonNull};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    type Store = *mut c_void;
    type Queue = *mut c_void;
    #[repr(C)]
    struct Context {
        version: CFIndex,
        info: *mut c_void,
        retain: Option<unsafe extern "C" fn(*const c_void) -> *const c_void>,
        release: Option<unsafe extern "C" fn(*const c_void)>,
        description: Option<unsafe extern "C" fn(*const c_void) -> CFStringRef>,
    }
    #[link(name = "SystemConfiguration", kind = "framework")]
    unsafe extern "C" {
        fn SCDynamicStoreCreate(
            allocator: CFAllocatorRef,
            name: CFStringRef,
            callback: Option<extern "C" fn(Store, CFArrayRef, *mut c_void)>,
            context: *mut Context,
        ) -> Store;
        fn SCDynamicStoreSetNotificationKeys(
            store: Store,
            keys: CFArrayRef,
            patterns: CFArrayRef,
        ) -> u8;
        fn SCDynamicStoreSetDispatchQueue(store: Store, queue: Queue) -> u8;
    }
    unsafe extern "C" {
        fn dispatch_queue_create(label: *const c_char, attr: *mut c_void) -> Queue;
        fn dispatch_sync_f(
            queue: Queue,
            context: *mut c_void,
            callback: extern "C" fn(*mut c_void),
        );
        fn dispatch_release(queue: Queue);
    }

    struct State {
        callback: Callback,
        active: AtomicBool,
        revision: AtomicU64,
    }
    impl State {
        fn emit(&self, event: Event) {
            if self.active.load(Ordering::Acquire) {
                (self.callback)(event);
            }
        }
    }
    extern "C" fn changed(_store: Store, _keys: CFArrayRef, context: *mut c_void) {
        // SAFETY: context is an owned Box<Arc<State>>, freed only after the serial
        // callback queue is unscheduled and drained in Monitor::drop.
        let state = unsafe { &*context.cast::<Arc<State>>() }.clone();
        let revision = state.revision.fetch_add(1, Ordering::AcqRel) + 1;
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            if state.revision.load(Ordering::Acquire) == revision {
                state.emit(Event::NetworkChanged);
            }
        });
    }
    extern "C" fn drained(_: *mut c_void) {}

    pub struct Monitor {
        store: Store,
        queue: Queue,
        context: *mut Arc<State>,
        state: Arc<State>,
        // Each pointer owns the +1 retain returned by addObserver. NotificationCenter
        // and observer registration/removal are thread-safe; tokens are never invoked.
        observers: Vec<*mut AnyObject>,
    }
    // SAFETY: all callbacks use Send+Sync state; framework store and dispatch queue are
    // thread-safe. Destruction has exclusive ownership and drains queued callbacks.
    unsafe impl Send for Monitor {}
    // SAFETY: no shared method mutates these opaque owned handles.
    unsafe impl Sync for Monitor {}

    pub fn start(callback: Callback) -> anyhow::Result<Monitor> {
        let state = Arc::new(State {
            callback,
            active: AtomicBool::new(true),
            revision: AtomicU64::new(0),
        });
        let context = Box::into_raw(Box::new(state.clone()));
        let mut info = Context {
            version: 0,
            info: context.cast(),
            retain: None,
            release: None,
            description: None,
        };
        let name = CFString::new("Kelvo remote route notifications");
        // SAFETY: CF inputs and context stay alive throughout the registration; NULL
        // allocator is the default allocator, callback signature matches the SDK.
        let store = unsafe {
            SCDynamicStoreCreate(
                ptr::null(),
                name.as_concrete_TypeRef(),
                Some(changed),
                &mut info,
            )
        };
        // SAFETY: static NUL-terminated label; NULL attributes request a serial queue.
        let queue = unsafe {
            dispatch_queue_create(
                c"com.tryopendata.kelvo.remote-network".as_ptr(),
                ptr::null_mut(),
            )
        };
        let mut monitor = Monitor {
            store,
            queue,
            context,
            state,
            observers: Vec::new(),
        };
        anyhow::ensure!(
            !store.is_null() && !queue.is_null(),
            "creating network-change observer failed"
        );
        let patterns = CFArray::from_CFTypes(&[
            CFString::new("State:/Network/Global/.*"),
            CFString::new("State:/Network/Interface/.*/(IPv4|IPv6|Link)"),
            CFString::new("State:/Network/Service/.*/(IPv4|IPv6|DNS)"),
        ]);
        // SAFETY: all CF objects and dispatch queue are live; API retains its inputs.
        let registered = unsafe {
            SCDynamicStoreSetNotificationKeys(store, ptr::null(), patterns.as_concrete_TypeRef())
                != 0
                && SCDynamicStoreSetDispatchQueue(store, queue) != 0
        };
        anyhow::ensure!(registered, "subscribing to network changes failed");
        let center = NSWorkspace::sharedWorkspace().notificationCenter();
        // SAFETY: these are AppKit's static notification-name objects.
        for (name, event) in unsafe {
            [
                (NSWorkspaceWillSleepNotification, Event::WillSleep),
                (NSWorkspaceDidWakeNotification, Event::DidWake),
            ]
        } {
            let state = monitor.state.clone();
            let block = RcBlock::new(move |_note: NonNull<NSNotification>| state.emit(event));
            // SAFETY: the block only captures Send+Sync state and accepts every posting
            // thread; no object filter or queue with additional requirements is used.
            let token = unsafe {
                center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block)
            };
            monitor.observers.push(Retained::into_raw(token).cast());
        }
        Ok(monitor)
    }

    impl Drop for Monitor {
        fn drop(&mut self) {
            self.state.active.store(false, Ordering::Release);
            let center = NSWorkspace::sharedWorkspace().notificationCenter();
            for token in self.observers.drain(..) {
                // SAFETY: each token owns one retain, has not been freed, and remains
                // alive until removed from its notification center.
                unsafe {
                    center.removeObserver(&*token);
                    drop(Retained::<AnyObject>::from_raw(token));
                }
            }
            // SAFETY: these handles are owned. Unregister before draining the serial
            // queue; context is freed only after no callback can access it.
            unsafe {
                if !self.store.is_null() {
                    SCDynamicStoreSetDispatchQueue(self.store, ptr::null_mut());
                }
                if !self.queue.is_null() {
                    dispatch_sync_f(self.queue, ptr::null_mut(), drained);
                }
                if !self.store.is_null() {
                    CFRelease(self.store.cast());
                }
                if !self.queue.is_null() {
                    dispatch_release(self.queue);
                }
                drop(Box::from_raw(self.context));
            }
        }
    }
}
