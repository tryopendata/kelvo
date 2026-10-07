//! The sampler: one thread, one timer, every collector (architecture.md, Engine).
//!
//! Each tick the engine samples the collectors whose cadence is due, assembles one frame
//! in the current layout (`NaN` for series not sampled this tick), updates the
//! latest-value cache, publishes the frame through the host's live hub (ring buffer and
//! bus), and feeds the
//! persisted series to the S10 and M1 accumulators, whose closed buckets go to the store
//! writer.
//!
//! Per-app network bytes (D-089) take their own path: each tick's per-process and
//! interface byte totals go to the `NetAppAcc`, whose closed 10 s buckets go to the
//! store writer and to the hub's last-hour ring.
//!
//! Inputs arrive on one queue ([`Inbox`]) and are handled in order: ticks, sleep and wake,
//! device hints, and commands (pause, settings, shutdown). Tests drive the same code
//! without a thread through [`Engine::pump`].

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

use crossbeam_channel::Receiver;
use kelvo_collect::{
    Cadence, Collector, Every, Interest, Interests, Probe, ProcessSample, SampleBuf, Tick,
};
use kelvo_schema::settings::SamplingSettings;
use kelvo_schema::{
    AlertSettings, Capabilities, Catalog, DetectorThresholds, Event, Gap, GapReason, HostId,
    MetricKind, Module, ModuleCap, PerformanceReason, PowerSource, SeriesKey, Settings, Tier,
};
use kelvo_store::{BucketRow, ProcRow, Writer};

use crate::bus::{BusMsg, EngineStatus, FrameLayout, LiveFrame, ProcessBatch};
use crate::clock::ClockReading;
use crate::detect::Detectors;
use crate::hints::DeviceHints;
use crate::inbox::{Command, Inbox, Input, SleepAck};
use crate::netacc::{Anchor, NET_BUCKET_MS, NetAppAcc, NetSlot};
use crate::power::{PowerEvent, PowerSignals, PowerState};
use crate::source::SourceSink;
use crate::ticker::{Ticker, leeway_for};

/// Base tick while backed off (battery with "slow down on battery", or Low Power Mode):
/// double the user's interval, capped at the longest interval Settings offers (D-061).
pub fn backoff_interval_ms(interval_ms: u32) -> u32 {
    interval_ms
        .saturating_mul(2)
        .min(SamplingSettings::MAX_INTERVAL_MS)
        .max(interval_ms)
}

/// The base tick for these settings and power state: [`backoff_interval_ms`] on battery
/// with "slow down on battery" or Performance mode on, and in Low Power Mode; otherwise
/// the user's interval.
pub fn effective_interval_ms(
    interval_ms: u32,
    slow_on_battery: bool,
    performance_mode: bool,
    power: PowerState,
) -> u32 {
    let base = interval_ms.max(100);
    if (power.on_battery && (slow_on_battery || performance_mode)) || power.low_power_mode {
        backoff_interval_ms(base)
    } else {
        base
    }
}

/// The slowest base tick the engine runs while backgrounded: no visible window shows
/// detail, only the menu bar (D-094).
pub const BACKGROUND_TICK_MS: u32 = 2_000;

/// The base tick while backgrounded, from [`effective_interval_ms`]: at least
/// [`BACKGROUND_TICK_MS`]. It replaces a back-off rather than stacking on it, so a 1 s
/// interval backed off on battery stays at 2 s.
pub fn background_interval_ms(effective_ms: u32) -> u32 {
    effective_ms.max(BACKGROUND_TICK_MS)
}

/// A value stays current in [`LiveFrame::held`] for this many sampling intervals, as a
/// fraction (5/2 = 2.5): one missed sample is tolerated, two are a gap.
pub const STALE_NUM: i64 = 5;
pub const STALE_DEN: i64 = 2;

/// Ticks of continuous time without a tick, and without a sleep event, after which the
/// engine records the hole as a `sleep` gap (the process was suspended, or a sleep
/// notification was missed). Fewer missing ticks are just skipped ticks.
const STALL_TICKS: i64 = 5;

/// A wall clock that moved this many base ticks more (or less) than the continuous
/// clock between two ticks was stepped (NTP, a manual change, a time zone bug), not
/// merely late (D-064). At least [`CLOCK_STEP_MIN_MS`], so ordinary NTP slews at a fast
/// interval are not steps.
const CLOCK_STEP_TICKS: i64 = 2;

/// The smallest wall-clock jump that counts as a step, whatever the interval.
const CLOCK_STEP_MIN_MS: i64 = 2_000;

/// How far the wall clock may move beyond the continuous clock before it was stepped.
fn clock_step_threshold(interval_ms: i64) -> i64 {
    (CLOCK_STEP_TICKS * interval_ms).max(CLOCK_STEP_MIN_MS)
}

/// The longest the engine holds persistence after the wall clock stepped back (D-070).
/// Rows the old clock wrote beyond it are discarded, so a clock that ran months ahead
/// does not stop history for months.
const MAX_PERSIST_HOLD_MS: i64 = 3_600_000;

const MINUTE_MS: i64 = 60_000;

/// Series not gated by a module switch (catalog: "nothing gates it on the CPU module").
const UNGATED: &[&str] = &["self.cpu"];

/// Top processes kept per snapshot (architecture.md, Store).
const PROC_SNAPSHOT_TOP: usize = 30;

/// A per-app network bucket closes once both byte streams have reported past its end, or
/// this many times the slower of the base tick and the idle period after its end,
/// whichever is first: a stream that stopped (module off, collector backing off) does
/// not hold buckets open.
const NET_CLOSE_PERIODS: i64 = 3;

fn net_close_grace(interval_ms: u32) -> i64 {
    NET_CLOSE_PERIODS * i64::from(interval_ms.max(kelvo_collect::IDLE_MS))
}

/// Collector errors are logged at most once per this span per collector.
const ERROR_LOG_EVERY_MS: i64 = 60_000;

/// What the engine needs from settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineSettings {
    /// Base tick chosen by the user, one of `SamplingSettings::INTERVALS_MS`.
    pub interval_ms: u32,
    pub slow_on_battery: bool,
    /// Modules switched off. Settings has one switch for Power and Sensors, so turning
    /// off Power puts both here.
    pub disabled: BTreeSet<Module>,
    /// Modules the menu bar shows a value from ([`Settings::menu_bar_shows`]). Their
    /// `Interest::Live` collectors run every tick with no window open (D-067).
    pub menu_bar: BTreeSet<Module>,
    /// The built-in alert rules switched on.
    pub alerts: AlertSettings,
    /// The Performance mode setting. Low Power Mode turns the mode on as well; the
    /// engine resolves both at tick time ([`PerformanceReason::resolve`], D-088).
    pub performance_mode: bool,
}

impl EngineSettings {
    pub fn from_settings(s: &Settings) -> Self {
        let disabled = Module::ALL
            .into_iter()
            .filter(|&m| s.module(m).is_some_and(|ms| !ms.enabled))
            .collect();
        let menu_bar = Module::ALL
            .into_iter()
            .filter(|&m| s.menu_bar_shows(m))
            .collect();
        Self {
            interval_ms: s.sampling.interval_ms,
            slow_on_battery: s.sampling.slow_on_battery,
            disabled,
            menu_bar,
            alerts: s.alerts,
            performance_mode: s.sampling.performance_mode,
        }
    }
}

/// Everything platform-specific the engine runs on.
pub struct EngineParts {
    pub collectors: Vec<Box<dyn Collector>>,
    pub ticker: Box<dyn Ticker>,
    pub power: Box<dyn PowerSignals>,
    pub hints: Option<Box<dyn DeviceHints>>,
}

impl EngineParts {
    /// This platform's collectors (filtered by the build's entitlements), timer, power
    /// signals and device hints. Learned CPU power scales persist through `scales`
    /// (D-065).
    pub fn platform(scales: Arc<dyn kelvo_collect::calib::ScaleStore>) -> Self {
        let collectors = kelvo_collect::filter_by_entitlements(
            kelvo_collect::platform_collectors(scales),
            kelvo_collect::allowed_entitlements(),
        );
        #[cfg(target_os = "macos")]
        {
            Self {
                collectors,
                ticker: Box::new(crate::macos::GcdTicker::new()),
                power: Box::new(crate::macos::MacPowerSignals::new()),
                hints: Some(Box::new(crate::macos::IoKitDeviceHints::new())),
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            Self {
                collectors,
                ticker: Box::new(crate::ticker::ThreadTicker::new()),
                power: Box::new(crate::power::NoPowerSignals),
                hints: None,
            }
        }
    }
}

/// `Shared::process_period` when no window wants process rows.
const NO_PROCESS_INTEREST: u32 = u32::MAX;

/// Performance mode (D-088) and the background (D-094): the process collector's period
/// while no window wants process rows (10 s otherwise).
pub const PERFORMANCE_IDLE_PROCESS_MS: u32 = 30_000;
/// The background (D-094): the temperature collectors' period while no window shows
/// detail, a temperature in the menu bar included (5 s otherwise).
pub const PERFORMANCE_IDLE_SENSOR_MS: u32 = 10_000;
/// Performance mode: the shortest process-row period a visible window gets.
pub const PERFORMANCE_VISIBLE_MS: u32 = 2_000;

/// What Performance mode, or the background, slows a collector down for (D-088, D-094).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PerformanceSlowdown {
    /// The process collector while no window wants process rows.
    IdleProcesses,
    /// A temperature collector in the background, a temperature in the menu bar
    /// included (D-094). A visible window shows temperatures, so it keeps them at 5 s.
    IdleTemperatures,
}

/// What Performance mode or the background slows `collector` down for this tick, if
/// anything. `backgrounded`: no visible window shows detail.
fn performance_slowdown(
    collector: &dyn Collector,
    cadence: Cadence,
    interests: Interests,
    backgrounded: bool,
) -> Option<PerformanceSlowdown> {
    match cadence {
        Cadence::Adaptive {
            interest: Interest::Processes,
            ..
        } if !interests.processes => Some(PerformanceSlowdown::IdleProcesses),
        _ if backgrounded && kelvo_collect::TEMPERATURE_COLLECTORS.contains(&collector.id()) => {
            Some(PerformanceSlowdown::IdleTemperatures)
        }
        _ => None,
    }
}

