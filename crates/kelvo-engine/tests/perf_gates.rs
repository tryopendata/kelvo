//! Performance gates on the real macOS collectors (D-062). Thresholds live in
//! `perf-budget.json` at the repo root, section `engine`; raising one needs a decision
//! entry.
//!
//! - Allocations per tick in steady state, per collector and for the engine core, counted
//!   by a global allocator on the test thread (the engine runs on it here). Only Rust heap
//!   allocations are seen: CoreFoundation and IOKit allocate through `malloc` directly.
//! - OS calls per tick by API family (`kelvo_collect::calls`), tray-only (with network
//!   history on, the default, and off), with a window open (process and detail
//!   interest), with a window showing per-process network rates, and with one showing
//!   per-process GPU time. Tray-only with network history off allows zero
//!   NetworkStatistics calls (D-082's zero idle cost); with it on, one query per process
//!   sample (D-089). No mode but the GPU one has an IOKit ceiling with room for a walk of
//!   the GPU's user clients.
//! - Store write volume per hour at 1 s, from a 10-minute run on a real SQLite file:
//!   rows and payload bytes (per-app network buckets included), plus commits and bytes appended to the WAL at the default
//!   commit interval (D-070). `wal_volume_by_commit_interval` (ignored, by hand)
//!   compares commit intervals over an hour.
//! - Coverage, so the gates above cannot pass by measuring nothing (D-067): every
//!   collector that probed `Supported` must take at least one sample per its slowest
//!   period, and on real Apple Silicon (not a VM) the IOReport, SMC and HID collectors
//!   must probe `Supported` and push values without an error in most samples (D-070).
//!   Elsewhere the test prints `[perf] skipped: no hardware`, which the CI step
//!   surfaces as a notice.
//!
//! The engine runs on the fake ticker, so 10 minutes take a few seconds; the collectors
//! read the real machine. A window's modes tick at 1 s, tray-only at the background's
//! 2 s (D-094), with the processes collector every 30 s and the temperature collectors
//! every 10 s. Each test prints what it measured.

#![cfg(target_os = "macos")]
#![allow(clippy::unwrap_used, clippy::print_stdout)]

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use common::*;
use kelvo_collect::calls::{self, Api, Calls};
use kelvo_collect::{
    Cadence, CollectError, Collector, CollectorId, Probe, SampleBuf, Tick, allowed_entitlements,
    filter_by_entitlements, platform_collectors,
};
use kelvo_schema::{Entitlement, Module};
use serde_json::Value;

// --- counting allocator ---------------------------------------------------------------

struct Counting;

thread_local! {
    static ALLOCS: Cell<u64> = const { Cell::new(0) };
}

// SAFETY: forwards to the system allocator unchanged; the counter is a const-initialised
// thread-local Cell with no destructor, so touching it never allocates or re-enters.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.with(|c| c.set(c.get() + 1));
        // SAFETY: same contract as the caller's.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOCS.with(|c| c.set(c.get() + 1));
        // SAFETY: same contract as the caller's.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCS.with(|c| c.set(c.get() + 1));
        // SAFETY: same contract as the caller's.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: same contract as the caller's.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn allocs() -> u64 {
    ALLOCS.with(Cell::get)
}

// --- a collector wrapper that attributes allocations and calls ------------------------

#[derive(Default)]
struct Stat {
    samples: AtomicU64,
    /// Samples that returned an error.
    errors: AtomicU64,
    /// Samples that returned `Ok` but pushed no value and no process row.
    empty: AtomicU64,
    allocs: AtomicU64,
    calls: [AtomicU64; Api::ALL.len()],
    /// The last probe said `Supported`.
    supported: AtomicBool,
    /// Process rows whose `(pid, start_time_us)` was not in the previous sample's rows:
    /// processes that started (or became readable) since.
    new_rows: AtomicU64,
}

#[derive(Clone, Copy, Debug, Default)]
struct StatSnap {
    samples: u64,
    errors: u64,
    empty: u64,
    allocs: u64,
    calls: Calls,
    new_rows: u64,
}

