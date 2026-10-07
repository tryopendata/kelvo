//! Runs the real engine on this machine and prints what it sees.
//!
//! ```text
//! cargo run -p kelvo-engine --example dump                  # one summary line per tick
//! cargo run -p kelvo-engine --example dump -- --full        # the whole Snapshot per tick
//! cargo run -p kelvo-engine --example dump -- --json        # one Snapshot JSON per line
//! cargo run --release -p kelvo-engine --example dump -- --quiet --seconds 600 --db /tmp/k.sqlite
//! ```
//!
//! Flags: `--interval-ms N` (500 to 60000, see `SamplingSettings::INTERVALS_MS`; default 1000), `--seconds N` (stop
//! after N seconds), `--db PATH` (also write history to a store there), `--quiet` (no
//! per-tick output, one line a minute; for overhead measurements), `--disable a,b` (turn
//! modules off by name, e.g. `sensors,gpu`; for attributing overhead), `--commit-ms N`
//! (the store's commit interval; default `kelvo_store::DEFAULT_COMMIT_INTERVAL`). Every
//! other module is switched on, Disk included.
//!
//! Overhead measurement (`make perf`, `scripts/perf.sh`): `--interest processes,detail`
//! registers the interest a visible dashboard would (no flag: tray-only mode;
//! `processes=5000` asks for process rows every 5 s, as the Overview does; `network` adds
//! per-process network rates, D-081; `gpu` per-process GPU time), and
//! `--perf` implies `--quiet`, measures this process's CPU time with `getrusage` from the
//! end of a warm-up (`--warmup N`, default 15 s) to shutdown, and prints one JSON line on
//! stdout. Quiet runs build no Snapshot per frame, so the reader adds almost nothing.
//!
//! The `--perf` line also attributes the CPU time: `collectors` is each collector's
//! thread CPU time (`CLOCK_THREAD_CPUTIME_ID` around every `sample` call, on the engine
//! thread) and `threads` is every thread of the process by name (`proc_pidinfo`
//! `PROC_PIDTHREADINFO`), both as percent of one core over the measured span.
//! `engine_core_pct` is the engine thread minus its collectors: frame assembly, rollups,
//! the bus. Use it with `--disable` and `--interest` to see where a change moved CPU.
//! `disk_write_bytes_per_h` is what this process wrote to disk over the measured span
//! (`proc_pid_rusage` `ri_diskio_byteswritten`, macOS only), per hour: with `--db`, the
//! store's WAL commits and checkpoints.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use kelvo_collect::{
    Cadence, CollectError, Collector, CollectorId, Entitlement, Module, Probe, SampleBuf, Tick,
};
use kelvo_engine::{Bus, BusMsg, EngineParts, LiveHub, LocalSource, Source, SourceSink};
use kelvo_schema::{Catalog, HostId, HostInfo, HostRecord, OsKind, Settings, Snapshot};
use kelvo_store::{Store, StoreConfig};

#[derive(Default)]
struct Args {
    interval_ms: Option<u32>,
    seconds: Option<u64>,
    db: Option<String>,
    full: bool,
    json: bool,
    quiet: bool,
    disable: Vec<String>,
    interest: Vec<String>,
    perf: bool,
    warmup: u64,
    commit_ms: Option<u64>,
}

fn parse_args() -> Result<Args> {
    let mut a = Args {
        warmup: 15,
        ..Args::default()
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        match flag.as_str() {
            "--interval-ms" => a.interval_ms = Some(it.next().context("--interval-ms N")?.parse()?),
            "--seconds" => a.seconds = Some(it.next().context("--seconds N")?.parse()?),
            "--db" => a.db = Some(it.next().context("--db PATH")?),
            "--full" => a.full = true,
            "--json" => a.json = true,
            "--quiet" => a.quiet = true,
            "--disable" => {
                let list = it.next().context("--disable a,b")?;
                a.disable = list.split(',').map(|m| m.trim().to_lowercase()).collect();
            }
            "--interest" => {
                let list = it.next().context("--interest processes,detail")?;
                a.interest = list.split(',').map(|m| m.trim().to_lowercase()).collect();
            }
            "--perf" => {
                a.perf = true;
                a.quiet = true;
            }
            "--warmup" => a.warmup = it.next().context("--warmup N")?.parse()?,
            "--commit-ms" => a.commit_ms = Some(it.next().context("--commit-ms N")?.parse()?),
            other => bail!("unknown flag {other}"),
        }
    }
    Ok(a)
}

