//! Fakes and a harness for driving the engine deterministically: a `FakeTicker`, fake
//! power signals, fake collectors, a real SQLite store in a temp dir, and a bus
//! subscriber that records everything.

#![allow(dead_code, clippy::unwrap_used)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use kelvo_collect::{
    Cadence, CollectError, Collector, CollectorId, IfaceNet, Interest, Interval, Probe, ProcessGpu,
    ProcessNet, ProcessSample, SampleBuf, Tick,
};
use kelvo_engine::{
    Bus, BusMsg, Engine, EngineControl, EngineParts, FakeClock, FakePower, FakePowerSignals,
    FakeTicker, LiveFrame, LiveHub, SourceSink, Subscriber,
};
use kelvo_schema::{
    ClusterInfo, CoreKind, Entitlement, HostId, HostInfo, HostRecord, Module, OsKind, SeriesKey,
    Settings, UnsupportedReason,
};
use kelvo_store::{DEFAULT_PRUNE_BATCH, Reader, Store, StoreConfig};
use uuid::Uuid;

/// 2026-10-05T00:00:00Z, the fake clock's start; a minute boundary.
pub const T0: i64 = 1_791_158_400_000;

pub use tempfile::TempDir;

/// A fresh, empty directory under the system temp dir, removed on drop.
pub fn temp_dir(name: &str) -> TempDir {
    tempfile::Builder::new()
        .prefix(&format!("kelvo-engine-it-{name}-"))
        .tempdir()
        .unwrap()
}

pub fn host_record() -> HostRecord {
    HostRecord {
        id: HostId(Uuid::from_u128(7)),
        is_local: true,
        display_name: "test mac".into(),
        info: HostInfo {
            os: OsKind::MacOs,
            os_version: "27.0".into(),
            model: Some("Mac15,9".into()),
            chip: Some("Apple M3 Max".into()),
            chip_known: true,
            cpu_topology: vec![ClusterInfo {
                name: "P0".into(),
                kind: CoreKind::Performance,
                cores: vec!["P0".into()],
                dvfs_mhz: vec![1260, 4056],
            }],
            mem_total_bytes: 36 << 30,
            boot_time_ms: T0 - 86_400_000,
            gpu_dvfs_mhz: Vec::new(),
            boot_mounts: Vec::new(),
        },
    }
}

pub fn key(s: &str) -> SeriesKey {
    SeriesKey::parse(s).unwrap()
}

#[derive(Default)]
pub struct FakeState {
    pub keys: Vec<SeriesKey>,
    /// Overrides the probe result (Unsupported, NotPresent).
    pub probe: Option<Probe>,
    /// Push nothing on sample when false.
    pub emit: bool,
    pub samples: u32,
    pub probes: u32,
    /// Push this many process rows per sample.
    pub processes: u32,
    /// Reported as the primary interface on the next sample only, as the network
    /// collector does on probe and its link-rate cadence.
    pub primary_iface: Option<kelvo_collect::PrimaryIface>,
}

/// The test's handle on a [`FakeCollector`].
#[derive(Clone)]
pub struct FakeHandle(pub Arc<Mutex<FakeState>>);

impl FakeHandle {
    pub fn set_keys(&self, keys: &[&str]) {
        self.0.lock().unwrap().keys = keys.iter().map(|k| key(k)).collect();
    }
    pub fn set_emit(&self, emit: bool) {
        self.0.lock().unwrap().emit = emit;
    }
    pub fn set_probe(&self, probe: Option<Probe>) {
        self.0.lock().unwrap().probe = probe;
    }
    pub fn set_primary_iface(&self, iface: Option<&str>) {
        self.0.lock().unwrap().primary_iface = Some(iface.map(Arc::from));
    }
    pub fn samples(&self) -> u32 {
        self.0.lock().unwrap().samples
    }
    pub fn probes(&self) -> u32 {
        self.0.lock().unwrap().probes
    }
}

/// Emits `tick.n + 1000 * i` for its i-th key on every due tick.
pub struct FakeCollector {
    id: &'static str,
    cadence: Cadence,
    modules: &'static [Module],
    h: FakeHandle,
}

pub fn fake(
    id: &'static str,
    cadence: Cadence,
    modules: &'static [Module],
    keys: &[&str],
) -> (Box<dyn Collector>, FakeHandle) {
    let h = FakeHandle(Arc::new(Mutex::new(FakeState {
        keys: keys.iter().map(|k| key(k)).collect(),
        emit: true,
        ..FakeState::default()
    })));
    (
        Box::new(FakeCollector {
            id,
            cadence,
            modules,
            h: h.clone(),
        }),
        h,
    )
}

