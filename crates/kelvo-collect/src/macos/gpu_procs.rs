//! Per-process GPU time from the GPU's IORegistry user clients (D-085).
//!
//! Every process that opens the GPU (a Metal device) gets an `AGXDeviceUserClient` as a
//! child of the `IOAccelerator` service in the IOService plane. The clients are not
//! registered, so `IOServiceGetMatchingServices` does not return them; they are walked
//! with the accelerator's child iterator. Each carries:
//! - `IOUserClientCreator`: `"pid N, name"` (the name truncated to 16 characters; rows
//!   take theirs from the processes collector, so only the pid is read here);
//! - `AppUsage`: one dictionary per command queue with `accumulatedGPUTime`, the GPU
//!   nanoseconds its command buffers used, monotonic for the client's lifetime. An empty
//!   array for a client that never submitted.
//!
//! Verified on macOS 27 (M3 Max, 40 GPU cores), unprivileged and ad-hoc signed: these are
//! plain registry reads and need no entitlement. The sandboxed (`appstore`) build does
//! not include the collector (`Entitlement::IoRegistryGpuClients`).
//!
//! A process's share is its clients' summed `accumulatedGPUTime` delta divided by the
//! wall time between samples: percent of the whole GPU, all cores. In the spike the
//! shares added up to the load's own measured Metal busy time (99.9% against 98.9% under
//! full load, 44.4% against 44.9% at half).
//!
//! Caveat: the counter moves when a command buffer completes. Work in millisecond-scale
//! buffers (UI, video, games) is attributed accurately; a compute job with buffers
//! seconds long lands in whichever interval the buffer finished in, and clients queued
//! behind it are charged its time too. Each process is clamped to 100% and all shares
//! are scaled down together when they add up to more than 100%, so a window never shows
//! an impossible GPU; the numbers are approximate under such jobs.
//!
//! Lifecycle: sampled only while a visible view asks for GPU time
//! (`Cadence::OnDemand(Interest::GpuProcesses)`), on the process ticks. The first
//! sample is a baseline. [`Collector::release`] forgets the accelerator and the
//! counters, so the next view starts over from a baseline.

use std::collections::HashMap;
use std::ffi::c_char;

use core_foundation::base::{CFType, TCFType};
use core_foundation::string::CFString;
use core_foundation_sys::array::{CFArrayGetCount, CFArrayGetTypeID, CFArrayGetValueAtIndex};
use core_foundation_sys::base::{CFGetTypeID, CFTypeRef};
use core_foundation_sys::dictionary::{CFDictionaryGetTypeID, CFDictionaryGetValue};
use core_foundation_sys::number::{
    CFNumberGetTypeID, CFNumberGetValue, CFNumberRef, kCFNumberSInt64Type,
};
use core_foundation_sys::string::{CFStringGetCString, CFStringGetTypeID, kCFStringEncodingUTF8};
use kelvo_schema::{Entitlement, Module, UnsupportedReason};

use super::iokit::{IoObject, Key, matching_services};
use crate::{
    Cadence, CollectError, Collector, CollectorId, Interest, Probe, ProcessGpu, SampleBuf, Tick,
};

pub const ID: CollectorId = CollectorId("gpu_per_process");

const ACCELERATOR: &std::ffi::CStr = c"IOAccelerator";

/// Rows the sample buffer keeps room for, so a tick with more GPU processes than any
/// before does not allocate (about 30 on the dev Mac).
const PID_ROOM: usize = 256;

/// Turns per-client cumulative GPU nanoseconds into per-pid nanoseconds since the last
/// pass. Clients are keyed by registry entry id, since a process can hold several
/// (WindowServer has two) and a pid can be reused.
#[derive(Debug, Default)]
struct Ledger {
    /// Registry id -> GPU ns at the previous pass.
    prev: HashMap<u64, u64>,
    /// The pass being read; swapped into `prev` by [`Ledger::settle`].
    cur: HashMap<u64, u64>,
    /// Pid -> GPU ns since the previous pass, for the pass being read.
    per_pid: HashMap<i32, u64>,
    /// `prev` holds every client that existed at the previous pass, so a client absent
    /// from it is new since then.
    primed: bool,
    /// The previous pass was cut short and its missed clients were kept
    /// ([`Ledger::keep_missed`]); a second cut pass in a row does not keep them again.
    kept_last: bool,
}