/// A collector's period in Performance mode or the background, from its normal
/// `period` this tick.
pub fn performance_period(slowdown: Option<PerformanceSlowdown>, period: u32) -> u32 {
    match slowdown {
        Some(PerformanceSlowdown::IdleProcesses) => period.max(PERFORMANCE_IDLE_PROCESS_MS),
        Some(PerformanceSlowdown::IdleTemperatures) => period.max(PERFORMANCE_IDLE_SENSOR_MS),
        None => period,
    }
}

/// State shared between the engine thread and its control handles. The ring buffer and
/// latest frame are not here: they belong to the host's [`crate::LiveHub`] (D-066).
struct Shared {
    host: HostId,
    caps: Mutex<Arc<Capabilities>>,
    status: Mutex<EngineStatus>,
    /// The shortest period any visible window wants process rows at, ms
    /// ([`NO_PROCESS_INTEREST`]: none).
    process_period: AtomicU32,
    /// Windows showing detail the tray does not (D-061).
    detail: AtomicU32,
    /// A visible window with process interest shows network rates (D-081).
    network: AtomicBool,
    /// A visible window with process interest shows per-process GPU time.
    gpu: AtomicBool,
    /// A visible window with process interest shows the ports processes listen on.
    ports: AtomicBool,
    /// The "Network history" setting (D-089): per-app bytes are sampled on every process
    /// tick and persisted. Off, the per-process network collector runs only while a view
    /// asks for rates (D-082).
    net_history: AtomicBool,
}

/// Adds or removes one unit of a counted interest; never goes below zero. Returns the
/// count before.
fn count_interest(n: &AtomicU32, interested: bool) -> u32 {
    if interested {
        n.fetch_add(1, Ordering::AcqRel)
    } else {
        n.try_update(Ordering::AcqRel, Ordering::Acquire, |v| {
            Some(v.saturating_sub(1))
        })
        .unwrap_or_else(|v| v)
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // The engine never panics while holding these locks (no unwrap in non-test code);
    // if a reader did, the data is still valid to read.
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// A cloneable handle for controlling and reading a running engine.
#[derive(Clone)]
pub struct EngineControl {
    inbox: Inbox,
    shared: Arc<Shared>,
}

impl EngineControl {
    pub fn host(&self) -> HostId {
        self.shared.host
    }

    /// Pause stops the collectors and writes a `paused` gap; the bus stays up.
    pub fn set_paused(&self, paused: bool) {
        self.inbox.command(Command::Pause(paused));
    }

    /// Applies interval, battery slowdown and module switches.
    pub fn apply_settings(&self, settings: &Settings) {
        self.inbox
            .command(Command::Settings(Box::new(settings.clone())));
    }

    /// Detaches from the store (`None`) or attaches to a new one; see
    /// [`crate::SourceControl::set_store`]. The receiver gets a message (or disconnects)
    /// once the engine has applied it.
    pub fn set_store(&self, store: Option<Writer>) -> crossbeam_channel::Receiver<()> {
        let (ack, done) = SleepAck::new();
        self.inbox.command(Command::SetStore(store, ack));
        done
    }

    /// Re-probes the collectors of `modules` on the next tick, as an IOKit device hint
    /// would. A changed series set publishes a new layout and capabilities revision.
    pub fn reprobe(&self, modules: &[Module]) {
        self.inbox.hint(modules);
    }

    /// How often a window wants process rows: `None` when no window does (the processes
    /// collector, `Cadence::Adaptive`, samples every 10 s), otherwise the shortest period
    /// any visible window asked for (`Some(0)`: every tick). The caller aggregates the
    /// windows (the shell's live registry); this replaces the previous value.
    pub fn set_process_interest(&self, period_ms: Option<u32>) {
        self.shared.process_period.store(
            period_ms.map_or(NO_PROCESS_INTEREST, |p| p.min(NO_PROCESS_INTEREST - 1)),
            Ordering::Release,
        );
    }

    pub fn process_interest(&self) -> Option<u32> {
        let p = self.shared.process_period.load(Ordering::Acquire);
        (p != NO_PROCESS_INTEREST).then_some(p)
    }

    /// Counts visible windows that show detail the tray does not (cluster frequency and
    /// residency, GPU states, component power). While none does ("tray-only mode"),
    /// `Cadence::Adaptive` collectors keyed on `Interest::Detail` (IOReport) run at their
    /// idle period instead of every tick (D-061), and the engine is backgrounded
    /// ([`EngineStatus::backgrounded`], D-094). Each `true` must be matched by one
    /// `false`.
    ///
    /// When the count crosses between zero and one the engine re-chooses its base tick
    /// at once, not on its next (up to [`BACKGROUND_TICK_MS`] away) tick, and the
    /// receiver gets a message (or disconnects) once the new status is published, so a
    /// caller can hold a window's stream until its first status has the visible tick.
    /// `None` when the count did not cross.
    pub fn set_detail_interest(&self, interested: bool) -> Option<crossbeam_channel::Receiver<()>> {
        let before = count_interest(&self.shared.detail, interested);
        let crossed = if interested { before == 0 } else { before == 1 };
        crossed.then(|| {
            let (ack, done) = SleepAck::new();
            self.inbox.command(Command::Detail(ack));
            done
        })
    }

    pub fn detail_interest(&self) -> u32 {
        self.shared.detail.load(Ordering::Acquire)
    }

    /// Whether a visible window with process interest shows per-process network rates.
    /// While one does, the per-process network collector runs on the process ticks;
    /// when none does it is released and holds nothing open (D-081). Counts only while
    /// [`EngineControl::set_process_interest`] has a period. The caller aggregates the
    /// windows; this replaces the previous value.
    pub fn set_network_process_interest(&self, interested: bool) {
        self.shared.network.store(interested, Ordering::Release);
    }

    pub fn network_process_interest(&self) -> bool {
        self.shared.network.load(Ordering::Acquire)
    }

    /// Whether a visible window with process interest shows per-process GPU time. Works
    /// like [`EngineControl::set_network_process_interest`]: the per-process GPU
    /// collector runs on the process ticks while it is set and is released when it is
    /// not (D-085).
    pub fn set_gpu_process_interest(&self, interested: bool) {
        self.shared.gpu.store(interested, Ordering::Release);
    }

    pub fn gpu_process_interest(&self) -> bool {
        self.shared.gpu.load(Ordering::Acquire)
    }

    /// Whether a visible window with process interest shows the TCP ports processes
    /// listen on. Works like [`EngineControl::set_gpu_process_interest`]: the ports
    /// collector fills the rows on the process ticks while it is set.
    pub fn set_port_process_interest(&self, interested: bool) {
        self.shared.ports.store(interested, Ordering::Release);
    }

    pub fn port_process_interest(&self) -> bool {
        self.shared.ports.load(Ordering::Acquire)
    }

    /// The "Network history" setting (D-089), on by default. On, the per-process network
    /// collector samples on every tick the processes collector samples (every 10 s with
    /// only the tray open) and stays open across view changes, and per-app bytes go to
    /// the store and to the hub's ring ([`crate::LiveHub::recent_net_buckets`]). Off, it
    /// runs only while a view asks for rates and is released when none does (D-082), and
    /// nothing is recorded; the open buckets are written first. Takes effect on the next
    /// tick.
    pub fn set_network_history(&self, on: bool) {
        self.shared.net_history.store(on, Ordering::Release);
    }

    pub fn network_history(&self) -> bool {
        self.shared.net_history.load(Ordering::Acquire)
    }

    pub fn capabilities(&self) -> Arc<Capabilities> {
        Arc::clone(&lock(&self.shared.caps))
    }

    pub fn status(&self) -> EngineStatus {
        lock(&self.shared.status).clone()
    }

    /// Asks the engine to stop. [`EngineHandle`] also waits for it.
    pub fn shutdown(&self) {
        self.inbox.command(Command::Shutdown);
    }
}

/// A running engine thread. Dropping it shuts the engine down and joins the thread.
pub struct EngineHandle {
    control: EngineControl,
    thread: Option<JoinHandle<()>>,
}

impl EngineHandle {
    pub fn control(&self) -> &EngineControl {
        &self.control
    }

    /// Stops the engine (flushing buckets and closing open gaps) and waits for it.
    pub fn stop(&mut self) {
        if let Some(thread) = self.thread.take() {
            self.control.shutdown();
            if thread.join().is_err() {
                tracing::error!("engine thread panicked");
            }
        }
    }
}

impl std::ops::Deref for EngineHandle {
    type Target = EngineControl;
    fn deref(&self) -> &EngineControl {
        &self.control
    }
}

impl Drop for EngineHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Default)]
struct RateLimit {
    last_ms: Option<i64>,
    suppressed: u32,
}

impl RateLimit {
    /// `Some(suppressed since the last log)` when it is time to log again.
    fn check(&mut self, now_ms: i64) -> Option<u32> {
        match self.last_ms {
            Some(last) if now_ms - last < ERROR_LOG_EVERY_MS && now_ms >= last => {
                self.suppressed += 1;
                None
            }
            _ => {
                self.last_ms = Some(now_ms);
                Some(std::mem::take(&mut self.suppressed))
            }
        }
    }
}

struct Slot {
    collector: Box<dyn Collector>,
    probe: Option<Probe>,
    /// Sampled at all: some of its probed series are in the layout, or it produces no
    /// series (process rows) and none of its modules is switched off.
    active: bool,
    /// When it was last sampled, for its cadence's minimum period.
    every: Every,
    /// Sampled since its [`Cadence::OnDemand`] interest began; it is released when the
    /// interest ends.
    demanded: bool,
    errors: RateLimit,
}

impl Slot {
    fn keys(&self) -> &[SeriesKey] {
        match &self.probe {
            Some(Probe::Supported(keys)) => keys,
            _ => &[],
        }
    }
}

struct SeriesMeta {
    /// Nominal minimum period from the catalog, in ms.
    period_ms: u32,
    /// A `Mean` or `Rate` series: its rollup weighs each sample by its span (D-092).
    span_weighted: bool,
    /// The slot whose collector produces the series.
    owner: Option<usize>,
}

