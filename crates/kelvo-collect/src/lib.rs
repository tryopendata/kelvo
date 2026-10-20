#![cfg_attr(test, allow(clippy::too_many_lines))]
//! Metric collectors: the [`Collector`] trait and one collector set per OS
//! (`macos/` for sysinfo, IOReport, SMC/HID and IOKit; `linux/` is a stub until v4).
//!
//! Depends on `kelvo-schema` only. Reads the OS; knows nothing about storage, timers
//! or the UI. The engine owns the single timer and calls [`Collector::sample`] on the
//! ticks the collector's [`Cadence`] selects (architecture.md, infra 6).
//!
//! # Contract for collector authors
//!
//! - [`Collector::probe`] runs once at start and again on capability hints. It returns the
//!   exact series the collector will emit, or why it cannot run.
//! - [`Collector::sample`] appends values to a [`SampleBuf`] the engine clears and reuses.
//!   Emit only series listed by the last probe; values for any other series are dropped.
//!   Steady state must not allocate: build [`SeriesKey`]s in `probe` and push clones of
//!   them (a key with up to two short labels clones without allocating).
//! - A collector whose series set changes at runtime (an interface or container came or
//!   went) bumps [`Collector::series_revision`] in `sample`. The engine then re-probes that
//!   collector alone before the next tick and rebuilds the layout; the module's other
//!   collectors are not re-probed and keep their baselines.
//! - Rates (`MetricKind::Rate`) are computed by the collector from its own previous
//!   counters. The first sample after a hint or start probe has no previous value and
//!   emits no rate. A probe for the collector's own revision may keep its counters, so
//!   series that stayed keep their rates.
//! - A missing private API is a capability ([`Probe::Unsupported`]), not an error. A
//!   transient failure returns [`CollectError`]; the engine logs it (rate-limited) and
//!   records no value for the tick.
//! - [`Collector::required_entitlements`] lists what the collector needs; an `appstore`
//!   build drops any collector that needs more than [`Entitlement::None`] (see
//!   [`allowed_entitlements`] and [`filter_by_entitlements`]).
//! - [`Collector::modules`] names the modules the series belong to, so an unsupported or
//!   absent collector still maps to a module state.

mod cadence;
pub mod calib;
pub mod calls;
pub mod host_info;
mod process;
pub mod process_control;

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;

use std::fmt;
use std::sync::Arc;

pub use cadence::{Cadence, Every, IDLE_MS, LIVE_OR_IDLE, period_elapsed};
pub use kelvo_schema::{Entitlement, Module, SeriesKey, UnsupportedReason};
pub use process::{ProcessGpu, ProcessNet, ProcessSample};

/// One engine tick, passed to every collector sampled on it.
///
/// Defined here rather than in `kelvo-engine` because collectors read it and the crate
/// graph runs collect -> engine. The engine re-exports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tick {
    /// Tick counter since the engine started, 0-based. Cadences do not use it: they are
    /// wall-clock periods on `continuous_ns` (see [`Every`]).
    pub n: u64,
    /// Wall-clock millisecond epoch at the tick.
    pub wall_ms: i64,
    /// Monotonic nanoseconds that keep advancing during sleep (`mach_continuous_time`
    /// on macOS, `CLOCK_BOOTTIME` on Linux). Rate collectors divide by its delta, and
    /// [`Every`] measures periods on it.
    pub continuous_ns: u64,
    /// The base tick in effect, in ms (the user's interval, or the battery back-off). The
    /// engine sets it; [`Every`] uses it to tolerate timer jitter.
    pub interval_ms: u32,
}

/// Stable identifier of a collector, used in logs, rate limiting and capability
/// bookkeeping. A string newtype rather than an enum so each platform module and each
/// collector owns its constant without touching a shared list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CollectorId(pub &'static str);

/// The temperature collectors (SMC sensors, HID thermal), named here so code that
/// treats them as a group compiles on every OS. The macOS modules use these as their IDs.
pub const TEMPERATURE_COLLECTORS: [CollectorId; 2] =
    [CollectorId("smc.sensors"), CollectorId("hid.thermal")];

/// How often the temperature collectors sample (D-055): each read costs about 0.1 ms of
/// kernel time per SMC key and 3 ms for all HID services.
pub const TEMPERATURE_PERIOD_MS: u32 = 5_000;