impl Stat {
    fn snap(&self) -> StatSnap {
        let mut calls = Calls::default();
        for (c, a) in calls.0.iter_mut().zip(&self.calls) {
            *c = a.load(Ordering::Relaxed);
        }
        StatSnap {
            samples: self.samples.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
            empty: self.empty.load(Ordering::Relaxed),
            allocs: self.allocs.load(Ordering::Relaxed),
            calls,
            new_rows: self.new_rows.load(Ordering::Relaxed),
        }
    }
}

struct Measured {
    inner: Box<dyn Collector>,
    stat: Arc<Stat>,
    /// Process row keys of the last sample that pushed rows, and this one's (swapped).
    rows: HashSet<(i32, i64)>,
    next_rows: HashSet<(i32, i64)>,
}

thread_local! {
    /// Allocations the wrapper's own bookkeeping made, kept out of every count.
    static HARNESS: Cell<u64> = const { Cell::new(0) };
}

fn harness_allocs() -> u64 {
    HARNESS.with(Cell::get)
}

impl Collector for Measured {
    fn id(&self) -> CollectorId {
        self.inner.id()
    }
    fn cadence(&self) -> Cadence {
        self.inner.cadence()
    }
    fn modules(&self) -> &'static [Module] {
        self.inner.modules()
    }
    fn required_entitlements(&self) -> &'static [Entitlement] {
        self.inner.required_entitlements()
    }
    fn probe(&mut self) -> Probe {
        let p = self.inner.probe();
        self.stat
            .supported
            .store(matches!(p, Probe::Supported(_)), Ordering::Relaxed);
        p
    }
    fn sample(&mut self, tick: &Tick, out: &mut SampleBuf) -> Result<(), CollectError> {
        let pushed = |out: &SampleBuf| {
            out.values().len()
                + out.processes().len()
                + out.process_net().len()
                + out.process_gpu().len()
        };
        let before = pushed(out);
        let before_rows = out.processes().len();
        let (a0, c0) = (allocs(), calls::current());
        let r = self.inner.sample(tick, out);
        let (a1, c1) = (allocs(), calls::current());
        let s = &self.stat;
        s.samples.fetch_add(1, Ordering::Relaxed);
        if r.is_err() {
            s.errors.fetch_add(1, Ordering::Relaxed);
        } else if pushed(out) == before {
            s.empty.fetch_add(1, Ordering::Relaxed);
        }
        s.allocs.fetch_add(a1 - a0, Ordering::Relaxed);
        for (slot, n) in s.calls.iter().zip(c1.since(&c0).0) {
            slot.fetch_add(n, Ordering::Relaxed);
        }
        let rows = out.processes().get(before_rows..).unwrap_or_default();
        if !rows.is_empty() {
            self.next_rows.clear();
            self.next_rows
                .extend(rows.iter().map(|p| (p.pid, p.start_time_us)));
            let new = self.next_rows.difference(&self.rows).count() as u64;
            s.new_rows.fetch_add(new, Ordering::Relaxed);
            std::mem::swap(&mut self.rows, &mut self.next_rows);
        }
        HARNESS.with(|h| h.set(h.get() + (allocs() - a1)));
        r
    }
}