impl Collector for FakeCollector {
    fn id(&self) -> CollectorId {
        CollectorId(self.id)
    }
    fn cadence(&self) -> Cadence {
        self.cadence
    }
    fn required_entitlements(&self) -> &'static [Entitlement] {
        &[Entitlement::None]
    }
    fn modules(&self) -> &'static [Module] {
        self.modules
    }
    fn probe(&mut self) -> Probe {
        let mut s = self.h.0.lock().unwrap();
        s.probes += 1;
        s.probe
            .clone()
            .unwrap_or_else(|| Probe::Supported(s.keys.clone()))
    }
    fn sample(&mut self, tick: &Tick, out: &mut SampleBuf) -> Result<(), CollectError> {
        let mut s = self.h.0.lock().unwrap();
        s.samples += 1;
        if s.emit {
            for (i, k) in s.keys.iter().enumerate() {
                out.push(k, tick.n as f32 + 1000.0 * i as f32);
            }
        }
        if let Some(iface) = s.primary_iface.take() {
            out.set_primary_iface(iface);
        }
        for p in 0..s.processes {
            out.push_process(ProcessSample {
                pid: 100 + p as i32,
                start_time_us: 1,
                name: format!("proc{p}").into(),
                cpu_pct: p as f32,
                mem_bytes: 1 << 20,
                compressed_bytes: None,
                threads: 4,
                idle_wakeups_per_s: 1.0,
                energy: 0.5,
                energy_j: 5.0,
                interval_s: 1.0,
                app: None,
                app_main: false,
                disk_read_bps: 0.0,
                disk_write_bps: 0.0,
                net_rx_bps: None,
                net_tx_bps: None,
                gpu_pct: None,
                ports: None,
                user: "me".into(),
            });
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct FakeNetState {
    pub probe: Option<Probe>,
    pub samples: u32,
    pub releases: u32,
    /// Sampled since the last release: the next sample is a baseline otherwise.
    pub open: bool,
    /// The previous sample's continuous time, while open.
    pub last_ns: Option<u64>,
    /// When each sample was taken (`Tick::n`).
    pub sampled_on: Vec<u64>,
}

/// Identity of the fake's pid 100; pid 101 has none ("other apps").
pub const FAKE_APP: &str = "Alpha";

/// A per-process network collector: on demand, a baseline after each release, then
/// rates `rx = 1000 * (pid - 99)` B/s and `tx = rx / 10` for pids 100 (identity
/// [`FAKE_APP`]) and 101 (no identity), with bytes over the real interval since the
/// previous sample.
pub struct FakeNet(pub Arc<Mutex<FakeNetState>>);

pub fn fake_net() -> (Box<dyn Collector>, Arc<Mutex<FakeNetState>>) {
    let s = Arc::new(Mutex::new(FakeNetState::default()));
    (Box::new(FakeNet(Arc::clone(&s))), s)
}

impl Collector for FakeNet {
    fn id(&self) -> CollectorId {
        CollectorId("net_per_process")
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
        let s = self.0.lock().unwrap();
        s.probe.clone().unwrap_or(Probe::Supported(Vec::new()))
    }
    fn sample(&mut self, tick: &Tick, out: &mut SampleBuf) -> Result<(), CollectError> {
        let mut s = self.0.lock().unwrap();
        s.samples += 1;
        s.sampled_on.push(tick.n);
        let prev = s.last_ns.replace(tick.continuous_ns);
        if !std::mem::replace(&mut s.open, true) {
            return Ok(());
        }
        let prev_ns = prev.unwrap_or(tick.continuous_ns);
        out.set_process_net_measured(Interval {
            prev_ns,
            now_ns: tick.continuous_ns,
        });
        let secs = (tick.continuous_ns - prev_ns) / 1_000_000_000;
        for pid in [101, 100] {
            let rx = 1000 * (pid - 99) as u64;
            out.push_process_net(ProcessNet {
                pid,
                identity: (pid == 100).then(|| Arc::from(FAKE_APP)),
                rx_bytes: rx * secs,
                tx_bytes: rx / 10 * secs,
                rx_bps: rx as f32,
                tx_bps: rx as f32 / 10.0,
                late_rx_bytes: 0,
                late_tx_bytes: 0,
            });
        }
        Ok(())
    }
    fn release(&mut self) {
        let mut s = self.0.lock().unwrap();
        s.releases += 1;
        s.open = false;
        s.last_ns = None;
    }
}

/// Interface totals of [`FakeIface`]: bytes and packets per second.
pub const IFACE_RX_BPS: u64 = 5_000;
pub const IFACE_TX_BPS: u64 = 600;

/// The network collector's interface totals: `Cadence` LIVE_OR_IDLE like the real one,
/// a baseline after each probe, then [`IFACE_RX_BPS`] and [`IFACE_TX_BPS`] over the real
/// interval, one packet per 100 bytes.
pub struct FakeIface(pub Arc<Mutex<FakeNetState>>);

pub fn fake_iface() -> (Box<dyn Collector>, Arc<Mutex<FakeNetState>>) {
    let s = Arc::new(Mutex::new(FakeNetState::default()));
    (Box::new(FakeIface(Arc::clone(&s))), s)
}

impl Collector for FakeIface {
    fn id(&self) -> CollectorId {
        CollectorId("network")
    }
    fn cadence(&self) -> Cadence {
        kelvo_collect::LIVE_OR_IDLE
    }
    fn required_entitlements(&self) -> &'static [Entitlement] {
        &[Entitlement::None]
    }
    fn modules(&self) -> &'static [Module] {
        &[Module::Network]
    }
    fn probe(&mut self) -> Probe {
        let mut s = self.0.lock().unwrap();
        s.last_ns = None;
        s.probe.clone().unwrap_or(Probe::Supported(Vec::new()))
    }
    fn sample(&mut self, tick: &Tick, out: &mut SampleBuf) -> Result<(), CollectError> {
        let mut s = self.0.lock().unwrap();
        s.samples += 1;
        s.sampled_on.push(tick.n);
        let Some(prev_ns) = s.last_ns.replace(tick.continuous_ns) else {
            return Ok(());
        };
        let secs = (tick.continuous_ns - prev_ns) / 1_000_000_000;
        out.set_iface_net(IfaceNet {
            interval: Interval {
                prev_ns,
                now_ns: tick.continuous_ns,
            },
            rx_bytes: IFACE_RX_BPS * secs,
            tx_bytes: IFACE_TX_BPS * secs,
            rx_packets: IFACE_RX_BPS * secs / 100,
            tx_packets: IFACE_TX_BPS * secs / 100,
        });
        Ok(())
    }
}