impl CollectorId {
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

impl fmt::Display for CollectorId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

/// Who wants a collector's data right now. Windows register interest through the engine;
/// with only the tray open, neither is set ("tray-only mode", D-061).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interest {
    /// A visible window shows process rows (the Processes page, the popover's top list).
    Processes,
    /// A visible window shows detail the tray does not: cluster frequency and residency,
    /// GPU states, component power.
    Detail,
    /// Someone shows this collector's values as they change: a visible window, or the
    /// menu bar showing one of the collector's modules. The engine decides it per
    /// collector (D-067).
    Live,
    /// A visible window shows per-process network rates (D-081). On the ticks process
    /// rows are sampled, so the rates cover the same span as the rows they join.
    NetworkProcesses,
    /// A visible window shows per-process GPU time (v1.2). Sampled on the process ticks,
    /// like [`Interest::NetworkProcesses`].
    GpuProcesses,
    /// A visible window shows the TCP ports processes listen on. Sampled on the process
    /// ticks, after the processes collector, whose rows it fills in.
    PortProcesses,
}

/// The interests in effect on a tick, for one collector.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Interests {
    pub processes: bool,
    pub detail: bool,
    pub live: bool,
    pub network_processes: bool,
    pub gpu_processes: bool,
    pub port_processes: bool,
}

impl Interests {
    pub fn has(self, i: Interest) -> bool {
        match i {
            Interest::Processes => self.processes,
            Interest::Detail => self.detail,
            Interest::Live => self.live,
            Interest::NetworkProcesses => self.network_processes,
            Interest::GpuProcesses => self.gpu_processes,
            Interest::PortProcesses => self.port_processes,
        }
    }
}

/// What a probe found.
#[derive(Clone, Debug, PartialEq)]
pub enum Probe {
    /// The collector will emit exactly these series. May be empty when the hardware
    /// exists but has nothing to report right now (no active interface).
    Supported(Vec<SeriesKey>),
    /// A collector exists for this hardware but cannot run.
    Unsupported { reason: UnsupportedReason },
    /// The host does not have this hardware (no battery on a Mac mini, zero fans). Maps
    /// to `ModuleCap::NotPresent`.
    NotPresent,
}

/// A transient sampling failure. The engine logs it rate-limited and records no value
/// for the tick; it never takes the sampler down.
#[derive(Debug, thiserror::Error)]
pub enum CollectError {
    /// An OS call returned an error code (`kern_return_t`, errno, `IOReturn`).
    #[error("{call} failed with code {code}")]
    Os { call: &'static str, code: i64 },
    /// The source answered in a shape the collector does not understand.
    #[error("unexpected data from {source_name}: {detail}")]
    UnexpectedShape {
        source_name: &'static str,
        detail: &'static str,
    },
    /// An asynchronous OS call did not report back within the collector's wait.
    #[error("{call} did not complete within the timeout")]
    Timeout { call: &'static str },
    /// `sample` was called without a successful probe.
    #[error("collector was not probed")]
    NotProbed,
}

/// One series value for this tick.
#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    pub key: SeriesKey,
    pub value: f32,
}

/// A span of the continuous clock (`Tick::continuous_ns`) that a batch of counter
/// deltas covers: from the previous sample (`prev_ns`, inclusive) to this one
/// (`now_ns`, exclusive).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Interval {
    pub prev_ns: u64,
    pub now_ns: u64,
}

/// Exact interface counter deltas over one interval, summed over the interfaces the
/// network collector reports as series (Wi-Fi, Ethernet, cellular). A side channel
/// next to the `net.rx`/`net.tx` rates, because history needs integer totals the engine
/// can split across buckets and add up, and `f32` rates do not round-trip bytes. Bytes
/// from an unprivileged `NET_RT_IFLIST2` are 1 KiB-granular (see the network collector);
/// packets are exact.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IfaceNet {
    pub interval: Interval,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_packets: u64,
    pub tx_packets: u64,
}

/// An interface's addresses ([`interface_addresses`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IfaceAddrs {
    pub ipv4: Vec<std::net::Ipv4Addr>,
    /// Link-local addresses are left out.
    pub ipv6: Vec<std::net::Ipv6Addr>,
}

#[cfg(target_os = "linux")]
pub use linux::ifaddrs::{egress_interface, interface_addresses};
#[cfg(target_os = "macos")]
pub use macos::ifaddrs::{egress_interface, interface_addresses};

/// No addresses on other platforms.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn interface_addresses(_iface: &str) -> IfaceAddrs {
    IfaceAddrs::default()
}