/// Collector id, its cadence, and what it did.
type Stats = Vec<(&'static str, Cadence, Arc<Stat>)>;

fn measured_collectors() -> (Vec<Box<dyn Collector>>, Stats) {
    let mut stats = Vec::new();
    let all = platform_collectors(Arc::new(kelvo_collect::calib::NoScaleStore));
    let collectors = filter_by_entitlements(all, allowed_entitlements())
        .into_iter()
        .map(|inner| {
            let stat = Arc::new(Stat::default());
            stats.push((inner.id().0, inner.cadence(), Arc::clone(&stat)));
            Box::new(Measured {
                inner,
                stat,
                rows: HashSet::new(),
                next_rows: HashSet::new(),
            }) as Box<dyn Collector>
        })
        .collect();
    (collectors, stats)
}

// --- the gate must have measured something ---------------------------------------------

/// The fewest samples a collector that probed `Supported` must take in `ticks` ticks,
/// whatever the interests: one per its slowest period, less one for phase, and at least
/// one. `OnDemand` collectors may legitimately take none. The background slows the
/// processes collector to 30 s and the temperature collectors to 10 s (D-094).
fn min_samples(id: &str, cadence: Cadence, ticks: u64, mode: Mode) -> u64 {
    let tick_ms = mode.tick_ms();
    let mut slowest_ms = match cadence {
        Cadence::EveryTick => tick_ms,
        Cadence::Every(ms) | Cadence::Adaptive { idle_ms: ms, .. } => ms.max(tick_ms),
        Cadence::OnDemand(_) => return 0,
    };
    if mode.backgrounded() {
        if matches!(cadence, Cadence::Adaptive { .. }) {
            slowest_ms = slowest_ms.max(kelvo_engine::PERFORMANCE_IDLE_PROCESS_MS);
        }
        if kelvo_collect::TEMPERATURE_COLLECTORS
            .iter()
            .any(|c| c.0 == id)
        {
            slowest_ms = slowest_ms.max(kelvo_engine::PERFORMANCE_IDLE_SENSOR_MS);
        }
    }
    (ticks * u64::from(tick_ms) / u64::from(slowest_ms))
        .saturating_sub(1)
        .max(1)
}

/// The private-API collectors every Apple Silicon Mac has. On real hardware each must
/// probe `Supported`, or the call and allocation gates would pass with nothing measured.
const HARDWARE_COLLECTORS: &[&str] = &["ioreport", "smc.power", "smc.sensors", "hid.thermal"];

/// Why this machine cannot be held to [`HARDWARE_COLLECTORS`], if it cannot.
fn no_hardware_reason() -> Option<&'static str> {
    if cfg!(feature = "appstore") {
        return Some("appstore edition drops the private-API collectors");
    }
    if !cfg!(target_arch = "aarch64") {
        return Some("not Apple Silicon");
    }
    let mut vm: libc::c_int = 0;
    let mut len = std::mem::size_of::<libc::c_int>();
    // SAFETY: a NUL-terminated name and an int-sized output buffer we own.
    let rc = unsafe {
        libc::sysctlbyname(
            c"kern.hv_vmm_present".as_ptr(),
            (&raw mut vm).cast(),
            &raw mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    // Hosted CI runners are virtual machines: no SMC, HID sensors or IOReport energy.
    (rc == 0 && vm == 1).then_some("virtual machine")
}

/// Failures for collectors that should have run and did not.
fn coverage_failures(r: &Run, stats: &Stats, mode: Mode) -> Vec<String> {
    let mut failures = Vec::new();
    for (id, cadence, stat) in stats {
        let supported = stat.supported.load(Ordering::Relaxed);
        let samples = r.per_collector.get(id).map_or(0, |s| s.samples);
        let need = min_samples(id, *cadence, r.ticks, mode);
        if supported && samples < need {
            failures.push(format!(
                "{id}: {samples} samples in {} ticks, expected at least {need}",
                r.ticks
            ));
        }
    }
    match no_hardware_reason() {
        Some(why) => println!(
            "[perf] skipped: no hardware ({why}); {} are not required to probe Supported",
            HARDWARE_COLLECTORS.join(", ")
        ),
        None => {
            for id in HARDWARE_COLLECTORS {
                let supported = stats
                    .iter()
                    .find(|(sid, _, _)| sid == id)
                    .is_some_and(|(_, _, s)| s.supported.load(Ordering::Relaxed));
                if !supported {
                    failures.push(format!(
                        "{id}: not Supported on Apple Silicon hardware, so its budgets measured nothing"
                    ));
                    continue;
                }
                // Sampled is not measured: a collector that errors or pushes nothing on
                // most samples costs almost nothing and would pass every budget.
                let s = r.per_collector.get(id).copied().unwrap_or_default();
                if s.errors + s.empty > s.samples / 2 {
                    failures.push(format!(
                        "{id}: {} errors and {} empty of {} samples, so its budgets measured nothing",
                        s.errors, s.empty, s.samples
                    ));
                }
            }
        }
    }
    failures
}

// --- budget -----------------------------------------------------------------------------

fn budget() -> Value {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../perf-budget.json");
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let v: Value = serde_json::from_str(&text).unwrap();
    v["engine"].clone()
}

fn num(v: &Value, path: &str) -> f64 {
    let mut cur = v;
    for p in path.split('.') {
        cur = &cur[p];
    }
    cur.as_f64()
        .unwrap_or_else(|| panic!("perf-budget.json engine.{path} missing"))
}

// --- the run ----------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    /// Network history on (the default): NetworkStatistics on the process ticks.
    TrayOnly,
    /// Network history off: D-082's tray-only state, no NetworkStatistics at all.
    TrayOnlyHistoryOff,
    Window,
    /// A window showing per-process network rates (D-081): the Overview's Network card or
    /// the Processes page's Network columns.
    WindowNetwork,
    /// A window showing per-process GPU time: the Overview's GPU card, the GPU page's
    /// process table or the Processes page's GPU column.
    WindowGpu,
}

impl Mode {
    fn name(self) -> &'static str {
        match self {
            Mode::TrayOnly => "trayOnly",
            Mode::TrayOnlyHistoryOff => "trayOnlyHistoryOff",
            Mode::Window => "window",
            Mode::WindowNetwork => "windowNetwork",
            Mode::WindowGpu => "windowGpu",
        }
    }

    /// No window shows detail: the engine is in the background (D-094).
    fn backgrounded(self) -> bool {
        matches!(self, Mode::TrayOnly | Mode::TrayOnlyHistoryOff)
    }

    fn tick_ms(self) -> u32 {
        if self.backgrounded() {
            kelvo_engine::BACKGROUND_TICK_MS
        } else {
            1_000
        }
    }
}