impl Ledger {
    fn begin(&mut self) {
        self.cur.clear();
        self.per_pid.clear();
    }

    /// One client in this pass, idle ones (`gpu_ns` 0) included, so each pass knows
    /// every client the last one saw. After the first pass a client absent from the last
    /// one is new since then (registry ids are never reused), so all its time is in this
    /// interval. A counter that went backwards adds nothing.
    fn client(&mut self, id: u64, pid: i32, gpu_ns: u64) {
        if self.cur.insert(id, gpu_ns).is_some() {
            return;
        }
        let before = match self.prev.get(&id) {
            Some(&before) => before,
            None if self.primed => 0,
            None => return,
        };
        if gpu_ns > before {
            *self.per_pid.entry(pid).or_default() += gpu_ns - before;
        }
    }

    /// A client seen but not read this pass: it keeps its last count, so the next pass
    /// does not take it for a new client and charge it its lifetime.
    fn keep(&mut self, id: u64) {
        if let Some(&ns) = self.prev.get(&id) {
            self.cur.entry(id).or_insert(ns);
        }
    }

    /// After a walk that missed clients: every client of the last pass not seen in this
    /// one keeps its count, as [`Ledger::keep`], so this pass still knows every client.
    /// Only for one pass in a row, so ids of clients that exited do not pile up while
    /// walks keep getting cut. Returns whether the clients were kept: false before a
    /// baseline (nothing to keep) and on a second cut pass in a row.
    fn keep_missed(&mut self) -> bool {
        if !self.primed || self.kept_last {
            self.kept_last = false;
            return false;
        }
        for (&id, &ns) in &self.prev {
            self.cur.entry(id).or_insert(ns);
        }
        self.kept_last = true;
        true
    }

    /// Ends a pass. False when it only set a baseline: the first pass since a reset, or
    /// the first after a pass that did not see every client. `whole`: this pass saw (or
    /// kept) every client, so the next one can take a client missing from it as new.
    fn settle(&mut self, whole: bool) -> bool {
        std::mem::swap(&mut self.prev, &mut self.cur);
        std::mem::replace(&mut self.primed, whole)
    }

    fn reset(&mut self) {
        self.prev.clear();
        self.cur.clear();
        self.per_pid.clear();
        self.primed = false;
        self.kept_last = false;
    }
}

/// Runs one pass into `ledger` and settles it; true when it measured (not a baseline).
/// `walk` reads every client into the ledger and returns whether it saw them all (the
/// registry did not change under it). An incomplete walk is retried once from the start;
/// if the retry is also incomplete, the clients it missed keep their last counts
/// ([`Ledger::keep_missed`]), or, when that is not allowed, the next pass is a baseline.
/// A walk that fails resets the ledger.
fn read_pass(
    ledger: &mut Ledger,
    mut walk: impl FnMut(&mut Ledger) -> Result<bool, CollectError>,
) -> Result<bool, CollectError> {
    for _ in 0..2 {
        ledger.begin();
        match walk(ledger) {
            Ok(true) => {
                ledger.kept_last = false;
                return Ok(ledger.settle(true));
            }
            Ok(false) => {}
            Err(e) => {
                ledger.reset();
                return Err(e);
            }
        }
    }
    let whole = ledger.keep_missed();
    Ok(ledger.settle(whole))
}

/// Percent of the GPU per pid over `wall_ns`: each clamped to 100, and all scaled down
/// together when they add up to more than 100 (long command buffers charge an interval
/// with time from earlier ones; see the module docs). Zero shares are skipped.
fn shares(per_pid: &HashMap<i32, u64>, wall_ns: u64, mut emit: impl FnMut(i32, f32)) {
    if wall_ns == 0 {
        return;
    }
    let pct = |ns: u64| (ns as f64 / wall_ns as f64 * 100.0).min(100.0);
    let sum: f64 = per_pid.values().map(|&ns| pct(ns)).sum();
    let scale = if sum > 100.0 { 100.0 / sum } else { 1.0 };
    for (&pid, &ns) in per_pid {
        let p = pct(ns) * scale;
        if p > 0.0 {
            emit(pid, p as f32);
        }
    }
}

