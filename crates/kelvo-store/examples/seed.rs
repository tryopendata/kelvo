//! Seeds a Kelvo history database with synthetic history so the dashboard's long ranges,
//! the Timeline and the process and network history have something to show during
//! development. Run it with `bun run seed` while Kelvo is not running.
//!
//! It reads the local host record and the series that have values from the database
//! already in the data directory, so cores, clusters, sensors, disks and interfaces match
//! the Mac it runs on. It then moves that database aside with
//! [`kelvo_store::move_aside`] (kept beside it as `history-reset-<ms>.sqlite`) and writes a
//! fresh one through the store's own `Writer`:
//!
//! - 10 s buckets for the last day and minute buckets for the whole range; pruning rolls
//!   minutes older than 7 days into 15-minute buckets, as it does in the app;
//! - process snapshots (every 10 s for the last 72 hours, every minute before, rolled into
//!   the per-minute and per-15-minute top 5 by pruning) and per-app network bytes;
//! - runs of two to five days, idling overnight, each ended by a night with the lid
//!   closed or a few hours with Kelvo quit, and the detector events (fan ramps,
//!   sustained processes, thermal state changes) the simulated load triggers.
//!
//! The days follow a working week: builds, calls and Time Machine during work hours,
//! video or a game in the evening, unplugged stretches the battery drains through. The
//! values are calibrated against a real M3 Max recording. Output is deterministic for a
//! given `--seed` and end time.
//!
//! ```text
//! cargo run --release -p kelvo-store --example seed -- [--prod | --dir <data dir>] [--days 30] [--seed 1]
//! ```
//!
//! The default directory is the debug build's (`com.tryopendata.kelvo.dev`), the one
//! `bun run dev` uses. `--prod` (`bun run seed:prod`) seeds the release build's
//! (`com.tryopendata.kelvo`) instead.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use kelvo_schema::{
    Catalog, Event, EventDetail, Gap, GapReason, HostId, HostRecord, SeriesKey, SeriesSelector,
    ThermalState, Tier, Unit,
};
use kelvo_store::{
    BucketRow, HistoryQuery, NetApp, NetBucket, ProcRow, Retention, Store, StoreConfig, StoreError,
    TierChoice,
};

const S10: i64 = 10_000;
const MIN: i64 = 60_000;
const HOUR: i64 = 60 * MIN;
const DAY: i64 = 24 * HOUR;
const DB_FILE: &str = "history.sqlite";
const DEV_IDENTIFIER: &str = "com.tryopendata.kelvo.dev";
/// The release build's bundle identifier (`tauri.conf.json`).
const PROD_IDENTIFIER: &str = "com.tryopendata.kelvo";
/// Kelvo's default retention (settings `history.retention_days`).
const RETENTION_DAYS: u16 = 30;
/// Process snapshots keep the top 30 by CPU, like the engine.
const PROC_TOP: usize = 30;

fn main() -> Result<()> {
    let args = Args::parse()?;
    let path = args.dir.join(DB_FILE);
    if !path.exists() {
        bail!(
            "no {DB_FILE} in {}.\nRun Kelvo once (`bun run dev`) so it records this Mac's \
             series, quit it, then seed.",
            args.dir.display()
        );
    }

    let now = wall_ms();
    // Minute-aligned, so the app's first bucket after launch never lands in a seeded one.
    let end = now - now.rem_euclid(MIN);
    let start = end - i64::from(args.days) * DAY;

    let source = read_source(&path, end)?;
    println!(
        "{} ({}), {} series with values",
        source.host.display_name,
        source.host.info.chip.as_deref().unwrap_or("unknown chip"),
        source.keys.len()
    );

    let moved = kelvo_store::move_aside(&path, now).map_err(locked_hint)?;
    if let Some(moved) = &moved {
        println!("Moved the existing database to {}", moved.display());
    }

    let store = Store::open(StoreConfig::new(&path)).context("creating the seeded database")?;
    let writer = store.writer();
    writer.upsert_host(source.host.clone())?;

    let offset = local_offset_ms();
    let mut rng = Rng(args.seed);
    let plan = Plan::new(&mut rng, start, end, offset);
    let mut sim = Sim::new(&source, rng.fork(), offset);
    let host = source.host.id;

    for gap in &plan.gaps {
        writer.write_gap(host, *gap)?;
    }

    let layout: Arc<[SeriesKey]> = source.keys.clone().into();
    let series: Vec<Series> = layout.iter().map(|k| Series::of(k, &source)).collect();
    let mut minute = MinuteAcc::new(layout.len());
    let mut counts = Counts::default();
    let mut last_flush_day = start / DAY;

    for span in &plan.awake {
        sim.wake(span.start, &plan);
        let mut t = span.start;
        while t < span.end {
            let tick = sim.step(t, &plan);

            let stats: Vec<f32> = series
                .iter()
                .flat_map(|s| s.stats(&tick, t, &mut sim.rng))
                .collect();
            let m = t - t.rem_euclid(MIN);
            if minute.ts != Some(m) {
                if let Some(row) = minute.take(host, &layout) {
                    writer.write_bucket(row)?;
                    counts.m1 += 1;
                }
                minute.ts = Some(m);
            }
            minute.add(&stats);
            if t >= end - DAY {
                writer.write_bucket(BucketRow {
                    host,
                    tier: Tier::S10,
                    bucket_ts: t,
                    series: Arc::clone(&layout),
                    stats,
                })?;
                counts.s10 += 1;
            }

            writer.write_net_bucket(host, t, tick.net.clone())?;
            counts.net += 1;

            if t >= end - 3 * DAY || t.rem_euclid(MIN) == 0 {
                writer.write_proc_snapshot(host, t, sim.processes(&tick))?;
                counts.procs += 1;
            }

            for event in sim.take_events() {
                writer.record_event(host, &event)?;
                counts.events += 1;
            }

            if t / DAY != last_flush_day {
                writer.flush()?;
                last_flush_day = t / DAY;
            }
            t += S10;
        }
        sim.sleep();
    }
    if let Some(row) = minute.take(host, &layout) {
        writer.write_bucket(row)?;
        counts.m1 += 1;
    }
    writer.flush()?;

    // Roll process snapshots, minutes and per-app network rows down the way the app's
    // hourly housekeeping does. Pruning works in batches, so repeat until nothing moves.
    let retention = Retention::with_days(RETENTION_DAYS.max(args.days));
    loop {
        let r = writer.prune(end, retention)?;
        if r.m1_rolled + r.proc_snaps_rolled + r.proc_top_rolled + r.s10_rows == 0 {
            break;
        }
    }
    writer.flush()?;
    let bytes = store.size_on_disk()?;
    store.close()?;

    println!(
        "Seeded {} days into {}: {} minute buckets, {} 10 s buckets, {} process snapshots, \
         {} network buckets, {} events, {} gaps; {:.1} MB",
        args.days,
        path.display(),
        counts.m1,
        counts.s10,
        counts.procs,
        counts.net,
        counts.events,
        plan.gaps.len(),
        bytes as f64 / 1e6
    );
    Ok(())
}

#[derive(Default)]
struct Counts {
    s10: u64,
    m1: u64,
    procs: u64,
    net: u64,
    events: u64,
}

struct Args {
    dir: PathBuf,
    days: u16,
    seed: u64,
}

impl Args {
    fn parse() -> Result<Self> {
        let home = std::env::var_os("HOME").context("HOME is not set")?;
        let support = PathBuf::from(home).join("Library/Application Support");
        let mut args = Args {
            dir: support.join(DEV_IDENTIFIER),
            days: 30,
            seed: 1,
        };
        let mut it = std::env::args().skip(1);
        while let Some(flag) = it.next() {
            let mut value = || it.next().with_context(|| format!("{flag} needs a value"));
            match flag.as_str() {
                "--dir" => args.dir = PathBuf::from(value()?),
                "--prod" => args.dir = support.join(PROD_IDENTIFIER),
                "--days" => args.days = value()?.parse().context("--days")?,
                "--seed" => args.seed = value()?.parse().context("--seed")?,
                "-h" | "--help" => {
                    println!(
                        "seed [--prod | --dir <data dir>] [--days N] [--seed N]\n\nDefault dir: \
                         ~/Library/Application Support/{DEV_IDENTIFIER}\n--prod:      \
                         ~/Library/Application Support/{PROD_IDENTIFIER}"
                    );
                    std::process::exit(0);
                }
                other => bail!("unknown argument {other}"),
            }
        }
        if args.days == 0 {
            bail!("--days must be at least 1");
        }
        Ok(args)
    }
}

fn locked_hint(e: StoreError) -> anyhow::Error {
    match e {
        StoreError::Locked { .. } => {
            anyhow::anyhow!("the history database is open: quit Kelvo first")
        }
        e => e.into(),
    }
}