struct Run {
    ticks: u64,
    tick_allocs: u64,
    per_collector: BTreeMap<&'static str, StatSnap>,
    calls: Calls,
    samples: u64,
}

/// Warms up past every collector's slowest period (60 s), then measures `ticks` ticks.
fn run(mode: Mode, warmup: u64, ticks: u64, h: &mut Harness, stats: &Stats) -> Run {
    if mode == Mode::TrayOnlyHistoryOff {
        h.ctl.set_network_history(false);
    }
    if !mode.backgrounded() {
        h.ctl.set_process_interest(Some(0));
        h.visible();
    }
    if mode == Mode::WindowNetwork {
        h.ctl.set_network_process_interest(true);
    }
    if mode == Mode::WindowGpu {
        h.ctl.set_gpu_process_interest(true);
    }
    for _ in 0..warmup {
        h.tick();
        h.drain();
    }
    let before: Vec<StatSnap> = stats.iter().map(|(_, _, s)| s.snap()).collect();
    let (a0, h0) = (allocs(), harness_allocs());
    for _ in 0..ticks {
        assert!(h.tick());
        // The bus keeps frames for slow readers; draining keeps that out of the counts
        // and matches a live app, where the shell reads every frame.
        while h.sub.try_recv().is_some() {}
    }
    let tick_allocs = allocs() - a0 - (harness_allocs() - h0);
    let mut per_collector = BTreeMap::new();
    let mut calls = Calls::default();
    let mut samples = 0;
    for ((id, _, s), b) in stats.iter().zip(before) {
        let a = s.snap();
        let d = StatSnap {
            samples: a.samples - b.samples,
            errors: a.errors - b.errors,
            empty: a.empty - b.empty,
            allocs: a.allocs - b.allocs,
            calls: a.calls.since(&b.calls),
            new_rows: a.new_rows - b.new_rows,
        };
        calls.add(&d.calls);
        samples += d.samples;
        per_collector.insert(*id, d);
    }
    Run {
        ticks,
        tick_allocs,
        per_collector,
        calls,
        samples,
    }
}

