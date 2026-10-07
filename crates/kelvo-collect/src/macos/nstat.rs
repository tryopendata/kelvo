//! Per-process network rates from the private NetworkStatistics framework (D-081).
//!
//! `NStatManager` reports byte counts per flow (TCP and UDP sources) with the owning
//! pid. The framework is only in the dyld shared cache, so it is opened with `dlopen` and
//! every function and dictionary key is looked up with `dlsym` once ([`api`]); a missing
//! framework, symbol or key makes the collector probe `Unsupported`, and the UI hides the
//! network columns (`Capabilities::process_network`). Nothing links against it (D-058:
//! the binary's load commands stay system-only; this `dlopen` of a `/System` framework is
//! the by-hand review that decision asks for).
//!
//! Verified on macOS 27 (Darwin 27.0.0, Apple Silicon), unprivileged and unsandboxed: it
//! needs no entitlement and sees only flows owned by the current user. Root and system
//! daemons (mDNSResponder, softwareupdated, VPN tunnels) are not attributed; seeing them
//! needs `com.apple.private.network.statistics`, which an ad-hoc signed app cannot hold.
//! Key names (`processID`, `rxBytes`, `txBytes`, `ifLoopback`, `interface`,
//! `uniqueProcessID`, `processName`) were read from the exported constants on that release only;
//! [`parse_counts`] is covered by fixture dictionaries, the live read by ignored tests.
//!
//! Lifecycle: the manager exists only while a visible view asks for network rates
//! (`Cadence::OnDemand(Interest::NetworkProcesses)`). The first sample creates it (a
//! serial dispatch queue, all TCP and UDP sources) and sets the baseline; the engine calls
//! [`Collector::release`] when the last such view hides, which destroys it, so nothing is
//! held or called otherwise.
//!
//! Flows that were open before the manager was created report `processID` 0 in their
//! counts until a description query names them (measured on macOS 27: 127 of 130 sources
//! without one), so the first sample also runs `NStatManagerQueryAllSourcesDescriptions`,
//! and later samples repeat it, at most every [`DESCRIBE_EVERY_MS`], while some flow's
//! owner is unknown. Their bytes never go to pid 0. When a description names the owner
//! of a flow opened after the baseline, what it moved meanwhile goes to the owner's byte
//! totals as `late` bytes ([`ProcessNet::late_rx_bytes`], history only, D-089), never
//! to `rx_bps`/`tx_bps`, so the wait does not show as a one-sample spike (D-082). A flow
//! that was open before the baseline drops its wait: those bytes may predate it.
//!
//! Per sample: `NStatManagerQueryAllSources`, a wait for its completion block, then the
//! [`Ledger`] turns cumulative per-flow counts into per-pid bytes since the last sample.
//! A query with no completion within [`QUERY_TIMEOUT`] fails the sample with
//! [`CollectError::Timeout`] and drops the manager, so the next sample starts a fresh one
//! (and a new baseline). After [`FAIL_LIMIT`] failures in a row (timeouts or a manager
//! that will not start) the collector backs off for [`BACKOFF`]: samples in between
//! report nothing, so rows stay "not measured", and the engine thread does not wait on
//! a stuck framework every process tick.
//! A flow that closed in between gets one last counts callback before its removed block,
//! so its final bytes are folded into the pid it belonged to.
//!
//! Only flows on an interface the network collector reports (Wi-Fi, Ethernet, cellular:
//! [`network::is_reported_interface`]) are counted, so the per-app bytes split the same
//! traffic as the interface totals (D-089). Loopback, VPN tunnels, bridges and AWDL are
//! skipped, judged by the flow's `interface` index: the interface-type flags describe the
//! underlying link, so a Tailscale flow on `utun4` reports `ifWiFi` (measured on macOS 27),
//! and one end of a 127.0.0.1 connection was seen without `ifLoopback`. Counting those
//! made apps exceed the interface, and the Apps table's shares pass 100%. A flow with no
//! interface index (about 0.05% of bytes on the dev Mac) is skipped too.
//!
//! Each entry carries bytes as well as rates, and the app identity the bytes belong to
//! (D-089), for network history. The identity is resolved in the callback that first
//! names a flow's owner ([`libproc::app_identity`]): a new flow's first unsolicited
//! counts arrive about 1.5 to 3 s after it opens, usually while the process is alive.
//! It is cached by `uniqueProcessID` (`kNStatSrcKeyUPID`, which, unlike the pid, is
//! never reused), and falls back to the dictionary's `processName`
//! (`kNStatSrcKeyProcessName`, present in every observed callback) when the process
//! is already gone. Names are interned per manager, so the per-sample path clones
//! reference counts and allocates nothing; closed flows keep their pid and identity
//! until the next settle, so a process that exits between samples keeps its name.

use std::collections::{HashMap, HashSet};
use std::ffi::{CStr, c_char, c_int, c_void};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError};
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
use kelvo_schema::{Entitlement, Module, UnsupportedReason};

use super::{libproc, network};
use crate::calls::{self, Api as Calls};
use crate::{
    Cadence, CollectError, Collector, CollectorId, Every, Interest, Interval, Probe, ProcessNet,
    SampleBuf, Tick,
};

pub const ID: CollectorId = CollectorId("net_per_process");

/// How long a sample waits for the query's completion block. A query took 0.6 to 2.4 ms
/// in the D-081 spike.
const QUERY_TIMEOUT: Duration = Duration::from_millis(250);

/// How often flows with an unknown owner are described again.
const DESCRIBE_EVERY_MS: u32 = 10_000;

/// Consecutive failures (a query timeout or a manager that would not start) before the
/// collector stops trying for [`BACKOFF`].
const FAIL_LIMIT: u32 = 3;

/// How long the collector waits before reopening a manager that kept failing.
const BACKOFF: Duration = Duration::from_secs(60);

const FRAMEWORK: &CStr =
    c"/System/Library/PrivateFrameworks/NetworkStatistics.framework/NetworkStatistics";

// ---- accounting (no FFI) ----------------------------------------------------------------

/// What one counts callback says about a flow.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Counts {
    /// `0` when the framework has not described the flow yet: a flow that existed
    /// before the manager reports its owner only after a description query.
    pub pid: i32,
    /// The owner's `uniqueProcessID` (never reused within a boot); `0` when unknown.
    pub upid: u64,
    /// Cumulative bytes received over the flow's life.
    pub rx: u64,
    pub tx: u64,
    /// The flow is on an interface whose bytes are in the interface totals.
    pub counted: bool,
}

/// An owner whose app identity is not known yet: the callback that learned it resolves
/// one ([`Ledger::identify`]) while the dictionary and, usually, the process are alive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Owner {
    pub pid: i32,
    pub upid: u64,
}

impl Owner {
    /// The identity cache key: the unique id, or the pid (tagged so it cannot collide
    /// with a unique id) on a release whose dictionaries lack one.
    fn key(self) -> u64 {
        if self.upid != 0 {
            self.upid
        } else {
            (1 << 63) | u64::from(self.pid.unsigned_abs())
        }
    }
}

/// An interned identity's address, so maps key on it without hashing the string: one
/// ledger interns each name once, so equal names share one allocation. `0` for none.
fn ident_addr(ident: &Option<Arc<str>>) -> usize {
    ident
        .as_ref()
        .map_or(0, |a| Arc::as_ptr(a).cast::<u8>() as usize)
}

#[derive(Debug)]
struct Flow {
    /// Owner, from a counts or description dictionary; `0` until one names it.
    pid: i32,
    /// The owner's unique id, when a dictionary gave one.
    upid: u64,
    /// The owner's app identity, once looked for.
    ident: Option<Arc<str>>,
    /// An identity was looked for (found or not), so later callbacks do not look again.
    identified: bool,
    /// Latest cumulative `(rx, tx)`.
    bytes: Option<(u64, u64)>,
    /// As [`Counts::counted`], from the latest counts.
    counted: bool,
    /// The `(rx, tx)` already accounted for, `None` before the first settle that saw it.
    base: Option<(u64, u64)>,
    /// Added after the baseline: the flow began after a settle that accounted for
    /// everything, so its whole count is new. A flow from before the baseline that
    /// reports late starts from its first count instead.
    fresh: bool,
    /// `(rx, tx)` a fresh flow moved before its owner was known, owed to the owner's
    /// byte totals at the next settle but not to its rate (see [`Flow::own`]).
    late: (u64, u64),
}

