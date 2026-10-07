//! The NetworkStatistics FFI: symbol loading, dictionary parsing and the live
//! [`Session`]. All of the module's `unsafe` is here.

use std::collections::HashMap;
use std::ffi::{CStr, c_char, c_int, c_void};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use block2::{Block, RcBlock};
use core_foundation_sys::base::{CFGetTypeID, CFTypeRef};
use core_foundation_sys::dictionary::{CFDictionaryGetValue, CFDictionaryRef};
use core_foundation_sys::number::{
    CFBooleanGetTypeID, CFBooleanGetValue, CFBooleanRef, CFNumberGetTypeID, CFNumberGetValue,
    CFNumberRef, kCFNumberSInt64Type,
};
use core_foundation_sys::string::{
    CFStringGetCString, CFStringGetTypeID, CFStringRef, kCFStringEncodingUTF8,
};
use kelvo_schema::lock::LockExt;

use super::ledger::{Counts, Ledger, Owner};
use super::{FRAMEWORK, QUERY_TIMEOUT, libproc, network};
use crate::CollectError;
use crate::calls::{self, Api as Calls};
use crate::macos::dispatch::{dispatch_queue_create, dispatch_release, dispatch_sync_f};

type Manager = *mut c_void;
type Source = *mut c_void;
type Queue = *mut c_void;

type AddedBlock = Block<dyn Fn(Source, *mut c_void)>;
/// Counts blocks receive the dictionary as a raw `CFDictionaryRef`, valid for the call.
type DictBlock = Block<dyn Fn(*const c_void)>;
type VoidBlock = Block<dyn Fn()>;

/// A dictionary key: one of the framework's exported `CFStringRef` constants.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Key(pub(super) CFStringRef);

// SAFETY: the keys are immutable CFString constants owned by a framework that is never
// unloaded; CFString reads are thread-safe.
unsafe impl Send for Key {}
// SAFETY: as above.
unsafe impl Sync for Key {}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Keys {
    pub pid: Key,
    pub rx: Key,
    pub tx: Key,
    pub loopback: Key,
    /// `kNStatSrcKeyInterface` ("interface"): the flow's interface index.
    pub interface: Key,
    /// `kNStatSrcKeyUPID` ("uniqueProcessID") and `kNStatSrcKeyProcessName`
    /// ("processName"), in counts and description dictionaries alike on macOS 27. Only
    /// identity needs them, so a release without them keeps the rates: identities are
    /// then cached by pid and have no fallback name.
    pub upid: Option<Key>,
    pub name: Option<Key>,
}

/// The framework's entry points, resolved once.
pub(super) struct Api {
    create: unsafe extern "C" fn(*const c_void, Queue, &AddedBlock) -> Manager,
    destroy: unsafe extern "C" fn(Manager),
    add_all_tcp: unsafe extern "C" fn(Manager) -> c_int,
    add_all_udp: unsafe extern "C" fn(Manager) -> c_int,
    query_all: unsafe extern "C" fn(Manager, &VoidBlock),
    query_all_descriptions: unsafe extern "C" fn(Manager, &VoidBlock),
    set_counts: unsafe extern "C" fn(Source, &DictBlock),
    set_description: unsafe extern "C" fn(Source, &DictBlock),
    set_removed: unsafe extern "C" fn(Source, &VoidBlock),
    keys: Keys,
}

/// # Safety
///
/// `T` must be a function pointer type matching the symbol's C signature, or a pointer.
unsafe fn sym<T: Copy>(handle: *mut c_void, name: &CStr) -> Option<T> {
    // SAFETY: `handle` came from dlopen; `name` is NUL-terminated.
    let p = unsafe { libc::dlsym(handle, name.as_ptr()) };
    if p.is_null() {
        return None;
    }
    debug_assert_eq!(size_of::<T>(), size_of::<*mut c_void>());
    // SAFETY: the caller names the type the symbol has; both are pointer-sized.
    Some(unsafe { std::mem::transmute_copy::<*mut c_void, T>(&p) })
}

/// # Safety
///
/// `name` must be an exported `CFStringRef` variable.
unsafe fn key(handle: *mut c_void, name: &CStr) -> Option<Key> {
    // SAFETY: per the caller, the symbol is a `CFStringRef` variable.
    let p: *const CFStringRef = unsafe { sym(handle, name)? };
    // SAFETY: a non-null symbol address points at the variable, which the framework
    // initialised at load.
    let s = unsafe { *p };
    (!s.is_null()).then_some(Key(s))
}