/// The pid in an `IOUserClientCreator` value, `"pid 475, WindowServer"`.
fn parse_creator(s: &[u8]) -> Option<i32> {
    let rest = s.strip_prefix(b"pid ")?;
    let end = rest.iter().position(|&b| b == b',')?;
    std::str::from_utf8(rest.get(..end)?).ok()?.parse().ok()
}

/// The creator pid of a client's `IOUserClientCreator` property, read into a stack
/// buffer so a pass does not allocate.
fn creator_pid(v: &CFType) -> Option<i32> {
    let r = v.as_CFTypeRef();
    // SAFETY: `r` is a live CF object (held by `v`).
    if unsafe { CFGetTypeID(r) } != unsafe { CFStringGetTypeID() } {
        return None;
    }
    let mut buf = [0 as c_char; 96];
    // SAFETY: `r` is a CFString; `buf` is writable for its length. A value that does not
    // fit returns false (a creator is "pid N, " plus at most 16 characters).
    let ok = unsafe {
        CFStringGetCString(
            r.cast(),
            buf.as_mut_ptr(),
            buf.len() as _,
            kCFStringEncodingUTF8,
        )
    };
    if ok == 0 {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0)?;
    let bytes: &[u8] = bytemuck_cast(buf.get(..len)?);
    parse_creator(bytes)
}

/// `c_char` is `i8` or `u8` by target; both are one byte.
fn bytemuck_cast(s: &[c_char]) -> &[u8] {
    // SAFETY: `c_char` and `u8` have the same size and alignment, and every bit pattern
    // is a valid `u8`.
    unsafe { std::slice::from_raw_parts(s.as_ptr().cast(), s.len()) }
}

/// The sum of `accumulatedGPUTime` over a client's `AppUsage` entries, in ns. `None` when
/// the value is not an array (no usage recorded); an empty array is `Some(0)`.
fn usage_ns(v: &CFType, key: &CFString) -> Option<u64> {
    let arr = v.as_CFTypeRef();
    // SAFETY: `arr` is a live CF object (held by `v`).
    if unsafe { CFGetTypeID(arr) } != unsafe { CFArrayGetTypeID() } {
        return None;
    }
    let mut total = 0u64;
    // SAFETY: `arr` is a CFArray.
    let n = unsafe { CFArrayGetCount(arr.cast()) };
    for i in 0..n {
        // SAFETY: `i` is in bounds; the value is borrowed from the array (get rule).
        let item: CFTypeRef = unsafe { CFArrayGetValueAtIndex(arr.cast(), i) };
        // SAFETY: `item` is a live element of the array.
        if item.is_null() || unsafe { CFGetTypeID(item) } != unsafe { CFDictionaryGetTypeID() } {
            continue;
        }
        // SAFETY: `item` is a CFDictionary; the key lives across the call; the value is
        // borrowed (get rule).
        let n: CFTypeRef =
            unsafe { CFDictionaryGetValue(item.cast(), key.as_concrete_TypeRef().cast()) };
        // SAFETY: `n` is a live value of the dictionary.
        if n.is_null() || unsafe { CFGetTypeID(n) } != unsafe { CFNumberGetTypeID() } {
            continue;
        }
        let mut ns: i64 = 0;
        // SAFETY: `n` is a CFNumber; `ns` receives an SInt64.
        let ok = unsafe {
            CFNumberGetValue(n as CFNumberRef, kCFNumberSInt64Type, (&raw mut ns).cast())
        };
        if ok && ns > 0 {
            total = total.saturating_add(ns as u64);
        }
    }
    Some(total)
}

pub struct GpuPerProcess {
    creator: Key,
    usage: Key,
    gpu_time: Key,
    /// The accelerators, looked up on the first sample after a release.
    accelerators: Vec<IoObject>,
    ledger: Ledger,
    last_ns: Option<u64>,
}

impl Default for GpuPerProcess {
    fn default() -> Self {
        Self::new()
    }
}

impl GpuPerProcess {
    pub fn new() -> Self {
        Self {
            creator: Key::new("IOUserClientCreator"),
            usage: Key::new("AppUsage"),
            gpu_time: Key::new("accumulatedGPUTime"),
            accelerators: Vec::new(),
            ledger: Ledger::default(),
            last_ns: None,
        }
    }