impl Flow {
    fn new(fresh: bool) -> Self {
        Flow {
            pid: 0,
            upid: 0,
            ident: None,
            identified: false,
            bytes: None,
            counted: false,
            base: None,
            fresh,
            late: (0, 0),
        }
    }

    /// The pid and the bytes not yet accounted for, with the identity, marking them
    /// accounted. `None` before the first counts and for flows not counted. A flow whose
    /// owner is still unknown keeps its bytes unaccounted until a description names it.
    fn take(&mut self) -> Option<(i32, Settled)> {
        let (rx, tx) = self.bytes?;
        if self.pid <= 0 {
            return None;
        }
        let (rx0, tx0) = self
            .base
            .unwrap_or(if self.fresh { (0, 0) } else { (rx, tx) });
        self.base = Some((rx, tx));
        let (late_rx, late_tx) = std::mem::take(&mut self.late);
        self.counted.then(|| {
            (
                self.pid,
                Settled {
                    ident: self.ident.clone(),
                    rx: rx.saturating_sub(rx0),
                    tx: tx.saturating_sub(tx0),
                    late_rx,
                    late_tx,
                },
            )
        })
    }

    fn unresolved(&self) -> bool {
        self.pid <= 0 && self.bytes.is_some()
    }

    /// Records the owner. When a flow that waited with bytes and no owner is attributed
    /// after the baseline, its rate starts from the bytes seen so far: charging the
    /// whole wait (up to [`DESCRIBE_EVERY_MS`]) to one sample would show a spike many
    /// times the real rate (D-082). A fresh flow's wait still goes to the owner's byte
    /// totals, as `late` (D-089: history keeps it); a flow from before the baseline
    /// drops it, since those bytes may predate the baseline. A different owner than
    /// before needs its identity looked up.
    fn own(&mut self, pid: i32, upid: u64, baselined: bool) {
        if pid <= 0 {
            return;
        }
        if baselined && self.unresolved() {
            if self.fresh
                && let Some((rx, tx)) = self.bytes
            {
                let (rx0, tx0) = self.base.unwrap_or((0, 0));
                self.late = (
                    self.late.0.saturating_add(rx.saturating_sub(rx0)),
                    self.late.1.saturating_add(tx.saturating_sub(tx0)),
                );
            }
            self.base = self.bytes;
        }
        if pid != self.pid || (upid != 0 && upid != self.upid) {
            self.ident = None;
            self.identified = false;
        }
        self.pid = pid;
        if upid != 0 {
            self.upid = upid;
        }
    }

    fn owner(&self) -> Owner {
        Owner {
            pid: self.pid,
            upid: self.upid,
        }
    }
}

/// The app identities a ledger knows: one interned copy of each name and the name of
/// each process seen, by unique id. Names arrive from the framework's callbacks; the
/// settle path only clones the `Arc`s, so it never allocates a string.
#[derive(Debug, Default)]
struct Names {
    by_owner: HashMap<u64, Arc<str>>,
    interned: HashSet<Arc<str>>,
    /// `by_owner` is pruned once it grows past this.
    prune_at: usize,
    /// Start of the current hour of [`NEW_NAMES_PER_HOUR`].
    window: Option<Instant>,
    new_in_window: u32,
}

/// Distinct new names a ledger takes per hour; past it, new processes go unnamed
/// ("other apps"), so processes that rewrite their argv cannot grow the name tables
/// (and the store's interned names) without bound.
const NEW_NAMES_PER_HOUR: u32 = 128;
const HOUR: Duration = Duration::from_secs(3_600);

impl Names {
    fn with_room() -> Self {
        Self {
            by_owner: HashMap::with_capacity(PID_ROOM),
            interned: HashSet::with_capacity(PID_ROOM),
            prune_at: PID_ROOM,
            ..Self::default()
        }
    }

    fn intern(&mut self, name: &str, now: Instant) -> Option<Arc<str>> {
        if let Some(a) = self.interned.get(name) {
            return Some(Arc::clone(a));
        }
        if self
            .window
            .is_none_or(|w| now.saturating_duration_since(w) >= HOUR)
        {
            self.window = Some(now);
            self.new_in_window = 0;
        }
        if self.new_in_window >= NEW_NAMES_PER_HOUR {
            return None;
        }
        self.new_in_window += 1;
        let a: Arc<str> = Arc::from(name);
        self.interned.insert(Arc::clone(&a));
        Some(a)
    }
}

/// Per-flow counts as the callbacks deliver them, and the bytes of closed flows not yet
/// handed out. Flows are keyed by the source pointer, which the framework reuses after a
/// removal, so a removal drops the entry before a new source can take the key.
#[derive(Debug, Default)]
pub(crate) struct Ledger {
    flows: HashMap<usize, Flow>,
    /// Bytes of flows removed since the last settle, per pid and identity (by
    /// [`ident_addr`]): a process that exits keeps its name for its last bytes.
    closed: HashMap<(i32, usize), Settled>,
    names: Names,
    baselined: bool,
}

/// One pid's and identity's bytes since the last settle.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Settled {
    pub ident: Option<Arc<str>>,
    /// Bytes moved over the settle interval: the rate.
    pub rx: u64,
    pub tx: u64,
    /// Bytes fresh flows moved earlier, before their owner was known: in the byte
    /// totals, not the rate.
    pub late_rx: u64,
    pub late_tx: u64,
}

/// What [`Ledger::settle`] fills: [`Settled`] by pid and identity address.
pub(crate) type SettleMap = HashMap<(i32, usize), Settled>;

fn add(map: &mut SettleMap, pid: i32, s: Settled) {
    let e = map
        .entry((pid, ident_addr(&s.ident)))
        .or_insert_with(|| Settled {
            ident: s.ident.clone(),
            ..Settled::default()
        });
    e.rx = e.rx.saturating_add(s.rx);
    e.tx = e.tx.saturating_add(s.tx);
    e.late_rx = e.late_rx.saturating_add(s.late_rx);
    e.late_tx = e.late_tx.saturating_add(s.late_tx);
}

/// Room for this many flows and pids before a map grows. Flows come and go every
/// second (about 130 open on the dev Mac), so a map sized to the first sample would
/// regrow, and allocate, whenever churn crossed its capacity.
const FLOW_ROOM: usize = 1024;
const PID_ROOM: usize = 256;

impl Ledger {
    /// A ledger with room for [`FLOW_ROOM`] flows, allocated once when the manager opens.
    fn with_room() -> Self {
        Self {
            flows: HashMap::with_capacity(FLOW_ROOM),
            closed: HashMap::with_capacity(PID_ROOM),
            names: Names::with_room(),
            baselined: false,
        }
    }

    pub(crate) fn added(&mut self, src: usize) {
        self.flows.insert(src, Flow::new(self.baselined));
    }

    fn flow(&mut self, src: usize) -> &mut Flow {
        let fresh = self.baselined;
        self.flows.entry(src).or_insert_with(|| Flow::new(fresh))
    }

    /// The flow's owner when it still needs an identity, after taking a cached one.
    fn needs_identity(&mut self, src: usize) -> Option<Owner> {
        let f = self.flows.get_mut(&src)?;
        if f.pid <= 0 || f.identified {
            return None;
        }
        if let Some(a) = self.names.by_owner.get(&f.owner().key()) {
            f.ident = Some(Arc::clone(a));
            f.identified = true;
            return None;
        }
        Some(f.owner())
    }