fn check(mode: Mode) {
    let b = budget();
    let warmup = num(&b, "warmupTicks") as u64;
    let ticks = num(&b, "ticks") as u64;
    let (collectors, stats) = measured_collectors();
    let mut h = Harness::new(mode.name(), collectors, &settings_all_on());
    let r = run(mode, warmup, ticks, &mut h, &stats);
    let per_tick = |n: u64| n as f64 / r.ticks as f64;

    let collector_allocs: u64 = r.per_collector.values().map(|s| s.allocs).sum();
    let core = r.tick_allocs - collector_allocs;
    println!(
        "[perf] {} {} ticks: {} samples, allocs/tick total {:.2}, engine core {:.2}",
        mode.name(),
        r.ticks,
        r.samples,
        per_tick(r.tick_allocs),
        per_tick(core)
    );
    let mut failures = coverage_failures(&r, &stats, mode);
    let ceiling = num(&b, &format!("allocsPerTick.{}.engine", mode.name()));
    if per_tick(core) > ceiling {
        failures.push(format!(
            "engine core {:.2} allocs/tick > {ceiling}",
            per_tick(core)
        ));
    }
    for (id, s) in &r.per_collector {
        let calls: Vec<String> = Api::ALL
            .iter()
            .filter(|a| s.calls.get(**a) > 0)
            .map(|a| format!("{} {:.2}", a.name(), per_tick(s.calls.get(*a))))
            .collect();
        println!(
            "[perf]   {id:<14} {:>4} samples ({} err, {} empty)  allocs/tick {:>7.2}  calls/tick {}",
            s.samples,
            s.errors,
            s.empty,
            per_tick(s.allocs),
            calls.join(", ")
        );
        // The processes collector allocates one name per process it has not seen, so
        // its count follows how many processes the machine started during the run.
        if s.new_rows > 0 {
            println!(
                "[perf]   {:<14} {} new process rows (one name allocation each)",
                "", s.new_rows
            );
        }
        let key = format!("allocsPerTick.{}.collectors.{id}", mode.name());
        let ceiling = b["allocsPerTick"][mode.name()]["collectors"][*id]
            .as_f64()
            .unwrap_or_else(|| {
                num(
                    &b,
                    &format!("allocsPerTick.{}.collectors._default", mode.name()),
                )
            });
        if per_tick(s.allocs) > ceiling {
            failures.push(format!("{key}: {:.2} > {ceiling}", per_tick(s.allocs)));
        }
    }
    if calls::ENABLED {
        // libproc calls scale with the number of processes on the machine, so their
        // ceiling is per listed process.
        let pids = pid_count();
        for api in Api::ALL {
            let (got, name) = match api {
                Api::Libproc => (per_tick(r.calls.get(api)) / pids, "libprocPerProcess"),
                _ => (per_tick(r.calls.get(api)), api.name()),
            };
            let ceiling = num(&b, &format!("callsPerTick.{}.{name}", mode.name()));
            println!("[perf]   calls/tick {name:<17} {got:>7.2} (ceiling {ceiling})");
            if got > ceiling {
                failures.push(format!(
                    "callsPerTick.{}.{name}: {got:.2} > {ceiling}",
                    mode.name()
                ));
            }
        }
        println!("[perf]   ({pids} processes listed)");
    } else {
        failures.push("kelvo-collect built without call-counters".into());
    }
    assert!(failures.is_empty(), "over budget:\n{}", failures.join("\n"));
}

fn pid_count() -> f64 {
    // SAFETY: a size query with a null buffer; returns the number of pids.
    let n = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    f64::from(n.max(1))
}

#[test]
fn tray_only_ticks_stay_within_allocation_and_call_budgets() {
    check(Mode::TrayOnly);
}

/// With network history off, tray-only makes no NetworkStatistics call: the zero idle
/// cost of D-082, kept for users who turn the setting off (D-089).
#[test]
fn tray_only_ticks_with_network_history_off_make_no_network_statistics_calls() {
    check(Mode::TrayOnlyHistoryOff);
}

#[test]
fn window_open_ticks_stay_within_allocation_and_call_budgets() {
    check(Mode::Window);
}

