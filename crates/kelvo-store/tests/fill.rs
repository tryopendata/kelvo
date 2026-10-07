//! Synthetic 30-day fill (architecture.md, Budget math): 31 simulated days of an engine's
//! output written through the real writer, pruned at the end of each day the way the app
//! prunes (rolling minutes older than 7 days into 15-minute rows, D-076), then measured on
//! disk. The budget is 150 MB for 150 persisted series, which must fit 30 days without the
//! byte cap; so must 250 series now. A third run with a smaller cap checks that the cap
//! (D-057) still trims through the 15-minute history at that scale.
//!
//! Ignored by default because it writes about a gigabyte through SQLite. Run it with
//! `make test-fill` (release build); CI runs it in the Linux job.

// clippy.toml allows unwrap inside #[test] functions only; the helpers in this test-only
// file panic on failure by design.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::*;
use kelvo_schema::{Gap, GapReason, HostId, SeriesKey, Tier};
use kelvo_store::{BucketRow, CapTrim, NetApp, NetBucket, ProcRow, Retention, Store, StoreConfig};

const DAYS: i64 = 31;
const BUDGET_150: u64 = 150 * 1_000_000;
/// A cap the 250-series fill exceeds, to exercise the trim at scale.
const CAP_SMALL: u64 = 111 * 1_000_000;
const PROCS_PER_SNAPSHOT: usize = 30;
/// More apps per network bucket than the store keeps, so every row is the full top 20
/// plus "other apps" (D-089).
const NET_APPS_PER_BUCKET: usize = 25;

/// `n` persisted series shaped like a real host: per-core load, cluster stats, memory,
/// power, thermal zones, fans, network and disk.
fn series(n: usize) -> Arc<[SeriesKey]> {
    let fixed = [
        "cpu.total",
        "cpu.user",
        "cpu.system",
        "mem.used",
        "mem.app",
        "mem.wired",
        "mem.compressed",
        "mem.cached",
        "mem.free",
        "mem.pressure",
        "power.cpu",
        "power.gpu",
        "power.ane",
        "power.dram",
        "power.package",
        "power.system",
        "gpu.util",
        "gpu.freq",
        "thermal.hottest",
        "battery.charge",
    ];
    let mut keys: Vec<SeriesKey> = fixed.iter().map(|k| key(k)).collect();
    let mut i = 0;
    while keys.len() < n {
        let k = match i % 6 {
            0 => format!("cpu.load{{core=P{i}}}"),
            1 => format!("thermal.zone{{sensor=PMU tdie{i}}}"),
            2 => format!("cpu.cluster.freq{{cluster=C{i}}}"),
            3 => format!("net.rx{{iface=en{i}}}"),
            4 => format!("disk.read{{dev=disk{i}}}"),
            _ => format!("fan.rpm{{fan={i}}}"),
        };
        keys.push(key(&k));
        i += 1;
    }
    keys.truncate(n);
    keys.into()
}

/// Deterministic, incompressible-looking stats.
fn stats(n: usize, seed: u64) -> Vec<f32> {
    let mut x = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
    (0..n * 3)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            (x % 100_000) as f32 / 1000.0
        })
        .collect()
}

fn procs(seed: u64) -> Vec<ProcRow> {
    (0..PROCS_PER_SNAPSHOT)
        .map(|i| ProcRow {
            name: format!("process-{}", (seed as usize + i) % 80),
            pid: 1000 + i as i32,
            cpu_pct: (PROCS_PER_SNAPSHOT - i) as f32 * 1.5,
            mem_bytes: ((i as u64 + 1) * 50) << 20,
            threads: 4 + i as u32,
            idle_wakeups_per_s: i as f32,
            energy: 0.5,
        })
        .collect()
}

/// A 10 s network bucket with its apps drawn from a pool of 60 names.
fn net(seed: u64) -> NetBucket {
    NetBucket {
        measured_ms: 10_000,
        iface_rx_bytes: 5_000_000 + seed % 1_000,
        iface_tx_bytes: 400_000 + seed % 997,
        iface_rx_pkts: 4_000 + seed % 89,
        iface_tx_pkts: 2_000 + seed % 83,
        apps: (0..NET_APPS_PER_BUCKET)
            .map(|i| NetApp {
                name: Some(format!("app-{}", (seed as usize + i) % 60)),
                rx_bytes: (NET_APPS_PER_BUCKET - i) as u64 * 10_000 + seed % 1_000,
                tx_bytes: i as u64 * 100 + seed % 100,
            })
            .collect(),
    }
}