    /// A description named the flow's owner. Returns the owner when its identity is
    /// not known yet: resolve it, then call [`Ledger::identify`].
    pub(crate) fn described(&mut self, src: usize, pid: i32, upid: u64) -> Option<Owner> {
        let baselined = self.baselined;
        self.flow(src).own(pid, upid, baselined);
        self.needs_identity(src)
    }

    /// New counts for a flow. Returns the owner when its identity is not known yet, as
    /// [`Ledger::described`].
    pub(crate) fn counts(&mut self, src: usize, c: Counts) -> Option<Owner> {
        let baselined = self.baselined;
        let f = self.flow(src);
        // Before the new bytes: what moved since the last counts is still this owner's.
        f.own(c.pid, c.upid, baselined);
        f.bytes = Some((c.rx, c.tx));
        f.counted = c.counted;
        self.needs_identity(src)
    }

    /// Records `owner`'s identity (`None`: nothing named it) for the flow and for the
    /// owner's later flows. `now` paces the cap on new names.
    pub(crate) fn identify(&mut self, src: usize, owner: Owner, name: Option<&str>, now: Instant) {
        let ident = name.and_then(|n| self.names.intern(n, now));
        if let Some(a) = &ident {
            self.names.by_owner.insert(owner.key(), Arc::clone(a));
        }
        if let Some(f) = self.flows.get_mut(&src)
            && f.owner() == owner
        {
            f.ident = ident;
            f.identified = true;
        }
    }

    pub(crate) fn removed(&mut self, src: usize) {
        let Some(mut flow) = self.flows.remove(&src) else {
            return;
        };
        if !self.baselined {
            return;
        }
        if let Some((pid, s)) = flow.take() {
            add(&mut self.closed, pid, s);
        }
    }

    /// Flows with bytes but no known owner: a description query would name them.
    pub(crate) fn unresolved(&self) -> usize {
        self.flows.values().filter(|f| f.unresolved()).count()
    }

    /// Fills `out` with the bytes each pid and identity moved since the last settle,
    /// closed flows included, and marks them accounted. The first call sets the
    /// baseline: it returns `false` and `out` stays empty.
    pub(crate) fn settle(&mut self, out: &mut SettleMap) -> bool {
        out.clear();
        self.prune_names();
        if !self.baselined {
            for f in self.flows.values_mut() {
                // History, owner known or not: never a rate.
                if f.base.is_none() {
                    f.base = f.bytes;
                }
            }
            self.closed.clear();
            self.baselined = true;
            return false;
        }
        for f in self.flows.values_mut() {
            if let Some((pid, s)) = f.take() {
                add(out, pid, s);
            }
        }
        for ((pid, _), s) in self.closed.drain() {
            add(out, pid, s);
        }
        true
    }

    /// Forgets the names of processes no open flow belongs to, once there are more
    /// than [`Names::prune_at`], and then names nothing refers to. Without allocating:
    /// a scan of the open flows per cached process, which runs rarely because the
    /// threshold doubles past what is still in use.
    fn prune_names(&mut self) {
        if self.names.by_owner.len() <= self.names.prune_at {
            return;
        }
        let flows = &self.flows;
        self.names
            .by_owner
            .retain(|&k, _| flows.values().any(|f| f.pid > 0 && f.owner().key() == k));
        // A name only the intern table holds is used by no flow, closed entry or row.
        self.names.interned.retain(|a| Arc::strong_count(a) > 1);
        self.names.prune_at = PID_ROOM.max(self.names.by_owner.len() * 2);
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.flows.len()
    }
}

/// Stops reopening a manager that keeps failing: after [`FAIL_LIMIT`] failures in a row,
/// nothing is tried until [`BACKOFF`] has passed on the continuous clock. The count is
/// reset only by a sample that worked, so after a backoff one more failure starts the
/// next one: a framework that stays stuck costs one attempt per backoff.
#[derive(Debug, Default)]
struct Breaker {
    failures: u32,
    until_ns: Option<u64>,
}

impl Breaker {
    /// Whether a manager may be opened at `now_ns`.
    fn allows(&self, now_ns: u64) -> bool {
        self.until_ns.is_none_or(|until| now_ns >= until)
    }

    fn failed(&mut self, now_ns: u64) {
        self.failures = self.failures.saturating_add(1);
        if self.failures >= FAIL_LIMIT {
            let backoff = u64::try_from(BACKOFF.as_nanos()).unwrap_or(u64::MAX);
            self.until_ns = Some(now_ns.saturating_add(backoff));
        }
    }

    fn succeeded(&mut self) {
        self.failures = 0;
        self.until_ns = None;
    }
}

fn relock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // The ledger holds plain counters updated in single steps; a panic elsewhere while
    // holding it leaves it usable.
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

// ---- FFI ------------------------------------------------------------------------------

type Manager = *mut c_void;
type Source = *mut c_void;
type Queue = *mut c_void;

type AddedBlock = Block<dyn Fn(Source, *mut c_void)>;
/// Counts blocks receive the dictionary as a raw `CFDictionaryRef`, valid for the call.
type DictBlock = Block<dyn Fn(*const c_void)>;
type VoidBlock = Block<dyn Fn()>;

unsafe extern "C" {
    fn dispatch_queue_create(label: *const c_char, attr: *mut c_void) -> Queue;
    fn dispatch_sync_f(queue: Queue, context: *mut c_void, work: extern "C" fn(*mut c_void));
    fn dispatch_release(object: *mut c_void);
}

/// A dictionary key: one of the framework's exported `CFStringRef` constants.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Key(CFStringRef);

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
struct Api {
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
fn api() -> Option<&'static Api> {
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
    relock(ledger).identify(src, owner, name.as_deref(), Instant::now());
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
struct Done {
    n: Mutex<u64>,
    cv: Condvar,
}

impl Done {
    /// Called by the completion block.
    fn complete(&self) {
        *relock(&self.n) += 1;
        self.cv.notify_all();
    }

    /// The count to wait past; take it before issuing the query.
    fn mark(&self) -> u64 {
        *relock(&self.n)
    }