/// No route lookup on other platforms.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn egress_interface() -> Option<String> {
    None
}

/// The reported interface carrying the default route (D-092): the network collector's
/// `iface` label value, or `None` when the route is on an interface it does not report
/// (a VPN tunnel) or there is none. Shared, so passing it on does not allocate.
pub type PrimaryIface = Option<Arc<str>>;

/// Per-tick output buffer. The engine owns one, clears it before each tick and reuses
/// its capacity, so pushing does not allocate once the buffer has grown to its working
/// size.
#[derive(Debug, Default)]
pub struct SampleBuf {
    values: Vec<Sample>,
    processes: Vec<ProcessSample>,
    net: Vec<ProcessNet>,
    /// Set when the per-process network collector measured this tick (it did not just
    /// set its baseline): the span `net` covers, and a process absent from it had no
    /// traffic.
    net_interval: Option<Interval>,
    /// The network collector's interface totals this tick, when it measured.
    iface_net: Option<IfaceNet>,
    /// The primary interface, when the network collector re-read it this tick.
    primary_iface: Option<PrimaryIface>,
    gpu: Vec<ProcessGpu>,
    /// The per-process GPU collector measured this tick (not a baseline), so a process
    /// absent from `gpu` used no GPU time: the wall time its shares are of, ms.
    gpu_span_ms: Option<u64>,
}

impl SampleBuf {
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends one series value. Non-finite values are dropped: a missing value is a
    /// missing push, never a NaN the store would have to interpret.
    pub fn push(&mut self, key: &SeriesKey, value: f32) {
        if value.is_finite() {
            self.values.push(Sample {
                key: key.clone(),
                value,
            });
        }
    }

    /// Appends one process row (processes collector only).
    pub fn push_process(&mut self, row: ProcessSample) {
        self.processes.push(row);
    }

    /// Marks this tick's per-process network traffic as measured over `interval`: every
    /// process not pushed with [`SampleBuf::push_process_net`] had no traffic. Not
    /// called on a baseline sample, so rows then carry no rate.
    pub fn set_process_net_measured(&mut self, interval: Interval) {
        self.net_interval = Some(interval);
    }

    pub fn process_net_measured(&self) -> bool {
        self.net_interval.is_some()
    }

    /// The span this tick's per-process network entries cover, when measured.
    pub fn process_net_interval(&self) -> Option<Interval> {
        self.net_interval
    }

    /// Records the interface totals (network collector only).
    pub fn set_iface_net(&mut self, totals: IfaceNet) {
        self.iface_net = Some(totals);
    }

    /// The interface totals measured this tick, if the network collector measured.
    pub fn iface_net(&self) -> Option<IfaceNet> {
        self.iface_net
    }

    /// Records the primary interface (network collector only, on probe and its 60 s
    /// link-rate cadence).
    pub fn set_primary_iface(&mut self, iface: PrimaryIface) {
        self.primary_iface = Some(iface);
    }

    /// The primary interface, if the network collector re-read it this tick.
    pub fn primary_iface(&self) -> Option<&PrimaryIface> {
        self.primary_iface.as_ref()
    }

    /// Makes room for `n` network rows. The buffer keeps its capacity across ticks, so
    /// calling this on every sample allocates once; without it the first tick with
    /// traffic, or the first with more busy processes than ever before, would allocate.
    pub fn reserve_process_net(&mut self, n: usize) {
        self.net.reserve(n);
    }

    /// Appends one process's network rates (per-process network collector only).
    pub fn push_process_net(&mut self, row: ProcessNet) {
        self.net.push(row);
    }

    /// Per-process network rates pushed this tick, sorted by pid after
    /// [`SampleBuf::sort_process_net`].
    pub fn process_net(&self) -> &[ProcessNet] {
        &self.net
    }

    /// Sorts the network rates by pid, for [`SampleBuf::net_of`].
    pub fn sort_process_net(&mut self) {
        self.net.sort_unstable_by_key(|n| n.pid);
    }

    /// The network rates pushed for `pid` this tick. Needs [`SampleBuf::sort_process_net`]
    /// first.
    pub fn net_of(&self, pid: i32) -> Option<&ProcessNet> {
        let i = self.net.binary_search_by_key(&pid, |n| n.pid).ok()?;
        self.net.get(i)
    }