/// The current layout plus what the hot path needs to fill it.
struct Layout {
    frame: Arc<FrameLayout>,
    index: HashMap<SeriesKey, usize>,
    meta: Vec<SeriesMeta>,
    /// Persisted series, in layout order: the store layout the accumulators write.
    persisted: Arc<[SeriesKey]>,
    persisted_idx: Vec<usize>,
}

impl Layout {
    fn empty() -> Self {
        Self {
            frame: Arc::new(FrameLayout {
                layout_no: 0,
                series: Arc::from(Vec::new()),
            }),
            index: HashMap::new(),
            meta: Vec::new(),
            persisted: Arc::from(Vec::new()),
            persisted_idx: Vec::new(),
        }
    }
}

#[derive(Default)]
struct Reprobe {
    all: bool,
    modules: BTreeSet<Module>,
}

impl Reprobe {
    fn is_empty(&self) -> bool {
        !self.all && self.modules.is_empty()
    }
}

/// The sampler. Build it with [`Engine::new`], then either [`Engine::spawn`] it on its
/// own thread or, in tests, call [`Engine::start`] and [`Engine::pump`].
pub struct Engine {
    host: HostId,
    catalog: Catalog,
    slots: Vec<Slot>,
    ticker: Box<dyn Ticker>,
    power: Box<dyn PowerSignals>,
    hints: Option<Box<dyn DeviceHints>>,
    inbox: Inbox,
    rx: Receiver<Input>,
    sink: SourceSink,
    shared: Arc<Shared>,

    layout: Layout,
    latest: Vec<f32>,
    /// When each series in `latest` was sampled (`i64::MIN`: never).
    sampled_at: Vec<i64>,
    /// When each slot's collector last read the OS (its `sample` ran, whether or not it
    /// produced values), `i64::MIN` when it has no baseline: never sampled, re-probed,
    /// released, or inactive on a tick. A span average or rate covers the time since
    /// that read, not since the series' previous value (D-092).
    slot_sampled_at: Vec<i64>,
    /// For each series sampled this tick, the span its value covers for the rollups (ms):
    /// the time since its collector's previous read; 0 when there was none, and then the
    /// series' current period counts. Reused.
    span_ms: Vec<i64>,
    /// Each slot's minimum period on the current tick, for held-value staleness.
    slot_period: Vec<u32>,
    /// Each series' sampling period on the current tick, in layout order: the larger of
    /// its catalog period, its collector's current period and the base tick. Reused.
    series_period: Vec<u32>,
    /// [`LiveFrame::holds`]: each series' hold for its current period. Replaced only when
    /// a period changes, so frames share it and steady state does not allocate.
    holds: Arc<[u32]>,
    /// This tick's raw values, reused across ticks; copied once into the frame.
    scratch: Vec<f32>,
    /// Rows the hub's rollups emitted, queued to the store. Reused.
    rows: Vec<BucketRow>,
    buf: SampleBuf,

    settings: EngineSettings,
    power_state: PowerState,
    /// The network collector's last primary-interface reading (D-092).
    primary_iface: kelvo_collect::PrimaryIface,
    interval_ms: u32,
    started: bool,
    stopped: bool,
    paused: bool,
    /// Set from `WillSleep` until `DidWake`, with the reading the event carried.
    asleep: Option<ClockReading>,
    /// When the open `sleep` gap began, while one is open. A close carries it, so the
    /// gap is still recorded if its open was lost with a failed commit (D-070).
    sleep_gap_start: Option<i64>,
    /// When the open `paused` gap began, while one is open.
    paused_gap_start: Option<i64>,
    /// Modules with an open `module_disabled` gap.
    disabled_gaps: BTreeSet<Module>,
    reprobe: Reprobe,
    last_tick: Option<Tick>,
    /// After the wall clock stepped back: nothing is persisted for ticks before this
    /// (the end of the newest bucket already written, at most [`MAX_PERSIST_HOLD_MS`]
    /// after the step), so no older bucket is overwritten with a second timeline (D-064,
    /// D-070). Cleared when another store is attached; shown as
    /// [`EngineStatus::history_held_until`].
    persist_from: Option<i64>,
    /// The cut of a `discard_from` the store has not confirmed. Retried before persisting
    /// resumes; the hold stays until it succeeds.
    discard_pending: Option<i64>,
    /// `[start, end]` of the `clock_changed` gap a pending discard must restore: the
    /// discard can delete or cut it (its cut may be an older step's), so a confirmed
    /// discard writes it again, reaching to the time of the discard when that is later.
    /// Until then nothing was persisted, and the span must still read as a gap. Steps
    /// while the discard is pending widen it. `None` when no discard is pending.
    clock_gap: Option<(i64, i64)>,
    /// Bumped on every wall-clock step; frames carry it (`LiveFrame::timeline`).
    timeline: u32,
    last_proc_bucket: Option<i64>,
    /// Paces process sampling to the period windows asked for.
    proc_every: Every,
    ticks: u64,
    store_errors: RateLimit,
    detect: Detectors,
    /// This tick's events, reused across ticks.
    events: Vec<Event>,
    /// Per-app network bytes in open 10 s buckets (D-089).
    net: NetAppAcc,
    /// Buckets closed this tick, reused across ticks.
    net_closed: Vec<NetSlot>,
    /// Network history as the last tick applied it.
    net_history: bool,
}

impl Engine {
    pub fn new(host: HostId, parts: EngineParts, sink: SourceSink, settings: &Settings) -> Self {
        let (inbox, rx) = Inbox::new();
        let settings = EngineSettings::from_settings(settings);
        let shared = Arc::new(Shared {
            host,
            caps: Mutex::new(Arc::new(Capabilities::default())),
            status: Mutex::new(EngineStatus::default()),
            process_period: AtomicU32::new(NO_PROCESS_INTEREST),
            detail: AtomicU32::new(0),
            network: AtomicBool::new(false),
            gpu: AtomicBool::new(false),
            ports: AtomicBool::new(false),
            net_history: AtomicBool::new(true),
        });
        let slots = parts
            .collectors
            .into_iter()
            .map(|collector| Slot {
                collector,
                probe: None,
                active: false,
                every: Every::new(0),
                demanded: false,
                errors: RateLimit::default(),
            })
            .collect();
        let interval_ms = settings.interval_ms;
        let detect = Detectors::new(DetectorThresholds::DEFAULT, settings.alerts);
        Self {
            host,
            catalog: Catalog::builtin(),
            slots,
            ticker: parts.ticker,
            power: parts.power,
            hints: parts.hints,
            inbox,
            rx,
            sink,
            shared,
            layout: Layout::empty(),
            latest: Vec::new(),
            sampled_at: Vec::new(),
            slot_sampled_at: Vec::new(),
            span_ms: Vec::new(),
            slot_period: Vec::new(),
            series_period: Vec::new(),
            holds: Arc::from([]),
            scratch: Vec::new(),
            rows: Vec::new(),
            buf: SampleBuf::new(),
            settings,
            power_state: PowerState::default(),
            primary_iface: None,
            interval_ms,
            started: false,
            stopped: false,
            paused: false,
            asleep: None,
            sleep_gap_start: None,
            paused_gap_start: None,
            disabled_gaps: BTreeSet::new(),
            reprobe: Reprobe::default(),
            last_tick: None,
            persist_from: None,
            discard_pending: None,
            clock_gap: None,
            timeline: 0,
            last_proc_bucket: None,
            proc_every: Every::new(0),
            ticks: 0,
            store_errors: RateLimit::default(),
            detect,
            events: Vec::new(),
            net: NetAppAcc::default(),
            net_closed: Vec::new(),
            net_history: true,
        }
    }

    pub fn control(&self) -> EngineControl {
        EngineControl {
            inbox: self.inbox.clone(),
            shared: Arc::clone(&self.shared),
        }
    }

    /// Restores the alert rules' cooldowns from stored alert events, before the engine
    /// starts: a restart must not re-fire a rule that fired minutes ago. The caller reads
    /// the last [`crate::detect::alert_history_ms`] of events.
    pub fn seed_alert_history(&mut self, events: &[Event]) {
        self.detect.seed_alert_history(events);
    }

    /// Starts the engine on its own thread at utility QoS.
    pub fn spawn(mut self) -> std::io::Result<EngineHandle> {
        let control = self.control();
        let thread = std::thread::Builder::new()
            .name("kelvo-engine".into())
            .spawn(move || {
                #[cfg(target_os = "macos")]
                crate::macos::set_current_thread_utility_qos();
                self.start();
                self.run();
            })?;
        Ok(EngineHandle {
            control,
            thread: Some(thread),
        })
    }

    /// Repairs the store session, probes every collector, publishes the first layout,
    /// capabilities and status, and starts the ticker.
    pub fn start(&mut self) {
        if self.started {
            return;
        }
        self.started = true;
        let now = self.ticker.now();
        if let Some(store) = &self.sink.store {
            match store.begin_session(self.host, now.wall_ms) {
                Ok(s) => self.seed_net_edge(s.net_written_to_ms, now.wall_ms),
                Err(e) => tracing::error!("store session start failed: {e}"),
            }
        }
        // Nothing before the start counts: until the first sample, that is the edge.
        self.net.restart_at(now.wall_ms, now.continuous_ns);
        self.sink.live.net_update(&[], &[], self.net.final_to());
        for slot in &mut self.slots {
            slot.probe = Some(slot.collector.probe());
        }
        self.slot_sampled_at = vec![i64::MIN; self.slots.len()];
        self.rebuild_layout();
        self.update_caps();
        self.power.start(self.inbox.clone());
        if let Some(hints) = &mut self.hints {
            hints.start(self.inbox.clone());
        }
        self.power_state = self.power.poll();
        self.interval_ms = self.effective_interval();
        self.sync_disabled_gaps(now.wall_ms);
        self.force_publish_status();
        tracing::info!(
            series = self.layout.frame.series.len(),
            persisted = self.layout.persisted.len(),
            collectors = self.slots.len(),
            interval_ms = self.interval_ms,
            "engine started"
        );
        self.start_ticker();
    }