    /// Whether anything is held between samples (for tests: nothing while nobody looks).
    pub fn is_open(&self) -> bool {
        !self.accelerators.is_empty() || self.last_ns.is_some()
    }

    /// One pass over every accelerator's clients into the ledger (see [`read_pass`]).
    /// True when it measured. An accelerator whose child iterator cannot be created
    /// fails the pass.
    fn pass(&mut self) -> Result<bool, CollectError> {
        let Self {
            creator,
            usage,
            gpu_time,
            accelerators,
            ledger,
            ..
        } = self;
        read_pass(ledger, |ledger| {
            let mut complete = true;
            for acc in accelerators.iter() {
                let mut children = acc.children().ok_or(CollectError::Os {
                    call: "IORegistryEntryGetChildIterator",
                    code: 0,
                })?;
                for c in children.by_ref() {
                    let Some(pid) = c.property(creator).and_then(|v| creator_pid(&v)) else {
                        continue;
                    };
                    let Some(id) = c.registry_id() else {
                        continue;
                    };
                    match c.property(usage).and_then(|v| usage_ns(&v, gpu_time)) {
                        Some(ns) => ledger.client(id, pid, ns),
                        None => ledger.keep(id),
                    }
                }
                complete &= children.is_valid();
            }
            Ok(complete)
        })
    }
}

impl Collector for GpuPerProcess {
    fn id(&self) -> CollectorId {
        ID
    }

    fn cadence(&self) -> Cadence {
        Cadence::OnDemand(Interest::GpuProcesses)
    }

    fn required_entitlements(&self) -> &'static [Entitlement] {
        &[Entitlement::IoRegistryGpuClients]
    }

    fn modules(&self) -> &'static [Module] {
        &[Module::Gpu]
    }

    fn probe(&mut self) -> Probe {
        // Supported when an accelerator has at least one client that names its creator
        // (Kelvo's own web view is one). Nothing is kept: sampling starts on demand.
        let found = matching_services(ACCELERATOR).iter().any(|acc| {
            acc.children().is_some_and(|mut cs| {
                cs.any(|c| {
                    c.property(&self.creator)
                        .and_then(|v| creator_pid(&v))
                        .is_some()
                })
            })
        });
        if found {
            Probe::Supported(Vec::new())
        } else {
            Probe::Unsupported {
                reason: UnsupportedReason::NoHardware,
            }
        }
    }

    fn sample(&mut self, tick: &Tick, out: &mut SampleBuf) -> Result<(), CollectError> {
        out.reserve_process_gpu(PID_ROOM);
        if self.accelerators.is_empty() {
            self.accelerators = matching_services(ACCELERATOR);
            self.ledger.reset();
            self.last_ns = None;
            if self.accelerators.is_empty() {
                return Err(CollectError::UnexpectedShape {
                    source_name: "IOAccelerator",
                    detail: "no accelerator service",
                });
            }
        }
        let measured = match self.pass() {
            Ok(m) => m,
            Err(e) => {
                // Look the accelerators up again next sample, from a new baseline.
                self.accelerators = Vec::new();
                self.last_ns = None;
                return Err(e);
            }
        };
        let prev = self.last_ns.replace(tick.continuous_ns);
        let (true, Some(prev)) = (measured, prev) else {
            return Ok(());
        };
        out.set_process_gpu_measured();
        shares(
            &self.ledger.per_pid,
            tick.continuous_ns.saturating_sub(prev),
            |pid, pct| out.push_process_gpu(ProcessGpu { pid, pct }),
        );
        Ok(())
    }

    fn release(&mut self) {
        self.accelerators = Vec::new();
        self.ledger.reset();
        self.last_ns = None;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::print_stdout)]
mod tests {
    use core_foundation::array::CFArray;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::number::CFNumber;

    use super::*;

    fn sorted(per_pid: &HashMap<i32, u64>, wall_ns: u64) -> Vec<(i32, f32)> {
        let mut v = Vec::new();
        shares(per_pid, wall_ns, |p, s| v.push((p, s)));
        v.sort_by_key(|r| r.0);
        v
    }