    /// Marks this tick's per-process GPU time as measured over the last `span_ms` of wall
    /// time: every process not pushed with [`SampleBuf::push_process_gpu`] used none. Not
    /// called on a baseline sample.
    pub fn set_process_gpu_measured(&mut self, span_ms: u64) {
        self.gpu_span_ms = Some(span_ms);
    }

    pub fn process_gpu_measured(&self) -> bool {
        self.gpu_span_ms.is_some()
    }

    /// The wall time this tick's GPU shares are of, ms (D-099): since the collector's
    /// previous pass, which is longer than the process interval when GPU joins only some
    /// process samples. `None` when GPU was not measured.
    pub fn process_gpu_span_ms(&self) -> Option<u64> {
        self.gpu_span_ms
    }

    /// Makes room for `n` GPU rows; see [`SampleBuf::reserve_process_net`].
    pub fn reserve_process_gpu(&mut self, n: usize) {
        self.gpu.reserve(n);
    }

    /// Appends one process's GPU share (per-process GPU collector only).
    pub fn push_process_gpu(&mut self, row: ProcessGpu) {
        self.gpu.push(row);
    }

    /// Per-process GPU shares pushed this tick, sorted by pid after
    /// [`SampleBuf::sort_process_gpu`].
    pub fn process_gpu(&self) -> &[ProcessGpu] {
        &self.gpu
    }

    /// Sorts the GPU shares by pid, for [`SampleBuf::gpu_of`].
    pub fn sort_process_gpu(&mut self) {
        self.gpu.sort_unstable_by_key(|g| g.pid);
    }

    /// The GPU share pushed for `pid` this tick. Needs [`SampleBuf::sort_process_gpu`]
    /// first.
    pub fn gpu_of(&self, pid: i32) -> Option<&ProcessGpu> {
        let i = self.gpu.binary_search_by_key(&pid, |g| g.pid).ok()?;
        self.gpu.get(i)
    }

    pub fn values(&self) -> &[Sample] {
        &self.values
    }

    pub fn processes(&self) -> &[ProcessSample] {
        &self.processes
    }

    /// The rows pushed this tick, for a collector that fills in a field of each (the
    /// ports collector).
    pub fn processes_mut(&mut self) -> &mut [ProcessSample] {
        &mut self.processes
    }

    /// Takes the process rows, leaving an empty vector with room for as many again plus
    /// an eighth.
    pub fn take_processes(&mut self) -> Vec<ProcessSample> {
        // The rows leave with the batch; size the next one like this one so it fills
        // without regrowing. The headroom covers processes that start between samples:
        // at exactly this size, any sample with one more row than the last reallocated.
        let cap = self.processes.len() + self.processes.len() / 8;
        std::mem::replace(&mut self.processes, Vec::with_capacity(cap))
    }

    /// Clears values and process rows, keeping capacity.
    pub fn clear(&mut self) {
        self.values.clear();
        self.processes.clear();
        self.net.clear();
        self.net_interval = None;
        self.iface_net = None;
        self.primary_iface = None;
        self.gpu.clear();
        self.gpu_span_ms = None;
    }

    /// The value pushed for `key` this tick, if any. Linear; meant for tests and tools.
    pub fn get(&self, key: &SeriesKey) -> Option<f32> {
        self.values.iter().find(|s| &s.key == key).map(|s| s.value)
    }
}

/// A source of series values. See the crate docs for the contract.
pub trait Collector: Send + 'static {
    fn id(&self) -> CollectorId;
    fn cadence(&self) -> Cadence;
    /// What the collector needs to run. `&[Entitlement::None]` for public APIs.
    fn required_entitlements(&self) -> &'static [Entitlement];
    /// The modules this collector's series belong to. The engine uses it to turn
    /// [`Probe::Unsupported`] and [`Probe::NotPresent`] (which carry no series) into a
    /// `ModuleCap` for each module. Defaults to none so adding it did not break existing
    /// collectors; every collector should override it.
    fn modules(&self) -> &'static [Module] {
        &[]
    }
    /// Called once at start, on capability-change hints and after
    /// [`Collector::series_revision`] changed. Reports which series the collector can
    /// produce.
    fn probe(&mut self) -> Probe;
    /// Bumped (any change counts) when the series the last probe listed no longer match
    /// what the collector would emit. Read after every `sample`, so it must be a field
    /// read. Default: the series set never changes between hint probes.
    fn series_revision(&self) -> u64 {
        0
    }
    /// Appends values for this tick. Must not allocate per call in steady state.
    fn sample(&mut self, tick: &Tick, out: &mut SampleBuf) -> Result<(), CollectError>;
    /// The [`Cadence::OnDemand`] interest this collector is sampled on has ended (the last
    /// window showing it hid). Free whatever sampling holds open, so nobody pays for it
    /// until the interest returns; the next sample starts over from a baseline. Default:
    /// nothing to free.
    fn release(&mut self) {}
    /// While probed Supported, sets the capability facts this collector owns beyond its
    /// series: `process_energy`/`process_wakeups`, `disk_devices` (D-115). Default: none.
    fn fill_capabilities(&self, _caps: &mut kelvo_schema::Capabilities) {}
}