struct Measured {
    bytes: u64,
    /// Largest size on disk (WAL included) any daily prune reported.
    max_after_prune: u64,
    last_trim: Option<CapTrim>,
    wal_before_close: u64,
    m1_rows: i64,
    m15_rows: i64,
    s10_rows: i64,
    snaps: i64,
    top_rows: i64,
    top15_rows: i64,
    net_rows: [i64; 3],
    elapsed: Duration,
}

fn fill(n: usize, retention: Retention) -> Measured {
    fill_with(n, retention, |_, _| {})
}

/// [`fill`], then `inspect` on the filled store (with the host) before it closes.
fn fill_with(n: usize, retention: Retention, inspect: impl FnOnce(&Store, HostId)) -> Measured {
    let started = Instant::now();
    let dir = temp_dir(&format!("fill-{n}-{}", retention.max_bytes));
    let path = dir.path().join("history.sqlite");
    let store = Store::open(StoreConfig::new(&path)).unwrap();
    let w = store.writer();
    let h = host(1);
    w.upsert_host(h.clone()).unwrap();
    let layout = series(n);
    assert_eq!(layout.len(), n);
    let (mut max_after_prune, mut last_trim) = (0, None);

    for day in 0..DAYS {
        let day_start = T0 + day * DAY;
        for minute in 0..24 * 60 {
            let ts = day_start + minute * MIN;
            let seed = ts as u64;
            w.write_bucket(BucketRow {
                host: h.id,
                tier: Tier::M1,
                bucket_ts: ts,
                series: Arc::clone(&layout),
                stats: stats(n, seed),
            })
            .unwrap();
            for s in 0..6 {
                let t = ts + s * 10_000;
                w.write_bucket(BucketRow {
                    host: h.id,
                    tier: Tier::S10,
                    bucket_ts: t,
                    series: Arc::clone(&layout),
                    stats: stats(n, seed + s as u64 + 1),
                })
                .unwrap();
                w.write_proc_snapshot(h.id, t, procs(seed + s as u64))
                    .unwrap();
                w.write_net_bucket(h.id, t, net(seed + s as u64)).unwrap();
            }
            // The real writer commits every 5 minutes (D-070); an hour per batch keeps the queue
            // bounded without changing what lands on disk.
            if minute % 60 == 59 {
                w.flush().unwrap();
            }
        }
        // A night's sleep, as the engine would record it.
        w.write_gap(
            h.id,
            Gap::host(
                day_start + 2 * HOUR,
                Some(day_start + 2 * HOUR + 1),
                GapReason::Sleep,
            )
            .unwrap(),
        )
        .unwrap();
        w.write_event(h.id, day_start + 3 * HOUR, "fans_ramped", vec![0xa0])
            .unwrap();
        w.flush().unwrap();
        let report = w.prune(day_start + DAY, retention).unwrap();
        max_after_prune = max_after_prune.max(report.size_bytes);
        last_trim = report.cap_trim.or(last_trim);
    }

    inspect(&store, h.id);
    let db = raw(&store);
    let m = Measured {
        bytes: 0,
        max_after_prune,
        last_trim,
        wal_before_close: std::fs::metadata(dir.path().join("history.sqlite-wal"))
            .map(|m| m.len())
            .unwrap_or(0),
        m1_rows: count(&db, "SELECT count(*) FROM tier_1m"),
        m15_rows: count(&db, "SELECT count(*) FROM tier_15m"),
        s10_rows: count(&db, "SELECT count(*) FROM tier_10s"),
        snaps: count(&db, "SELECT count(*) FROM proc_snap"),
        top_rows: count(&db, "SELECT count(*) FROM proc_top_1m"),
        top15_rows: count(&db, "SELECT count(*) FROM proc_top_15m"),
        net_rows: ["proc_net_10s", "proc_net_1m", "proc_net_15m"]
            .map(|t| count(&db, &format!("SELECT count(*) FROM {t}"))),
        elapsed: Duration::ZERO,
    };
    drop(db);
    store.close().unwrap();
    let bytes = kelvo_store::size_on_disk(&path).unwrap();
    Measured {
        bytes,
        elapsed: started.elapsed(),
        ..m
    }
}