fn host_record() -> HostRecord {
    HostRecord {
        // The dump tool is not the app: a fixed id keeps its throwaway databases apart
        // from a real host.
        id: HostId(uuid::Uuid::from_u128(0x6b65_6c76_6f2d_6475_6d70)),
        is_local: true,
        display_name: "dump".into(),
        info: HostInfo {
            os: if cfg!(target_os = "macos") {
                OsKind::MacOs
            } else {
                OsKind::Linux
            },
            os_version: String::new(),
            model: None,
            chip: None,
            chip_known: false,
            cpu_topology: Vec::new(),
            mem_total_bytes: 0,
            boot_time_ms: 0,
            gpu_dvfs_mhz: Vec::new(),
            boot_mounts: Vec::new(),
        },
    }
}

fn fmt(v: Option<f32>, scale: f32, unit: &str) -> String {
    v.map_or_else(|| "-".to_string(), |v| format!("{:.1}{unit}", v * scale))
}

fn summary(s: &Snapshot) -> String {
    let cpu = s.cpu.as_ref();
    let gpu = s.gpu.as_ref();
    let mem = s.memory.as_ref();
    let pwr = s.power.as_ref();
    let sen = s.sensors.as_ref();
    let net_rx: f32 = s
        .network
        .as_ref()
        .map(|n| n.interfaces.iter().filter_map(|i| i.rx_bps).sum())
        .unwrap_or(0.0);
    format!(
        "cpu {} gpu {} mem {} pwr sys {} gpu {} cpu {} temp {} net rx {} self {}",
        fmt(cpu.and_then(|c| c.total), 1.0, "%"),
        fmt(gpu.and_then(|g| g.util), 1.0, "%"),
        fmt(mem.and_then(|m| m.used), 1e-9, "GB"),
        fmt(pwr.and_then(|p| p.system), 1.0, "W"),
        fmt(pwr.and_then(|p| p.gpu), 1.0, "W"),
        fmt(pwr.and_then(|p| p.cpu), 1.0, "W"),
        fmt(sen.and_then(|t| t.hottest_c), 1.0, "C"),
        fmt(Some(net_rx), 1e-3, "kB/s"),
        fmt(s.self_cpu, 1.0, "%"),
    )
}

/// Bytes this process has written to disk (macOS; 0 elsewhere).
fn disk_bytes_written() -> u64 {
    #[cfg(target_os = "macos")]
    {
        // SAFETY: proc_pid_rusage fills one rusage_info_v4 we own, for our own pid.
        let mut ri: libc::rusage_info_v4 = unsafe { std::mem::zeroed() };
        // SAFETY: as above; the flavor matches the struct.
        let rc = unsafe {
            libc::proc_pid_rusage(
                std::process::id() as libc::c_int,
                libc::RUSAGE_INFO_V4,
                (&raw mut ri).cast(),
            )
        };
        if rc == 0 {
            ri.ri_diskio_byteswritten
        } else {
            0
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        0
    }
}

/// User plus system CPU time of this process (all threads), in seconds.
fn cpu_seconds() -> (f64, f64) {
    // SAFETY: getrusage writes one `rusage` struct we own; RUSAGE_SELF is always valid.
    let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: as above.
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut ru) } != 0 {
        return (0.0, 0.0);
    }
    let secs = |t: libc::timeval| t.tv_sec as f64 + t.tv_usec as f64 / 1e6;
    (secs(ru.ru_utime), secs(ru.ru_stime))
}