/// The entitlements this build may hold. An `appstore` build (sandboxed) allows only
/// [`Entitlement::None`]; every other build allows everything.
pub fn allowed_entitlements() -> &'static [Entitlement] {
    if cfg!(feature = "appstore") {
        &[Entitlement::None]
    } else {
        &[
            Entitlement::None,
            Entitlement::IoReport,
            Entitlement::SmcUserClient,
            Entitlement::HidSensors,
            Entitlement::NetworkStatistics,
            Entitlement::IoRegistryGpuClients,
        ]
    }
}

/// Replaces every collector that needs an entitlement outside `allowed` with a
/// [`Denied`] stand-in that probes `Unsupported { reason: MissingEntitlement }` and never
/// samples, so the engine reports "not available in this edition" through the normal
/// capabilities path. Collectors that are allowed pass through unchanged, in order.
pub fn filter_by_entitlements(
    collectors: Vec<Box<dyn Collector>>,
    allowed: &[Entitlement],
) -> Vec<Box<dyn Collector>> {
    collectors
        .into_iter()
        .map(|c| {
            if c.required_entitlements()
                .iter()
                .all(|e| allowed.contains(e))
            {
                c
            } else {
                Box::new(Denied {
                    id: c.id(),
                    cadence: c.cadence(),
                    entitlements: c.required_entitlements(),
                    modules: c.modules(),
                }) as Box<dyn Collector>
            }
        })
        .collect()
}

/// Stand-in for a collector this build may not run (see [`filter_by_entitlements`]).
/// The real collector is dropped before it ever probes, so no private API is touched.
pub struct Denied {
    id: CollectorId,
    cadence: Cadence,
    entitlements: &'static [Entitlement],
    modules: &'static [Module],
}

impl Collector for Denied {
    fn id(&self) -> CollectorId {
        self.id
    }

    fn cadence(&self) -> Cadence {
        self.cadence
    }

    fn required_entitlements(&self) -> &'static [Entitlement] {
        self.entitlements
    }

    fn modules(&self) -> &'static [Module] {
        self.modules
    }

    fn probe(&mut self) -> Probe {
        Probe::Unsupported {
            reason: UnsupportedReason::MissingEntitlement,
        }
    }

    fn sample(&mut self, _tick: &Tick, _out: &mut SampleBuf) -> Result<(), CollectError> {
        Err(CollectError::NotProbed)
    }
}