/// What the seed copies from the existing database.
struct Source {
    host: HostRecord,
    /// Series with at least one value, in key order: the seeded layout.
    keys: Vec<SeriesKey>,
    /// The newest average of each of those series.
    latest: HashMap<SeriesKey, f32>,
}

fn read_source(path: &std::path::Path, end: i64) -> Result<Source> {
    let store = Store::open(StoreConfig::new(path)).map_err(locked_hint)?;
    let mut reader = store.reader()?;
    let host = reader
        .local_host()?
        .context("the database has no local host; run Kelvo once first")?;
    let selectors: Vec<SeriesSelector> = Catalog::builtin()
        .defs()
        .iter()
        .filter(|d| d.persisted)
        .map(|d| SeriesSelector {
            metric: d.id.clone(),
            labels: Default::default(),
        })
        .collect();
    let mut latest = HashMap::new();
    // Oldest tier first, so newer points overwrite older ones.
    for tier in [Tier::M15, Tier::M1, Tier::S10] {
        let result = reader.history(&HistoryQuery {
            host: host.id,
            selectors: selectors.clone(),
            from_ms: end - 400 * DAY,
            to_ms: end + DAY,
            tier: TierChoice::Fixed(tier),
            max_points: 2,
        })?;
        for s in result.series {
            if let Some(p) = s.points.iter().rev().find(|p| p.avg.is_finite()) {
                latest.insert(s.key, p.avg);
            }
        }
    }
    drop(reader);
    store.close()?;
    let mut keys: Vec<SeriesKey> = latest.keys().cloned().collect();
    keys.sort();
    if keys.is_empty() {
        bail!("the database has no recorded series; run Kelvo for a minute first");
    }
    Ok(Source { host, keys, latest })
}

fn wall_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// The local UTC offset, from `date +%z` (`-0700`). Zero when it cannot be read.
fn local_offset_ms() -> i64 {
    let out = std::process::Command::new("date").arg("+%z").output();
    let Ok(out) = out else { return 0 };
    let s = String::from_utf8_lossy(&out.stdout);
    let s = s.trim();
    let (sign, digits) = match s.strip_prefix('-') {
        Some(d) => (-1, d),
        None => (1, s.trim_start_matches('+')),
    };
    let (Some(h), Some(m)) = (digits.get(0..2), digits.get(2..4)) else {
        return 0;
    };
    let (Ok(h), Ok(m)) = (h.parse::<i64>(), m.parse::<i64>()) else {
        return 0;
    };
    sign * (h * HOUR + m * MIN)
}

// ---------------------------------------------------------------------------------------
// Randomness

/// SplitMix64: small, fast and deterministic, which is all a seed needs.
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `[0, 1)`.
    fn f(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f()
    }

    fn chance(&mut self, p: f32) -> bool {
        self.f() < p
    }

    /// Roughly standard normal (Irwin-Hall with four terms).
    fn normal(&mut self) -> f32 {
        (self.f() + self.f() + self.f() + self.f() - 2.0) * 1.73
    }

    /// Log-normal with median 1: bursty byte counts.
    fn burst(&mut self, sigma: f32) -> f32 {
        (self.normal() * sigma).exp()
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        let i = (self.f() * items.len() as f32) as usize;
        items.get(i).or(items.first()).copied().unwrap_or("")
    }

    fn fork(&mut self) -> Rng {
        Rng(self.next_u64())
    }
}

/// A mean-reverting random walk (Ornstein-Uhlenbeck), stepped once per 10 s bucket.
struct Wander {
    x: f32,
    mean: f32,
    /// Fraction of the distance to the mean closed per step.
    pull: f32,
    sigma: f32,
}

impl Wander {
    fn new(mean: f32, pull: f32, sigma: f32) -> Self {
        Self {
            x: mean,
            mean,
            pull,
            sigma,
        }
    }

    fn step(&mut self, rng: &mut Rng) -> f32 {
        self.x += (self.mean - self.x) * self.pull + self.sigma * rng.normal();
        self.x
    }
}

/// Moves `x` toward `target` with time constant `tau_s`, over one 10 s step.
fn ease(x: f32, target: f32, tau_s: f32) -> f32 {
    x + (target - x) * (1.0 - (-10.0 / tau_s).exp())
}

// ---------------------------------------------------------------------------------------
// The plan: when the Mac is awake, what it is doing, when it is unplugged

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Kind {
    Build,
    Call,
    Video,
    Download,
    Gaming,
    Backup,
    Dashboard,
    Music,
    Cleanup,
}

struct Episode {
    kind: Kind,
    start: i64,
    end: i64,
    level: f32,
    app: &'static str,
}

#[derive(Clone, Copy)]
struct Span {
    start: i64,
    end: i64,
}

struct Plan {
    awake: Vec<Span>,
    gaps: Vec<Gap>,
    /// Sorted by start.
    episodes: Vec<Episode>,
    unplugged: Vec<Span>,
    /// The browser used each local day, by day index from the start.
    browsers: Vec<&'static str>,
    first_day: i64,
    offset: i64,
}

const BROWSERS: &[&str] = &["Safari", "Google Chrome"];
const CALL_APPS: &[&str] = &["zoom.us", "Slack", "FaceTime"];
const GAMES: &[&str] = &["Baldur's Gate 3", "Cyberpunk 2077", "Factorio"];

impl Plan {
    fn new(rng: &mut Rng, start: i64, end: i64, offset: i64) -> Self {
        let local = |t: i64| t + offset;
        let first_day = local(start).div_euclid(DAY);
        let last_day = local(end).div_euclid(DAY);
        let at_day = |day: i64, h: f32| day * DAY - offset + (h * HOUR as f32) as i64;

        // Kelvo runs for a few days at a time: the Mac idles overnight on its charger. A
        // run ends with a night the lid was closed, or with Kelvo quit for a few hours (an
        // update, a restart). Each piece carries the reason of the gap after it.
        let mut pieces = vec![(start, end, GapReason::Sleep)];
        let mut day = first_day;
        loop {
            day += 2 + (rng.f() * 4.0) as i64;
            if day >= last_day {
                break;
            }
            let (from, to, reason) = if rng.chance(0.6) {
                let bed = at_day(day, 23.3 + rng.range(-1.0, 1.2));
                let wake = at_day(day + 1, 7.5 + rng.range(-0.5, 1.5));
                (bed, wake, GapReason::Sleep)
            } else {
                let quit = at_day(day, rng.range(10.0, 18.0));
                let back = quit + (rng.range(0.5, 5.0) * HOUR as f32) as i64;
                (quit, back, GapReason::AppNotRunning)
            };
            split(&mut pieces, from, to, reason);
        }
        let pieces: Vec<(i64, i64, GapReason)> = pieces
            .into_iter()
            .filter(|(s, e, _)| e - s >= MIN)
            .map(|(s, e, after)| (s - s.rem_euclid(S10), e - e.rem_euclid(S10), after))
            .collect();

        let mut episodes = Vec::new();
        let mut unplugged = Vec::new();
        let mut browsers = Vec::new();

        for day in first_day..=last_day {
            let midnight = day * DAY - offset;
            let at = |h: f32| midnight + (h * HOUR as f32) as i64;
            // 1970-01-01 was a Thursday; 0 is Sunday.
            let weekday = (day + 4).rem_euclid(7);
            let weekend = weekday == 0 || weekday == 6;
            browsers.push(rng.pick(BROWSERS));

            let work = |rng: &mut Rng| {
                if weekend {
                    at(rng.range(10.0, 22.0))
                } else {
                    at(rng.range(9.0, 17.8))
                }
            };
            let mins = |rng: &mut Rng, lo: f32, hi: f32| (rng.range(lo, hi) * MIN as f32) as i64;
            let mut add = |kind, start: i64, len: i64, level: f32, app: &'static str| {
                episodes.push(Episode {
                    kind,
                    start,
                    end: start + len,
                    level,
                    app,
                });
            };

            let builds = if weekend {
                (rng.f() * 3.0) as usize
            } else {
                3 + (rng.f() * 7.0) as usize
            };
            // Mostly incremental rebuilds; now and then a clean build that heats the Mac.
            for _ in 0..builds {
                let s = work(rng);
                if rng.chance(0.15) {
                    add(
                        Kind::Build,
                        s,
                        mins(rng, 4.0, 20.0),
                        rng.range(0.8, 1.0),
                        "rustc",
                    );
                } else {
                    add(
                        Kind::Build,
                        s,
                        mins(rng, 0.5, 2.0),
                        rng.range(0.35, 0.65),
                        "rustc",
                    );
                }
            }
            if !weekend {
                for _ in 0..1 + (rng.f() * 3.0) as usize {
                    let s = at((9.0 + (rng.f() * 16.0).floor() * 0.5).min(17.0));
                    let app = rng.pick(CALL_APPS);
                    add(Kind::Call, s, mins(rng, 25.0, 60.0), 1.0, app);
                }
                if rng.chance(0.6) {
                    let s = at(rng.range(9.5, 14.0));
                    add(Kind::Music, s, mins(rng, 60.0, 180.0), 1.0, "Spotify");
                }
            }
            if rng.chance(0.55) {
                let s = at(rng.range(19.5, 22.0));
                add(Kind::Video, s, mins(rng, 40.0, 120.0), 1.0, "");
            }
            if weekend && rng.chance(0.4) {
                let s = at(rng.range(13.0, 17.0));
                add(Kind::Video, s, mins(rng, 30.0, 90.0), 1.0, "");
            }
            if rng.chance(if weekend { 0.5 } else { 0.1 }) {
                let s = at(rng.range(19.8, 21.5));
                let game = rng.pick(GAMES);
                add(
                    Kind::Gaming,
                    s,
                    mins(rng, 60.0, 150.0),
                    rng.range(0.7, 1.0),
                    game,
                );
            }
            for _ in 0..(rng.f() * 3.0) as usize {
                let s = at(rng.range(8.0, 23.0));
                let app = rng.pick(&[
                    "Safari",
                    "Google Chrome",
                    "App Store",
                    "softwareupdated",
                    "cargo",
                ]);
                add(
                    Kind::Download,
                    s,
                    mins(rng, 1.0, 12.0),
                    rng.range(0.3, 1.0),
                    app,
                );
            }
            // Time Machine runs hourly; the first backup after a night is the big one.
            let mut h = 8.0;
            while h < 24.0 {
                let level = if h < 9.0 { 1.0 } else { rng.range(0.2, 0.6) };
                let s = at(h + rng.range(0.0, 0.9));
                add(Kind::Backup, s, mins(rng, 2.0, 7.0), level, "backupd");
                h += 1.0;
            }
            for hour in 9..18 {
                if rng.chance(0.25) {
                    let s = at(hour as f32 + rng.range(0.0, 0.8));
                    add(Kind::Dashboard, s, mins(rng, 3.0, 15.0), 1.0, "Kelvo");
                }
            }
            if rng.chance(0.15) {
                let s = work(rng);
                add(Kind::Cleanup, s, MIN, rng.range(5.0, 25.0), "");
            }

            let mut out = |s: i64, len: i64| {
                unplugged.push(Span {
                    start: s,
                    end: s + len,
                })
            };
            if weekend {
                if rng.chance(0.6) {
                    out(at(rng.range(11.0, 17.0)), mins(rng, 120.0, 300.0));
                }
            } else {
                if rng.chance(0.45) {
                    out(at(rng.range(13.5, 15.5)), mins(rng, 90.0, 210.0));
                }
                if rng.chance(0.35) {
                    out(at(rng.range(20.0, 21.0)), mins(rng, 60.0, 150.0));
                }
            }
        }
        episodes.sort_by_key(|e| e.start);