/// CPU time of the calling thread, in ns.
fn thread_cpu_ns() -> u64 {
    // SAFETY: clock_gettime writes one timespec we own; the clock id is a constant the
    // OS supports.
    let mut ts: libc::timespec = unsafe { std::mem::zeroed() };
    // SAFETY: as above.
    if unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) } != 0 {
        return 0;
    }
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}

/// Samples taken and thread CPU ns spent in them, per collector.
#[derive(Default)]
struct Spent {
    samples: AtomicU64,
    cpu_ns: AtomicU64,
}

/// A collector that times its own `sample` calls (on the engine thread).
struct Timed {
    inner: Box<dyn Collector>,
    spent: Arc<Spent>,
}

impl Collector for Timed {
    fn id(&self) -> CollectorId {
        self.inner.id()
    }
    fn cadence(&self) -> Cadence {
        self.inner.cadence()
    }
    fn required_entitlements(&self) -> &'static [Entitlement] {
        self.inner.required_entitlements()
    }
    fn modules(&self) -> &'static [Module] {
        self.inner.modules()
    }
    fn probe(&mut self) -> Probe {
        self.inner.probe()
    }
    fn sample(&mut self, tick: &Tick, out: &mut SampleBuf) -> Result<(), CollectError> {
        let t0 = thread_cpu_ns();
        let r = self.inner.sample(tick, out);
        let dt = thread_cpu_ns().saturating_sub(t0);
        self.spent.samples.fetch_add(1, Ordering::Relaxed);
        self.spent.cpu_ns.fetch_add(dt, Ordering::Relaxed);
        r
    }
}

type SpentTable = Vec<(&'static str, Arc<Spent>)>;

fn timed(collectors: Vec<Box<dyn Collector>>) -> (Vec<Box<dyn Collector>>, SpentTable) {
    let mut table = Vec::new();
    let wrapped = collectors
        .into_iter()
        .map(|inner| {
            let spent = Arc::new(Spent::default());
            table.push((inner.id().0, Arc::clone(&spent)));
            Box::new(Timed { inner, spent }) as Box<dyn Collector>
        })
        .collect();
    (wrapped, table)
}

fn spent_snapshot(table: &SpentTable) -> Vec<(u64, u64)> {
    table
        .iter()
        .map(|(_, s)| {
            (
                s.samples.load(Ordering::Relaxed),
                s.cpu_ns.load(Ordering::Relaxed),
            )
        })
        .collect()
}

/// User plus system CPU ns of every thread in this process, summed by thread name
/// (unnamed threads as `unnamed`).
#[cfg(target_os = "macos")]
fn thread_times() -> BTreeMap<String, u64> {
    // sys/proc_info.h: lists the thread handles (u64 each) for PROC_PIDTHREADINFO.
    const PROC_PIDLISTTHREADS: libc::c_int = 6;
    let pid = std::process::id() as libc::c_int;
    let mut ids = vec![0u64; 256];
    let bytes = (ids.len() * std::mem::size_of::<u64>()) as libc::c_int;
    // SAFETY: the buffer holds `bytes` bytes and outlives the call.
    let n =
        unsafe { libc::proc_pidinfo(pid, PROC_PIDLISTTHREADS, 0, ids.as_mut_ptr().cast(), bytes) };
    let count = usize::try_from(n).unwrap_or(0) / std::mem::size_of::<u64>();
    let mut out = BTreeMap::new();
    for &id in ids.iter().take(count) {
        // SAFETY: a plain C struct; all zeroes is a valid value.
        let mut ti: libc::proc_threadinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::proc_threadinfo>() as libc::c_int;
        // SAFETY: `ti` is `size` bytes and outlives the call.
        let got = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTHREADINFO,
                id,
                (&raw mut ti).cast(),
                size,
            )
        };
        if got != size {
            continue;
        }
        let name: Vec<u8> = ti
            .pth_name
            .iter()
            .take_while(|&&c| c != 0)
            .map(|&c| c as u8)
            .collect();
        let name = if name.is_empty() {
            "unnamed".to_string()
        } else {
            String::from_utf8_lossy(&name).into_owned()
        };
        *out.entry(name).or_insert(0) += ti.pth_user_time + ti.pth_system_time;
    }
    out
}