fn report(n: usize, m: &Measured) {
    println!(
        "fill {n} series: {:.1} MB on disk after pruning and close ({} bytes; largest right after a \
         prune {:.1} MB; WAL before close {:.1} MB); tier_1m {} rows, tier_15m {} rows, \
         tier_10s {} rows, proc_snap {}, proc_top_1m {}, proc_top_15m {}; proc_net 10s/1m/15m {:?}; \
         cap trim {:?}; {:.1}s",
        m.bytes as f64 / 1e6,
        m.bytes,
        m.max_after_prune as f64 / 1e6,
        m.wal_before_close as f64 / 1e6,
        m.m1_rows,
        m.m15_rows,
        m.s10_rows,
        m.snaps,
        m.top_rows,
        m.top15_rows,
        m.net_rows,
        m.last_trim,
        m.elapsed.as_secs_f64()
    );
}

#[test]
#[ignore = "writes ~1 GB through SQLite; run with `make test-fill` (release)"]
fn thirty_days_fit_the_budget() {
    let m150 = fill(150, Retention::default());
    report(150, &m150);
    // Retention leaves 7 days of minutes, 23 more days of 15-minute rows, 24 h of 10 s
    // buckets, 72 h of snapshots, the top-5 minutes from there to 7 days, and the top-5
    // quarters before that.
    assert_eq!(m150.m1_rows, 7 * 24 * 60);
    assert_eq!(m150.m15_rows, 23 * 24 * 4);
    assert_eq!(m150.s10_rows, 24 * 360);
    assert_eq!(m150.snaps, 72 * 360);
    assert_eq!(m150.top_rows, (7 - 3) * 24 * 60);
    assert_eq!(m150.top15_rows, 23 * 24 * 4);
    // Network: 72 h of 10 s buckets, minutes for 7 days, quarters before that.
    assert_eq!(m150.net_rows, [72 * 360, 7 * 24 * 60, 23 * 24 * 4]);
    assert!(
        m150.bytes <= BUDGET_150,
        "150 series take {} bytes, over the {BUDGET_150} byte budget",
        m150.bytes
    );
    assert!(m150.last_trim.is_none(), "150 series fit without the cap");
    assert!(m150.max_after_prune <= BUDGET_150);

    // 250 series now fit 30 days too.
    let m250 = fill(250, Retention::default());
    report(250, &m250);
    assert!(m250.last_trim.is_none(), "250 series fit without the cap");
    assert_eq!(m250.m15_rows, 23 * 24 * 4);
    assert!(m250.bytes <= BUDGET_150, "{} bytes after close", m250.bytes);

    // A cap below what 250 series need: the cap trims the 15-minute history (oldest
    // first) and stays met right after every prune.
    let cap = CAP_SMALL;
    let small = fill(
        250,
        Retention {
            max_bytes: cap,
            ..Retention::default()
        },
    );
    report(250, &small);
    let trim = small.last_trim.expect("250 series hit the smaller cap");
    assert!(trim.cap_met);
    assert!(
        small.max_after_prune <= cap,
        "{} bytes right after a prune",
        small.max_after_prune
    );
    assert!(small.bytes <= cap, "{} bytes after close", small.bytes);
    assert!(
        (1..23 * 24 * 4).contains(&small.m15_rows),
        "the oldest quarters were trimmed, not all of them"
    );
    assert_eq!(small.m1_rows, 7 * 24 * 60, "minutes untouched");
}

/// A threshold from `perf-budget.json`, section `store`.
fn budget_ms(name: &str) -> u128 {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../perf-budget.json");
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    v["store"][name]
        .as_u64()
        .unwrap_or_else(|| panic!("perf-budget.json store.{name} missing"))
        .into()
}