/// A per-process GPU collector: on demand, a baseline after each release, then 30% for
/// pid 100 and 12.5% for pid 102. Shares [`FakeNetState`] for its bookkeeping.
pub struct FakeGpu(pub Arc<Mutex<FakeNetState>>);

pub fn fake_gpu() -> (Box<dyn Collector>, Arc<Mutex<FakeNetState>>) {
    let s = Arc::new(Mutex::new(FakeNetState::default()));
    (Box::new(FakeGpu(Arc::clone(&s))), s)
}

impl Collector for FakeGpu {
    fn id(&self) -> CollectorId {
        CollectorId("gpu_per_process")
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
        let s = self.0.lock().unwrap();
        s.probe.clone().unwrap_or(Probe::Supported(Vec::new()))
    }
    fn sample(&mut self, tick: &Tick, out: &mut SampleBuf) -> Result<(), CollectError> {
        let mut s = self.0.lock().unwrap();
        s.samples += 1;
        let prev = s.last_ns.replace(tick.continuous_ns);
        if !std::mem::replace(&mut s.open, true) {
            return Ok(());
        }
        // Shares of the wall time since the previous pass, as the real collector's.
        let span_ns = prev.map_or(0, |p| tick.continuous_ns.saturating_sub(p));
        out.set_process_gpu_measured(span_ns / 1_000_000);
        out.push_process_gpu(ProcessGpu {
            pid: 102,
            pct: 12.5,
        });
        out.push_process_gpu(ProcessGpu {
            pid: 100,
            pct: 30.0,
        });
        Ok(())
    }
    fn release(&mut self) {
        let mut s = self.0.lock().unwrap();
        s.releases += 1;
        s.open = false;
        s.last_ns = None;
    }
}

pub fn unsupported() -> Probe {
    Probe::Unsupported {
        reason: UnsupportedReason::UnknownChip,
    }
}

/// An engine on a fake ticker and fake power signals, with a real store, driven on the
/// test thread.
pub struct Harness {
    pub engine: Engine,
    pub ctl: EngineControl,
    pub clock: FakeClock,
    pub power: FakePower,
    pub sub: Subscriber,
    pub bus: Bus,
    /// The host's live hub the engine publishes through: ring, latest frame, layout.
    pub live: LiveHub,
    pub store: Option<Store>,
    pub host: HostId,
    pub dir: TempDir,
}

pub fn settings_all_on() -> Settings {
    let mut s = Settings::default();
    for m in s.modules.values_mut() {
        m.enabled = true;
    }
    s
}