    /// Handles inputs until shutdown, blocking between them.
    pub fn run(mut self) {
        while let Ok(input) = self.rx.recv() {
            if !self.handle(input) {
                break;
            }
        }
    }

    /// Handles every queued input without blocking. Returns `false` once shut down.
    pub fn pump(&mut self) -> bool {
        while let Ok(input) = self.rx.try_recv() {
            if !self.handle(input) {
                return false;
            }
        }
        !self.stopped
    }

    fn handle(&mut self, input: Input) -> bool {
        match input {
            Input::Tick(t) => {
                self.inbox.tick_taken();
                if self.running() {
                    self.on_tick(t);
                }
            }
            Input::Power { event, at, ack } => self.on_power(event, at, ack),
            Input::Hint(modules) => self.reprobe.modules.extend(modules),
            Input::Cmd(Command::Pause(p)) => self.set_paused(p),
            Input::Cmd(Command::Settings(s)) => self.apply_settings(&s),
            Input::Cmd(Command::SetStore(store, ack)) => {
                self.set_store(store);
                ack.done();
            }
            Input::Cmd(Command::Detail(ack)) => {
                let extra = self.on_detail_change();
                // The caller waits for the status only, not the sample behind it.
                ack.done();
                if let Some(t) = extra {
                    self.on_tick(t);
                }
            }
            Input::Cmd(Command::Shutdown) => {
                self.shutdown();
                return false;
            }
        }
        true
    }

    fn running(&self) -> bool {
        self.started && !self.stopped && !self.paused && self.asleep.is_none()
    }

    /// No visible window shows detail (D-094).
    fn backgrounded(&self) -> bool {
        self.shared.detail.load(Ordering::Acquire) == 0
    }

    /// The base tick with a window visible: the interval, or its back-off.
    fn visible_interval(&self) -> u32 {
        effective_interval_ms(
            self.settings.interval_ms,
            self.settings.slow_on_battery,
            self.settings.performance_mode,
            self.power_state,
        )
    }

    fn effective_interval(&self) -> u32 {
        let tick = self.visible_interval();
        if self.backgrounded() {
            background_interval_ms(tick)
        } else {
            tick
        }
    }

    /// Detail interest crossed between zero and one: re-choose the base tick and publish
    /// the status before the caller's window resumes. Returns the tick to sample now,
    /// once the caller is answered: a faster tick samples at once,
    /// unless the new ticker's first tick is close, the last sample is recent or a tick
    /// is already queued, so the window opens on a fresh frame without a sample a few ms
    /// from another (or one stamped after a queued tick, which would read as a clock
    /// step).
    fn on_detail_change(&mut self) -> Option<Tick> {
        let before = self.interval_ms;
        self.apply_interval();
        self.publish_status();
        if self.interval_ms >= before || !self.running() || self.inbox.tick_pending() {
            return None;
        }
        let now = self.ticker.now();
        let interval = i64::from(self.interval_ms);
        let to_next = interval - now.wall_ms.rem_euclid(interval);
        // Before the first tick there is nothing stale to replace.
        let stale = self.last_tick.is_some_and(|t| {
            let ns = now.continuous_ns.saturating_sub(t.continuous_ns);
            i64::try_from(ns / 1_000_000).unwrap_or(i64::MAX) >= interval
        });
        (to_next > interval / 2 && stale).then(|| self.inbox.extra_tick(now))
    }

    fn start_ticker(&mut self) {
        if !self.running() {
            return;
        }
        let period = Duration::from_millis(u64::from(self.interval_ms));
        self.ticker
            .start(self.inbox.clone(), period, leeway_for(period));
    }

    /// Why Performance mode is in effect: the setting, or Low Power Mode (D-088).
    fn performance(&self) -> PerformanceReason {
        PerformanceReason::resolve(
            self.settings.performance_mode,
            self.power_state.low_power_mode,
        )
    }

    fn status_now(&self) -> EngineStatus {
        let ps = self.power_state;
        EngineStatus {
            interval_ms: self.interval_ms,
            paused: self.paused,
            asleep: self.asleep.is_some(),
            on_battery: ps.on_battery,
            low_power_mode: ps.low_power_mode,
            backed_off: self.visible_interval() != self.settings.interval_ms.max(100),
            backgrounded: self.backgrounded(),
            display_idle: ps.display_asleep || ps.screen_locked,
            history_held_until: self.persist_from,
            performance: self.performance(),
            power_source: PowerSource::resolve(ps.on_battery, ps.charging),
            primary_iface: self.primary_iface.clone(),
        }
    }

    fn publish_status(&self) {
        let status = self.status_now();
        let mut cur = lock(&self.shared.status);
        if *cur != status {
            *cur = status.clone();
            drop(cur);
            self.sink.live.publish(BusMsg::Status(status));
        }
    }

    fn force_publish_status(&self) {
        let status = self.status_now();
        *lock(&self.shared.status) = status.clone();
        self.sink.live.publish(BusMsg::Status(status));
    }

    /// Re-reads power state; backs off or restores the base tick on a change.
    fn update_power(&mut self) {
        let ps = self.power.poll();
        if ps == self.power_state {
            return;
        }
        self.power_state = ps;
        self.apply_interval();
        self.publish_status();
    }

    fn apply_interval(&mut self) {
        let eff = self.effective_interval();
        if eff != self.interval_ms {
            tracing::info!(from = self.interval_ms, to = eff, "base tick changed");
            self.interval_ms = eff;
            if self.running() {
                self.ticker.stop();
                self.start_ticker();
            }
        }
    }

    // ---- store helpers -------------------------------------------------------------

    fn store_result(&mut self, what: &str, r: kelvo_store::Result<()>, now_ms: i64) {
        if let Err(e) = r
            && let Some(suppressed) = self.store_errors.check(now_ms)
        {
            tracing::error!(suppressed, "store {what} failed: {e}");
        }
    }

    fn open_gap(&mut self, start_ms: i64, module: Option<Module>, reason: GapReason) {
        if let Some(store) = &self.sink.store {
            let r = store.open_gap(self.host, start_ms, module, reason);
            self.store_result("open gap", r, start_ms);
        }
    }

    /// Closes the open gap of `reason` and `module` at `end_ms`; `start_ms`, when known,
    /// lets the store write the whole gap if its open never reached the file.
    fn close_gap(
        &mut self,
        reason: GapReason,
        module: Option<Module>,
        start_ms: Option<i64>,
        end_ms: i64,
    ) {
        if let Some(store) = &self.sink.store {
            let r = store.close_gap(self.host, reason, module, start_ms, end_ms);
            self.store_result("close gap", r, end_ms);
        }
    }

    fn write_gap(&mut self, gap: Gap) {
        if let Some(store) = &self.sink.store {
            let at = gap.start_ms;
            let r = store.write_gap(self.host, gap);
            self.store_result("write gap", r, at);
        }
    }

    fn write_rows(&mut self, now_ms: i64) {
        let rows = std::mem::take(&mut self.rows);
        if let Some(store) = &self.sink.store {
            for row in rows {
                if let Err(e) = store.write_bucket(row)
                    && let Some(suppressed) = self.store_errors.check(now_ms)
                {
                    tracing::error!(suppressed, "store write bucket failed: {e}");
                }
            }
        }
    }

    /// Emits the open buckets of both tiers without resetting them (sleep, pause,
    /// shutdown).
    fn flush_buckets(&mut self, now_ms: i64) {
        self.sink.live.rollups().flush(self.host, &mut self.rows);
        self.write_rows(now_ms);
        self.flush_net(now_ms);
    }

    /// Writes one per-app network bucket, unless nothing in it was measured or
    /// persistence is held after the clock stepped back (the store would upsert into the
    /// old timeline's rows, or a pending discard would delete it).
    fn write_net(&mut self, which: NetWrite, now_ms: i64) {
        if self.persist_from.is_some() {
            return;
        }
        let slot = match which {
            NetWrite::Open(i) => self.net.open().get(i),
            NetWrite::Closed(i) => self.net_closed.get(i),
        };
        let Some(slot) = slot.filter(|s| s.is_measured()) else {
            return;
        };
        if let Some(store) = &self.sink.store {
            let r = store.write_net_bucket(self.host, slot.start_ms, slot.to_net_bucket());
            self.store_result("network bucket", r, now_ms);
        }
    }

    /// Writes the open per-app network buckets as they are, keeping them open (sleep,
    /// pause, shutdown, and before a reset). A bucket written again later replaces this
    /// row with a superset.
    fn flush_net(&mut self, now_ms: i64) {
        for i in 0..self.net.open().len() {
            self.write_net(NetWrite::Open(i), now_ms);
        }
    }

    /// Starts per-app counting after the store's newest per-app row (the previous run's
    /// bucket, flushed at shutdown), so the bucket this run opens first does not replace
    /// that row with a part of it. A row ahead of the clock (it went back while the app
    /// was not running) is ignored rather than stall counting until the clock catches up.
    fn seed_net_edge(&mut self, written_to_ms: Option<i64>, now_ms: i64) {
        if let Some(end) = written_to_ms.filter(|&e| e <= now_ms + NET_BUCKET_MS) {
            self.net.written_to(end);
        }
    }

    /// Writes the open per-app network buckets and closes them as they are, into the
    /// store and the hub's ring; nothing before continuous time `from_ns` counts from
    /// now on. The edge the written buckets reached stays: re-enabled within the same
    /// bucket, new samples start at its end rather than overwrite its row.
    fn reset_net(&mut self, now_ms: i64, from_ns: u64) {
        let edge = self
            .net
            .open()
            .last()
            .map(NetSlot::end_ms)
            .max(self.net.final_to());
        self.sink.live.net_update(self.net.open(), &[], edge);
        self.flush_net(now_ms);
        self.net.close_all();
        self.net.restart_at(now_ms, from_ns);
    }

    /// Moves the open per-app buckets to the hub's ring as closed, already written, and
    /// starts a new timeline at `now` (a clock step).
    fn close_net_as_is(&mut self, now: ClockReading) {
        self.sink.live.net_update(self.net.open(), &[], None);
        self.net.clear();
        self.net.restart_at(now.wall_ms, now.continuous_ns);
        self.sink.live.net_update(&[], &[], self.net.final_to());
    }