    #[test]
    fn the_first_pass_is_a_baseline_then_clients_add_up_per_pid() {
        let mut l = Ledger::default();
        l.begin();
        l.client(10, 475, 6_000_000_000);
        l.client(11, 475, 1_000_000_000);
        l.client(20, 900, 50);
        assert!(!l.settle(true), "lifetime GPU time is not a share");
        assert!(l.per_pid.is_empty());

        l.begin();
        l.client(10, 475, 6_100_000_000);
        l.client(11, 475, 1_050_000_000);
        l.client(20, 900, 50);
        assert!(l.settle(true));
        assert_eq!(
            l.per_pid,
            HashMap::from([(475, 150_000_000)]),
            "two clients of one process add up; an idle one moves nothing"
        );
    }

    #[test]
    fn a_counter_that_went_back_adds_nothing() {
        let mut l = Ledger::default();
        l.begin();
        l.client(1, 7, 1_000);
        l.client(2, 8, 5_000);
        l.settle(true);

        l.begin();
        l.client(1, 7, 900);
        l.client(2, 8, 5_500);
        assert!(l.settle(true));
        assert_eq!(l.per_pid, HashMap::from([(8, 500)]));
    }

    /// Registry ids are never reused, so a client the last pass did not have opened the
    /// GPU since: all its time is in this interval.
    #[test]
    fn a_client_new_since_the_last_pass_counts_from_zero() {
        let mut l = Ledger::default();
        l.begin();
        l.client(1, 7, 1_000);
        l.settle(true);

        l.begin();
        l.client(1, 7, 1_000);
        l.client(3, 9, 40_000);
        assert!(l.settle(true));
        assert_eq!(l.per_pid, HashMap::from([(9, 40_000)]));
    }

    #[test]
    fn an_idle_client_that_starts_submitting_counts_its_first_interval() {
        let mut l = Ledger::default();
        l.begin();
        l.client(5, 11, 0);
        l.settle(true);

        l.begin();
        l.client(5, 11, 0);
        assert!(l.settle(true));
        assert!(l.per_pid.is_empty(), "idle moves nothing");

        l.begin();
        l.client(5, 11, 25_000);
        assert!(l.settle(true));
        assert_eq!(l.per_pid, HashMap::from([(11, 25_000)]));
    }

    #[test]
    fn a_client_whose_usage_could_not_be_read_is_not_new_next_time() {
        let mut l = Ledger::default();
        l.begin();
        l.client(1, 7, 1_000_000);
        l.settle(true);

        l.begin();
        l.keep(1);
        assert!(l.settle(true));
        assert!(l.per_pid.is_empty());

        l.begin();
        l.client(1, 7, 1_000_500);
        l.settle(true);
        assert_eq!(l.per_pid, HashMap::from([(7, 500)]), "not its lifetime");
    }

    /// One fake walk: clients `(id, pid, ns)` and how many of them it gets to see.
    type FakeWalk = (Vec<(u64, i32, u64)>, usize);

    /// Runs one pass over fake walks, one walk per attempt: each reads the first `seen`
    /// clients and reports whether that was all of them. Asserts every walk was used.
    fn pass(l: &mut Ledger, mut walks: Vec<FakeWalk>) -> bool {
        let measured = read_pass(l, |l| {
            let (clients, seen) = walks.remove(0);
            for &(id, pid, ns) in clients.iter().take(seen) {
                l.client(id, pid, ns);
            }
            Ok(seen == clients.len())
        })
        .unwrap();
        assert!(walks.is_empty(), "walks left over: {walks:?}");
        measured
    }

    fn all(a: u64, b: u64) -> Vec<(u64, i32, u64)> {
        vec![(1, 7, a), (2, 8, b)]
    }

    #[test]
    fn a_walk_cut_short_by_a_registry_change_is_read_again() {
        let mut l = Ledger::default();
        pass(&mut l, vec![(all(1_000, 2_000), 2)]);

        // The first walk ends after one client; the retry sees both.
        assert!(pass(
            &mut l,
            vec![(all(1_400, 2_300), 1), (all(1_500, 2_500), 2)]
        ));
        assert_eq!(
            l.per_pid,
            HashMap::from([(7, 500), (8, 500)]),
            "the retry's counts only, nothing from the cut walk twice"
        );
    }