        let awake: Vec<Span> = pieces
            .iter()
            .map(|&(s, e, _)| Span { start: s, end: e })
            .collect();
        let gaps: Vec<Gap> = pieces
            .windows(2)
            .filter_map(|w| match w {
                [(_, end, reason), (next, _, _)] => Gap::host(*end, Some(*next), *reason).ok(),
                _ => None,
            })
            .collect();

        Plan {
            awake,
            gaps,
            episodes,
            unplugged,
            browsers,
            first_day,
            offset,
        }
    }

    fn browser(&self, t: i64) -> &'static str {
        let i = ((t + self.offset).div_euclid(DAY) - self.first_day).max(0) as usize;
        self.browsers.get(i).copied().unwrap_or("Safari")
    }

    fn unplugged(&self, t: i64) -> bool {
        self.unplugged.iter().any(|s| s.start <= t && t < s.end)
    }
}

/// Cuts `[from, to)` out of the awake pieces, as a gap of `reason`.
fn split(pieces: &mut Vec<(i64, i64, GapReason)>, from: i64, to: i64, reason: GapReason) {
    let mut out = Vec::new();
    for (s, e, after) in pieces.drain(..) {
        if to <= s || from >= e {
            out.push((s, e, after));
            continue;
        }
        if from > s {
            out.push((s, from, reason));
        }
        if to < e {
            out.push((to, e, after));
        }
    }
    *pieces = out;
}

// ---------------------------------------------------------------------------------------
// The simulation: one machine state per 10 s bucket

/// Everything the series generators read for one bucket.
struct Tick {
    cpu_total: f32,
    cpu_user: f32,
    e_load: Vec<f32>,
    p_load: Vec<f32>,
    /// Active residency per cluster, by label (`E0`, `P0`, `P1`).
    cluster_active: HashMap<String, f32>,
    cluster_power: HashMap<String, f32>,
    loadavg: [f32; 3],
    gpu_util: f32,
    power_cpu: f32,
    power_gpu: f32,
    power_system: f32,
    mem: Mem,
    cpu_temp: f32,
    gpu_temp: f32,
    ambient: f32,
    ssd_temp: f32,
    battery_temp: f32,
    wifi_temp: f32,
    thermal_state: f32,
    fan: f32,
    disk_read: f32,
    disk_write: f32,
    disk_used: f32,
    disk_total: f32,
    net_rx: f32,
    net_tx: f32,
    net: NetBucket,
    battery: Battery,
    self_cpu: f32,
    active: HashMap<Kind, (f32, &'static str)>,
}

#[derive(Clone, Copy, Default)]
struct Mem {
    used: f32,
    app: f32,
    wired: f32,
    compressed: f32,
    cached: f32,
    free: f32,
    pressure: f32,
    level: f32,
    swap_used: f32,
    swap_in: f32,
    swap_out: f32,
}

#[derive(Clone, Copy)]
struct Battery {
    charge: f32,
    charging: bool,
    external: bool,
    power: f32,
    health: f32,
    cycles: f32,
}

/// A long-running process the snapshots draw from.
struct Proc {
    name: &'static str,
    pid: i32,
    /// Share of the CPU it takes when nothing in particular runs.
    base: f32,
    mem_mb: f32,
    threads: u32,
    wakeups: f32,
    /// The episode that wakes it up, and how much CPU it then wants.
    boost: Option<(Kind, f32)>,
}

struct Sim {
    rng: Rng,
    offset: i64,
    n_e: usize,
    n_p: usize,
    activity: Wander,
    work_set: Wander,
    cache: Wander,
    ambient: Wander,
    loadavg: [f32; 3],
    cpu_temp: f32,
    gpu_temp: f32,
    soak: f32,
    ssd_heat: f32,
    net_heat: f32,
    fan: f32,
    fans_on: bool,
    /// Fan readings over the last two minutes, for the ramp detector.
    fan_hist: Vec<(i64, f32)>,
    last_ramp: i64,
    hot_steps: u32,
    thermal_state: f32,
    sustained_seen: Vec<i64>,
    battery: Battery,
    discharged: f32,
    health_start: f32,
    health_end: f32,
    start: i64,
    span_ms: f32,
    /// Used space today, the end of the trend.
    disk_used: f32,
    disk_scratch: f32,
    disk_total: f32,
    mem_total: f32,
    swap_used: f32,
    procs: Vec<Proc>,
    next_pid: i32,
    build_pids: Vec<i32>,
    events: Vec<Event>,
}

impl Sim {
    fn new(source: &Source, mut rng: Rng, offset: i64) -> Self {
        let latest = |metric: &str, fallback: f32| {
            source
                .latest
                .iter()
                .find(|(k, _)| k.metric.as_str() == metric)
                .map(|(_, v)| *v)
                .unwrap_or(fallback)
        };
        let count = |prefix: char| {
            source
                .keys
                .iter()
                .filter(|k| k.metric.as_str() == "cpu.load")
                .filter_map(|k| k.labels.get("core"))
                .filter(|c| c.starts_with(prefix))
                .count()
        };
        let health_end = latest("battery.health", 95.0);
        let cycles_end = latest("battery.cycles", 120.0);
        let disk_total = latest("disk.total", 994.0e9);
        let disk_used = latest("disk.used", disk_total * 0.35);
        let procs = base_procs(&mut rng);
        Sim {
            offset,
            n_e: count('E').max(1),
            n_p: count('P'),
            activity: Wander::new(0.0, 0.04, 0.035),
            work_set: Wander::new(0.5, 0.0008, 0.012),
            cache: Wander::new(0.5, 0.002, 0.02),
            ambient: Wander::new(24.0, 0.001, 0.05),
            loadavg: [1.5; 3],
            cpu_temp: 42.0,
            gpu_temp: 38.0,
            soak: 0.0,
            ssd_heat: 0.0,
            net_heat: 0.0,
            fan: 0.0,
            fans_on: false,
            fan_hist: Vec::new(),
            last_ramp: i64::MIN / 2,
            hot_steps: 0,
            thermal_state: 0.0,
            sustained_seen: Vec::new(),
            battery: Battery {
                charge: 100.0,
                charging: false,
                external: true,
                power: 0.0,
                health: health_end + 0.9,
                cycles: (cycles_end - 4.0).max(0.0),
            },
            discharged: 0.0,
            health_start: health_end + 0.9,
            health_end,
            start: 0,
            span_ms: 1.0,
            // About 300 MB a day of growth, net of cleanups.
            disk_used,
            disk_scratch: 0.0,
            disk_total,
            mem_total: source.host.info.mem_total_bytes as f32,
            swap_used: 6.2e6,
            procs,
            next_pid: 52_000,
            build_pids: Vec::new(),
            events: Vec::new(),
            rng,
        }
    }