    /// Adds this tick's per-app and interface bytes to the open buckets, writes the ones
    /// that closed, and updates the hub's ring. `persist`: false while persistence is
    /// held after the clock stepped back (closed buckets then reach only the ring).
    fn account_net(&mut self, t: &Tick, persist: bool) {
        let on = self.shared.net_history.load(Ordering::Acquire);
        if on != self.net_history {
            self.net_history = on;
            tracing::info!(on, "network history");
            if on {
                self.net.restart_at(t.wall_ms, t.continuous_ns);
            } else {
                self.reset_net(t.wall_ms, t.continuous_ns);
            }
        }
        if !on {
            return;
        }
        let anchor = Anchor {
            wall_ms: t.wall_ms,
            continuous_ns: t.continuous_ns,
        };
        let mut changed = false;
        if let Some(iv) = self.buf.process_net_interval() {
            self.net.add_apps(anchor, iv, self.buf.process_net());
            changed = true;
        }
        if let Some(totals) = self.buf.iface_net() {
            self.net.add_iface(anchor, &totals);
            changed = true;
        }
        self.net.close_due(
            t.wall_ms,
            net_close_grace(self.interval_ms),
            &mut self.net_closed,
        );
        if !changed && self.net_closed.is_empty() {
            return;
        }
        // The ring first: a query between the two then finds the closed bucket in the
        // ring, never a committed row next to the ring's older, open copy of it.
        self.sink
            .live
            .net_update(&self.net_closed, self.net.open(), self.net.final_to());
        if persist {
            for i in 0..self.net_closed.len() {
                self.write_net(NetWrite::Closed(i), t.wall_ms);
            }
        }
        self.net.recycle(&mut self.net_closed);
    }

    /// Sends the pending `discard_from` (D-070). Returns whether nothing is left pending.
    ///
    /// The discard deletes every gap that starts at or after its cut, the gaps this engine
    /// holds open among them (a module switched off, or a pause, while the clock was
    /// ahead). Those are opened again, at `now_ms` or the cut if that is earlier, so the
    /// span stays covered and a later close finds its row. Reopening a gap that began
    /// before the cut, and so is still open, is a no-op in the store. The step's
    /// `clock_changed` gap is written again too (see `clock_gap`).
    fn discard_ahead(&mut self, now_ms: i64) -> bool {
        let Some(from) = self.discard_pending else {
            return true;
        };
        let Some(store) = &self.sink.store else {
            self.discard_pending = None;
            self.clock_gap = None;
            return true;
        };
        if let Err(e) = store.discard_from(self.host, from) {
            self.store_result("discard", Err(e), now_ms);
            return false;
        }
        self.discard_pending = None;
        let at = now_ms.min(from);
        let mut reopened = false;
        if let Some((start, end)) = self.clock_gap.take() {
            match Gap::host(start, Some(end.max(now_ms)), GapReason::ClockChanged) {
                Ok(gap) => {
                    self.write_gap(gap);
                    reopened = true;
                }
                Err(e) => tracing::warn!("clock step gap rejected: {e}"),
            }
        }
        if self.paused_gap_start.is_some_and(|s| s >= from) {
            self.open_gap(at, None, GapReason::Paused);
            self.paused_gap_start = Some(at);
            reopened = true;
        }
        for m in self.disabled_gaps.clone() {
            self.open_gap(at, Some(m), GapReason::ModuleDisabled);
            reopened = true;
        }
        // The discard is committed; the reopened gaps must be too, or readers see the
        // span with no gap until the next commit, and a crash loses them (D-070).
        if reopened {
            self.flush_store(now_ms);
        }
        true
    }

    fn flush_store(&mut self, now_ms: i64) {
        if let Some(store) = &self.sink.store {
            let r = store.flush();
            self.store_result("flush", r, now_ms);
        }
    }

    // ---- tick ----------------------------------------------------------------------

    fn on_tick(&mut self, t: Tick) {
        let ts = t.wall_ms;
        self.ticks += 1;
        self.update_power();

        if let Some(prev) = self.last_tick {
            let held_ns = t.continuous_ns.saturating_sub(prev.continuous_ns);
            let held_ms = i64::try_from(held_ns / 1_000_000).unwrap_or(i64::MAX);
            let interval = i64::from(self.interval_ms);
            // How far the wall clock moved beyond what actually elapsed.
            let step = (ts - prev.wall_ms).saturating_sub(held_ms);
            if step.abs() > clock_step_threshold(interval) || ts <= prev.wall_ms {
                self.on_clock_step(ts, step);
            } else if held_ms > STALL_TICKS * interval {
                // No tick and no sleep event for several intervals: nothing was measured.
                let start = prev.wall_ms + interval;
                if start < ts {
                    match Gap::host(start, Some(ts), GapReason::Sleep) {
                        Ok(gap) => self.write_gap(gap),
                        Err(e) => tracing::warn!("stall gap rejected: {e}"),
                    }
                }
                tracing::warn!(held_ms, "ticks stalled without a sleep event");
            }
        }
        self.last_tick = Some(t);

        if !self.reprobe.is_empty() {
            let which = std::mem::take(&mut self.reprobe);
            self.do_reprobe(&which);
        }

        // Sample. Process interest counts on the ticks its period is due, so a window
        // that wants rows every 5 s does not make the collector run every tick.
        let interval = self.interval_ms;
        let t = Tick {
            interval_ms: interval,
            ..t
        };
        let performance = self.performance().is_on();
        let detail = self.shared.detail.load(Ordering::Acquire) > 0;
        // Performance mode's slow idle periods also apply in the background (D-094).
        let slowed = performance || !detail;
        self.detect.set_process_idle_ms(if slowed {
            PERFORMANCE_IDLE_PROCESS_MS
        } else {
            kelvo_collect::IDLE_MS
        });
        let mut process_period = self.shared.process_period.load(Ordering::Acquire);
        if performance && process_period != NO_PROCESS_INTEREST {
            // Visible windows get process rows at most every 2 s (D-088).
            process_period = process_period.max(PERFORMANCE_VISIBLE_MS);
        }
        let wants_processes = process_period != NO_PROCESS_INTEREST;
        // Network rates join the process rows, so they are sampled on the same ticks and
        // cover the same span.
        let wants_network = wants_processes && self.shared.network.load(Ordering::Acquire);
        let wants_gpu = wants_processes && self.shared.gpu.load(Ordering::Acquire);
        let wants_ports = wants_processes && self.shared.ports.load(Ordering::Acquire);
        // Network history samples per-app bytes on every tick the processes collector
        // samples, tray-only included, and keeps the session open meanwhile (D-089). Not
        // through `wants_network`: on demand means every tick or never.
        let history = self.shared.net_history.load(Ordering::Acquire);
        let processes_due = wants_processes && self.proc_every.due_with(process_period, &t);
        let interests = Interests {
            processes: processes_due,
            detail,
            // A visible window shows everything; otherwise per collector, below.
            live: detail,
            network_processes: wants_network && processes_due,
            gpu_processes: wants_gpu && processes_due,
            port_processes: wants_ports && processes_due,
        };
        // Who holds each interest, due this tick or not: an on-demand collector is
        // released only when its interest is gone.
        let held = Interests {
            processes: wants_processes,
            network_processes: wants_network || history,
            gpu_processes: wants_gpu,
            port_processes: wants_ports,
            ..interests
        };
        // Set once the processes collector sampled this tick; the per-process network
        // collector comes after it in the slot order.
        let mut processes_sampled = false;
        let menu_bar = &self.settings.menu_bar;
        let n = self.layout.frame.series.len();
        self.scratch.clear();
        self.scratch.resize(n, f32::NAN);
        self.span_ms.resize(n, 0);
        self.buf.clear();
        self.slot_period.resize(self.slots.len(), 0);
        self.slot_sampled_at.resize(self.slots.len(), i64::MIN);
        for ((slot, slot_period), slot_at) in self
            .slots
            .iter_mut()
            .zip(self.slot_period.iter_mut())
            .zip(self.slot_sampled_at.iter_mut())
        {
            let interests = Interests {
                live: interests.live
                    || slot
                        .collector
                        .modules()
                        .iter()
                        .any(|m| menu_bar.contains(m)),
                network_processes: interests.network_processes || (history && processes_sampled),
                ..interests
            };
            let cadence = slot.collector.cadence();
            if slot.demanded
                && let Cadence::OnDemand(i) = cadence
                && !(slot.active && held.has(i))
            {
                slot.collector.release();
                slot.demanded = false;
                slot.every.reset();
                *slot_at = i64::MIN;
            }
            let mut period = cadence.period_ms(interests);
            if slowed {
                let slowdown = performance_slowdown(&*slot.collector, cadence, interests, !detail);
                period = period.map(|p| performance_period(slowdown, p));
            }
            *slot_period = period.unwrap_or(0);
            let Some(period) = period.filter(|_| slot.active) else {
                // Not sampling: its next read starts a new baseline.
                *slot_at = i64::MIN;
                continue;
            };
            if !slot.every.due_with(period, &t) {
                continue;
            }
            if matches!(cadence, Cadence::OnDemand(_)) {
                slot.demanded = true;
            }
            let start = self.buf.values().len();
            processes_sampled |= matches!(
                cadence,
                Cadence::Adaptive {
                    interest: Interest::Processes,
                    ..
                }
            );
            if let Err(e) = slot.collector.sample(&t, &mut self.buf)
                && let Some(suppressed) = slot.errors.check(ts)
            {
                tracing::warn!(
                    collector = %slot.collector.id(),
                    suppressed,
                    "sample failed: {e}"
                );
            }
            // A value from this read covers the time since the collector's previous read,
            // even when that read produced nothing for it (a counter reset, a dropped
            // rate): the collector's baseline moved then.
            let read_span = if *slot_at == i64::MIN {
                0
            } else {
                ts.saturating_sub(*slot_at).max(1)
            };
            *slot_at = ts;
            for s in self.buf.values().get(start..).unwrap_or_default() {
                let Some(&i) = self.layout.index.get(&s.key) else {
                    continue;
                };
                if let (Some(v), Some(l), Some(at), Some(span)) = (
                    self.scratch.get_mut(i),
                    self.latest.get_mut(i),
                    self.sampled_at.get_mut(i),
                    self.span_ms.get_mut(i),
                ) {
                    *v = s.value;
                    *l = s.value;
                    *span = read_span;
                    *at = ts;
                }
            }
        }
        if let Some(primary) = self.buf.primary_iface()
            && *primary != self.primary_iface
        {
            self.primary_iface = primary.clone();
            self.publish_status();
        }
        let procs = if self.buf.processes().is_empty() {
            Vec::new()
        } else {
            let mut procs = self.buf.take_processes();
            if self.buf.process_net_measured() {
                self.buf.sort_process_net();
                for p in &mut procs {
                    // Absent from the network batch: no traffic over the interval.
                    let (rx, tx) = self
                        .buf
                        .net_of(p.pid)
                        .map_or((0.0, 0.0), |n| (n.rx_bps, n.tx_bps));
                    p.net_rx_bps = Some(rx);
                    p.net_tx_bps = Some(tx);
                }
            }
            if self.buf.process_gpu_measured() {
                self.buf.sort_process_gpu();
                for p in &mut procs {
                    // Absent from the GPU batch: no GPU time over the interval.
                    p.gpu_pct = Some(self.buf.gpu_of(p.pid).map_or(0.0, |g| g.pct));
                }
            }
            procs
        };

        // One allocation each for the raw values, the held values and the frame: the
        // frame is shared with the hub's ring, the bus and every subscriber.
        let values: Arc<[f32]> = Arc::from(self.scratch.as_slice());
        // A held value stays current for 2.5 times its sampling period as it is now (the
        // larger of the catalog period, its collector's current period and the base
        // tick), so a collector that drops to its idle rate does not blink to a gap.
        let slot_period = &self.slot_period;
        self.series_period.clear();
        self.series_period.extend(self.layout.meta.iter().map(|m| {
            let own = m
                .owner
                .and_then(|o| slot_period.get(o).copied())
                .unwrap_or(0);
            m.period_ms.max(own).max(interval)
        }));
        if self.holds.len() != self.series_period.len()
            || self
                .holds
                .iter()
                .zip(&self.series_period)
                .any(|(&h, &p)| h != hold_ms(p))
        {
            self.holds = self.series_period.iter().map(|&p| hold_ms(p)).collect();
        }
        let held: Arc<[f32]> = self
            .latest
            .iter()
            .zip(&self.sampled_at)
            .zip(&self.series_period)
            .map(|((&v, &at), &period)| {
                if at.saturating_add(stale_ms(period)) >= ts {
                    v
                } else {
                    f32::NAN
                }
            })
            .collect();
        let frame = Arc::new(LiveFrame {
            ts_ms: ts,
            interval_ms: interval,
            layout: Arc::clone(&self.layout.frame),
            timeline: self.timeline,
            values,
            held,
            holds: Arc::clone(&self.holds),
        });
        self.sink.live.publish(BusMsg::Frame(Arc::clone(&frame)));

        // Rollups, unless the wall clock stepped back over buckets already written.
        if let Some(from) = self.persist_from {
            // A discard the store has not confirmed keeps the hold: persisting now would
            // upsert into the old clock's rows.
            if ts < from || !self.discard_ahead(ts) {
                self.account_net(&t, false);
                if !procs.is_empty() {
                    self.publish_processes(ts, procs);
                }
                return;
            }
            self.persist_from = None;
            self.publish_status();
            tracing::info!("wall clock passed the held span; persisting again");
        }
        self.account_net(&t, true);
        let persisted = &self.layout.persisted;
        let (meta, spans, periods) = (&self.layout.meta, &self.span_ms, &self.series_period);
        // Each value with its rollup weight (D-092): for a span average, the span it
        // covers (`span_ms`; its current period when the read was a new baseline); for a
        // gauge, 1.
        let vals = self.layout.persisted_idx.iter().map(|&i| {
            let v = frame.values.get(i).copied().unwrap_or(f32::NAN);
            let weight = match (meta.get(i), spans.get(i), periods.get(i)) {
                (Some(m), Some(&span), _) if m.span_weighted && span > 0 => span as f64,
                (Some(m), _, Some(&period)) if m.span_weighted => f64::from(period.max(1)),
                _ => 1.0,
            };
            (v, weight)
        });
        let minute_closed =
            self.sink
                .live
                .rollups()
                .add(self.host, ts, persisted, vals, &mut self.rows);
        self.write_rows(ts);
        if minute_closed {
            tracing::info!(
                ticks = self.ticks,
                series = n,
                persisted = self.layout.persisted.len(),
                layout_no = self.layout.frame.layout_no,
                interval_ms = self.interval_ms,
                "engine minute"
            );
        }

        let batch = (!procs.is_empty()).then_some(procs.as_slice());
        self.detect_events(ts, &frame.values, batch);

        if !procs.is_empty() {
            self.on_processes(ts, procs);
        }
    }