    #[test]
    fn clients_a_cut_walk_missed_keep_their_counts() {
        let mut l = Ledger::default();
        pass(&mut l, vec![(all(1_000, 9_000_000), 2)]);

        // Cut short both times: client 2 is not seen this pass.
        assert!(pass(
            &mut l,
            vec![(all(1_200, 9_000_100), 1), (all(1_300, 9_000_200), 1)]
        ));
        assert_eq!(l.per_pid, HashMap::from([(7, 300)]));

        // The next complete pass charges client 2 its interval, not its lifetime.
        assert!(pass(&mut l, vec![(all(1_300, 9_000_300), 2)]));
        assert_eq!(l.per_pid, HashMap::from([(8, 300)]));
    }

    /// The first pass as a view opens is cut short twice: it saw only some clients, so
    /// the next pass cannot tell a missed client from a new one and is a baseline too.
    #[test]
    fn a_cut_baseline_does_not_make_missed_clients_look_new() {
        let mut l = Ledger::default();
        assert!(!pass(
            &mut l,
            vec![(all(1_000, 9_000_000), 1), (all(1_000, 9_000_000), 1)]
        ));
        assert!(
            !pass(&mut l, vec![(all(1_100, 9_000_100), 2)]),
            "still a baseline"
        );
        // Nothing is emitted for a baseline; the missed client was not charged either.
        assert!(
            !l.per_pid.contains_key(&8),
            "lifetime charge: {:?}",
            l.per_pid
        );
        assert!(pass(&mut l, vec![(all(1_200, 9_000_300), 2)]));
        assert_eq!(l.per_pid, HashMap::from([(7, 100), (8, 200)]));
    }

    /// Missed clients are kept for one cut pass only: kept forever, ids of clients that
    /// exited would never leave the ledger while walks keep getting cut.
    #[test]
    fn missed_clients_are_kept_for_one_cut_pass_in_a_row() {
        let mut l = Ledger::default();
        let three = |a, b, c| vec![(1, 7, a), (2, 8, b), (3, 9, c)];
        pass(&mut l, vec![(three(1_000, 2_000, 3_000), 3)]);
        // Client 3 exits; client 2 is missed by every cut walk.
        let cut = |a, b| vec![(all(a, b), 1), (all(a, b), 1)];
        assert!(pass(&mut l, cut(1_100, 2_100)));
        assert!(
            l.prev.contains_key(&2) && l.prev.contains_key(&3),
            "kept once"
        );
        assert!(
            pass(&mut l, cut(1_200, 2_200)),
            "the seen clients still measure"
        );
        assert_eq!(l.per_pid, HashMap::from([(7, 100)]));
        assert_eq!(l.prev.len(), 1, "not kept twice: {:?}", l.prev);
        // The ledger no longer knows every client, so the next pass is a baseline.
        assert!(!pass(&mut l, vec![(all(1_300, 9_999_999), 2)]));
        // Nothing is emitted for a baseline; the missed client was not charged either.
        assert!(
            !l.per_pid.contains_key(&8),
            "lifetime charge: {:?}",
            l.per_pid
        );
        assert!(pass(&mut l, vec![(all(1_400, 10_000_099), 2)]));
        assert_eq!(l.per_pid, HashMap::from([(7, 100), (8, 100)]));
    }

    #[test]
    fn a_walk_that_fails_starts_over_from_a_baseline() {
        let mut l = Ledger::default();
        pass(&mut l, vec![(all(1_000, 2_000), 2)]);
        let failed = read_pass(&mut l, |_| {
            Err(CollectError::Os {
                call: "IORegistryEntryGetChildIterator",
                code: 0,
            })
        });
        assert!(failed.is_err());
        assert!(!pass(&mut l, vec![(all(5_000, 6_000), 2)]), "a baseline");
        assert!(l.per_pid.is_empty());
    }

    #[test]
    fn a_reset_starts_over_from_a_baseline() {
        let mut l = Ledger::default();
        l.begin();
        l.client(1, 7, 1_000);
        l.settle(true);
        l.reset();
        l.begin();
        l.client(1, 7, 2_000);
        assert!(!l.settle(true));
        assert!(l.per_pid.is_empty());
    }

    #[test]
    fn shares_are_percent_of_wall_time() {
        let per_pid = HashMap::from([(1, 876_700_000), (2, 106_000_000), (3, 0)]);
        assert_eq!(
            sorted(&per_pid, 1_000_000_000),
            vec![(1, 87.67), (2, 10.6)],
            "spike numbers: gpuload and WindowServer over one second"
        );
    }