    /// Waits up to `timeout` for a completion after `mark`. `false` on timeout.
    fn wait_past(&self, mark: u64, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut n = relock(&self.n);
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
struct Session {
    api: &'static Api,
    manager: Manager,
    queue: Queue,
    ledger: Arc<Mutex<Ledger>>,
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
    fn start(api: &'static Api) -> Result<Session, CollectError> {
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
                relock(&ledger).added(id);
                let counts = {
                    let ledger = Arc::clone(&ledger);
                    let reported = Arc::clone(&reported);
                    RcBlock::new(move |dict: *const c_void| {
                        let dict: CFDictionaryRef = dict.cast();
                        let is_reported = |i| relock(&reported).get(i);
                        // SAFETY: the framework passes a CFDictionary valid for the call.
                        let Some(c) = (unsafe { parse_counts(dict, &keys, is_reported) }) else {
                            return;
                        };
                        let need = relock(&ledger).counts(id, c);
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
                        let need = relock(&ledger).described(id, o.pid, o.upid);
                        if let Some(owner) = need {
                            // SAFETY: as above.
                            unsafe { identify(&ledger, id, owner, dict, &keys) };
                        }
                    })
                };
                let removed = {
                    let ledger = Arc::clone(&ledger);
                    RcBlock::new(move || relock(&ledger).removed(id))
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
    fn query(&self, describe: bool) -> Result<(), CollectError> {
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

// ---- collector --------------------------------------------------------------------------

/// Per-process network rates (see the module docs).
pub struct NetPerProcess {
    session: Option<Session>,
    /// Continuous time of the last settle.
    last_ns: Option<u64>,
    /// Bytes per pid and identity of one settle, reused.
    bytes: SettleMap,
    /// Paces description queries while some flow's owner is unknown.
    describe: Every,
    /// Backs off from a framework that keeps failing. Kept across releases, so a view
    /// opening again does not pay for a stuck framework sooner.
    breaker: Breaker,
}

impl Default for NetPerProcess {
    fn default() -> Self {
        Self::new()
    }
}

impl NetPerProcess {
    pub fn new() -> Self {
        Self {
            session: None,
            last_ns: None,
            bytes: HashMap::new(),
            describe: Every::new(DESCRIBE_EVERY_MS),
            breaker: Breaker::default(),
        }
    }

    /// Whether a manager is open (for tests: zero cost when nobody is looking).
    pub fn is_open(&self) -> bool {
        self.session.is_some()
    }
}

impl Collector for NetPerProcess {
    fn id(&self) -> CollectorId {
        ID
    }

    fn cadence(&self) -> Cadence {
        Cadence::OnDemand(Interest::NetworkProcesses)
    }

    fn required_entitlements(&self) -> &'static [Entitlement] {
        &[Entitlement::NetworkStatistics]
    }

    fn modules(&self) -> &'static [Module] {
        &[Module::Network]
    }

    fn probe(&mut self) -> Probe {
        // An open session survives a re-probe (an interface came or went): its flows
        // are still valid and dropping it would cost a baseline.
        if api().is_some() {
            Probe::Supported(Vec::new())
        } else {
            Probe::Unsupported {
                reason: UnsupportedReason::NoHardware,
            }
        }
    }

    fn sample(&mut self, tick: &Tick, out: &mut SampleBuf) -> Result<(), CollectError> {
        let now = tick.continuous_ns;
        out.reserve_process_net(PID_ROOM);
        if self.session.is_none() && !self.breaker.allows(now) {
            // Backing off: no rates, so rows stay "not measured" until it ends.
            return Ok(());
        }
        let session = match &mut self.session {
            Some(s) => s,
            None => {
                let api = api().ok_or(CollectError::NotProbed)?;
                self.last_ns = None;
                self.describe.reset();
                self.bytes.reserve(PID_ROOM);
                match Session::start(api) {
                    Ok(s) => self.session.insert(s),
                    Err(e) => {
                        self.breaker.failed(now);
                        return Err(e);
                    }
                }
            }
        };
        // Flows that were open before the manager report pid 0 until described; the
        // first sample describes everything, later ones only while an owner is unknown.
        let queried = session.query(false).and_then(|()| {
            let unresolved = relock(&session.ledger).unresolved() > 0;
            if (unresolved || self.last_ns.is_none()) && self.describe.due(tick) {
                session.query(true)
            } else {
                Ok(())
            }
        });
        if let Err(e) = queried {
            // A completion that never came may come later, or never: start over with
            // a fresh manager and baseline rather than wait on this one again.
            self.session = None;
            self.last_ns = None;
            self.breaker.failed(now);
            return Err(e);
        }
        self.breaker.succeeded();
        let measured = relock(&session.ledger).settle(&mut self.bytes);
        let prev = self.last_ns.replace(tick.continuous_ns);
        let (true, Some(prev)) = (measured, prev) else {
            return Ok(());
        };
        let secs = tick.continuous_ns.saturating_sub(prev) as f64 / 1e9;
        if secs <= 0.0 {
            return Ok(());
        }
        out.set_process_net_measured(Interval {
            prev_ns: prev,
            now_ns: tick.continuous_ns,
        });
        push_settled(&self.bytes, secs, out);
        Ok(())
    }

    fn release(&mut self) {
        self.session = None;
        self.last_ns = None;
    }
}

/// One [`ProcessNet`] per settled pid and identity that moved anything, over an
/// interval of `secs`. Late bytes are carried apart: history only, never the rate.
fn push_settled(bytes: &SettleMap, secs: f64, out: &mut SampleBuf) {
    for (&(pid, _), s) in bytes {
        if s.rx == 0 && s.tx == 0 && s.late_rx == 0 && s.late_tx == 0 {
            continue;
        }
        out.push_process_net(ProcessNet {
            pid,
            // A reference count, not a copy: names are interned by the ledger.
            identity: s.ident.clone(),
            rx_bytes: s.rx,
            tx_bytes: s.tx,
            rx_bps: (s.rx as f64 / secs) as f32,
            tx_bps: (s.tx as f64 / secs) as f32,
            late_rx_bytes: s.late_rx,
            late_tx_bytes: s.late_tx,
        });
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::print_stdout)]
mod tests {
    use core_foundation::base::{CFType, TCFType};
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::number::CFNumber;
    use core_foundation::string::CFString;

    use super::*;

    fn c(pid: i32, rx: u64, tx: u64) -> Counts {
        Counts {
            pid,
            upid: 0,
            rx,
            tx,
            counted: true,
        }
    }

    fn settle(l: &mut Ledger) -> Option<Vec<(i32, u64, u64)>> {
        settle_named(l).map(|v| v.into_iter().map(|(p, _, r, t)| (p, r, t)).collect())
    }

    /// A settled entry with its identity as a string: `(pid, identity, rx, tx)`.
    type Named = (i32, Option<String>, u64, u64);

    fn settle_named(l: &mut Ledger) -> Option<Vec<Named>> {
        let mut out = HashMap::new();
        let measured = l.settle(&mut out);
        let mut v: Vec<_> = out
            .into_iter()
            .map(|((p, _), s)| (p, s.ident.map(|a| a.to_string()), s.rx, s.tx))
            .collect();
        v.sort_unstable();
        measured.then_some(v)
    }

    /// `(pid, rx, tx, late rx, late tx)`: one settled entry.
    type Late = (i32, u64, u64, u64, u64);

    fn settle_late(l: &mut Ledger) -> Option<Vec<Late>> {
        let mut out = HashMap::new();
        let measured = l.settle(&mut out);
        let mut v: Vec<_> = out
            .into_iter()
            .map(|((p, _), s)| (p, s.rx, s.tx, s.late_rx, s.late_tx))
            .collect();
        v.sort_unstable();
        measured.then_some(v)
    }

    #[test]
    fn the_first_settle_is_a_baseline_then_deltas_sum_per_pid() {
        let mut l = Ledger::default();
        l.added(1);
        l.added(2);
        l.added(3);
        l.counts(1, c(100, 5_000, 700));
        l.counts(2, c(100, 1_000, 0));
        l.counts(3, c(200, 9_999, 9_999));
        assert_eq!(
            settle(&mut l),
            None,
            "baseline: lifetime bytes are not a rate"
        );

        l.counts(1, c(100, 6_000, 800));
        l.counts(2, c(100, 1_500, 0));
        l.counts(3, c(200, 9_999, 9_999));
        assert_eq!(
            settle(&mut l),
            Some(vec![(100, 1_500, 100), (200, 0, 0)]),
            "two flows of one pid add up; an idle flow moves nothing"
        );
        assert_eq!(settle(&mut l), Some(vec![(100, 0, 0), (200, 0, 0)]));
    }

    #[test]
    fn a_closed_flow_keeps_its_last_bytes() {
        let mut l = Ledger::default();
        l.added(1);
        l.counts(1, c(7, 1_000, 10));
        settle(&mut l);
        // It moves 4,000 more bytes, gets its final counts and is removed between
        // settles. The pointer is reused at once by another process's new flow.
        l.counts(1, c(7, 5_000, 10));
        l.removed(1);
        l.added(1);
        l.counts(1, c(8, 300, 30));
        assert_eq!(settle(&mut l), Some(vec![(7, 4_000, 0), (8, 300, 30)]));
        assert_eq!(
            settle(&mut l),
            Some(vec![(8, 0, 0)]),
            "the closed bytes went once"
        );
    }

    #[test]
    fn a_flow_opened_and_closed_between_settles_counts_whole() {
        let mut l = Ledger::default();
        settle(&mut l);
        l.added(9);
        l.counts(9, c(3, 20_000, 500));
        l.removed(9);
        assert_eq!(settle(&mut l), Some(vec![(3, 20_000, 500)]));
        assert_eq!(l.len(), 0);
    }

    #[test]
    fn flows_from_before_the_baseline_never_count_their_history() {
        let mut l = Ledger::default();
        // Added before the baseline but no counts yet: its first counts arrive later.
        l.added(1);
        settle(&mut l);
        l.counts(1, c(5, 80_000_000, 1_000));
        assert_eq!(settle(&mut l), Some(vec![(5, 0, 0)]));
        l.counts(1, c(5, 80_001_000, 1_000));
        assert_eq!(settle(&mut l), Some(vec![(5, 1_000, 0)]));
        // Removed before the baseline: nothing to fold.
        let mut l = Ledger::default();
        l.added(2);
        l.counts(2, c(6, 10, 10));
        l.removed(2);
        assert_eq!(settle(&mut l), None);
        assert_eq!(settle(&mut l), Some(vec![]));
    }

    /// Flows open before the manager report pid 0 until a description names them
    /// (measured on macOS 27); their bytes never go to pid 0, and once the owner is known
    /// the flow counts from there: the wait is not charged to one sample as a spike.
    #[test]
    fn a_flow_counts_from_when_its_owner_is_learned() {
        let mut l = Ledger::default();
        l.added(1);
        l.counts(1, c(0, 1_000, 0));
        assert_eq!(l.unresolved(), 1);
        assert_eq!(settle(&mut l), None);
        l.counts(1, c(0, 3_000, 0));
        assert_eq!(settle(&mut l), Some(vec![]), "nothing for pid 0");
        l.described(1, 42, 0);
        assert_eq!(l.unresolved(), 0);
        l.counts(1, c(0, 4_000, 0));
        assert_eq!(
            settle(&mut l),
            Some(vec![(42, 1_000, 0)]),
            "only what moved after the owner was known"
        );
        l.described(1, 0, 0);
        l.counts(1, c(0, 4_500, 0));
        assert_eq!(
            settle(&mut l),
            Some(vec![(42, 500, 0)]),
            "a later 0 is not an owner"
        );
    }

    /// A flow opened after the baseline that reports pid 0 at first: once a
    /// description names its owner, what it moved meanwhile goes to the owner's byte
    /// totals as late bytes, never to the rate (D-089 amending D-082).
    #[test]
    fn a_fresh_flow_named_late_keeps_its_bytes_out_of_the_rate() {
        let mut l = Ledger::default();
        settle(&mut l);
        l.added(1);
        l.counts(1, c(0, 30_000_000, 500));
        assert_eq!(settle_late(&mut l), Some(vec![]), "nothing for pid 0");
        l.counts(1, c(0, 31_000_000, 520));
        l.described(1, 42, 0);
        l.counts(1, c(0, 31_400_000, 600));
        assert_eq!(
            settle_late(&mut l),
            Some(vec![(42, 400_000, 80, 31_000_000, 520)]),
            "the rate is what moved after the owner was known; the rest is late"
        );
        l.counts(1, c(0, 31_500_000, 600));
        assert_eq!(
            settle_late(&mut l),
            Some(vec![(42, 100_000, 0, 0, 0)]),
            "late bytes go once: 31,000,000 + 400,000 + 100,000, all the flow moved"
        );

        // Named by its own counts, then closed before a settle: still conserved.
        l.added(2);
        l.counts(2, c(0, 7_000, 0));
        l.counts(2, c(43, 9_000, 0));
        l.removed(2);
        assert_eq!(
            settle_late(&mut l),
            Some(vec![(42, 0, 0, 0, 0), (43, 2_000, 0, 7_000, 0)])
        );
    }

    #[test]
    fn an_uncounted_flow_named_late_charges_nothing() {
        let mut l = Ledger::default();
        settle(&mut l);
        l.added(1);
        l.counts(
            1,
            Counts {
                counted: false,
                ..c(0, 5_000, 5_000)
            },
        );
        settle(&mut l);
        l.described(1, 42, 0);
        l.counts(
            1,
            Counts {
                counted: false,
                ..c(0, 6_000, 6_000)
            },
        );
        assert_eq!(settle_late(&mut l), Some(vec![]));
    }

    /// The collector's rows: the rate is the interval's bytes over its length; late
    /// bytes ride along for history and never reach it.
    #[test]
    fn late_bytes_are_carried_apart_from_the_rate() {
        let mut l = Ledger::default();
        settle(&mut l);
        l.added(1);
        l.counts(1, c(0, 30_000_000, 500));
        settle(&mut l);
        l.described(1, 42, 0);
        l.counts(1, c(0, 30_400_000, 600));
        let mut bytes = HashMap::new();
        assert!(l.settle(&mut bytes));
        let mut buf = SampleBuf::new();
        push_settled(&bytes, 2.0, &mut buf);
        let n = buf.process_net();
        assert_eq!(n.len(), 1);
        assert_eq!(
            (n[0].rx_bytes, n[0].tx_bytes, n[0].rx_bps, n[0].tx_bps),
            (400_000, 100, 200_000.0, 50.0)
        );
        assert_eq!((n[0].late_rx_bytes, n[0].late_tx_bytes), (30_000_000, 500));
    }

    #[test]
    fn a_flow_from_before_the_baseline_named_late_drops_its_wait() {
        let mut l = Ledger::default();
        l.added(1);
        l.counts(1, c(0, 1_000, 0));
        settle(&mut l);
        l.counts(1, c(0, 50_000, 0));
        settle(&mut l);
        l.described(1, 42, 0);
        l.counts(1, c(0, 51_000, 0));
        assert_eq!(settle_late(&mut l), Some(vec![(42, 1_000, 0, 0, 0)]));
    }

    #[test]
    fn an_owner_first_named_by_counts_is_charged_from_its_previous_counts() {
        let mut l = Ledger::default();
        l.added(1);
        l.counts(1, c(0, 1_000, 0));
        settle(&mut l);
        l.counts(1, c(0, 50_000, 0));
        assert_eq!(settle(&mut l), Some(vec![]));
        l.counts(1, c(42, 52_000, 10));
        assert_eq!(settle(&mut l), Some(vec![(42, 2_000, 10)]));
    }

    #[test]
    fn a_description_before_the_baseline_keeps_the_baseline() {
        // The first sample describes, then settles: the baseline is the bytes then.
        let mut l = Ledger::default();
        l.added(1);
        l.counts(1, c(0, 1_000, 0));
        l.described(1, 42, 0);
        assert_eq!(settle(&mut l), None);
        l.counts(1, c(0, 1_500, 0));
        assert_eq!(settle(&mut l), Some(vec![(42, 500, 0)]));
    }

    fn cu(pid: i32, upid: u64, rx: u64, tx: u64) -> Counts {
        Counts {
            upid,
            ..c(pid, rx, tx)
        }
    }

    fn owner(pid: i32, upid: u64) -> Owner {
        Owner { pid, upid }
    }

    /// The first callback that names an owner asks for its identity once; the owner's
    /// later flows take it from the cache, and every entry of one name shares one
    /// allocation.
    #[test]
    fn an_identity_is_resolved_once_per_process() {
        let now = Instant::now();
        let mut l = Ledger::with_room();
        settle(&mut l);
        l.added(1);
        assert_eq!(l.counts(1, cu(7, 70, 100, 0)), Some(owner(7, 70)));
        l.identify(1, owner(7, 70), Some("Google Chrome"), now);
        assert_eq!(l.counts(1, cu(7, 70, 200, 0)), None, "already known");
        l.added(2);
        assert_eq!(l.counts(2, cu(7, 70, 50, 0)), None, "cached by unique id");
        // Another helper of the same app: its own lookup, the same interned name.
        l.added(3);
        assert_eq!(l.counts(3, cu(8, 80, 5, 5)), Some(owner(8, 80)));
        l.identify(3, owner(8, 80), Some("Google Chrome"), now);
        let mut out = HashMap::new();
        assert!(l.settle(&mut out));
        let names: Vec<_> = out.values().map(|s| s.ident.clone().unwrap()).collect();
        assert_eq!(names.len(), 2, "one entry per pid");
        assert!(Arc::ptr_eq(&names[0], &names[1]));
        let mut got: Vec<_> = out.into_iter().map(|((p, _), s)| (p, s.rx, s.tx)).collect();
        got.sort_unstable();
        assert_eq!(got, vec![(7, 250, 0), (8, 5, 5)]);
    }

    /// A short-lived process: its flow opens, moves bytes, gets its final counts and is
    /// removed, and the process exits, all between two settles. Its bytes keep the name
    /// resolved at the first callback.
    #[test]
    fn a_process_that_exits_between_samples_keeps_its_identity() {
        let mut l = Ledger::with_room();
        settle(&mut l);
        l.added(4);
        let need = l.counts(4, cu(900, 9_000, 1_000, 10)).unwrap();
        l.identify(4, need, Some("curl"), Instant::now());
        l.counts(4, cu(900, 9_000, 4_000_000, 600));
        l.removed(4);
        assert_eq!(
            settle_named(&mut l),
            Some(vec![(900, Some("curl".into()), 4_000_000, 600)])
        );
        assert_eq!(settle_named(&mut l), Some(vec![]), "counted once");
    }

    /// A reused pid is a new process: a new lookup, and its bytes stay apart from the
    /// old process's in the same settle.
    #[test]
    fn a_reused_pid_is_named_again() {
        let now = Instant::now();
        let mut l = Ledger::with_room();
        settle(&mut l);
        l.added(1);
        let need = l.counts(1, cu(5, 50, 100, 0)).unwrap();
        l.identify(1, need, Some("curl"), now);
        l.removed(1);
        l.added(1);
        assert_eq!(l.counts(1, cu(5, 51, 30, 0)), Some(owner(5, 51)));
        l.identify(1, owner(5, 51), Some("git"), now);
        assert_eq!(
            settle_named(&mut l),
            Some(vec![
                (5, Some("curl".into()), 100, 0),
                (5, Some("git".into()), 30, 0)
            ])
        );
    }

    #[test]
    fn an_unnamed_owner_still_counts_and_is_not_looked_up_again() {
        let mut l = Ledger::with_room();
        settle(&mut l);
        l.added(1);
        let need = l.counts(1, cu(3, 30, 10, 0)).unwrap();
        l.identify(1, need, None, Instant::now());
        assert_eq!(l.counts(1, cu(3, 30, 20, 0)), None);
        assert_eq!(settle_named(&mut l), Some(vec![(3, None, 20, 0)]));
    }

    #[test]
    fn new_names_are_capped_per_hour() {
        let t0 = Instant::now();
        let mut l = Ledger::with_room();
        settle(&mut l);
        let n = NEW_NAMES_PER_HOUR as usize;
        for i in 0..=n {
            let src = i + 1;
            let pid = i as i32 + 1;
            l.added(src);
            let need = l.counts(src, cu(pid, pid as u64, 1, 0)).unwrap();
            l.identify(src, need, Some(&format!("app{i}")), t0);
        }
        let out = settle_named(&mut l).unwrap();
        assert_eq!(out.iter().filter(|r| r.1.is_some()).count(), n);
        assert_eq!(
            out.iter().find(|r| r.0 == n as i32 + 1).unwrap().1,
            None,
            "over the cap: other apps"
        );
        // A known name is not new; an hour later new names are taken again.
        l.added(10_000);
        let need = l.counts(10_000, cu(10_000, 10_000, 1, 0)).unwrap();
        l.identify(10_000, need, Some("app0"), t0);
        l.added(10_001);
        let need = l.counts(10_001, cu(10_001, 10_001, 1, 0)).unwrap();
        l.identify(10_001, need, Some("late"), t0 + HOUR);
        let out = settle_named(&mut l).unwrap();
        let name = |pid| out.iter().find(|r| r.0 == pid).unwrap().1.clone();
        assert_eq!(name(10_000).as_deref(), Some("app0"));
        assert_eq!(name(10_001).as_deref(), Some("late"));
    }

    /// Names of processes with no open flow are forgotten once the cache is full, and
    /// a name nothing uses leaves the intern table.
    #[test]
    fn names_of_gone_processes_are_pruned() {
        let now = Instant::now();
        let mut l = Ledger::with_room();
        settle(&mut l);
        for i in 0..=PID_ROOM {
            let pid = i as i32 + 1;
            l.added(i);
            let need = l.counts(i, cu(pid, pid as u64, 1, 0)).unwrap();
            // Many processes, few apps, as with browser helpers.
            l.identify(i, need, Some(&format!("p{}", i % 4)), now);
            if i > 0 {
                l.removed(i);
            }
        }
        settle(&mut l);
        assert_eq!(l.names.by_owner.len(), 1, "only the open flow's owner");
        // The closed bytes still held their names during that prune; the next one
        // drops them.
        assert_eq!(l.names.interned.len(), 4);
        l.names.prune_at = 0;
        settle(&mut l);
        assert_eq!(l.names.interned.len(), 1);
        assert!(l.names.interned.contains("p0"));
        assert_eq!(l.names.prune_at, PID_ROOM);
    }

    /// A query whose completion never arrives must not make later queries wait for a
    /// count that is one ahead forever.
    #[test]
    fn a_lost_completion_does_not_stall_later_waits() {
        let done = Done::default();
        let mark = done.mark();
        assert!(
            !done.wait_past(mark, Duration::from_millis(5)),
            "never completed"
        );
        let mark = done.mark();
        done.complete();
        assert!(done.wait_past(mark, Duration::from_millis(5)));
        let mark = done.mark();
        assert!(
            !done.wait_past(mark, Duration::from_millis(5)),
            "not yet again"
        );
    }

    #[test]
    fn a_completion_from_another_thread_ends_the_wait() {
        let done = Arc::new(Done::default());
        let mark = done.mark();
        let d = Arc::clone(&done);
        let t = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(10));
            d.complete();
        });
        assert!(done.wait_past(mark, Duration::from_secs(5)));
        t.join().unwrap();
    }

    #[test]
    fn repeated_failures_back_off_until_the_period_passes() {
        let sec = 1_000_000_000u64;
        let backoff = BACKOFF.as_secs() * sec;
        let mut b = Breaker::default();
        for i in 0..FAIL_LIMIT - 1 {
            b.failed(i as u64 * sec);
            assert!(b.allows(i as u64 * sec + 1), "failure {i} retries at once");
        }
        let t = 10 * sec;
        b.failed(t);
        assert!(!b.allows(t + 1));
        assert!(!b.allows(t + backoff - 1));
        assert!(b.allows(t + backoff));
        // Still failing after the backoff: one attempt, then the next backoff.
        b.failed(t + backoff);
        assert!(!b.allows(t + backoff + 1));
        assert!(b.allows(t + 2 * backoff));
        // A sample that worked clears it.
        b.succeeded();
        b.failed(t + 2 * backoff);
        assert!(b.allows(t + 2 * backoff + 1));
    }

    #[test]
    fn uncounted_flows_charge_nothing() {
        let mut l = Ledger::default();
        settle(&mut l);
        l.added(1);
        l.counts(
            1,
            Counts {
                counted: false,
                ..c(4, 1 << 30, 1 << 30)
            },
        );
        l.added(2);
        l.counts(2, c(4, 10, 20));
        l.removed(1);
        assert_eq!(settle(&mut l), Some(vec![(4, 10, 20)]));
    }

    #[test]
    fn a_counter_going_backwards_is_not_a_huge_rate() {
        let mut l = Ledger::default();
        l.added(1);
        l.counts(1, c(1, 1_000, 1_000));
        settle(&mut l);
        l.counts(1, c(1, 10, 10));
        assert_eq!(settle(&mut l), Some(vec![(1, 0, 0)]));
    }

    /// A counts dictionary the way NetworkStatistics shaped it on macOS 27 (D-081): SInt64
    /// numbers, interface-type flags only when true, plus keys the parser ignores.
    struct Fixture {
        names: [CFString; 7],
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                names: [
                    "processID",
                    "rxBytes",
                    "txBytes",
                    "ifLoopback",
                    "uniqueProcessID",
                    "processName",
                    "interface",
                ]
                .map(CFString::new),
            }
        }