fn load() -> Option<Api> {
    // SAFETY: a NUL-terminated path. The handle is never closed, so every pointer read
    // from it stays valid for the life of the process.
    let h = unsafe { libc::dlopen(FRAMEWORK.as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL) };
    if h.is_null() {
        return None;
    }
    // SAFETY: each type matches the signature verified in the D-081 spike: the functions
    // take the manager or source as an opaque pointer and Objective-C blocks by
    // reference; the keys are `CFStringRef` variables.
    unsafe {
        Some(Api {
            create: sym(h, c"NStatManagerCreate")?,
            destroy: sym(h, c"NStatManagerDestroy")?,
            add_all_tcp: sym(h, c"NStatManagerAddAllTCP")?,
            add_all_udp: sym(h, c"NStatManagerAddAllUDP")?,
            query_all: sym(h, c"NStatManagerQueryAllSources")?,
            query_all_descriptions: sym(h, c"NStatManagerQueryAllSourcesDescriptions")?,
            set_counts: sym(h, c"NStatSourceSetCountsBlock")?,
            set_description: sym(h, c"NStatSourceSetDescriptionBlock")?,
            set_removed: sym(h, c"NStatSourceSetRemovedBlock")?,
            keys: Keys {
                pid: key(h, c"kNStatSrcKeyPID")?,
                rx: key(h, c"kNStatSrcKeyRxBytes")?,
                tx: key(h, c"kNStatSrcKeyTxBytes")?,
                loopback: key(h, c"kNStatSrcKeyInterfaceTypeLoopback")?,
                interface: key(h, c"kNStatSrcKeyInterface")?,
                upid: key(h, c"kNStatSrcKeyUPID"),
                name: key(h, c"kNStatSrcKeyProcessName"),
            },
        })
    }
}

/// The framework, loaded on first use; `None` if anything is missing.
pub(super) fn api() -> Option<&'static Api> {
    static API: OnceLock<Option<Api>> = OnceLock::new();
    API.get_or_init(load).as_ref()
}

/// The value for `key` in `dict`, unretained.
///
/// # Safety
///
/// `dict` is a valid CFDictionary for the duration of the call.
unsafe fn value(dict: CFDictionaryRef, key: Key) -> Option<CFTypeRef> {
    // SAFETY: valid dictionary per the caller; the key is a valid CFString.
    let v = unsafe { CFDictionaryGetValue(dict, key.0.cast()) };
    (!v.is_null()).then_some(v)
}

/// # Safety
///
/// As [`value`].
unsafe fn int(dict: CFDictionaryRef, key: Key) -> Option<i64> {
    // SAFETY: per the caller.
    let v = unsafe { value(dict, key)? };
    // SAFETY: `v` is a valid CF object borrowed from the dictionary.
    if unsafe { CFGetTypeID(v) } != unsafe { CFNumberGetTypeID() } {
        return None;
    }
    let mut out: i64 = 0;
    // SAFETY: `v` is a CFNumber; `out` is an i64 for the SInt64 conversion. A lossy
    // conversion still writes the value, which is fine for byte counters.
    unsafe { CFNumberGetValue(v as CFNumberRef, kCFNumberSInt64Type, (&raw mut out).cast()) };
    Some(out)
}

/// # Safety
///
/// As [`value`].
unsafe fn boolean(dict: CFDictionaryRef, key: Key) -> Option<bool> {
    // SAFETY: per the caller.
    let v = unsafe { value(dict, key)? };
    // SAFETY: `v` is a valid CF object borrowed from the dictionary.
    if unsafe { CFGetTypeID(v) } != unsafe { CFBooleanGetTypeID() } {
        return None;
    }
    // SAFETY: `v` is a CFBoolean.
    Some(unsafe { CFBooleanGetValue(v as CFBooleanRef) })
}