    /// Runs the detectors and alert rules on a persisted tick. Each event is queued to
    /// the store with a commit request behind it, so readers see it within one writer
    /// round trip rather than at the next batch commit (D-083), and published on the bus
    /// right away, before that commit lands.
    fn detect_events(&mut self, ts: i64, values: &[f32], batch: Option<&[ProcessSample]>) {
        let mut events = std::mem::take(&mut self.events);
        self.detect
            .on_tick(ts, values, &self.series_period, batch, &mut events);
        if !events.is_empty() {
            if let Some(store) = &self.sink.store {
                let r = events
                    .iter()
                    .try_for_each(|e| store.record_event(self.host, e))
                    .and_then(|()| store.commit_soon());
                self.store_result("event", r, ts);
            }
            for e in events.drain(..) {
                tracing::info!(kind = e.detail.kind(), processes = ?e.processes, "event");
                self.sink.live.publish(BusMsg::Event(Arc::new(e)));
            }
        }
        self.events = events;
    }

    /// The wall clock moved `step` ms more than the continuous clock since the last tick
    /// (negative: it went back), and now reads `ts`. Treated like a sleep: the open buckets
    /// are flushed and dropped, and the hole gets a `clock_changed` gap. Going back,
    /// nothing is persisted until the clock passes the newest bucket already written
    /// (D-064). Frames from here on carry the next `timeline`, which tells subscribers to
    /// drop what they hold from the stepped time on; the hub's ring does the same when the
    /// stepped frame arrives.
    fn on_clock_step(&mut self, ts: i64, step: i64) {
        tracing::warn!(step_ms = step, "wall clock stepped");
        self.flush_buckets(ts - step);
        let written_to = {
            let mut rollups = self.sink.live.rollups();
            let end = rollups.bucket_end();
            rollups.reset();
            if step < 0 {
                rollups.drop_after(ts);
            }
            end.into_iter().chain(self.persist_from).max()
        };
        self.detect.reset();
        self.last_proc_bucket = None;
        // `flush_buckets` wrote the open per-app buckets; a sample reaching back before
        // the step would put time that was not lived on the new timeline.
        self.close_net_as_is(self.ticker.now());
        if step < 0 {
            self.sink.live.net_drop_after(ts);
            self.sink.live.usage_drop_after(ts);
        }
        for at in self.sampled_at.iter_mut().chain(&mut self.slot_sampled_at) {
            if *at != i64::MIN {
                *at = at.saturating_add(step);
            }
        }
        let gap = if step > 0 {
            // Forward: the minutes the clock skipped were never lived.
            Gap::host(ts - step, Some(ts), GapReason::ClockChanged)
        } else {
            let written_to = written_to.unwrap_or(ts).max(ts);
            // Hold for at most an hour, ending on a minute boundary so the first minute
            // written again is whole (D-070).
            let cap = ts
                .saturating_add(MAX_PERSIST_HOLD_MS - 1)
                .div_euclid(MINUTE_MS)
                .saturating_mul(MINUTE_MS)
                .saturating_add(MINUTE_MS);
            let until = written_to.min(cap);
            if written_to > until {
                // The old clock wrote past the hold: drop those rows so the new timeline
                // never upserts into them.
                tracing::warn!(
                    from_ms = until,
                    to_ms = written_to,
                    "discarding history the wall clock wrote ahead of itself"
                );
                self.discard_pending = Some(self.discard_pending.map_or(until, |p| p.min(until)));
            }
            // A pending discard keeps the hold even when this step needs none: the next
            // tick past it retries the discard before anything is persisted again.
            self.persist_from = (until > ts || self.discard_pending.is_some()).then_some(until);
            Gap::host(ts, Some(until), GapReason::ClockChanged)
        };
        match gap {
            Ok(gap) if gap.end_ms.is_some_and(|e| e > gap.start_ms) => {
                let end = gap.end_ms.unwrap_or(gap.start_ms);
                // While a discard is pending nothing is persisted, so an earlier step's
                // gap that began before this one widens to cover both instead of being
                // forgotten.
                self.clock_gap = self.discard_pending.map(|_| match self.clock_gap {
                    Some((s, e)) if s < gap.start_ms => (s, e.max(end)),
                    _ => (gap.start_ms, end),
                });
                self.write_gap(gap);
            }
            Ok(_) => {}
            Err(e) => tracing::warn!("clock step gap rejected: {e}"),
        }
        // After the gap: the discard commits what was queued before it, the gap with it.
        self.discard_ahead(ts);
        self.timeline = self.timeline.wrapping_add(1);
        self.bump_layout();
        self.publish_status();
    }

    /// Republishes the current series under a new layout number after the wall clock
    /// stepped. Subscribers reset on the frame's `timeline`, not on this.
    fn bump_layout(&mut self) {
        let frame = Arc::new(FrameLayout {
            layout_no: self.layout.frame.layout_no + 1,
            series: Arc::clone(&self.layout.frame.series),
        });
        self.layout.frame = Arc::clone(&frame);
        self.sink.live.publish(BusMsg::Layout(frame));
    }

