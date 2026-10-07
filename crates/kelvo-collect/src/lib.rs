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
//!   Emit only series listed by the last probe. Steady state must not allocate: build
//!   [`SeriesKey`]s in `probe` and push clones of them (a key with up to two short labels
//!   clones without allocating).
//! - Rates (`MetricKind::Rate`) are computed by the collector from its own previous
//!   counters. The first sample after a probe has no previous value and emits no rate.
//! - A missing private API is a capability ([`Probe::Unsupported`]), not an error. A
//!   transient failure returns [`CollectError`]; the engine logs it (rate-limited) and
//!   records no value for the tick.
//! - [`Collector::required_entitlements`] lists what the collector needs; an `appstore`
//!   build drops any collector that needs more than [`Entitlement::None`] (see
//!   [`allowed_entitlements`] and [`filter_by_entitlements`]).
//! - [`Collector::modules`] names the modules the series belong to, so an unsupported or
//!   absent collector still maps to a module state.

pub mod calib;
pub mod calls;
mod process;
pub mod process_control;

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;

use std::fmt;
use std::sync::Arc;

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
}

/// The interests in effect on a tick, for one collector.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Interests {
    pub processes: bool,
    pub detail: bool,
    pub live: bool,
    pub network_processes: bool,
    pub gpu_processes: bool,
}

impl Interests {
    pub fn has(self, i: Interest) -> bool {
        match i {
            Interest::Processes => self.processes,
            Interest::Detail => self.detail,
            Interest::Live => self.live,
            Interest::NetworkProcesses => self.network_processes,
            Interest::GpuProcesses => self.gpu_processes,
        }
    }
}

/// How often the engine samples a collector. Periods are wall-clock minimums measured on
/// the continuous clock, not tick multiples, so they mean the same at every base tick: a
/// collector is sampled at most once per tick, and no more often than its period. When
/// the period is shorter than the base tick it is sampled every tick (D-061).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cadence {
    EveryTick,
    /// At least this many ms apart (and at most once per tick).
    Every(u32),
    /// Sampled only while someone has this interest (per-process network, v1.2). When
    /// the interest ends the engine calls [`Collector::release`].
    OnDemand(Interest),
    /// Every tick while someone has `interest`, otherwise at least `idle_ms` apart
    /// (processes: 10 s idle; IOReport: 10 s in tray-only mode; the per-tick collectors
    /// on [`Interest::Live`]: [`IDLE_MS`] when neither a window nor the menu bar shows
    /// them).
    Adaptive {
        idle_ms: u32,
        interest: Interest,
    },
}

/// The idle period of a collector nobody shows live (D-067): one sample per 10 s history
/// bucket. Counter-derived rates (CPU load, network and disk bytes) cover the whole span
/// since the previous sample, so their S10 and M1 averages stay exact. Gauges (memory,
/// GPU utilization, SMC watts) would get one point per bucket, so they stay on
/// [`Cadence::EveryTick`]; only counter-derived collectors use this (D-070).
pub const IDLE_MS: u32 = 10_000;

/// The cadence of a collector that runs every tick while shown and at [`IDLE_MS`]
/// otherwise.
pub const LIVE_OR_IDLE: Cadence = Cadence::Adaptive {
    idle_ms: IDLE_MS,
    interest: Interest::Live,
};

impl Cadence {
    /// The minimum period right now: `Some(0)` for every tick, `None` for not at all.
    pub fn period_ms(self, interests: Interests) -> Option<u32> {
        match self {
            Cadence::EveryTick => Some(0),
            Cadence::Every(ms) => Some(ms),
            Cadence::OnDemand(i) => interests.has(i).then_some(0),
            Cadence::Adaptive { idle_ms, interest } => {
                Some(if interests.has(interest) { 0 } else { idle_ms })
            }
        }
    }
}

/// Whether `elapsed_ns` since the last sample covers a minimum period of `period_ms` at
/// a base tick of `interval_ms`. Consecutive ticks can be closer than the interval by
/// the timer's leeway (10% of the interval), so a quarter of the smaller of the two is
/// forgiven: at 1 s a 5 s period is due from 4.75 s, at 2 s three ticks after the last
/// sample (6 s), at 30 s on every tick.
pub fn period_elapsed(elapsed_ns: u64, period_ms: u32, interval_ms: u32) -> bool {
    let slack_ms = period_ms.min(interval_ms) / 4;
    let need_ns = u64::from(period_ms.saturating_sub(slack_ms)) * 1_000_000;
    elapsed_ns >= need_ns
}

/// A wall-clock minimum period: the engine keeps one per collector, and a collector
/// keeps one for series it reads less often than it runs (`cpu.loadavg` every 5 s,
/// `fan.max` every 60 s). [`Every::due`] is true on the first call and then once the
/// period has passed on the continuous clock, whatever the base tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Every {
    period_ms: u32,
    last_ns: Option<u64>,
}

impl Every {
    pub const fn new(period_ms: u32) -> Self {
        Self {
            period_ms,
            last_ns: None,
        }
    }

    /// Whether the period has passed at `tick`; if so, `tick` becomes the last sample.
    pub fn due(&mut self, tick: &Tick) -> bool {
        self.due_with(self.period_ms, tick)
    }

    /// [`Every::due`] with a period that varies from call to call (adaptive cadences).
    pub fn due_with(&mut self, period_ms: u32, tick: &Tick) -> bool {
        let due = match self.last_ns {
            None => true,
            Some(last) => period_elapsed(
                tick.continuous_ns.saturating_sub(last),
                period_ms,
                tick.interval_ms,
            ),
        };
        if due {
            self.last_ns = Some(tick.continuous_ns);
        }
        due
    }

    /// Forgets the last sample, so the next call is due (after a probe or a wake).
    pub fn reset(&mut self) {
        self.last_ns = None;
    }

    pub fn period_ms(&self) -> u32 {
        self.period_ms
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

#[cfg(target_os = "macos")]
pub use macos::ifaddrs::interface_addresses;

/// No addresses off macOS until the Linux collectors exist (v4).
#[cfg(not(target_os = "macos"))]
pub fn interface_addresses(_iface: &str) -> IfaceAddrs {
    IfaceAddrs::default()
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
    /// absent from `gpu` used no GPU time.
    gpu_measured: bool,
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

    /// Marks this tick's per-process GPU time as measured: every process not pushed with
    /// [`SampleBuf::push_process_gpu`] used none. Not called on a baseline sample.
    pub fn set_process_gpu_measured(&mut self) {
        self.gpu_measured = true;
    }

    pub fn process_gpu_measured(&self) -> bool {
        self.gpu_measured
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
        self.gpu_measured = false;
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
    /// Called once at start and on capability-change hints. Reports which series the
    /// collector can produce.
    fn probe(&mut self) -> Probe;
    /// Appends values for this tick. Must not allocate per call in steady state.
    fn sample(&mut self, tick: &Tick, out: &mut SampleBuf) -> Result<(), CollectError>;
    /// The [`Cadence::OnDemand`] interest this collector is sampled on has ended (the last
    /// window showing it hid). Free whatever sampling holds open, so nobody pays for it
    /// until the interest returns; the next sample starts over from a baseline. Default:
    /// nothing to free.
    fn release(&mut self) {}
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