/// A string value, copied; `None` when missing, not a string or not UTF-8 in 256
/// bytes (process names are at most 32).
///
/// # Safety
///
/// As [`value`].
unsafe fn string(dict: CFDictionaryRef, key: Key) -> Option<String> {
    // SAFETY: per the caller.
    let v = unsafe { value(dict, key)? };
    // SAFETY: `v` is a valid CF object borrowed from the dictionary.
    if unsafe { CFGetTypeID(v) } != unsafe { CFStringGetTypeID() } {
        return None;
    }
    let mut buf = [0 as c_char; 256];
    // SAFETY: `v` is a CFString; `buf` has `buf.len()` writable bytes.
    let ok = unsafe {
        CFStringGetCString(
            v as CFStringRef,
            buf.as_mut_ptr(),
            buf.len() as _,
            kCFStringEncodingUTF8,
        )
    };
    if ok == 0 {
        return None;
    }
    // SAFETY: on success the buffer holds a NUL-terminated string.
    let s = unsafe { CStr::from_ptr(buf.as_ptr()) };
    s.to_str().ok().map(str::to_owned)
}

/// The owner's unique id, `0` when the dictionary has none.
///
/// # Safety
///
/// As [`value`].
unsafe fn upid(dict: CFDictionaryRef, keys: &Keys) -> u64 {
    // SAFETY: per the caller.
    keys.upid
        .and_then(|k| unsafe { int(dict, k) })
        .and_then(|v| u64::try_from(v).ok())
        .unwrap_or(0)
}

/// The app identity NetworkStatistics recorded for the flow (`processName`, through the
/// same aliases as [`libproc::identity_rule`]): the fallback when the process is gone
/// before its identity was resolved.
///
/// # Safety
///
/// `dict` is null or a valid CFDictionary for the duration of the call.
pub(crate) unsafe fn recorded_identity(dict: CFDictionaryRef, keys: &Keys) -> Option<String> {
    if dict.is_null() {
        return None;
    }
    // SAFETY: non-null and valid per the caller.
    let name = keys.name.and_then(|k| unsafe { string(dict, k) })?;
    libproc::identity_rule("", "", &name, None)
}

/// Reads a counts dictionary. `None` when the pid or a byte counter is missing or not a
/// number. The flow is counted when it is not flagged loopback (the framework includes
/// the interface-type flags that are true) and `reported` accepts its interface index; a
/// missing or zero index is not counted.
///
/// # Safety
///
/// `dict` is null or a valid CFDictionary for the duration of the call.
pub(crate) unsafe fn parse_counts(
    dict: CFDictionaryRef,
    keys: &Keys,
    reported: impl FnOnce(u32) -> bool,
) -> Option<Counts> {
    if dict.is_null() {
        return None;
    }
    // SAFETY: non-null and valid per the caller.
    unsafe {
        let pid = i32::try_from(int(dict, keys.pid)?).ok()?;
        let rx = u64::try_from(int(dict, keys.rx)?).ok()?;
        let tx = u64::try_from(int(dict, keys.tx)?).ok()?;
        let loopback = boolean(dict, keys.loopback).unwrap_or(false);
        let iface = int(dict, keys.interface)
            .and_then(|i| u32::try_from(i).ok())
            .filter(|&i| i > 0);
        Some(Counts {
            pid,
            upid: upid(dict, keys),
            rx,
            tx,
            counted: !loopback && iface.is_some_and(reported),
        })
    }
}

/// The owning pid and unique id from a description dictionary, when it names a pid.
///
/// # Safety
///
/// As [`parse_counts`].
pub(crate) unsafe fn parse_owner(dict: CFDictionaryRef, keys: &Keys) -> Option<Owner> {
    if dict.is_null() {
        return None;
    }
    // SAFETY: non-null and valid per the caller.
    let pid = i32::try_from(unsafe { int(dict, keys.pid)? }).ok()?;
    // SAFETY: as above.
    (pid > 0).then(|| Owner {
        pid,
        upid: unsafe { upid(dict, keys) },
    })
}

/// Resolves a new owner's identity and records it: from the live process when it is
/// still the one that owned the flow, else from what the dictionary recorded. Runs in
/// a framework callback, which arrives about 1.5 to 3 s after a flow opens (and once
/// more as it closes), so the process is usually still alive. Allocates, once per
/// process: the settle path only clones the interned name.
///
/// # Safety
///
/// `dict` is null or a valid CFDictionary for the duration of the call.
unsafe fn identify(
    ledger: &Mutex<Ledger>,
    src: usize,
    owner: Owner,
    dict: CFDictionaryRef,
    keys: &Keys,
) {
    let name = libproc::app_identity(owner.pid, owner.upid)
        // SAFETY: per the caller.
        .or_else(|| unsafe { recorded_identity(dict, keys) });
    ledger
        .lock_ok()
        .identify(src, owner, name.as_deref(), Instant::now());
}