    fn on_processes(&mut self, ts: i64, procs: Vec<ProcessSample>) {
        let bucket = Tier::S10.bucket_start(ts);
        if bucket.is_some() && bucket != self.last_proc_bucket {
            self.last_proc_bucket = bucket;
            let mut top: Vec<&ProcessSample> = procs.iter().collect();
            top.sort_by(|a, b| b.cpu_pct.total_cmp(&a.cpu_pct));
            let rows: Vec<ProcRow> = top
                .into_iter()
                .take(PROC_SNAPSHOT_TOP)
                .map(|p| ProcRow {
                    name: p.name.to_string(),
                    pid: p.pid,
                    cpu_pct: p.cpu_pct,
                    mem_bytes: p.mem_bytes,
                    threads: p.threads,
                    idle_wakeups_per_s: p.idle_wakeups_per_s,
                    energy: p.energy,
                })
                .collect();
            if let Some(store) = &self.sink.store {
                let r = store.write_proc_snapshot(self.host, ts, rows);
                self.store_result("process snapshot", r, ts);
            }
        }
        self.publish_processes(ts, procs);
    }

    fn publish_processes(&self, ts: i64, procs: Vec<ProcessSample>) {
        self.sink
            .live
            .publish(BusMsg::Processes(Arc::new(ProcessBatch {
                ts_ms: ts,
                rows: procs,
            })));
    }

    // ---- layout and capabilities ---------------------------------------------------

    fn series_enabled(&self, key: &SeriesKey) -> Option<bool> {
        let def = self.catalog.validate(key).ok()?;
        let ungated = UNGATED.contains(&def.id.as_str());
        Some(ungated || !self.settings.disabled.contains(&def.module))
    }

    /// Recomputes the layout from the last probes and the module switches. Publishes a
    /// new layout (before any frame uses it) when the series set changed.
    fn rebuild_layout(&mut self) {
        let mut series: Vec<SeriesKey> = self
            .slots
            .iter()
            .flat_map(|s| s.keys().iter())
            .filter(|k| match self.series_enabled(k) {
                Some(enabled) => enabled,
                None => {
                    tracing::debug!(key = %k, "series not in the catalog; skipped");
                    false
                }
            })
            .cloned()
            .collect();
        series.sort();
        series.dedup();

        let changed = *self.layout.frame.series != *series || self.layout.frame.layout_no == 0;
        if changed {
            let layout_no = self.layout.frame.layout_no + 1;
            let series: Arc<[SeriesKey]> = series.into();
            let index: HashMap<SeriesKey, usize> = series
                .iter()
                .enumerate()
                .map(|(i, k)| (k.clone(), i))
                .collect();
            let mut meta = Vec::with_capacity(series.len());
            let mut persisted = Vec::new();
            let mut persisted_idx = Vec::new();
            for (i, k) in series.iter().enumerate() {
                let def = self.catalog.validate(k).ok();
                meta.push(SeriesMeta {
                    period_ms: def.map_or(0, |d| u32::from(d.period_s) * 1_000),
                    span_weighted: def
                        .is_some_and(|d| matches!(d.kind, MetricKind::Mean | MetricKind::Rate)),
                    owner: self.slots.iter().position(|s| s.keys().contains(k)),
                });
                if def.is_some_and(|d| d.persisted) {
                    persisted.push(k.clone());
                    persisted_idx.push(i);
                }
            }
            // Carry the latest values over by key.
            let mut latest = vec![f32::NAN; series.len()];
            let mut sampled = vec![i64::MIN; series.len()];
            for (old_i, k) in self.layout.frame.series.iter().enumerate() {
                if let (Some(&new_i), Some(&v), Some(&f)) = (
                    index.get(k),
                    self.latest.get(old_i),
                    self.sampled_at.get(old_i),
                ) && let (Some(l), Some(fr)) = (latest.get_mut(new_i), sampled.get_mut(new_i))
                {
                    *l = v;
                    *fr = f;
                }
            }
            self.latest = latest;
            self.sampled_at = sampled;
            self.layout = Layout {
                frame: Arc::new(FrameLayout { layout_no, series }),
                index,
                meta,
                persisted: persisted.into(),
                persisted_idx,
            };
            self.detect.bind(&self.layout.frame.series);
            self.sink
                .live
                .publish(BusMsg::Layout(Arc::clone(&self.layout.frame)));
            tracing::info!(
                layout_no,
                series = self.layout.frame.series.len(),
                "layout changed"
            );
        }
        for slot in &mut self.slots {
            slot.active = match &slot.probe {
                Some(Probe::Supported(keys)) if keys.is_empty() => slot
                    .collector
                    .modules()
                    .iter()
                    .all(|m| !self.settings.disabled.contains(m)),
                _ => slot
                    .keys()
                    .iter()
                    .any(|k| self.layout.index.contains_key(k)),
            };
        }
        // With no network collector sampling, nobody keeps the primary interface current.
        if self.primary_iface.is_some()
            && !self
                .slots
                .iter()
                .any(|s| s.active && s.collector.modules().contains(&Module::Network))
        {
            self.primary_iface = None;
            self.publish_status();
        }
        self.publish_history_periods();
    }

    /// Each persisted series' slowest period under the current settings, for the hold
    /// `query_history` reports (D-092): the longest of its catalog period, its
    /// collector's period with nothing shown (slowed further, as in the background or
    /// Performance mode) and the slowest base tick: the background tick or the backed-off
    /// one (D-094).
    fn publish_history_periods(&self) {
        let floor = background_interval_ms(backoff_interval_ms(self.settings.interval_ms.max(100)));
        let idle = Interests::default();
        let periods = self.layout.persisted_idx.iter().filter_map(|&i| {
            let key = self.layout.frame.series.get(i)?;
            let meta = self.layout.meta.get(i)?;
            let own = meta
                .owner
                .and_then(|o| self.slots.get(o))
                .map_or(0, |slot| {
                    let cadence = slot.collector.cadence();
                    let period = cadence.period_ms(idle).unwrap_or(0);
                    performance_period(
                        performance_slowdown(&*slot.collector, cadence, idle, true),
                        period,
                    )
                });
            Some((key.clone(), meta.period_ms.max(own).max(floor)))
        });
        let now = self.ticker.now().wall_ms;
        self.sink.live.rollups().set_periods(periods, floor, now);
    }

    fn do_reprobe(&mut self, which: &Reprobe) {
        for (slot, at) in self.slots.iter_mut().zip(self.slot_sampled_at.iter_mut()) {
            let modules = slot.collector.modules();
            let hit = which.all || modules.iter().any(|m| which.modules.contains(m));
            if hit {
                slot.probe = Some(slot.collector.probe());
                slot.every.reset();
                // A probe resets the collector's rate state: its next read is a baseline.
                *at = i64::MIN;
            }
        }
        self.rebuild_layout();
        self.update_caps();
    }

    fn update_caps(&mut self) {
        let mut modules: BTreeMap<Module, ModuleCap> = BTreeMap::new();
        let mut counts: BTreeMap<Module, u32> = BTreeMap::new();
        for slot in &self.slots {
            let Some(probe) = &slot.probe else { continue };
            match probe {
                Probe::Supported(keys) => {
                    for m in slot.collector.modules() {
                        counts.entry(*m).or_insert(0);
                    }
                    for k in keys {
                        if let Ok(def) = self.catalog.validate(k) {
                            *counts.entry(def.module).or_insert(0) += 1;
                        }
                    }
                }
                Probe::Unsupported { reason } => {
                    for m in slot.collector.modules() {
                        match modules.get(m) {
                            Some(ModuleCap::Unsupported(_)) => {}
                            _ => {
                                modules.insert(*m, ModuleCap::Unsupported(*reason));
                            }
                        }
                    }
                }
                Probe::NotPresent => {
                    for m in slot.collector.modules() {
                        modules.entry(*m).or_insert(ModuleCap::NotPresent);
                    }
                }
            }
        }
        for (m, series) in counts {
            modules.insert(m, ModuleCap::Available { series });
        }
        // A collector that can supply network rates whenever a view asks (D-081).
        let process_network = self.slots.iter().any(|s| {
            s.collector.cadence() == Cadence::OnDemand(Interest::NetworkProcesses)
                && matches!(s.probe, Some(Probe::Supported(_)))
        });
        let process_gpu = self.slots.iter().any(|s| {
            s.collector.cadence() == Cadence::OnDemand(Interest::GpuProcesses)
                && matches!(s.probe, Some(Probe::Supported(_)))
        });
        let mut caps = lock(&self.shared.caps);
        if caps.modules != modules
            || caps.process_network != process_network
            || caps.process_gpu != process_gpu
        {
            let next = Arc::new(Capabilities {
                modules,
                revision: caps.revision + 1,
                process_network,
                process_gpu,
            });
            *caps = Arc::clone(&next);
            drop(caps);
            self.sink.live.publish(BusMsg::Caps(next));
        }
    }

    // ---- power, pause, settings ----------------------------------------------------

    fn on_power(&mut self, event: PowerEvent, at: ClockReading, ack: Option<SleepAck>) {
        match event {
            PowerEvent::WillSleep => {
                if self.asleep.is_none() && self.started && !self.stopped {
                    if self.running() {
                        self.flush_buckets(at.wall_ms);
                        self.ticker.stop();
                        self.release_on_demand();
                        self.open_gap(at.wall_ms, None, GapReason::Sleep);
                        self.sleep_gap_start = Some(at.wall_ms);
                    }
                    self.flush_store(at.wall_ms);
                    self.asleep = Some(at);
                    self.publish_status();
                    tracing::info!("sleeping");
                }
            }
            PowerEvent::DidWake => {
                if let Some(since) = self.asleep.take() {
                    // The wall clock may have been stepped during sleep; the continuous
                    // clock measures what actually elapsed.
                    let end = since.wall_ms + at.continuous_ms_since(&since);
                    // The wall clock stepped during sleep: handled like a step between
                    // two ticks once the sleep gap is closed.
                    let step = at.wall_ms - end;
                    let stepped = step.abs() > clock_step_threshold(i64::from(self.interval_ms));
                    if let Some(start) = self.sleep_gap_start.take() {
                        self.close_gap(GapReason::Sleep, None, Some(start), end);
                        if self.paused {
                            self.open_gap(end, None, GapReason::Paused);
                            self.paused_gap_start = Some(end);
                        }
                        // Commit the closed gap, and the pause that follows it, now: with
                        // a 5-minute commit interval, "last woke" would otherwise lag the
                        // wake and the pause would not show (D-070).
                        self.flush_store(end);
                    }
                    tracing::info!(slept_ms = end - since.wall_ms, "woke");
                    if stepped {
                        self.on_clock_step(at.wall_ms, step);
                    }
                    self.resume_sampling();
                    self.publish_status();
                } else {
                    // A wake without a sleep we saw: devices may still have changed.
                    self.reprobe.all = true;
                }
            }
        }
        if let Some(ack) = ack {
            ack.done();
        }
    }