    #[test]
    fn shares_never_add_up_to_more_than_the_whole_gpu() {
        // A 1.8 s command buffer finishing inside a 1 s window: the job's process is
        // charged 180%, and a client queued behind it 30%.
        let per_pid = HashMap::from([(1, 1_800_000_000), (2, 300_000_000)]);
        let v = sorted(&per_pid, 1_000_000_000);
        // Clamped to 100 and 30, then scaled by 100/130.
        assert!((v[0].1 - 76.923).abs() < 0.01, "{v:?}");
        assert!((v[1].1 - 23.077).abs() < 0.01, "{v:?}");
        let sum: f32 = v.iter().map(|r| r.1).sum();
        assert!((sum - 100.0).abs() < 0.01);
    }

    #[test]
    fn creator_strings_give_the_pid() {
        assert_eq!(parse_creator(b"pid 475, WindowServer"), Some(475));
        assert_eq!(parse_creator(b"pid 1, launchd"), Some(1));
        assert_eq!(parse_creator(b"pid x, nope"), None);
        assert_eq!(parse_creator(b"WindowServer"), None);
        let cf = CFString::new("pid 31337, com.apple.WebKi");
        assert_eq!(creator_pid(&cf.as_CFType()), Some(31337));
        assert_eq!(creator_pid(&CFNumber::from(3i64).as_CFType()), None);
    }

    /// An `AppUsage` array the way macOS 27 shaped it: one dictionary per command queue.
    fn usage(times: &[i64]) -> CFType {
        let dicts: Vec<CFDictionary<CFString, CFType>> = times
            .iter()
            .map(|&t| {
                CFDictionary::from_CFType_pairs(&[
                    (CFString::new("API"), CFString::new("Metal").as_CFType()),
                    (
                        CFString::new("accumulatedGPUTime"),
                        CFNumber::from(t).as_CFType(),
                    ),
                    (
                        CFString::new("lastSubmittedTime"),
                        CFNumber::from(123i64).as_CFType(),
                    ),
                ])
            })
            .collect();
        CFArray::from_CFTypes(&dicts).as_CFType()
    }

    #[test]
    fn app_usage_sums_every_command_queue() {
        let key = CFString::new("accumulatedGPUTime");
        assert_eq!(usage_ns(&usage(&[1_000, 2_500]), &key), Some(3_500));
        assert_eq!(usage_ns(&usage(&[]), &key), Some(0), "never submitted");
        assert_eq!(usage_ns(&CFString::new("x").as_CFType(), &key), None);
    }

    /// Prints the top processes by GPU share from the live registry. Run with a GPU load
    /// going: `GPU_SECS=2 cargo test -p kelvo-collect live_gpu_top -- --ignored --nocapture`.
    #[test]
    #[ignore = "reads the live IORegistry; run by hand with --ignored --nocapture"]
    fn live_gpu_top() {
        let secs: u64 = std::env::var("GPU_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(2);
        let mut c = GpuPerProcess::new();
        assert!(matches!(c.probe(), Probe::Supported(_)), "no GPU clients");
        let start = std::time::Instant::now();
        let tick = |ns: u64| Tick {
            n: 0,
            wall_ms: 0,
            continuous_ns: ns,
            interval_ms: 1000,
        };
        let mut buf = SampleBuf::new();
        let t0 = std::time::Instant::now();
        c.sample(&tick(1), &mut buf).unwrap();
        println!("baseline pass {:?}", t0.elapsed());
        assert!(!buf.process_gpu_measured());
        std::thread::sleep(std::time::Duration::from_secs(secs));
        buf.clear();
        let t1 = std::time::Instant::now();
        c.sample(&tick(1 + start.elapsed().as_nanos() as u64), &mut buf)
            .unwrap();
        println!("pass {:?}", t1.elapsed());
        assert!(buf.process_gpu_measured());
        let mut rows: Vec<_> = buf.process_gpu().to_vec();
        rows.sort_by(|a, b| b.pct.total_cmp(&a.pct));
        let sum: f32 = rows.iter().map(|r| r.pct).sum();
        println!("sum {sum:.2}% over {secs} s");
        for r in rows.iter().take(10) {
            println!("{:>7} {:6.2}%", r.pid, r.pct);
        }
        c.release();
        assert!(!c.is_open());
    }
}