        fn keys(&self) -> Keys {
            let k = |i: usize| Key(self.names[i].as_concrete_TypeRef());
            Keys {
                pid: k(0),
                rx: k(1),
                tx: k(2),
                loopback: k(3),
                upid: Some(k(4)),
                name: Some(k(5)),
                interface: k(6),
            }
        }

        fn with<T>(
            &self,
            pairs: &[(&str, CFType)],
            f: impl FnOnce(CFDictionaryRef, &Keys) -> T,
        ) -> T {
            let pairs: Vec<(CFString, CFType)> = pairs
                .iter()
                .map(|(k, v)| (CFString::new(k), v.clone()))
                .collect();
            let dict = CFDictionary::from_CFType_pairs(&pairs);
            f(dict.as_concrete_TypeRef(), &self.keys())
        }

        fn parse(&self, pairs: &[(&str, CFType)]) -> Option<Counts> {
            // SAFETY: a valid dictionary alive for the call; keys alive with `self`.
            self.with(pairs, |d, k| unsafe { parse_counts(d, k, |i| i == EN0) })
        }
    }

    /// The interface index the fixtures' `reported` accepts, as en0 is on the dev Mac.
    const EN0: u32 = 14;

    fn n(v: i64) -> CFType {
        CFNumber::from(v).as_CFType()
    }