#[cfg(not(target_os = "macos"))]
fn thread_times() -> BTreeMap<String, u64> {
    BTreeMap::new()
}

struct Baseline {
    at: Instant,
    /// The wall clock at `at`: `Instant` stops while the Mac sleeps, this does not.
    at_wall: std::time::SystemTime,
    cpu: (f64, f64),
    disk: u64,
    frames: u64,
    collectors: Vec<(u64, u64)>,
    threads: BTreeMap<String, u64>,
}

fn main() -> Result<()> {
    let args = parse_args()?;
    let mut settings = Settings::default();
    for (module, m) in &mut settings.modules {
        let name = format!("{module:?}").to_lowercase();
        m.enabled = !args.disable.contains(&name);
    }
    if let Some(ms) = args.interval_ms {
        settings.sampling.interval_ms = ms;
    }
    settings.validate().context("settings")?;
    let settings_interval = settings.sampling.interval_ms;

    let store = match &args.db {
        Some(path) => {
            let mut cfg = StoreConfig::new(path);
            if let Some(ms) = args.commit_ms {
                cfg.commit_interval = Duration::from_millis(ms);
            }
            Some(Store::open(cfg).context("opening the store")?)
        }
        None => None,
    };
    let record = host_record();
    let host = record.id;
    let mut parts = EngineParts::platform(Arc::new(kelvo_engine::NoScaleStore));
    let spent = if args.perf {
        let (wrapped, table) = timed(std::mem::take(&mut parts.collectors));
        parts.collectors = wrapped;
        table
    } else {
        Vec::new()
    };
    let source = Arc::new(LocalSource::new(record, parts, settings));
    let bus = Bus::default();
    let mut sub = bus.subscribe();
    let mut handle = Arc::clone(&source)
        .start(SourceSink {
            live: LiveHub::new(bus),
            store: store.as_ref().map(Store::writer),
        })
        .context("starting the engine")?;

    if let Some(ctl) = source.engine() {
        for what in &args.interest {
            match what.as_str() {
                "processes" => ctl.set_process_interest(Some(0)),
                p if p.starts_with("processes=") => ctl.set_process_interest(Some(
                    p.trim_start_matches("processes=")
                        .parse()
                        .context("processes=MS")?,
                )),
                "detail" => drop(ctl.set_detail_interest(true)),
                // Per-process network rates (D-081); needs `processes` too.
                "network" => ctl.set_network_process_interest(true),
                // Per-process GPU time; needs `processes` too.
                "gpu" => ctl.set_gpu_process_interest(true),
                other => bail!("unknown interest {other}"),
            }
        }
    }
    let catalog = Catalog::builtin();
    let started = Instant::now();
    let warmup = Duration::from_secs(args.warmup);
    let mut baseline: Option<Baseline> = None;
    // The deadline runs on its own thread: while the Mac sleeps the engine publishes
    // nothing, and a check inside the receive loop would never run. Shutting the engine
    // down drops its bus sender, which ends the loop below.
    if let (Some(secs), Some(ctl)) = (args.seconds, source.engine().cloned()) {
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(secs));
            ctl.shutdown();
        });
    }
    let mut frames = 0u64;
    // The base tick the engine chose: with no window interested it is the background's.
    let mut tick_ms = settings_interval;
    let mut last_minute = Instant::now();
    while let Some(msg) = sub.blocking_recv() {
        match msg {
            BusMsg::Layout(l) => {
                let persisted = l
                    .series
                    .iter()
                    .filter(|k| catalog.get(k.metric.as_str()).is_some_and(|d| d.persisted))
                    .count();
                eprintln!(
                    "layout {}: {} series ({persisted} persisted)",
                    l.layout_no,
                    l.series.len()
                );
            }
            BusMsg::Caps(c) => {
                eprintln!("capabilities r{}:", c.revision);
                for (m, cap) in &c.modules {
                    eprintln!("  {:<8} {cap:?}", m.as_str());
                }
            }
            BusMsg::Status(s) => {
                tick_ms = s.interval_ms;
                eprintln!("status: {s:?}");
            }
            BusMsg::Processes(_) => {}
            BusMsg::Event(e) => eprintln!("event: {e:?}"),
            BusMsg::Frame(f) => {
                frames += 1;
                if args.perf && baseline.is_none() && started.elapsed() >= warmup {
                    baseline = Some(Baseline {
                        at: Instant::now(),
                        at_wall: std::time::SystemTime::now(),
                        cpu: cpu_seconds(),
                        disk: disk_bytes_written(),
                        frames,
                        collectors: spent_snapshot(&spent),
                        threads: thread_times(),
                    });
                }
                if args.json {
                    println!("{}", serde_json::to_string(&f.snapshot(host, &catalog)?)?);
                } else if args.full {
                    println!("{:#?}", f.snapshot(host, &catalog)?);
                } else if !args.quiet {
                    println!("{} {}", f.ts_ms, summary(&f.snapshot(host, &catalog)?));
                } else if last_minute.elapsed() >= Duration::from_secs(60) {
                    last_minute = Instant::now();
                    eprintln!("{frames} frames, {}", summary(&f.snapshot(host, &catalog)?));
                }
            }
        }
    }
    // Read before stopping: the engine thread's times leave with the thread.
    let end = (Instant::now(), cpu_seconds());
    let end_disk = disk_bytes_written();
    let end_threads = thread_times();
    let end_collectors = spent_snapshot(&spent);
    handle.stop();
    if args.perf {
        let Some(b) = baseline else {
            bail!("the run ended inside the {} s warm-up", args.warmup);
        };
        let (u0, s0) = b.cpu;
        let wall = end.0.duration_since(b.at).as_secs_f64();
        let wall_clock = b.at_wall.elapsed().map_or(wall, |d| d.as_secs_f64());
        if wall_clock - wall > 5.0 {
            bail!(
                "the Mac slept during the run ({wall_clock:.0} s of wall time, {wall:.0} s awake)"
            );
        }
        let (user, sys) = (end.1.0 - u0, end.1.1 - s0);
        let pct = |ns: u64| ns as f64 / 1e9 / wall * 100.0;
        let mut collectors = serde_json::Map::new();
        let mut collectors_ns = 0;
        for (((id, _), (s1, c1)), (s0, c0)) in spent.iter().zip(&end_collectors).zip(&b.collectors)
        {
            collectors_ns += c1 - c0;
            collectors.insert(
                (*id).to_string(),
                serde_json::json!({ "samples": s1 - s0, "pct": pct(c1 - c0) }),
            );
        }
        let since = |name: &str| {
            end_threads
                .get(name)
                .copied()
                .unwrap_or(0)
                .saturating_sub(b.threads.get(name).copied().unwrap_or(0))
        };
        let threads: serde_json::Map<_, _> = end_threads
            .keys()
            .map(|name| (name.clone(), serde_json::json!(pct(since(name)))))
            .collect();
        println!(
            "{}",
            serde_json::json!({
                "cpu_pct": (user + sys) / wall * 100.0,
                "user_s": user,
                "sys_s": sys,
                "wall_s": wall,
                "frames": frames - b.frames,
                "interval_ms": settings_interval,
                "tick_ms": tick_ms,
                "interest": args.interest.join(","),
                "collectors": collectors,
                "collectors_pct": pct(collectors_ns),
                "engine_core_pct": pct(since("kelvo-engine").saturating_sub(collectors_ns)),
                "threads": threads,
                "disk_write_bytes_per_h": end_disk.saturating_sub(b.disk) as f64 * 3600.0 / wall,
            })
        );
    }
    if let Some(store) = store {
        store.close().context("closing the store")?;
    }
    eprintln!(
        "{frames} frames in {:.1} s",
        started.elapsed().as_secs_f64()
    );
    Ok(())
}