/// Which interface indexes are reported ([`network::is_reported_interface`]), looked up
/// once per index per manager: counts callbacks arrive for every flow every sample, and
/// the lookup copies SystemConfiguration's interface list. A manager lives only while a
/// view shows network rates, so an index the system reuses for a new interface is looked
/// up again by the next one.
struct Reported(HashMap<u32, bool>);

impl Reported {
    fn with_room() -> Self {
        Self(HashMap::with_capacity(64))
    }

    fn get(&mut self, index: u32) -> bool {
        *self
            .0
            .entry(index)
            .or_insert_with(|| network::is_reported_interface(index))
    }
}

/// Completions of the query block, counted. A wait takes the count when its query is
/// issued and returns at the next completion after it, so a completion that never came
/// (or two the framework coalesced) cannot leave every later wait short of a target.
#[derive(Default)]
pub(super) struct Done {
    n: Mutex<u64>,
    cv: Condvar,
}

impl Done {
    /// Called by the completion block.
    pub(super) fn complete(&self) {
        *self.n.lock_ok() += 1;
        self.cv.notify_all();
    }

    /// The count to wait past; take it before issuing the query.
    pub(super) fn mark(&self) -> u64 {
        *self.n.lock_ok()
    }

    /// Waits up to `timeout` for a completion after `mark`. `false` on timeout.
    pub(super) fn wait_past(&self, mark: u64, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut n = self.n.lock_ok();
        while *n <= mark {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            n = self
                .cv
                .wait_timeout(n, left)
                .map_or_else(|e| e.into_inner().0, |(g, _)| g);
        }
        true
    }
}

/// What [`destroy_on_queue`] needs, on the dropping thread's stack.
struct Doomed {
    destroy: unsafe extern "C" fn(Manager),
    manager: Manager,
}

extern "C" fn destroy_on_queue(ctx: *mut c_void) {
    // SAFETY: `ctx` is the `Doomed` that `Session::drop` passes to `dispatch_sync_f`,
    // which returns only after this function has, so the borrow is live throughout.
    let d = unsafe { &*ctx.cast::<Doomed>() };
    // SAFETY: a live manager, destroyed once (see `Session::drop`).
    unsafe { (d.destroy)(d.manager) };
}

/// A live manager: created on the first sample after the interest begins, destroyed on
/// release.
pub(super) struct Session {
    api: &'static Api,
    manager: Manager,
    queue: Queue,
    pub(super) ledger: Arc<Mutex<Ledger>>,
    done: Arc<Done>,
    /// The query completion block, built once and reused for every query.
    on_done: RcBlock<dyn Fn()>,
    /// Kept for the manager's lifetime.
    _on_added: RcBlock<dyn Fn(Source, *mut c_void)>,
}

// SAFETY: the manager and queue are only used from the engine thread that owns the
// collector (the framework calls back on `queue`, touching only the Arc'd state, which is
// Mutex-protected). The blocks are reference counted with atomic Block_copy/Block_release,
// and their captures (Arc, Key, &'static Api) are Send + Sync.
unsafe impl Send for Session {}