    /// Releases every on-demand collector that holds something open (the NetworkStatistics
    /// manager, the GPU client counters). Ticks release them when their interest ends,
    /// but none come while paused or asleep, and a view left open must not keep them
    /// held then (D-082). The next sample after resuming is a fresh baseline.
    fn release_on_demand(&mut self) {
        for (slot, at) in self.slots.iter_mut().zip(self.slot_sampled_at.iter_mut()) {
            if slot.demanded {
                slot.collector.release();
                slot.demanded = false;
                slot.every.reset();
                *at = i64::MIN;
            }
        }
    }

    /// After sleep or pause: reset rate state through a re-probe, restart the ticker.
    fn resume_sampling(&mut self) {
        if !self.running() {
            return;
        }
        self.reprobe.all = true;
        self.last_tick = None;
        self.detect.reset();
        for slot in &mut self.slots {
            slot.every.reset();
        }
        // The span asleep or paused was not measured; the open buckets keep what was.
        let now = self.ticker.now();
        self.net.restart_at(now.wall_ms, now.continuous_ns);
        self.start_ticker();
    }

    fn set_paused(&mut self, paused: bool) {
        if paused == self.paused || !self.started || self.stopped {
            return;
        }
        let now = self.ticker.now();
        if paused {
            if self.asleep.is_none() {
                self.flush_buckets(now.wall_ms);
                self.ticker.stop();
                self.release_on_demand();
                self.open_gap(now.wall_ms, None, GapReason::Paused);
                self.paused_gap_start = Some(now.wall_ms);
                // Commit the open now: with a 5-minute commit interval, history would
                // otherwise show no pause for up to that long (D-070).
                self.flush_store(now.wall_ms);
            }
            self.paused = true;
        } else {
            self.paused = false;
            let closed = self.paused_gap_start.take();
            if let Some(start) = closed {
                self.close_gap(GapReason::Paused, None, Some(start), now.wall_ms);
            }
            if self.asleep.is_some() {
                self.open_gap(now.wall_ms, None, GapReason::Sleep);
                self.sleep_gap_start = Some(now.wall_ms);
            }
            // Readers would otherwise draw the open pause over new samples until the
            // next commit, up to 5 minutes away (D-070).
            if closed.is_some() || self.asleep.is_some() {
                self.flush_store(now.wall_ms);
            }
            self.resume_sampling();
        }
        self.publish_status();
    }

    fn sync_disabled_gaps(&mut self, now_ms: i64) {
        let want = self.settings.disabled.clone();
        let opened: Vec<Module> = want.difference(&self.disabled_gaps).copied().collect();
        let closed: Vec<Module> = self.disabled_gaps.difference(&want).copied().collect();
        // Commit the change at once: an open `module_disabled` gap left in the batch
        // would read as still open over new samples until the next commit (D-070).
        let changed = !opened.is_empty() || !closed.is_empty();
        for m in opened {
            self.open_gap(now_ms, Some(m), GapReason::ModuleDisabled);
        }
        for m in closed {
            self.close_gap(GapReason::ModuleDisabled, Some(m), None, now_ms);
            self.reprobe.modules.insert(m);
        }
        self.disabled_gaps = want;
        if changed {
            self.flush_store(now_ms);
        }
    }

    fn apply_settings(&mut self, settings: &Settings) {
        let next = EngineSettings::from_settings(settings);
        if next == self.settings {
            return;
        }
        if next.alerts != self.settings.alerts {
            self.detect.set_alerts(next.alerts);
        }
        let was_network_off = self.settings.disabled.contains(&Module::Network);
        let cadence_only = EngineSettings {
            menu_bar: next.menu_bar.clone(),
            alerts: next.alerts,
            performance_mode: next.performance_mode,
            ..self.settings.clone()
        } == next;
        let performance_changed = next.performance_mode != self.settings.performance_mode;
        self.settings = next;
        if cadence_only {
            // The menu bar and Performance mode change cadences from the next tick and
            // the alert rules are swapped above. Performance mode can also change the
            // battery back-off and is in the status.
            if performance_changed {
                self.apply_interval();
                self.publish_status();
            }
            return;
        }
        let now = self.ticker.now();
        if self.settings.disabled.contains(&Module::Network) != was_network_off {
            // The network collectors start or stop; buckets restart from here.
            self.reset_net(now.wall_ms, now.continuous_ns);
        }
        self.sync_disabled_gaps(now.wall_ms);
        self.rebuild_layout();
        self.apply_interval();
        self.publish_status();
    }

    /// Closes, in the store, every gap the engine holds open, at `now_ms`. The flags
    /// that say which state the engine is in (paused, asleep, disabled modules) stay.
    fn close_open_gaps(&mut self, now_ms: i64) {
        if let Some(start) = self.paused_gap_start.take() {
            self.close_gap(GapReason::Paused, None, Some(start), now_ms);
        }
        if let Some(start) = self.sleep_gap_start.take() {
            self.close_gap(GapReason::Sleep, None, Some(start), now_ms);
        }
        for m in std::mem::take(&mut self.disabled_gaps) {
            self.close_gap(GapReason::ModuleDisabled, Some(m), None, now_ms);
        }
    }

    fn set_store(&mut self, store: Option<Writer>) {
        if !self.started || self.stopped {
            self.sink.store = store;
            return;
        }
        let now = self.ticker.now().wall_ms;
        // Leave the old store consistent: what was measured, and no gap left open.
        if self.running() {
            self.flush_buckets(now);
        }
        self.close_open_gaps(now);
        // The old store has the open per-app buckets (flushed above, or at the pause or
        // sleep); the new one (history was reset) starts from here, and the ring forgets
        // what the old one had.
        self.net.clear();
        let at = self.ticker.now();
        self.net.restart_at(at.wall_ms, at.continuous_ns);
        self.sink.live.net_clear();
        self.sink.live.net_update(&[], &[], self.net.final_to());
        self.sink.live.rollups().clear();
        self.flush_store(now);
        self.sink.store = store;
        self.detect.reset();
        // The hold protected the old file's buckets; another file has none of them.
        self.discard_pending = None;
        self.clock_gap = None;
        if self.persist_from.take().is_some() {
            self.publish_status();
        }
        let Some(store) = &self.sink.store else {
            tracing::info!("store detached; running live-only");
            return;
        };
        match store.begin_session(self.host, now) {
            Ok(s) => {
                let written_to = s.net_written_to_ms;
                self.seed_net_edge(written_to, now);
            }
            Err(e) => tracing::error!("store session start failed: {e}"),
        }
        if self.paused {
            self.open_gap(now, None, GapReason::Paused);
            self.paused_gap_start = Some(now);
        } else if self.asleep.is_some() {
            self.open_gap(now, None, GapReason::Sleep);
            self.sleep_gap_start = Some(now);
        }
        let disabled = self.settings.disabled.clone();
        for &m in &disabled {
            self.open_gap(now, Some(m), GapReason::ModuleDisabled);
        }
        self.disabled_gaps = disabled;
        tracing::info!("store attached");
    }

    fn shutdown(&mut self) {
        if self.stopped {
            return;
        }
        let now = self.ticker.now();
        if self.started {
            if self.running() {
                self.flush_buckets(now.wall_ms);
            }
            self.ticker.stop();
            self.close_open_gaps(now.wall_ms);
            self.flush_store(now.wall_ms);
        }
        self.stopped = true;
        self.force_publish_status();
        tracing::info!("engine stopped");
    }
}

/// Which per-app network bucket `Engine::write_net` writes.
#[derive(Clone, Copy)]
enum NetWrite {
    Open(usize),
    Closed(usize),
}

/// How long after a sample its value stays current: 2.5 times its sampling period (the
/// larger of the catalog period, the collector's current period and the base tick).
fn stale_ms(period_ms: u32) -> i64 {
    i64::from(period_ms) * STALE_NUM / STALE_DEN
}

/// How long a sample taken every `period_ms` stays current, as a [`LiveFrame::holds`]
/// entry: [`stale_ms`]. A consumer that receives rows less often than the engine takes
/// them (a thinned live stream) holds them by its row period the same way.
pub fn hold_ms(period_ms: u32) -> u32 {
    u32::try_from(stale_ms(period_ms)).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staleness_bound() {
        assert_eq!(stale_ms(1_000), 2_500);
        assert_eq!(stale_ms(60_000), 150_000);
    }

    #[test]
    fn backoff_doubles_up_to_the_longest_interval() {
        assert_eq!(backoff_interval_ms(500), 1_000);
        assert_eq!(backoff_interval_ms(1_000), 2_000);
        assert_eq!(backoff_interval_ms(5_000), 10_000);
        assert_eq!(backoff_interval_ms(30_000), 60_000);
        assert_eq!(backoff_interval_ms(60_000), 60_000);
    }

    #[test]
    fn power_switch_covers_sensors() {
        let mut s = Settings::default();
        if let Some(p) = s.modules.get_mut(&Module::Power) {
            p.enabled = false;
        }
        let e = EngineSettings::from_settings(&s);
        assert!(e.disabled.contains(&Module::Power));
        assert!(e.disabled.contains(&Module::Sensors));
        assert!(e.disabled.contains(&Module::Disk), "disk is off by default");
        assert!(!e.disabled.contains(&Module::Cpu));
    }

    #[test]
    fn rate_limit_logs_once_a_minute() {
        let mut r = RateLimit::default();
        assert_eq!(r.check(0), Some(0));
        assert_eq!(r.check(1_000), None);
        assert_eq!(r.check(59_999), None);
        assert_eq!(r.check(60_000), Some(2));
    }
}
