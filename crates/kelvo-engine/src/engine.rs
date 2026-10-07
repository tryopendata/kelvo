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
//!
//! The engine's `impl` is split by concern: [`cadence`] (base tick and Performance
//! mode), [`control`] (the handles other threads hold), [`tick`] (the per-tick path),
//! [`persist`] (store writes), [`layout`] (layout and capabilities) and [`lifecycle`]
//! (power, pause, settings, store swap and shutdown).

mod cadence;
mod control;
mod layout;
mod lifecycle;
mod persist;
mod tick;

pub use cadence::{
    BACKGROUND_TICK_MS, PERFORMANCE_IDLE_PROCESS_MS, PERFORMANCE_IDLE_SENSOR_MS,
    PERFORMANCE_VISIBLE_MS, PerformanceSlowdown, background_interval_ms, backoff_interval_ms,
    effective_interval_ms, performance_period,
};
pub use control::{EngineControl, EngineHandle};

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, AtomicU32};
use std::sync::{Arc, Mutex};

use crossbeam_channel::Receiver;
use kelvo_collect::{Collector, Every, Probe, SampleBuf, Tick};
use kelvo_schema::{
    AlertSettings, Capabilities, Catalog, DetectorThresholds, Event, HostId, Module, SeriesKey,
    Settings,
};
use kelvo_store::BucketRow;

use self::control::{NO_PROCESS_INTEREST, Shared};
use self::layout::{Layout, Reprobe};
use crate::bus::EngineStatus;
use crate::clock::ClockReading;
use crate::detect::Detectors;
use crate::hints::DeviceHints;
use crate::inbox::{Command, Inbox, Input};
use crate::netacc::{NetAppAcc, NetSlot};
use crate::power::{PowerSignals, PowerState};
use crate::source::SourceSink;
use crate::ticker::Ticker;

/// A value stays current in [`LiveFrame::held`] for this many sampling intervals, as a
/// fraction (5/2 = 2.5): one missed sample is tolerated, two are a gap.
///
/// [`LiveFrame::held`]: crate::LiveFrame::held
pub const STALE_NUM: i64 = 5;
pub const STALE_DEN: i64 = 2;

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
    ///
    /// [`PerformanceReason::resolve`]: kelvo_schema::PerformanceReason::resolve
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
    ///
    /// [`Cadence::OnDemand`]: kelvo_collect::Cadence::OnDemand
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
    ///
    /// [`LiveFrame::holds`]: crate::LiveFrame::holds
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
    ///
    /// [`MAX_PERSIST_HOLD_MS`]: tick::MAX_PERSIST_HOLD_MS
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
    /// When GPU time next joins a process sample with no GPU view open (D-099),
    /// `continuous_ns`: once per usage bucket, not on every 1 s process tick.
    gpu_always_next_ns: u64,
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
            gpu_always_next_ns: 0,
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
}

/// How long after a sample its value stays current: 2.5 times its sampling period (the
/// larger of the catalog period, the collector's current period and the base tick).
fn stale_ms(period_ms: u32) -> i64 {
    i64::from(period_ms) * STALE_NUM / STALE_DEN
}

/// How long a sample taken every `period_ms` stays current, as a [`LiveFrame::holds`]
/// entry: [`stale_ms`]. A consumer that receives rows less often than the engine takes
/// them (a thinned live stream) holds them by its row period the same way.
///
/// [`LiveFrame::holds`]: crate::LiveFrame::holds
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