    #[test]
    fn parses_a_counts_dictionary() {
        let f = Fixture::new();
        let base = [
            ("processID", n(1912)),
            ("uniqueProcessID", n(1_032_823)),
            ("processName", CFString::new("curl").as_CFType()),
            ("provider", CFString::new("TCP").as_CFType()),
            ("rxBytes", n(24_527_869)),
            ("txBytes", n(520)),
            ("rxWiFiBytes", n(24_527_869)),
            ("ifWiFi", CFBoolean::true_value().as_CFType()),
            ("interface", n(i64::from(EN0))),
        ];
        assert_eq!(
            f.parse(&base),
            Some(Counts {
                upid: 1_032_823,
                ..c(1912, 24_527_869, 520)
            })
        );
        // SAFETY: a valid dictionary alive for the call.
        let recorded = f.with(&base, |d, k| unsafe { recorded_identity(d, k) });
        assert_eq!(recorded.as_deref(), Some("curl"));

        let mut not_lo = base.to_vec();
        not_lo.push(("ifLoopback", CFBoolean::false_value().as_CFType()));
        assert!(f.parse(&not_lo).unwrap().counted);
    }

    /// Regression: apps summed to 30x the interface on the Network page, shares past
    /// 1,500%, because flows that never touch a reported interface were counted. The
    /// dictionaries are the ones macOS 27 sent for a transfer to this Mac's own
    /// Tailscale address and over 127.0.0.1.
    #[test]
    fn counts_only_flows_on_a_reported_interface() {
        let f = Fixture::new();
        let flow = |extra: &[(&str, CFType)]| {
            let mut pairs = vec![
                ("processID", n(1912)),
                ("rxBytes", n(20_971_520)),
                ("txBytes", n(0)),
            ];
            pairs.extend_from_slice(extra);
            f.parse(&pairs).unwrap().counted
        };
        let yes = || CFBoolean::true_value().as_CFType();
        assert!(flow(&[("ifWiFi", yes()), ("interface", n(i64::from(EN0)))]));
        // A tunnel reports the type of the link under it: utun4 says Wi-Fi.
        assert!(!flow(&[("ifWiFi", yes()), ("interface", n(23))]));
        assert!(!flow(&[("ifLoopback", yes()), ("interface", n(1))]));
        // Flagged loopback on a reported index still is not counted.
        assert!(!flow(&[
            ("ifLoopback", yes()),
            ("interface", n(i64::from(EN0)))
        ]));
        // No index, or 0: nothing says the bytes are in the interface totals.
        assert!(!flow(&[("ifWiFi", yes())]));
        assert!(!flow(&[("ifWiFi", yes()), ("interface", n(0))]));
        assert!(!flow(&[
            ("ifWiFi", yes()),
            ("interface", CFString::new("en0").as_CFType())
        ]));
    }