/// D-083: the detectors and alert rules allocate nothing on a tick that fires nothing,
/// even with a process batch every tick (a window showing processes) and runs being
/// tracked. Events themselves allocate; they are rare.
#[test]
fn detectors_allocate_nothing_per_tick() {
    use kelvo_engine::ProcessSample;
    use kelvo_engine::detect::Detectors;
    use kelvo_schema::{AlertSettings, DetectorThresholds, SeriesKey};

    let series: Vec<SeriesKey> = [
        "fan.rpm{fan=0}",
        "fan.rpm{fan=1}",
        "thermal.state",
        "power.package",
        "power.ane",
    ]
    .iter()
    .map(|k| SeriesKey::parse(k).unwrap())
    .collect();
    let mut d = Detectors::new(
        DetectorThresholds::DEFAULT,
        AlertSettings {
            hot_process: true,
            thermal_serious: true,
        },
    );
    d.bind(&series);
    let rows: Vec<ProcessSample> = (0..1000)
        .map(|i| ProcessSample {
            pid: i,
            start_time_us: 1,
            name: format!("proc{i}").into(),
            // One process above both thresholds: a run is tracked every tick.
            cpu_pct: if i == 7 { 250.0 } else { (i % 50) as f32 },
            mem_bytes: 0,
            compressed_bytes: None,
            threads: 1,
            idle_wakeups_per_s: 0.0,
            energy: (i % 13) as f32,
            energy_j: 0.0,
            app: None,
            app_main: false,
            disk_read_bps: 0.0,
            disk_write_bps: 0.0,
            net_rx_bps: None,
            net_tx_bps: None,
            gpu_pct: None,
            user: "u".into(),
        })
        .collect();
    let mut out = Vec::with_capacity(16);
    let warmup = 130i64;
    let ticks = 120i64;
    let mut before = 0;
    for t in 0..warmup + ticks {
        if t == warmup {
            before = allocs();
        }
        let wobble = (t % 7) as f32;
        let values = [1800.0 + wobble, 1700.0, 0.0, 4.0 + wobble / 10.0, 0.0];
        d.on_tick(T0 + t * 1000, &values, &[1000; 5], Some(&rows), &mut out);
        // The sustained run reports at 120 s, inside warm-up; the alert needs 300 s.
        if t < warmup - 1 {
            out.clear();
        }
    }
    let per_tick = (allocs() - before) as f64 / ticks as f64;
    let ceiling = num(&budget(), "allocsPerTick.detectors");
    println!("[perf] detectors {ticks} ticks with 1,000-row batches: allocs/tick {per_tick:.2}");
    assert!(out.is_empty(), "nothing should fire after warm-up: {out:?}");
    assert!(
        per_tick <= ceiling,
        "detectors allocate {per_tick:.2}/tick, ceiling {ceiling}"
    );
}

/// With network rates shown the collector queries NetworkStatistics once per process
/// sample, here every tick.
#[test]
fn network_process_ticks_stay_within_allocation_and_call_budgets() {
    check(Mode::WindowNetwork);
}

/// With GPU time shown the collector walks the GPU's user clients once per process
/// sample (IOKit calls); without, the IOKit ceilings of the modes above leave no room for
/// that walk, which is the zero-idle-cost check for per-process GPU (D-085).
#[test]
fn gpu_process_ticks_stay_within_allocation_and_call_budgets() {
    check(Mode::WindowGpu);
}

/// What one tray-only store run wrote to the WAL, scaled to an hour.
struct WalVolume {
    series: usize,
    commits_h: f64,
    wal_frames_h: f64,
    wal_bytes_h: f64,
}

/// Runs tray-only (2 s ticks in the background, D-094) for `minutes` of fake time,
/// committing every `commit_s` seconds of it.
/// The writer's own commit timer runs on wall time, which the fake ticker outpaces by
/// orders of magnitude, so the harness store never commits on its own; the flushes here
/// stand in for that timer. Returns the harness, the file growth and the WAL volume.
fn store_run(name: &str, minutes: u64, commit_s: u64) -> (Harness, u64, WalVolume) {
    let (collectors, _) = measured_collectors();
    let mut h = Harness::new(name, collectors, &settings_all_on());
    let path = h.dir.0.join("history.sqlite");
    let size = |p: &std::path::Path| std::fs::metadata(p).map_or(0, |m| m.len());
    let writer = h.store.as_ref().unwrap().writer();
    writer.flush().unwrap();
    let file0 = size(&path) + size(&path.with_extension("sqlite-wal"));
    let w0 = writer.write_stats().unwrap();
    // Tray-only is the app's resting state; processes still land every 30 s.
    let start = h.clock.now().wall_ms;
    let elapsed_s = |h: &Harness| u64::try_from(h.clock.now().wall_ms - start).unwrap() / 1_000;
    let mut committed_s = 0;
    while elapsed_s(&h) < minutes * 60 {
        assert!(h.tick());
        while h.sub.try_recv().is_some() {}
        if elapsed_s(&h) >= committed_s + commit_s {
            committed_s = elapsed_s(&h) / commit_s * commit_s;
            writer.flush().unwrap();
        }
    }
    writer.flush().unwrap();
    let w1 = writer.write_stats().unwrap();
    let file1 = size(&path) + size(&path.with_extension("sqlite-wal"));
    let scale = 60.0 / minutes as f64;
    let frames = (w1.wal_frames - w0.wal_frames) as f64;
    let wal = WalVolume {
        series: h.live.layout().map_or(0, |l| l.series.len()),
        commits_h: (w1.commits - w0.commits) as f64 * scale,
        wal_frames_h: frames * scale,
        wal_bytes_h: frames * (w1.page_size + 24) as f64 * scale,
    };
    (h, file1.saturating_sub(file0), wal)
}