    /// Start of an awake span: the night (or the quit stretch) passed.
    fn wake(&mut self, t: i64, plan: &Plan) {
        if self.start == 0 {
            self.start = t;
            self.span_ms = plan
                .awake
                .last()
                .map(|s| (s.end - t).max(1) as f32)
                .unwrap_or(1.0);
        }
        // Charged overnight when it slept on the charger.
        if self.battery.external {
            self.battery.charge = 100.0;
        }
        self.cpu_temp = self.ambient.x + 12.0;
        self.gpu_temp = self.ambient.x + 9.0;
        self.soak = 0.0;
        self.fan = 0.0;
        self.fans_on = false;
        self.fan_hist.clear();
        self.activity.x = 0.0;
        // Swap only empties on a restart, which happens now and then overnight.
        if self.rng.chance(0.15) {
            self.swap_used = 6.2e6;
        }
        // macOS frees some caches overnight.
        self.cache.x = (self.cache.x - 0.2).max(0.0);
        self.loadavg = [0.8, 1.0, 1.2];
    }

    /// End of an awake span. Asleep on battery drains a little.
    fn sleep(&mut self) {
        if !self.battery.external {
            self.battery.charge = (self.battery.charge - 1.2).max(5.0);
        }
    }

    fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    fn step(&mut self, t: i64, plan: &Plan) -> Tick {
        let local_h = ((t + self.offset).rem_euclid(DAY)) as f32 / HOUR as f32;
        let weekday = ((t + self.offset).div_euclid(DAY) + 4).rem_euclid(7);
        let weekend = weekday == 0 || weekday == 6;

        let mut active: HashMap<Kind, (f32, &'static str)> = HashMap::new();
        for e in plan.episodes.iter().take_while(|e| e.start <= t) {
            if t < e.end {
                let entry = active.entry(e.kind).or_insert((0.0, e.app));
                if e.level > entry.0 {
                    *entry = (e.level, e.app);
                }
            }
        }
        let lvl = |k: Kind| active.get(&k).map(|a| a.0).unwrap_or(0.0);
        let (build, call, video, download, gaming, backup, dashboard, music) = (
            lvl(Kind::Build),
            lvl(Kind::Call),
            lvl(Kind::Video),
            lvl(Kind::Download),
            lvl(Kind::Gaming),
            lvl(Kind::Backup),
            lvl(Kind::Dashboard),
            lvl(Kind::Music),
        );
        if let Some(&(gb, _)) = active.get(&Kind::Cleanup) {
            self.disk_scratch = (self.disk_scratch - gb * 1e9 / 6.0).max(0.0);
        }

        let progress = ((t - self.start) as f32 / self.span_ms).clamp(0.0, 1.0);
        let rng = &mut self.rng;
        let baseline = match (weekend, local_h) {
            (false, h) if (9.0..12.0).contains(&h) || (13.0..18.0).contains(&h) => 0.42,
            (false, h) if (12.0..13.0).contains(&h) => 0.22,
            (true, h) if (10.0..22.0).contains(&h) => 0.2,
            // Overnight the Mac idles with the display off.
            (_, h) if (1.0..7.0).contains(&h) => 0.03,
            _ => 0.14,
        };
        let a = (baseline + self.activity.step(rng)).clamp(0.02, 1.0);
        let spike = if rng.chance(0.03) {
            rng.range(10.0, 35.0)
        } else {
            0.0
        };

        // CPU: a demand figure spread over the cores the way the scheduler does it (E
        // cores first, the second P cluster only under real load).
        let demand = (4.0
            + a * 22.0
            + build * 72.0
            + call * 10.0
            + gaming * 38.0
            + video * 4.0
            + backup * 5.0
            + dashboard * 2.0
            + music * 1.0
            + spike
            + rng.normal() * 1.5)
            .clamp(1.0, 100.0);
        let e_load: Vec<f32> = (0..self.n_e)
            .map(|i| {
                ((12.0 + demand * 1.4) * (1.0 - i as f32 * 0.12) + rng.normal() * 5.0)
                    .clamp(0.0, 100.0)
            })
            .collect();
        let p_demand = (demand - 6.0).max(0.0) * 1.25;
        let half = self.n_p.div_ceil(2).max(1);
        let p_load: Vec<f32> = (0..self.n_p)
            .map(|i| {
                let cluster_share = if i < half {
                    1.0 - (i as f32) * 0.02
                } else if demand > 45.0 {
                    0.95
                } else {
                    0.35
                };
                (p_demand * cluster_share + rng.normal() * 2.0).clamp(0.0, 100.0)
            })
            .collect();
        let n_cores = (self.n_e + self.n_p).max(1) as f32;
        let cpu_total = (e_load.iter().sum::<f32>() + p_load.iter().sum::<f32>()) / n_cores;
        let cpu_user = cpu_total * rng.range(0.62, 0.72);

        let mean = |v: &[f32]| {
            if v.is_empty() {
                0.0
            } else {
                v.iter().sum::<f32>() / v.len() as f32
            }
        };
        let e_active = (mean(&e_load) * 1.55).clamp(0.0, 100.0);
        let p0_active = (mean(p_load.get(..half).unwrap_or(&[])) * 2.6).clamp(0.0, 100.0);
        let p1_active = (mean(p_load.get(half..).unwrap_or(&[])) * 2.6).clamp(0.0, 100.0);
        let p_power = |active: f32| (active / 100.0).powf(1.4) * 18.0;
        let mut cluster_active = HashMap::new();
        cluster_active.insert("E0".to_string(), e_active);
        cluster_active.insert("P0".to_string(), p0_active);
        cluster_active.insert("P1".to_string(), p1_active);
        let mut cluster_power = HashMap::new();
        cluster_power.insert("E0".to_string(), 0.15 + e_active / 100.0);
        cluster_power.insert("P0".to_string(), p_power(p0_active));
        cluster_power.insert("P1".to_string(), p_power(p1_active));
        let power_cpu = cluster_power.values().sum::<f32>();

        // Load average: exponential means of runnable threads over 1, 5 and 15 minutes.
        let runnable = cpu_total / 100.0 * n_cores * 1.15 + 0.6;
        for (la, tau) in self.loadavg.iter_mut().zip([60.0, 300.0, 900.0]) {
            *la = ease(*la, runnable, tau);
        }

        let gpu_util = (2.0
            + a * 6.0
            + call * 14.0
            + video * 9.0
            + gaming * 82.0
            + build * 1.5
            + rng.normal() * 2.0)
            .clamp(0.0, 100.0);
        let power_gpu = 0.02 + (gpu_util / 100.0).powf(1.3) * 24.0;

        // Battery and power.
        let external = !plan.unplugged(t) || self.battery.charge < 12.0;
        if external != self.battery.external && !external {
            self.battery.charging = false;
        }
        self.battery.external = external;
        let base_w = 5.5 + power_cpu + power_gpu + rng.range(-0.4, 0.6);
        let cap_wh = 100.0 * self.battery.health / 100.0;
        let power_system = base_w;
        if external {
            if self.battery.charge < 99.9 {
                let w = if self.battery.charge < 80.0 {
                    rng.range(55.0, 65.0)
                } else {
                    3.0 + 60.0 * (100.0 - self.battery.charge) / 20.0
                };
                self.battery.charge =
                    (self.battery.charge + w * 10.0 / 3600.0 / cap_wh * 100.0).min(100.0);
                self.battery.charging = true;
                self.battery.power = w;
            } else {
                self.battery.charging = false;
                self.battery.power = 0.0;
            }
        } else {
            let drop = base_w * 10.0 / 3600.0 / cap_wh * 100.0;
            self.battery.charge = (self.battery.charge - drop).max(3.0);
            self.discharged += drop;
            if self.discharged >= 100.0 {
                self.discharged -= 100.0;
                self.battery.cycles += 1.0;
            }
            self.battery.charging = false;
            self.battery.power = -base_w;
        }
        self.battery.health = self.health_start + (self.health_end - self.health_start) * progress;

        // Memory.
        let total = self.mem_total.max(8.0e9);
        let ws = self.work_set.step(rng).clamp(0.0, 1.0);
        let app = total * (0.33 + 0.12 * ws + gaming * 0.10 + build * 0.05 + call * 0.02);
        let wired = total * (0.05 + 0.03 * a + gaming * 0.02);
        let compressed = total * (0.064 + 0.008 * ws + rng.normal() * 0.0005);
        let used = app + wired + compressed;
        let cache = self.cache.step(rng).clamp(0.0, 1.0);
        let cached = (total * 0.36 * (0.8 + 0.4 * cache)).min(total - used - 1.0e9);
        let free = (total - used - cached).max(0.0);
        let pressure = (12.0 + (used / total - 0.5) * 38.0 + rng.normal() * 0.6).clamp(5.0, 100.0);
        let level = if pressure >= 80.0 {
            2.0
        } else if pressure >= 50.0 {
            1.0
        } else {
            0.0
        };
        let (swap_in, swap_out) = if gaming > 0.0 && rng.chance(0.04) {
            let pages = rng.range(20.0, 400.0);
            self.swap_used += pages * 16_384.0;
            (rng.range(0.0, 40.0), pages)
        } else {
            (0.0, 0.0)
        };
        let mem = Mem {
            used,
            app,
            wired,
            compressed,
            cached,
            free,
            pressure,
            level,
            swap_used: self.swap_used,
            swap_in,
            swap_out,
        };

        // Disk and network rates, bytes per second.
        let rng = &mut self.rng;
        let download_rate = download * rng.range(8.0e6, 45.0e6);
        let disk_read = (60.0e3 + a * 1.5e6 + build * 30.0e6 + backup * 110.0e6 + gaming * 6.0e6)
            * rng.burst(0.9);
        let disk_write = (250.0e3 + a * 1.0e6 + build * 40.0e6 + backup * 4.0e6) * rng.burst(0.9)
            + download_rate;
        // Used space: a slow trend that ends at today's figure, plus downloads and build
        // output that pile up until a cleanup clears them.
        self.disk_scratch =
            (self.disk_scratch + download_rate * 10.0 * 0.5 + build * 2.0e6).clamp(0.0, 12.0e9);
        let disk_used = self.disk_used - 8.0e9 * (1.0 - progress) + self.disk_scratch;

        let net = net_bucket(rng, plan.browser(t), &active, a, download_rate);
        let net_rx = net.iface_rx_bytes as f32 / 10.0;
        let net_tx = net.iface_tx_bytes as f32 / 10.0;

        // Temperatures, fans and the thermal state.
        let ambient = self.ambient.step(rng).clamp(20.0, 30.0);
        // A fast response to package power plus a slow heat soak under sustained load.
        self.soak = ease(self.soak, power_cpu + power_gpu, 600.0);
        self.cpu_temp = ease(
            self.cpu_temp,
            ambient + 18.0 + power_cpu * 1.2 + self.soak * 0.2,
            60.0,
        );
        self.gpu_temp = ease(
            self.gpu_temp,
            ambient + 12.0 + power_gpu * 1.4 + (self.cpu_temp - ambient) * 0.25,
            70.0,
        );
        self.ssd_heat = ease(
            self.ssd_heat,
            ((disk_read + disk_write) / 150.0e6).min(1.0),
            120.0,
        );
        self.net_heat = ease(self.net_heat, ((net_rx + net_tx) / 20.0e6).min(1.0), 90.0);
        let ssd_temp = ambient + 5.0 + self.ssd_heat * 14.0 + (self.cpu_temp - ambient) * 0.08;
        let battery_temp = ambient
            + 5.5
            + if self.battery.charging { 4.0 } else { 0.0 }
            + if external { 0.0 } else { base_w * 0.08 }
            + (self.cpu_temp - ambient) * 0.04;
        let wifi_temp = ambient + 9.0 + self.net_heat * 18.0 + (self.cpu_temp - ambient) * 0.25;

        self.fans_on = self.cpu_temp > 76.0 || (self.fans_on && self.cpu_temp > 64.0);
        let fan_target = if self.fans_on {
            (1200.0 + (self.cpu_temp - 64.0).max(0.0) * 140.0).min(5800.0)
        } else {
            0.0
        };
        self.fan = ease(self.fan, fan_target, 15.0);
        if !self.fans_on && self.fan < 300.0 {
            self.fan = 0.0;
        }

        // Fair after three minutes above 95 °C, nominal again once well below it.
        if self.cpu_temp > 95.0 {
            self.hot_steps += 1;
        } else {
            self.hot_steps = 0;
        }
        let state = if self.hot_steps >= 18 {
            1.0
        } else if self.cpu_temp < 80.0 {
            0.0
        } else {
            self.thermal_state
        };
        if state != self.thermal_state {
            self.events.push(Event {
                ts_ms: t,
                start_ms: t,
                processes: active_names(&active),
                detail: EventDetail::ThermalState {
                    from: ThermalState::from_value(self.thermal_state),
                    to: if state == 1.0 {
                        ThermalState::Fair
                    } else {
                        ThermalState::Nominal
                    },
                },
            });
            self.thermal_state = state;
        }

        // Fan ramp: the fastest fan rose 1,000 rpm within two minutes.
        self.fan_hist.push((t, self.fan));
        self.fan_hist.retain(|(ts, _)| t - ts <= 2 * MIN);
        if let Some(&(from_t, from)) = self.fan_hist.iter().min_by(|a, b| a.1.total_cmp(&b.1))
            && self.fan - from >= 1000.0
            && t - self.last_ramp > 20 * MIN
        {
            self.events.push(Event {
                ts_ms: t,
                start_ms: from_t,
                processes: active_names(&active),
                detail: EventDetail::FansRamped {
                    from_rpm: from,
                    to_rpm: self.fan,
                },
            });
            self.last_ramp = t;
        }

        // A build or a game that kept one process busy for five minutes.
        for e in plan
            .episodes
            .iter()
            .take_while(|e| e.start <= t)
            .filter(|e| matches!(e.kind, Kind::Build | Kind::Gaming))
        {
            let at = e.start + 5 * MIN;
            if e.end > at && t >= at && t - S10 < at && !self.sustained_seen.contains(&e.start) {
                self.sustained_seen.push(e.start);
                let cpu_pct = if e.kind == Kind::Build {
                    e.level * 520.0
                } else {
                    e.level * 180.0
                };
                self.events.push(Event {
                    ts_ms: t,
                    start_ms: e.start,
                    processes: vec![e.app.to_string()],
                    detail: EventDetail::SustainedProcess {
                        process: e.app.to_string(),
                        cpu_pct,
                        secs: 300,
                    },
                });
            }
        }

        let self_cpu = (0.45 + self.rng.normal() * 0.12 + dashboard * 3.0).max(0.15);

        Tick {
            cpu_total,
            cpu_user,
            e_load,
            p_load,
            cluster_active,
            cluster_power,
            loadavg: self.loadavg,
            gpu_util,
            power_cpu,
            power_gpu,
            power_system,
            mem,
            cpu_temp: self.cpu_temp,
            gpu_temp: self.gpu_temp,
            ambient,
            ssd_temp,
            battery_temp,
            wifi_temp,
            thermal_state: self.thermal_state,
            fan: self.fan,
            disk_read,
            disk_write,
            disk_used,
            disk_total: self.disk_total,
            net_rx,
            net_tx,
            net,
            battery: self.battery,
            self_cpu,
            active,
        }
    }
}

impl Sim {
    /// The top processes by CPU for one bucket, scaled so they add up to the machine's
    /// CPU use.
    fn processes(&mut self, tick: &Tick) -> Vec<ProcRow> {
        let rng = &mut self.rng;
        // A build spawns a fresh set of compiler processes; reuse them for its duration.
        let build = tick.active.get(&Kind::Build).map(|a| a.0).unwrap_or(0.0);
        if build > 0.0 && self.build_pids.is_empty() {
            let n = 4 + (rng.f() * 6.0) as usize;
            self.build_pids = (0..n)
                .map(|_| {
                    self.next_pid += 1 + (rng.f() * 40.0) as i32;
                    self.next_pid
                })
                .collect();
        } else if build == 0.0 {
            self.build_pids.clear();
        }

        let mut rows: Vec<ProcRow> = Vec::new();
        for p in &self.procs {
            let boost = match p.boost {
                Some((kind, cpu)) => tick
                    .active
                    .get(&kind)
                    .filter(|(_, app)| app.is_empty() || *app == p.name || kind != Kind::Call)
                    .map(|(lvl, _)| cpu * lvl)
                    .unwrap_or(0.0),
                None => 0.0,
            };
            let woke = boost > 0.0;
            if p.boost.is_some() && p.base == 0.0 && !woke {
                continue;
            }
            let cpu = (p.base * (0.5 + rng.f()) + boost * rng.range(0.8, 1.2)).max(0.0);
            rows.push(ProcRow {
                name: p.name.to_string(),
                pid: p.pid,
                cpu_pct: cpu,
                mem_bytes: (p.mem_mb * rng.range(0.95, 1.08) * 1_048_576.0) as u64,
                threads: p.threads + (rng.f() * 4.0) as u32,
                idle_wakeups_per_s: p.wakeups * rng.range(0.6, 1.5),
                energy: 0.0,
            });
        }
        for (i, pid) in self.build_pids.iter().enumerate() {
            rows.push(ProcRow {
                name: "rustc".into(),
                pid: *pid,
                cpu_pct: build * rng.range(60.0, 100.0) * if i == 0 { 1.4 } else { 1.0 },
                mem_bytes: (rng.range(250.0, 1400.0) * 1_048_576.0) as u64,
                threads: 6 + (rng.f() * 10.0) as u32,
                idle_wakeups_per_s: rng.range(0.0, 3.0),
                energy: 0.0,
            });
        }
        if let Some((lvl, game)) = tick.active.get(&Kind::Gaming) {
            rows.push(ProcRow {
                name: (*game).to_string(),
                pid: 48_311,
                cpu_pct: lvl * rng.range(140.0, 220.0),
                mem_bytes: (rng.range(7000.0, 9500.0) * 1_048_576.0) as u64,
                threads: 64 + (rng.f() * 20.0) as u32,
                idle_wakeups_per_s: rng.range(200.0, 600.0),
                energy: 0.0,
            });
        }

        // Scale to the machine total, in percent of one core.
        let n_cores = (self.n_e + self.n_p).max(1) as f32;
        let target = tick.cpu_total * n_cores * 0.92;
        let sum: f32 = rows.iter().map(|r| r.cpu_pct).sum();
        if sum > 0.0 {
            let k = target / sum;
            for r in &mut rows {
                r.cpu_pct *= k;
                r.energy = r.cpu_pct * 0.85 + r.idle_wakeups_per_s * 0.01;
            }
        }
        rows.sort_by(|a, b| b.cpu_pct.total_cmp(&a.cpu_pct));
        rows.truncate(PROC_TOP);
        rows
    }
}

fn active_names(active: &HashMap<Kind, (f32, &'static str)>) -> Vec<String> {
    let mut names: Vec<(f32, &str)> = active
        .iter()
        .filter(|(k, (_, app))| {
            !app.is_empty() && matches!(k, Kind::Build | Kind::Gaming | Kind::Call)
        })
        .map(|(_, (lvl, app))| (*lvl, *app))
        .collect();
    names.sort_by(|a, b| b.0.total_cmp(&a.0));
    names.into_iter().map(|(_, n)| n.to_string()).collect()
}

/// Name, base CPU share, MB, threads, idle wake-ups per second, the episode that boosts it.
type ProcSpec = (&'static str, f32, f32, u32, f32, Option<(Kind, f32)>);

#[rustfmt::skip] // A table reads better one process per line.
const PROCS: &[ProcSpec] = &[
    ("kernel_task", 6.0, 160.0, 580, 900.0, Some((Kind::Backup, 20.0))),
    ("WindowServer", 9.0, 1200.0, 24, 120.0, Some((Kind::Gaming, 45.0))),
    ("Google Chrome", 3.0, 950.0, 48, 40.0, None),
    ("Google Chrome Helper (Renderer)", 6.0, 780.0, 22, 30.0, Some((Kind::Video, 35.0))),
    ("Google Chrome Helper (GPU)", 3.0, 420.0, 18, 25.0, Some((Kind::Video, 15.0))),
    ("Safari", 2.0, 520.0, 30, 15.0, None),
    ("com.apple.WebKit.WebContent", 4.0, 680.0, 20, 20.0, Some((Kind::Video, 30.0))),
    ("Slack", 1.5, 410.0, 40, 12.0, None),
    ("Slack Helper (Renderer)", 3.0, 560.0, 21, 18.0, Some((Kind::Call, 45.0))),
    ("Code Helper (Renderer)", 4.0, 890.0, 24, 22.0, None),
    ("Code", 1.5, 360.0, 36, 10.0, None),
    ("rust-analyzer", 3.0, 2400.0, 28, 6.0, Some((Kind::Build, 60.0))),
    ("cargo", 0.0, 140.0, 12, 2.0, Some((Kind::Build, 25.0))),
    ("ghostty", 1.0, 210.0, 14, 9.0, None),
    ("Kelvo", 0.5, 120.0, 22, 3.0, Some((Kind::Dashboard, 3.0))),
    ("mds_stores", 0.8, 90.0, 8, 4.0, Some((Kind::Backup, 35.0))),
    ("backupd", 0.0, 70.0, 10, 3.0, Some((Kind::Backup, 40.0))),
    ("zoom.us", 0.0, 650.0, 52, 80.0, Some((Kind::Call, 85.0))),
    ("FaceTime", 0.0, 210.0, 30, 60.0, Some((Kind::Call, 55.0))),
    ("Spotify", 0.0, 380.0, 44, 30.0, Some((Kind::Music, 6.0))),
    ("coreaudiod", 0.3, 30.0, 12, 10.0, Some((Kind::Call, 8.0))),
    ("softwareupdated", 0.0, 60.0, 9, 2.0, Some((Kind::Download, 12.0))),
    ("Steam Helper", 0.0, 320.0, 30, 40.0, Some((Kind::Gaming, 8.0))),
    ("launchd", 0.4, 22.0, 4, 6.0, None),
    ("logd", 0.6, 26.0, 6, 8.0, None),
    ("bluetoothd", 0.3, 18.0, 6, 9.0, None),
    ("Finder", 0.3, 180.0, 12, 3.0, None),
    ("Dock", 0.2, 120.0, 6, 2.0, None),
    ("Mail", 0.4, 290.0, 26, 4.0, None),
    ("Notes", 0.1, 160.0, 12, 1.0, None),
    ("cloudd", 0.4, 60.0, 14, 4.0, None),
    ("photoanalysisd", 0.2, 140.0, 10, 2.0, None),
    ("airportd", 0.3, 14.0, 6, 5.0, None),
    ("powerd", 0.1, 9.0, 4, 1.0, None),
    ("trustd", 0.2, 16.0, 5, 2.0, None),
    ("corespotlightd", 0.3, 50.0, 7, 3.0, Some((Kind::Download, 6.0))),
    ("syspolicyd", 0.1, 20.0, 5, 1.0, None),
    ("ControlCenter", 0.2, 100.0, 8, 4.0, None),
];

fn base_procs(rng: &mut Rng) -> Vec<Proc> {
    PROCS
        .iter()
        .map(|&(name, base, mem_mb, threads, wakeups, boost)| Proc {
            name,
            pid: 300 + (rng.f() * 30_000.0) as i32,
            base,
            mem_mb,
            threads,
            wakeups,
            boost,
        })
        .collect()
}

/// One 10 s bucket of per-app network bytes; the interface totals add what no app
/// accounts for (system traffic, protocol overhead).
fn net_bucket(
    rng: &mut Rng,
    browser: &'static str,
    active: &HashMap<Kind, (f32, &'static str)>,
    a: f32,
    download_rate: f32,
) -> NetBucket {
    let mut apps: Vec<NetApp> = Vec::new();
    let mut add = |name: &str, rx: f32, tx: f32| {
        if rx + tx < 1.0 {
            return;
        }
        match apps.iter_mut().find(|x| x.name.as_deref() == Some(name)) {
            Some(x) => {
                x.rx_bytes += rx as u64;
                x.tx_bytes += tx as u64;
            }
            None => apps.push(NetApp {
                name: Some(name.to_string()),
                rx_bytes: rx as u64,
                tx_bytes: tx as u64,
            }),
        }
    };
    let secs = 10.0;
    if rng.chance(0.08 + a * 0.4) {
        let rx = 180.0e3 * rng.burst(1.2) * secs;
        add(browser, rx, rx * 0.06);
    }
    add("Slack", 1200.0 * rng.burst(0.8) * secs, 400.0 * secs);
    add("apsd", 150.0 * secs, 90.0 * secs);
    add("mDNSResponder", 250.0 * rng.burst(0.5) * secs, 180.0 * secs);
    if rng.chance(0.02) {
        add("cloudd", 2.0e6 * rng.burst(1.0), 4.0e5 * rng.burst(1.0));
    }
    if rng.chance(0.01) {
        add("Mail", 4.0e5 * rng.burst(1.0), 2.0e4);
    }
    if rng.chance(0.006) {
        add("Dropbox", 1.5e6 * rng.burst(1.0), 8.0e5 * rng.burst(1.0));
    }
    if let Some((lvl, app)) = active.get(&Kind::Call) {
        add(
            app,
            lvl * 290.0e3 * rng.range(0.8, 1.2) * secs,
            lvl * 210.0e3 * secs,
        );
    }
    if active.contains_key(&Kind::Video) {
        let rx = if rng.chance(0.3) {
            2.6e6 * rng.range(0.8, 1.3)
        } else {
            90.0e3
        };
        add(browser, rx * secs, 12.0e3 * secs);
    }
    if active.contains_key(&Kind::Music) {
        let rx = if rng.chance(0.02) {
            5.0e6
        } else {
            2.0e3 * secs
        };
        add("Spotify", rx, 800.0 * secs);
    }
    if let Some((_, app)) = active.get(&Kind::Download) {
        add(app, download_rate * secs, download_rate * 0.015 * secs);
    }
    if let Some((_, game)) = active.get(&Kind::Gaming) {
        add(game, 55.0e3 * rng.burst(0.4) * secs, 28.0e3 * secs);
        add("Steam Helper", 4.0e3 * secs, 1.0e3 * secs);
    }
    let app_rx: u64 = apps.iter().map(|x| x.rx_bytes).sum();
    let app_tx: u64 = apps.iter().map(|x| x.tx_bytes).sum();
    let iface_rx = app_rx + app_rx / 30 + (900.0 * secs) as u64;
    let iface_tx = app_tx + app_tx / 20 + (500.0 * secs) as u64;
    NetBucket {
        measured_ms: 10_000,
        iface_rx_bytes: iface_rx,
        iface_tx_bytes: iface_tx,
        iface_rx_pkts: iface_rx / 1100 + 20,
        iface_tx_pkts: iface_tx / 420 + 15,
        apps,
    }
}

// ---------------------------------------------------------------------------------------
// Series generators: one per series in the layout, reading a `Tick`

enum Gen {
    CpuTotal,
    CpuUser,
    CpuSystem,
    CoreE(usize),
    CoreP(usize),
    ClusterActive(String),
    ClusterFreq(String),
    ClusterPower(String),
    LoadAvg(usize),
    GpuUtil(f32),
    GpuFreq,
    PowerCpu,
    PowerGpu,
    PowerSystem,
    Mem(fn(&Mem) -> f32, Spread),
    ThermalCpu,
    ThermalGpu,
    Hottest,
    Die(f32),
    Dev(f32),
    Constant(f32),
    Ssd {
        integer: bool,
    },
    BatteryTemp,
    Wifi,
    Ambient(f32),
    ThermalState,
    Fan(f32),
    DiskRead {
        primary: bool,
    },
    DiskWrite {
        primary: bool,
    },
    DiskReadTotal,
    DiskWriteTotal,
    DiskUsed,
    DiskFree,
    DiskTotal(f32),
    NetRx {
        primary: bool,
    },
    NetTx {
        primary: bool,
    },
    NetRxTotal,
    NetTxTotal,
    BatteryCharge,
    BatteryCharging,
    BatteryExternal,
    BatteryPower,
    BatteryHealth,
    BatteryCycles,
    SelfCpu,
    /// A series the seed has no model for: its last recorded value, held.
    Held(f32),
}

#[derive(Clone, Copy)]
enum Spread {
    /// Levels that move a little within 10 s.
    Level,
    /// Per-second rates that come in bursts.
    Bursty,
    /// Integer codes and flags.
    Exact,
}

impl Gen {
    fn of(key: &SeriesKey, source: &Source) -> Gen {
        let label = |k: &str| key.labels.get(k).unwrap_or("").to_string();
        let index = |s: &str| {
            s.get(1..)
                .and_then(|n| n.parse::<usize>().ok())
                .unwrap_or(0)
        };
        let hash = |s: &str| {
            s.bytes()
                .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(u32::from(b)))
        };
        let jitter = |s: &str, lo: f32, hi: f32| lo + (hi - lo) * (hash(s) % 1000) as f32 / 1000.0;
        let primary = |label_key: &str, preferred: &str| {
            let mine = label(label_key);
            let first = source
                .keys
                .iter()
                .filter(|k| k.metric == key.metric)
                .filter_map(|k| k.labels.get(label_key))
                .min_by_key(|v| (*v != preferred, v.to_string()))
                .unwrap_or("");
            mine == first
        };
        let latest = source.latest.get(key).copied().unwrap_or(0.0);

        match key.metric.as_str() {
            "cpu.total" => Gen::CpuTotal,
            "cpu.user" => Gen::CpuUser,
            "cpu.system" => Gen::CpuSystem,
            "cpu.load" => {
                let core = label("core");
                if core.starts_with('E') {
                    Gen::CoreE(index(&core))
                } else {
                    Gen::CoreP(index(&core))
                }
            }
            "cpu.loadavg" => Gen::LoadAvg(match label("window").as_str() {
                "1" => 0,
                "5" => 1,
                _ => 2,
            }),
            "cpu.cluster.active" => Gen::ClusterActive(label("cluster")),
            "cpu.cluster.freq" => Gen::ClusterFreq(label("cluster")),
            "cpu.cluster.power" => Gen::ClusterPower(label("cluster")),
            "gpu.util" => Gen::GpuUtil(1.0),
            "gpu.render" => Gen::GpuUtil(0.96),
            "gpu.tiler" => Gen::GpuUtil(1.01),
            "gpu.freq" => Gen::GpuFreq,
            "power.cpu" => Gen::PowerCpu,
            "power.gpu" => Gen::PowerGpu,
            "power.system" => Gen::PowerSystem,
            "mem.used" => Gen::Mem(|m| m.used, Spread::Level),
            "mem.app" => Gen::Mem(|m| m.app, Spread::Level),
            "mem.wired" => Gen::Mem(|m| m.wired, Spread::Level),
            "mem.compressed" => Gen::Mem(|m| m.compressed, Spread::Level),
            "mem.cached" => Gen::Mem(|m| m.cached, Spread::Level),
            "mem.free" => Gen::Mem(|m| m.free, Spread::Level),
            "mem.pressure" => Gen::Mem(|m| m.pressure, Spread::Level),
            "mem.pressure_level" => Gen::Mem(|m| m.level, Spread::Exact),
            "mem.swap_used" => Gen::Mem(|m| m.swap_used, Spread::Exact),
            "mem.swap_in" => Gen::Mem(|m| m.swap_in, Spread::Bursty),
            "mem.swap_out" => Gen::Mem(|m| m.swap_out, Spread::Bursty),
            "thermal.cpu" => Gen::ThermalCpu,
            "thermal.gpu" => Gen::ThermalGpu,
            "thermal.hottest" => Gen::Hottest,
            "thermal.state" => Gen::ThermalState,
            "thermal.zone" | "thermal.sensor" => {
                let name = if key.metric.as_str() == "thermal.zone" {
                    label("sensor")
                } else {
                    label("name")
                };
                let lower = name.to_lowercase();
                if lower.contains("tdie") {
                    Gen::Die(jitter(&name, -0.8, 0.8))
                } else if lower.contains("tdev") {
                    Gen::Dev(jitter(&name, 0.15, 0.6))
                } else if lower.contains("tcal") {
                    Gen::Constant(latest)
                } else if lower.contains("nand") {
                    Gen::Ssd { integer: true }
                } else if lower.contains("ssd") {
                    Gen::Ssd { integer: false }
                } else if lower.contains("battery") {
                    Gen::BatteryTemp
                } else if lower.contains("wifi") {
                    Gen::Wifi
                } else {
                    Gen::Ambient(jitter(&name, 4.0, 10.0))
                }
            }
            "fan.rpm" => Gen::Fan(jitter(&label("fan"), 0.97, 1.02)),
            "disk.read" => Gen::DiskRead {
                primary: primary("dev", "disk0"),
            },
            "disk.write" => Gen::DiskWrite {
                primary: primary("dev", "disk0"),
            },
            "disk.read_total" => Gen::DiskReadTotal,
            "disk.write_total" => Gen::DiskWriteTotal,
            "disk.used" => Gen::DiskUsed,
            "disk.free" => Gen::DiskFree,
            "disk.total" => Gen::DiskTotal(latest),
            "net.rx" => Gen::NetRx {
                primary: primary("iface", "en0"),
            },
            "net.tx" => Gen::NetTx {
                primary: primary("iface", "en0"),
            },
            "net.rx_total" => Gen::NetRxTotal,
            "net.tx_total" => Gen::NetTxTotal,
            "battery.charge" => Gen::BatteryCharge,
            "battery.charging" => Gen::BatteryCharging,
            "battery.external" => Gen::BatteryExternal,
            "battery.power" => Gen::BatteryPower,
            "battery.temp" => Gen::BatteryTemp,
            "battery.health" => Gen::BatteryHealth,
            "battery.cycles" => Gen::BatteryCycles,
            "self.cpu" => Gen::SelfCpu,
            _ => Gen::Held(latest),
        }
    }

    /// `(min, max, avg)` for one 10 s bucket.
    fn stats(&self, tick: &Tick, rng: &mut Rng) -> [f32; 3] {
        use Spread::*;
        let pct = |v: f32| v.clamp(0.0, 100.0);
        let (avg, spread) = match self {
            Gen::CpuTotal => (tick.cpu_total, Level),
            Gen::CpuUser => (tick.cpu_user, Level),
            Gen::CpuSystem => (tick.cpu_total - tick.cpu_user, Level),
            Gen::CoreE(i) => (tick.e_load.get(*i).copied().unwrap_or(0.0), Level),
            Gen::CoreP(i) => (tick.p_load.get(*i).copied().unwrap_or(0.0), Level),
            Gen::ClusterActive(c) => (tick.cluster_active.get(c).copied().unwrap_or(0.0), Level),
            Gen::ClusterFreq(c) => {
                let active = tick.cluster_active.get(c).copied().unwrap_or(0.0) / 100.0;
                let hz = if c.starts_with('E') {
                    1.1e9 + active * 1.47e9
                } else if active < 0.01 {
                    1.1e9
                } else {
                    (1.6e9 + active * 2.45e9).min(4.056e9)
                };
                (hz, Level)
            }
            Gen::ClusterPower(c) => (tick.cluster_power.get(c).copied().unwrap_or(0.0), Level),
            Gen::LoadAvg(i) => (tick.loadavg.get(*i).copied().unwrap_or(0.0), Exact),
            Gen::GpuUtil(k) => (pct(tick.gpu_util * k + rng.normal() * 0.8), Level),
            Gen::GpuFreq => {
                let mhz = if tick.gpu_util < 3.0 {
                    338.0
                } else {
                    338.0 + tick.gpu_util / 100.0 * 1042.0
                };
                (mhz * 1e6, Level)
            }
            Gen::PowerCpu => (tick.power_cpu, Level),
            Gen::PowerGpu => (tick.power_gpu, Level),
            Gen::PowerSystem => (tick.power_system, Level),
            Gen::Mem(f, spread) => (f(&tick.mem), *spread),
            Gen::ThermalCpu => (tick.cpu_temp, Level),
            Gen::ThermalGpu => (tick.gpu_temp, Level),
            Gen::Hottest => (tick.cpu_temp.max(51.85) + 0.3, Exact),
            Gen::Die(off) => (tick.cpu_temp - 5.0 + off, Level),
            Gen::Dev(k) => (
                tick.ambient + 5.0 + (tick.cpu_temp - tick.ambient) * k,
                Level,
            ),
            Gen::Constant(v) => (*v, Exact),
            Gen::Ssd { integer: true } => (tick.ssd_temp.round(), Exact),
            Gen::Ssd { integer: false } => (tick.ssd_temp, Level),
            Gen::BatteryTemp => (tick.battery_temp, Level),
            Gen::Wifi => (tick.wifi_temp, Level),
            Gen::Ambient(off) => (
                tick.ambient + off + (tick.cpu_temp - tick.ambient) * 0.1,
                Level,
            ),
            Gen::ThermalState => (tick.thermal_state, Exact),
            Gen::Fan(k) => (if tick.fan > 0.0 { tick.fan * k } else { 0.0 }, Level),
            Gen::DiskRead { primary } => (if *primary { tick.disk_read } else { 0.0 }, Bursty),
            Gen::DiskWrite { primary } => (if *primary { tick.disk_write } else { 0.0 }, Bursty),
            Gen::DiskReadTotal => (tick.disk_read, Bursty),
            Gen::DiskWriteTotal => (tick.disk_write, Bursty),
            Gen::DiskUsed => (tick.disk_used, Exact),
            Gen::DiskFree => ((tick.disk_total - tick.disk_used).max(0.0), Exact),
            Gen::DiskTotal(v) => (*v, Exact),
            Gen::NetRx { primary } => (if *primary { tick.net_rx } else { 0.0 }, Bursty),
            Gen::NetTx { primary } => (if *primary { tick.net_tx } else { 0.0 }, Bursty),
            Gen::NetRxTotal => (tick.net_rx, Bursty),
            Gen::NetTxTotal => (tick.net_tx, Bursty),
            Gen::BatteryCharge => (tick.battery.charge, Exact),
            Gen::BatteryCharging => (f32::from(u8::from(tick.battery.charging)), Exact),
            Gen::BatteryExternal => (f32::from(u8::from(tick.battery.external)), Exact),
            Gen::BatteryPower => (tick.battery.power, Exact),
            Gen::BatteryHealth => (tick.battery.health, Exact),
            Gen::BatteryCycles => (tick.battery.cycles, Exact),
            Gen::SelfCpu => (tick.self_cpu, Level),
            Gen::Held(v) => (*v, Exact),
        };
        match spread {
            Exact => [avg, avg, avg],
            Level => {
                let w = avg.abs() * 0.04 + 0.2;
                let lo = avg - w * rng.f();
                let hi = avg + w * rng.f();
                [lo.max(0.0).min(avg), hi.max(avg), avg]
            }
            Bursty => {
                let lo = avg * rng.range(0.0, 0.5);
                let hi = avg * rng.range(1.3, 3.5);
                [lo, hi, avg]
            }
        }
    }
}

/// One series of the layout: its model and what the catalog says about sampling and range.
struct Series {
    model: Gen,
    /// Sampled once a minute (disk capacity, battery health): only the minute's first
    /// 10 s bucket has a value, the rest are NaN, as in the app.
    minutely: bool,
    percent: bool,
}

impl Series {
    fn of(key: &SeriesKey, source: &Source) -> Series {
        let def = Catalog::builtin().get(key.metric.as_str());
        Series {
            model: Gen::of(key, source),
            minutely: def.is_some_and(|d| d.period_s >= 60),
            percent: def.is_some_and(|d| d.unit == Unit::Percent),
        }
    }

    fn stats(&self, tick: &Tick, t: i64, rng: &mut Rng) -> [f32; 3] {
        if self.minutely && t.rem_euclid(MIN) != 0 {
            return [f32::NAN; 3];
        }
        let mut stats = self.model.stats(tick, rng);
        if self.percent {
            stats.iter_mut().for_each(|v| *v = v.clamp(0.0, 100.0));
        }
        stats
    }
}

/// Folds 10 s buckets into the open minute: min of mins, max of maxes, mean of averages.
struct MinuteAcc {
    ts: Option<i64>,
    stats: Vec<f32>,
    counts: Vec<u32>,
}

impl MinuteAcc {
    fn new(series: usize) -> Self {
        Self {
            ts: None,
            stats: vec![f32::NAN; series * 3],
            counts: vec![0; series],
        }
    }

    fn add(&mut self, stats: &[f32]) {
        for (i, (acc, s)) in self.stats.chunks_mut(3).zip(stats.chunks(3)).enumerate() {
            let (&[lo, hi, avg], [a_lo, a_hi, a_avg]) = (s, acc) else {
                continue;
            };
            if !avg.is_finite() {
                continue;
            }
            let n = self.counts.get_mut(i);
            let Some(n) = n else { continue };
            if *n == 0 {
                (*a_lo, *a_hi, *a_avg) = (lo, hi, avg);
            } else {
                *a_lo = a_lo.min(lo);
                *a_hi = a_hi.max(hi);
                *a_avg += avg;
            }
            *n += 1;
        }
    }

    fn take(&mut self, host: HostId, layout: &Arc<[SeriesKey]>) -> Option<BucketRow> {
        let ts = self.ts.take()?;
        let mut stats = std::mem::replace(&mut self.stats, vec![f32::NAN; layout.len() * 3]);
        for (acc, n) in stats.chunks_mut(3).zip(&self.counts) {
            if let Some(avg) = acc.get_mut(2)
                && *n > 0
            {
                *avg /= *n as f32;
            }
        }
        self.counts.iter_mut().for_each(|n| *n = 0);
        Some(BucketRow {
            host,
            tier: Tier::M1,
            bucket_ts: ts,
            series: Arc::clone(layout),
            stats,
        })
    }
}