/// Every collector this platform has, unfiltered. Callers pass the result through
/// [`filter_by_entitlements`] with [`allowed_entitlements`]. Learned power calibration
/// scales persist through `scales` ([`calib::ScaleStore`]).
pub fn platform_collectors(scales: Arc<dyn calib::ScaleStore>) -> Vec<Box<dyn Collector>> {
    #[cfg(target_os = "macos")]
    {
        macos::collectors(scales)
    }
    #[cfg(target_os = "linux")]
    {
        drop(scales);
        linux::collectors()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        drop(scales);
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use kelvo_schema::{Labels, MetricId};

    use super::*;

    fn tick(at_ms: u64, interval_ms: u32) -> Tick {
        Tick {
            n: 0,
            wall_ms: 0,
            continuous_ns: at_ms * 1_000_000,
            interval_ms,
        }
    }

    #[test]
    fn reserved_process_rows_survive_clear() {
        let mut buf = SampleBuf::new();
        buf.reserve_process_net(64);
        buf.reserve_process_gpu(64);
        let (net, gpu) = (buf.net.capacity(), buf.gpu.capacity());
        assert!(net >= 64 && gpu >= 64);
        for _ in 0..3 {
            buf.clear();
            buf.reserve_process_net(64);
            buf.reserve_process_gpu(64);
            for pid in 0..64 {
                buf.push_process_net(ProcessNet {
                    pid,
                    identity: None,
                    rx_bytes: 1,
                    tx_bytes: 1,
                    rx_bps: 1.0,
                    tx_bps: 1.0,
                    late_rx_bytes: 0,
                    late_tx_bytes: 0,
                });
                buf.push_process_gpu(ProcessGpu { pid, pct: 1.0 });
            }
            assert_eq!((buf.net.capacity(), buf.gpu.capacity()), (net, gpu));
        }
    }

    #[test]
    fn cadence_periods() {
        let none = Interests::default();
        let detail = Interests {
            detail: true,
            ..none
        };
        let procs = Interests {
            processes: true,
            ..none
        };
        assert_eq!(Cadence::EveryTick.period_ms(none), Some(0));
        assert_eq!(Cadence::Every(5_000).period_ms(none), Some(5_000));
        assert_eq!(Cadence::OnDemand(Interest::Detail).period_ms(none), None);
        assert_eq!(
            Cadence::OnDemand(Interest::Detail).period_ms(detail),
            Some(0)
        );
        let a = Cadence::Adaptive {
            idle_ms: 10_000,
            interest: Interest::Processes,
        };
        assert_eq!(a.period_ms(detail), Some(10_000), "another interest");
        assert_eq!(a.period_ms(procs), Some(0));
    }

    /// Sample times (ms) of an [`Every`] over `ticks` ticks at `interval_ms`, where every
    /// odd tick is `late_ms` late (timer leeway).
    fn sampled(period_ms: u32, interval_ms: u32, ticks: u64, late_ms: u64) -> Vec<u64> {
        let mut e = Every::new(period_ms);
        (0..ticks)
            .map(|n| n * u64::from(interval_ms) + if n % 2 == 1 { late_ms } else { 0 })
            .filter(|&at| e.due(&tick(at, interval_ms)))
            .collect::<Vec<_>>()
    }

    #[test]
    fn every_is_a_wall_clock_minimum_at_any_interval() {
        // 5 s at 1 s: every 5th tick, also when the ticks in between run late.
        assert_eq!(sampled(5_000, 1_000, 16, 0), [0, 5_000, 10_000, 15_000]);
        assert_eq!(sampled(5_000, 1_000, 11, 90), [0, 5_090, 10_000]);
        // 5 s at 0.5 s: every 10th tick.
        assert_eq!(sampled(5_000, 500, 21, 40), [0, 5_000, 10_000]);
        // 5 s at 2 s: never much closer than the period, so every third tick.
        assert_eq!(sampled(5_000, 2_000, 7, 0), [0, 6_000, 12_000]);
        // Periods at or under the interval: every tick.
        assert_eq!(sampled(5_000, 30_000, 3, 0), [0, 30_000, 60_000]);
        assert_eq!(sampled(60_000, 60_000, 3, 0), [0, 60_000, 120_000]);
        // 60 s at 30 s: every other tick. 60 s at 1 s: every 60th.
        assert_eq!(sampled(60_000, 30_000, 5, 0), [0, 60_000, 120_000]);
        assert_eq!(sampled(60_000, 1_000, 121, 0), [0, 60_000, 120_000]);
    }

    #[test]
    fn every_resets_and_varies() {
        let mut e = Every::new(10_000);
        assert!(e.due(&tick(0, 1_000)));
        assert!(!e.due(&tick(1_000, 1_000)));
        assert!(e.due_with(0, &tick(2_000, 1_000)), "interest: every tick");
        assert!(!e.due(&tick(3_000, 1_000)));
        e.reset();
        assert!(e.due(&tick(4_000, 1_000)));
    }

    #[test]
    fn sample_buf_drops_non_finite_and_keeps_capacity() {
        let key = SeriesKey::new(
            MetricId::from_static("cpu.load"),
            Labels::single("core", "P0"),
        );
        let mut buf = SampleBuf::new();
        buf.push(&key, 12.5);
        buf.push(&key, f32::NAN);
        buf.push(&key, f32::INFINITY);
        assert_eq!(buf.values().len(), 1);
        assert_eq!(buf.get(&key), Some(12.5));
        let cap = buf.values.capacity();
        buf.clear();
        assert!(buf.values().is_empty());
        assert_eq!(buf.values.capacity(), cap);
    }
}

#[cfg(unix)]
pub mod disk_space;
#[cfg(unix)]
pub mod docker;
pub mod docker_storage;