    #[test]
    fn rejects_missing_or_mistyped_values() {
        let f = Fixture::new();
        let no_pid = [("rxBytes", n(1)), ("txBytes", n(1))];
        assert_eq!(f.parse(&no_pid), None);
        let text_rx = [
            ("processID", n(1)),
            ("rxBytes", CFString::new("1").as_CFType()),
            ("txBytes", n(1)),
        ];
        assert_eq!(f.parse(&text_rx), None);
        let negative = [("processID", n(1)), ("rxBytes", n(-5)), ("txBytes", n(1))];
        assert_eq!(f.parse(&negative), None);
        let huge_pid = [
            ("processID", n(i64::from(i32::MAX) + 1)),
            ("rxBytes", n(1)),
            ("txBytes", n(1)),
        ];
        assert_eq!(f.parse(&huge_pid), None);
        // SAFETY: null is allowed.
        assert_eq!(
            unsafe { parse_counts(std::ptr::null(), &f.keys(), |_| true) },
            None
        );
    }

    /// Reads the live framework while the machine moves traffic, and prints the top
    /// processes by bytes over a few seconds next to the closed-flow and fresh-flow
    /// counts. Compare with `nettop -P -L 1 -J bytes_in,bytes_out` run over the same span
    /// (it reports cumulative totals; take two and subtract).
    #[test]
    #[ignore = "reads the live NetworkStatistics framework; run by hand with --ignored --nocapture"]
    fn live_top_processes() {
        let mut c = NetPerProcess::new();
        assert!(matches!(c.probe(), Probe::Supported(_)), "API unavailable");
        let secs: u64 = std::env::var("NSTAT_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(5);
        let tick = |n| Tick {
            n,
            wall_ms: 0,
            continuous_ns: super::super::sysctl::continuous_ns(),
            interval_ms: 1_000,
        };
        let mut buf = SampleBuf::new();
        let t0 = Instant::now();
        c.sample(&tick(0), &mut buf).unwrap();
        println!("create + baseline: {:?}", t0.elapsed());
        assert!(
            !buf.process_net_measured(),
            "the first sample is a baseline"
        );
        let mut totals: HashMap<i32, (f64, f64)> = HashMap::new();
        for i in 1..=secs {
            std::thread::sleep(Duration::from_secs(1));
            buf.clear();
            let t = Instant::now();
            c.sample(&tick(i), &mut buf).unwrap();
            let took = t.elapsed();
            assert!(buf.process_net_measured());
            for n in buf.process_net() {
                let e = totals.entry(n.pid).or_default();
                e.0 += f64::from(n.rx_bps);
                e.1 += f64::from(n.tx_bps);
            }
            println!(
                "sample {i}: {took:?}, {} pids with traffic",
                buf.process_net().len()
            );
        }
        let mut top: Vec<_> = totals.into_iter().collect();
        top.sort_by(|a, b| (b.1.0 + b.1.1).total_cmp(&(a.1.0 + a.1.1)));
        println!("top processes, mean bytes/s over {secs} s:");
        for (pid, (rx, tx)) in top.iter().take(10) {
            let name = super::super::libproc::bsd_info(*pid)
                .map(|b| super::super::libproc::name_of(&b).into_owned())
                .unwrap_or_default();
            println!(
                "  {pid:>6} {name:<28} rx {:>12.0} tx {:>12.0}",
                rx / secs as f64,
                tx / secs as f64
            );
        }
        c.release();
        assert!(!c.is_open());
    }

    /// A `curl` that downloads 4 MB and exits between two samples: the second sample
    /// must still charge its bytes to "curl" (the identity resolved while it ran, or
    /// the recorded process name once it was gone). Needs internet access.
    #[test]
    #[ignore = "reads the live NetworkStatistics framework and downloads 4 MB; run by hand with --ignored --nocapture"]
    fn live_short_curl_keeps_its_identity() {
        let mut c = NetPerProcess::new();
        assert!(matches!(c.probe(), Probe::Supported(_)), "API unavailable");
        let tick = |n| Tick {
            n,
            wall_ms: 0,
            continuous_ns: super::super::sysctl::continuous_ns(),
            interval_ms: 1_000,
        };
        let mut buf = SampleBuf::new();
        c.sample(&tick(0), &mut buf).unwrap();
        assert!(!buf.process_net_measured(), "baseline");
        let t = Instant::now();
        let status = std::process::Command::new("/usr/bin/curl")
            .args([
                "-sS",
                "-o",
                "/dev/null",
                "https://speed.cloudflare.com/__down?bytes=4000000",
            ])
            .status()
            .unwrap();
        assert!(status.success(), "curl failed: {status}");
        println!("curl ran {:?} and exited", t.elapsed());
        // Its flows' final counts and removals arrive on the framework's queue.
        std::thread::sleep(Duration::from_secs(2));
        buf.clear();
        c.sample(&tick(1), &mut buf).unwrap();
        let interval = buf.process_net_interval().unwrap();
        assert!(interval.now_ns > interval.prev_ns);
        for n in buf.process_net() {
            println!(
                "  pid {:>6} {:<28} rx {:>10} tx {:>8}",
                n.pid,
                n.identity.as_deref().unwrap_or("-"),
                n.rx_bytes,
                n.tx_bytes
            );
        }
        let curl: u64 = buf
            .process_net()
            .iter()
            .filter(|n| n.identity.as_deref() == Some("curl"))
            .map(|n| n.rx_bytes + n.late_rx_bytes)
            .sum();
        println!("curl rx {curl} bytes");
        assert!(curl > 0, "no bytes charged to curl");
        c.release();
    }

    /// Regression for apps exceeding the interface: moves 20 MB to this process over
    /// 127.0.0.1, ::1 and every IPv4 address on an interface the network collector does
    /// not report (a Tailscale `utun`, a VM bridge), and requires none of it charged to
    /// this process. Before the interface check, the tunnel's 20 MB was charged in both
    /// directions and one run in four charged a loopback receiver.
    #[test]
    #[ignore = "reads the live NetworkStatistics framework; run by hand with --ignored --nocapture"]
    fn live_traffic_off_the_reported_interfaces_is_not_charged() {
        use std::io::{Read, Write};
        use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, TcpListener, TcpStream};

        let mut addrs = vec![
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V6(Ipv6Addr::LOCALHOST),
        ];
        // SAFETY: the list is freed below, once.
        let list = unsafe { libc::if_nameindex() };
        assert!(!list.is_null());
        let mut cur = list;
        // SAFETY: `if_nameindex` returns an array ended by a zero index and null name.
        while unsafe { (*cur).if_index } != 0 {
            // SAFETY: as above; `cur` is before the end entry.
            let (index, name) = unsafe { ((*cur).if_index, CStr::from_ptr((*cur).if_name)) };
            let name = name.to_string_lossy();
            if !network::is_reported_interface(index) && !name.starts_with("lo") {
                let found = super::super::ifaddrs::interface_addresses(&name);
                addrs.extend(found.ipv4.into_iter().map(IpAddr::V4));
            }
            // SAFETY: not past the end entry.
            cur = unsafe { cur.add(1) };
        }
        // SAFETY: from `if_nameindex` above.
        unsafe { libc::if_freenameindex(list) };

        let me = i32::try_from(std::process::id()).unwrap();
        let mut c = NetPerProcess::new();
        assert!(matches!(c.probe(), Probe::Supported(_)), "API unavailable");
        let tick = |n| Tick {
            n,
            wall_ms: 0,
            continuous_ns: super::super::sysctl::continuous_ns(),
            interval_ms: 1_000,
        };
        let mut buf = SampleBuf::new();
        c.sample(&tick(0), &mut buf).unwrap();
        let chunk = [7u8; 64 * 1024];
        for (i, addr) in addrs.iter().enumerate() {
            let Ok(listener) = TcpListener::bind((*addr, 0)) else {
                println!("{addr}: cannot listen, skipped");
                continue;
            };
            let to = listener.local_addr().unwrap();
            let Ok(mut s) = TcpStream::connect(to) else {
                println!("{addr}: cannot connect, skipped");
                continue;
            };
            let server = std::thread::spawn(move || {
                let (mut conn, _) = listener.accept().unwrap();
                let mut b = [0u8; 64 * 1024];
                while matches!(conn.read(&mut b), Ok(n) if n > 0) {}
            });
            for _ in 0..320 {
                s.write_all(&chunk).unwrap();
            }
            drop(s);
            server.join().unwrap();
            // Final counts arrive on the framework's queue a moment after the close.
            std::thread::sleep(Duration::from_secs(3));
            buf.clear();
            c.sample(&tick(i as u64 + 1), &mut buf).unwrap();
            let charged: u64 = buf
                .process_net()
                .iter()
                .filter(|n| n.pid == me)
                .map(|n| n.rx_bytes + n.tx_bytes + n.late_rx_bytes + n.late_tx_bytes)
                .sum();
            println!("{addr}: 20 MB moved, {charged} bytes charged");
            assert_eq!(
                charged, 0,
                "traffic over {addr} was charged to this process"
            );
        }
        c.release();
    }