fn print_wal(label: &str, commit_s: u64, w: &WalVolume) {
    println!(
        "[perf] store WAL ({label}, commit every {commit_s} s, {} series): {:.0} commits/h, {:.0} frames/h, {:.0} WAL bytes/h ({:.1} MB/h)",
        w.series,
        w.commits_h,
        w.wal_frames_h,
        w.wal_bytes_h,
        w.wal_bytes_h / 1e6
    );
}

/// The WAL volume at the old (30 s) and current (5 min) commit intervals over an hour
/// of fake time, side by side (D-070). Slow (two hours of real collectors on the fake
/// ticker), so run by hand:
/// `cargo test -p kelvo-engine --test perf_gates wal_volume_by_commit_interval -- --ignored --nocapture`
#[test]
#[ignore = "an hour of fake time per interval; run by hand to compare commit intervals"]
fn wal_volume_by_commit_interval() {
    for commit_s in [30, 300] {
        let (_, growth, w) = store_run(&format!("wal-{commit_s}"), 60, commit_s);
        print_wal("60 min run", commit_s, &w);
        println!("[perf]   file growth {growth} bytes in the hour");
    }
}

#[test]
fn store_write_volume_per_hour_at_one_second() {
    let b = budget();
    let minutes = num(&b, "store.minutes") as u64;
    let commit_s = kelvo_store::DEFAULT_COMMIT_INTERVAL.as_secs();
    let (h, growth, wal) = store_run("store-volume", minutes, commit_s);
    let path = h.dir.0.join("history.sqlite");
    print_wal(&format!("{minutes} min run"), commit_s, &wal);
    let db =
        rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let scale = 60.0 / minutes as f64;
    let mut rows_h = 0.0;
    let mut bytes_h = 0.0;
    for (table, col) in [
        ("tier_10s", "blob"),
        ("tier_1m", "blob"),
        ("proc_snap", "blob"),
        ("proc_top_1m", "blob"),
        ("proc_net_10s", "blob"),
        ("proc_net_1m", "blob"),
        ("proc_net_15m", "blob"),
        ("gaps", "NULL"),
        ("events", "payload"),
    ] {
        let (rows, bytes): (i64, i64) = db
            .query_row(
                &format!("SELECT count(*), coalesce(sum(length({col})), 0) FROM {table}"),
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        println!(
            "[perf] store {table:<12} {:>7.0} rows/h {:>10.0} payload bytes/h",
            rows as f64 * scale,
            bytes as f64 * scale
        );
        rows_h += rows as f64 * scale;
        bytes_h += bytes as f64 * scale;
    }
    let growth_h = growth as f64 * scale;
    println!(
        "[perf] store total {rows_h:.0} rows/h, {bytes_h:.0} payload bytes/h, file growth {growth_h:.0} bytes/h ({minutes} min run)"
    );
    let (max_rows, max_bytes) = (
        num(&b, "store.rowsPerHour"),
        num(&b, "store.payloadBytesPerHour"),
    );
    assert!(rows_h <= max_rows, "{rows_h:.0} rows/h > {max_rows}");
    assert!(
        bytes_h <= max_bytes,
        "{bytes_h:.0} payload bytes/h > {max_bytes}"
    );
    let max_wal = num(&b, "store.walBytesPerHour");
    assert!(
        wal.wal_bytes_h <= max_wal,
        "{:.0} WAL bytes/h > {max_wal}",
        wal.wal_bytes_h
    );
}