impl Session {
    pub(super) fn start(api: &'static Api) -> Result<Session, CollectError> {
        let ledger = Arc::new(Mutex::new(Ledger::with_room()));
        let done = Arc::new(Done::default());
        let on_done = {
            let done = Arc::clone(&done);
            RcBlock::new(move || done.complete())
        };
        let reported = Arc::new(Mutex::new(Reported::with_room()));
        let on_added = {
            let ledger = Arc::clone(&ledger);
            let keys = api.keys;
            RcBlock::new(move |src: Source, _ctx: *mut c_void| {
                let id = src as usize;
                ledger.lock_ok().added(id);
                let counts = {
                    let ledger = Arc::clone(&ledger);
                    let reported = Arc::clone(&reported);
                    RcBlock::new(move |dict: *const c_void| {
                        let dict: CFDictionaryRef = dict.cast();
                        let is_reported = |i| reported.lock_ok().get(i);
                        // SAFETY: the framework passes a CFDictionary valid for the call.
                        let Some(c) = (unsafe { parse_counts(dict, &keys, is_reported) }) else {
                            return;
                        };
                        let need = ledger.lock_ok().counts(id, c);
                        if let Some(owner) = need {
                            // SAFETY: as above.
                            unsafe { identify(&ledger, id, owner, dict, &keys) };
                        }
                    })
                };
                let description = {
                    let ledger = Arc::clone(&ledger);
                    RcBlock::new(move |dict: *const c_void| {
                        let dict: CFDictionaryRef = dict.cast();
                        // SAFETY: the framework passes a CFDictionary valid for the call.
                        let Some(o) = (unsafe { parse_owner(dict, &keys) }) else {
                            return;
                        };
                        let need = ledger.lock_ok().described(id, o.pid, o.upid);
                        if let Some(owner) = need {
                            // SAFETY: as above.
                            unsafe { identify(&ledger, id, owner, dict, &keys) };
                        }
                    })
                };
                let removed = {
                    let ledger = Arc::clone(&ledger);
                    RcBlock::new(move || ledger.lock_ok().removed(id))
                };
                // SAFETY: `src` is the live source this callback announces; the framework
                // copies the blocks it keeps.
                unsafe {
                    (api.set_counts)(src, &counts);
                    (api.set_description)(src, &description);
                    (api.set_removed)(src, &removed);
                }
            })
        };
        // SAFETY: a static NUL-terminated label; a null attribute is a serial queue.
        let queue = unsafe { dispatch_queue_create(c"kelvo.nstat".as_ptr(), std::ptr::null_mut()) };
        if queue.is_null() {
            return Err(CollectError::Os {
                call: "dispatch_queue_create",
                code: 0,
            });
        }
        calls::count(Calls::NetStat);
        // SAFETY: default allocator (null), a valid queue and a heap block.
        let manager = unsafe { (api.create)(std::ptr::null(), queue, &on_added) };
        if manager.is_null() {
            // SAFETY: we own the one reference from dispatch_queue_create.
            unsafe { dispatch_release(queue) };
            return Err(CollectError::Os {
                call: "NStatManagerCreate",
                code: 0,
            });
        }
        let session = Session {
            api,
            manager,
            queue,
            ledger,
            done,
            on_done,
            _on_added: on_added,
        };
        calls::count(Calls::NetStat);
        calls::count(Calls::NetStat);
        // SAFETY: a live manager.
        let (tcp, udp) = unsafe { ((api.add_all_tcp)(manager), (api.add_all_udp)(manager)) };
        if tcp != 1 && udp != 1 {
            // Dropping the session destroys the manager.
            return Err(CollectError::Os {
                call: "NStatManagerAddAll",
                code: i64::from(tcp),
            });
        }
        Ok(session)
    }

    /// Queries every source's counts (or, with `describe`, its description) and waits
    /// for the completion.
    pub(super) fn query(&self, describe: bool) -> Result<(), CollectError> {
        calls::count(Calls::NetStat);
        let (f, call) = if describe {
            (
                self.api.query_all_descriptions,
                "NStatManagerQueryAllSourcesDescriptions",
            )
        } else {
            (self.api.query_all, "NStatManagerQueryAllSources")
        };
        let mark = self.done.mark();
        // SAFETY: a live manager and a heap block the framework copies.
        unsafe { f(self.manager, &self.on_done) };
        if self.done.wait_past(mark, QUERY_TIMEOUT) {
            Ok(())
        } else {
            Err(CollectError::Timeout { call })
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        calls::count(Calls::NetStat);
        let doomed = Doomed {
            destroy: self.api.destroy,
            manager: self.manager,
        };
        // SAFETY: ordering. The framework runs every callback (added, counts,
        // description, removed, query completion) on `queue`, which is serial. Running
        // the destroy as a block on that queue means the callbacks already queued run
        // first, and none runs while it does, so an added block never calls
        // `NStatSourceSet*Block` on a source of a destroyed manager. `dispatch_sync_f`
        // returns after the destroy, so `doomed` outlives its use. This thread is never
        // on `queue` (only framework callbacks are), so the sync cannot deadlock on
        // itself, and the callbacks take only the ledger and completion mutexes, which
        // nothing holds while dropping. The manager is live and destroyed once.
        unsafe {
            dispatch_sync_f(
                self.queue,
                (&raw const doomed).cast_mut().cast(),
                destroy_on_queue,
            );
        }
        // SAFETY: after the destroy; we own the one reference from
        // dispatch_queue_create, and libdispatch keeps the queue until anything still
        // pending on it has run.
        unsafe { dispatch_release(self.queue) };
    }
}