    /// Opens and destroys managers in a tight loop while loopback connections open,
    /// move data and close in another thread, so added, counts and removed callbacks are
    /// queued when each manager is destroyed. Half the iterations destroy right after
    /// `NStatManagerAddAll*`, with the added callbacks for every flow still pending.
    /// Pass: no crash, every iteration starts. `NSTAT_CHURN` sets the iterations.
    #[test]
    #[ignore = "drives the live NetworkStatistics framework; run by hand with --ignored --nocapture"]
    fn live_open_release_churn() {
        use std::io::{Read, Write};
        use std::net::{TcpListener, TcpStream};
        use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

        let iterations: u64 = std::env::var("NSTAT_CHURN")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(200);
        let api = api().expect("API unavailable");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let moved = Arc::new(AtomicU64::new(0));
        let server = std::thread::spawn(move || {
            for conn in listener.incoming() {
                let Ok(mut conn) = conn else { break };
                std::thread::spawn(move || {
                    let mut buf = [0u8; 64 * 1024];
                    while matches!(conn.read(&mut buf), Ok(n) if n > 0) {}
                });
            }
        });
        let client = {
            let (stop, moved) = (Arc::clone(&stop), Arc::clone(&moved));
            std::thread::spawn(move || {
                let chunk = [7u8; 64 * 1024];
                while !stop.load(Ordering::Relaxed) {
                    // A new flow every few chunks, so sources come and go throughout.
                    let Ok(mut s) = TcpStream::connect(addr) else {
                        continue;
                    };
                    for _ in 0..8 {
                        if s.write_all(&chunk).is_err() {
                            break;
                        }
                        moved.fetch_add(chunk.len() as u64, Ordering::Relaxed);
                    }
                }
            })
        };
        let mut c = NetPerProcess::new();
        assert!(matches!(c.probe(), Probe::Supported(_)));
        let mut buf = SampleBuf::new();
        let t0 = Instant::now();
        let mut timeouts = 0;
        for i in 0..iterations {
            if i % 2 == 0 {
                drop(Session::start(api).unwrap());
            } else {
                buf.clear();
                let tick = Tick {
                    n: i,
                    wall_ms: 0,
                    continuous_ns: super::super::sysctl::continuous_ns(),
                    interval_ms: 1_000,
                };
                match c.sample(&tick, &mut buf) {
                    Ok(()) => {}
                    Err(CollectError::Timeout { .. }) => timeouts += 1,
                    Err(e) => panic!("iteration {i}: {e}"),
                }
                c.release();
            }
        }
        let took = t0.elapsed();
        stop.store(true, Ordering::Relaxed);
        client.join().unwrap();
        // The server thread blocks in accept; it ends with the test process.
        drop(server);
        println!(
            "{iterations} open/destroy cycles in {took:?}, {timeouts} query timeouts, {} MB over loopback",
            moved.load(Ordering::Relaxed) / 1_000_000
        );
        assert!(moved.load(Ordering::Relaxed) > 0, "no traffic flowed");
    }
}