/// v1.1 acceptance: on 30 days of synthetic history (150 series), the Timeline's 30d
/// range (six lane queries plus the heatmap) reads in under 500 ms, and a 30-day CSV of
/// every series exports in under 3 s. Each read runs on a new connection, as the first
/// render after launch would. Prints the numbers for the decision log.
#[test]
#[ignore = "fills ~1 GB through SQLite; run with `make test-read-perf` (release)"]
fn thirty_day_reads_meet_the_budget() {
    use kelvo_schema::SeriesSelector;
    use kelvo_store::{ExportQuery, HistoryQuery, TierChoice};

    let sel = |m: &str| SeriesSelector {
        metric: key(m).metric,
        labels: Default::default(),
    };
    // The Timeline's lanes (v1-local-monitor.md 4.6), one request each, as it sends them.
    let lanes: Vec<Vec<SeriesSelector>> = vec![
        vec![sel("cpu.total")],
        vec![sel("gpu.util"), sel("gpu.freq")],
        vec![sel("mem.pressure")],
        vec![sel("power.system"), sel("power.cpu")],
        vec![sel("thermal.hottest")],
        vec![sel("net.rx")],
    ];
    let now = T0 + DAYS * DAY;
    fill_with(150, Retention::default(), |store, h| {
        let read = |from: i64, max_points: u32| {
            let started = Instant::now();
            let mut r = store.reader().unwrap();
            let mut points = 0;
            let mut tier = None;
            for selectors in &lanes {
                let res = r
                    .history(&HistoryQuery {
                        host: h,
                        selectors: selectors.clone(),
                        from_ms: from,
                        to_ms: now,
                        tier: TierChoice::Auto,
                        max_points,
                    })
                    .unwrap();
                points += res.series.iter().map(|s| s.points.len()).sum::<usize>();
                tier = Some((res.tier, res.bucket_ms));
            }
            (started.elapsed(), points, tier.unwrap())
        };
        // About 2 points per pixel of an 1,100-pixel plot.
        let (week, week_points, week_tier) = read(now - 7 * DAY, 2200);
        let (month, month_points, month_tier) = read(now - 30 * DAY, 2200);
        assert_eq!(week_tier, (Tier::M1, 5 * MIN));
        assert_eq!(month_tier, (Tier::M15, 30 * MIN));

        // The heatmap: 720 hourly cells (UTC hours stand in for local ones).
        let started = Instant::now();
        let cells: Vec<(i64, i64)> = (0..30 * 24)
            .map(|i| (now - 30 * DAY + i * HOUR, now - 30 * DAY + (i + 1) * HOUR))
            .collect();
        let hours = store
            .reader()
            .unwrap()
            .heatmap(h, &key("cpu.total"), &cells)
            .unwrap();
        let heatmap = started.elapsed();
        assert_eq!(hours.iter().filter(|v| v.is_some()).count(), 30 * 24);

        // Every series, 30 days, to a file.
        let dir = temp_dir("export-perf");
        let path = dir.path().join("export.csv");
        let started = Instant::now();
        let mut out = std::io::BufWriter::new(std::fs::File::create(&path).unwrap());
        let summary = store
            .reader()
            .unwrap()
            .export_csv(
                &ExportQuery {
                    host: h,
                    selectors: series(150)
                        .iter()
                        .map(|k| SeriesSelector {
                            metric: k.metric.clone(),
                            labels: k.labels.clone(),
                        })
                        .collect(),
                    from_ms: now - 30 * DAY,
                    to_ms: now,
                    tier: TierChoice::Auto,
                },
                &mut out,
            )
            .unwrap();
        std::io::Write::flush(&mut out).unwrap();
        let export = started.elapsed();
        let bytes = std::fs::metadata(&path).unwrap().len();
        assert_eq!(summary.series.len(), 150);
        assert_eq!(summary.rows, 30 * 24 * 4);

        println!(
            "30-day reads, 150 series: Timeline 7d {:.1} ms ({week_points} points, 6 queries), \
             30d {:.1} ms ({month_points} points), heatmap {:.1} ms; CSV export {:.1} ms \
             ({} rows, {:.1} MB)",
            week.as_secs_f64() * 1e3,
            month.as_secs_f64() * 1e3,
            heatmap.as_secs_f64() * 1e3,
            export.as_secs_f64() * 1e3,
            summary.rows,
            bytes as f64 / 1e6,
        );
        let timeline = (month + heatmap).as_millis();
        assert!(
            timeline < budget_ms("timeline30dMs"),
            "30d Timeline reads took {timeline} ms"
        );
        assert!(
            export.as_millis() < budget_ms("export30dMs"),
            "30-day export took {} ms",
            export.as_millis()
        );
    });
}