impl Harness {
    pub fn new(name: &str, collectors: Vec<Box<dyn Collector>>, settings: &Settings) -> Self {
        Self::with_bus(name, collectors, settings, Bus::default())
    }

    pub fn with_bus(
        name: &str,
        collectors: Vec<Box<dyn Collector>>,
        settings: &Settings,
        bus: Bus,
    ) -> Self {
        Self::build(name, collectors, settings, bus, Some(DEFAULT_PRUNE_BATCH))
    }

    /// A store that prunes `batch` rows per transaction, so a test gets many batches from
    /// few rows.
    pub fn with_prune_batch(
        name: &str,
        collectors: Vec<Box<dyn Collector>>,
        settings: &Settings,
        batch: u64,
    ) -> Self {
        Self::build(name, collectors, settings, Bus::default(), Some(batch))
    }

    /// No store at all: history unavailable, the engine runs live-only.
    pub fn live_only(name: &str, collectors: Vec<Box<dyn Collector>>, settings: &Settings) -> Self {
        Self::build(name, collectors, settings, Bus::default(), None)
    }

    /// `prune_batch`: the store's batch size, `None` for no store.
    fn build(
        name: &str,
        collectors: Vec<Box<dyn Collector>>,
        settings: &Settings,
        bus: Bus,
        prune_batch: Option<u64>,
    ) -> Self {
        let dir = temp_dir(name);
        let record = host_record();
        let store = prune_batch.map(|batch| {
            let store = open_store_with(&dir, batch);
            store.writer().upsert_host(record.clone()).unwrap();
            store
        });
        let (ticker, clock) = FakeTicker::new();
        let (power_signals, power) = FakePowerSignals::new();
        let sub = bus.subscribe();
        let live = LiveHub::new(bus.clone());
        let engine = Engine::new(
            record.id,
            EngineParts {
                collectors,
                ticker: Box::new(ticker),
                power: Box::new(power_signals),
                hints: None,
            },
            SourceSink {
                live: live.clone(),
                store: store.as_ref().map(Store::writer),
            },
            settings,
        );
        let ctl = engine.control();
        let mut h = Self {
            engine,
            ctl,
            clock,
            power,
            sub,
            bus,
            live,
            store,
            host: record.id,
            dir,
        };
        h.engine.start();
        assert!(h.engine.pump());
        h
    }

    /// One tick, processed. Returns whether the ticker delivered it.
    pub fn tick(&mut self) -> bool {
        let delivered = self.clock.tick();
        self.engine.pump();
        delivered
    }

    pub fn ticks(&mut self, n: usize) {
        for _ in 0..n {
            assert!(self.tick(), "ticker not running");
        }
    }

    pub fn pump(&mut self) {
        self.engine.pump();
    }

    /// A window that shows detail is visible: the engine leaves the background and ticks
    /// at the visible interval (D-094). For tests written for a 1 s tick.
    pub fn visible(&mut self) {
        drop(self.ctl.set_detail_interest(true));
        self.engine.pump();
    }

    /// Everything published since the last drain.
    pub fn drain(&mut self) -> Vec<BusMsg> {
        let mut out = Vec::new();
        while let Some(m) = self.sub.try_recv() {
            out.push(m);
        }
        out
    }

    pub fn frames(&mut self) -> Vec<Arc<LiveFrame>> {
        self.drain()
            .into_iter()
            .filter_map(|m| match m {
                BusMsg::Frame(f) => Some(f),
                _ => None,
            })
            .collect()
    }

    pub fn reader(&self) -> Reader {
        let store = self.store.as_ref().unwrap();
        store.writer().flush().unwrap();
        store.reader().unwrap()
    }
}

/// A store in `dir` that commits only on flush.
pub fn open_store(dir: &TempDir) -> Store {
    open_store_with(dir, DEFAULT_PRUNE_BATCH)
}

/// [`open_store`] pruning `prune_batch` rows per transaction.
pub fn open_store_with(dir: &TempDir, prune_batch: u64) -> Store {
    let mut cfg = StoreConfig::new(dir.path().join("history.sqlite"));
    cfg.commit_interval = Duration::from_secs(3600);
    cfg.prune_batch = prune_batch;
    Store::open(cfg).unwrap()
}

/// The value of `k` in a frame's raw values or held values.
pub fn value(frame: &LiveFrame, k: &str, held: bool) -> f32 {
    let i = frame
        .layout
        .series
        .iter()
        .position(|s| *s == key(k))
        .unwrap_or_else(|| panic!("{k} not in layout {:?}", frame.layout.series));
    if held { frame.held[i] } else { frame.values[i] }
}
