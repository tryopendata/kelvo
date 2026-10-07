//! The 15-minute tier (D-076): pruning rolls minutes older than the M1 window into
//! `tier_15m` and `proc_top_15m`, reads pick the tier by where the range starts, and the
//! 15-minute history is pruned with the rest of retention.

// clippy.toml allows unwrap inside #[test] functions only; the helpers in this test-only
// file panic on failure by design.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;

use common::*;
use kelvo_schema::{Gap, GapReason, Labels, MetricId, SeriesKey, SeriesSelector, Tier};
use kelvo_store::{
    BucketRow, HistoryQuery, ProcResolution, ProcRow, Retention, TierChoice, Writer,
};

const Q: i64 = 15 * MIN;
const NAN: f32 = f32::NAN;

fn sel(metric: &'static str) -> SeriesSelector {
    SeriesSelector {
        metric: MetricId::from_static(metric),
        labels: Labels::new(),
    }
}

fn query(h: kelvo_schema::HostId, from: i64, to: i64, tier: TierChoice) -> HistoryQuery {
    HistoryQuery {
        host: h,
        selectors: vec![sel("cpu.total"), sel("gpu.util")],
        from_ms: from,
        to_ms: to,
        tier,
        max_points: 100_000,
    }
}

fn minute(w: &Writer, h: kelvo_schema::HostId, ts: i64, l: &Arc<[SeriesKey]>, stats: Vec<f32>) {
    w.write_bucket(BucketRow {
        host: h,
        tier: Tier::M1,
        bucket_ts: ts,
        series: Arc::clone(l),
        stats,
    })
    .unwrap();
}

/// `(bucket_ts, layout series count, stats)` of every `tier_15m` row, in time order.
fn m15_rows(db: &rusqlite::Connection) -> Vec<(i64, Vec<f32>)> {
    db.prepare("SELECT bucket_ts, blob FROM tier_15m ORDER BY bucket_ts, layout_id")
        .unwrap()
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)))
        .unwrap()
        .map(|row| {
            let (ts, b) = row.unwrap();
            let stats = b
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| f32::from_le_bytes(*c))
                .collect();
            (ts, stats)
        })
        .collect()
}

fn same(a: &[f32], b: &[f32]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| (x.is_nan() && y.is_nan()) || x == y)
}

#[test]
fn minutes_past_the_window_fold_into_15_minute_rows() {
    let dir = TempDir::new("m15-fold");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    let h = host(1);
    w.upsert_host(h.clone()).unwrap();
    let both = layout(&["cpu.total", "gpu.util"]);
    let cpu_only = layout(&["cpu.total"]);

    // Quarter 0: ten minutes of the wide layout (GPU sampled in one of them), then the
    // layout changes for the last five minutes.
    for i in 0..10 {
        let v = i as f32;
        let gpu = if i == 3 {
            [5.0, 7.0, 6.0]
        } else {
            [NAN, NAN, NAN]
        };
        let mut stats = vec![v - 1.0, v + 1.0, v];
        stats.extend(gpu);
        minute(&w, h.id, T0 + i * MIN, &both, stats);
    }
    for i in 10..15 {
        let v = 100.0 + i as f32;
        minute(&w, h.id, T0 + i * MIN, &cpu_only, vec![v - 1.0, v + 1.0, v]);
    }
    // Quarter 1: in the layout, never sampled.
    for i in 15..30 {
        minute(&w, h.id, T0 + i * MIN, &both, vec![NAN; 6]);
    }
    // Quarter 2 is a sleep: an explicit gap, no rows.
    w.write_gap(
        h.id,
        Gap::host(T0 + 2 * Q, Some(T0 + 3 * Q), GapReason::Sleep).unwrap(),
    )
    .unwrap();
    // Quarter 3: steady.
    for i in 45..60 {
        minute(
            &w,
            h.id,
            T0 + i * MIN,
            &both,
            vec![0.0, 2.0, 1.0, 9.0, 11.0, 10.0],
        );
    }
    // The last hour, inside the window.
    let now = T0 + 8 * DAY;
    for i in 0..60 {
        minute(
            &w,
            h.id,
            now - HOUR + i * MIN,
            &both,
            vec![49.0, 51.0, 50.0, 1.0, 1.0, 1.0],
        );
    }
    w.flush().unwrap();

    let report = w.prune(now, Retention::default()).unwrap();
    assert_eq!(report.m1_rolled, 10 + 5 + 15 + 15);
    assert_eq!(
        report.m15_written, 4,
        "two layouts in quarter 0, then 1 and 3"
    );
    assert_eq!(report.m1_rows, 0, "nothing past the 30-day retention");

    let db = raw(&store);
    let rows = m15_rows(&db);
    let expect: Vec<(i64, Vec<f32>)> = vec![
        // min of mins, max of maxes, mean of averages; GPU from its one minute.
        (T0, vec![-1.0, 10.0, 4.5, 5.0, 7.0, 6.0]),
        // The layout after the change: its own row, never merged into the other.
        (T0, vec![109.0, 115.0, 112.0]),
        // Never sampled: NaN, not zeros.
        (T0 + Q, vec![NAN; 6]),
        (T0 + 3 * Q, vec![0.0, 2.0, 1.0, 9.0, 11.0, 10.0]),
    ];
    assert_eq!(rows.len(), expect.len(), "{rows:?}");
    for ((ts, got), (ets, want)) in rows.iter().zip(&expect) {
        assert_eq!(ts, ets);
        assert!(same(got, want), "{ts}: {got:?} vs {want:?}");
    }
    assert_eq!(
        count(&db, "SELECT count(*) FROM tier_1m"),
        60,
        "only the window's minutes stay"
    );
    // The sleep is still a gap, and no row was made up for it.
    assert_eq!(count(&db, "SELECT count(*) FROM gaps"), 1);
    assert_eq!(
        count(&db, "SELECT ts FROM pruned WHERE tier = 'm1_rolled'"),
        now - 7 * DAY
    );

    // A second prune finds nothing more to roll and leaves the rows alone.
    let before = dump(&db, "tier_15m");
    let again = w.prune(now + MIN, Retention::default()).unwrap();
    assert_eq!((again.m1_rolled, again.m15_written), (0, 0));
    assert_eq!(dump(&db, "tier_15m"), before);

    // Auto: a range that starts before the window reads 15-minute buckets, and still
    // shows the minutes inside the window, merged into the same slots.
    let mut r = store.reader().unwrap();
    let all = r.history(&query(h.id, T0, now, TierChoice::Auto)).unwrap();
    assert_eq!(all.tier, Tier::M15);
    let cpu = &all.series[0];
    assert_eq!(cpu.key, key("cpu.total"));
    let t: Vec<i64> = cpu.points.iter().map(|p| p.t).collect();
    assert_eq!(
        t,
        [
            T0,
            T0 + 3 * Q,
            now - HOUR,
            now - 3 * Q,
            now - 2 * Q,
            now - Q
        ],
        "no point for the unsampled quarter or the sleep"
    );
    let first = cpu.points[0];
    assert_eq!((first.min, first.max), (-1.0, 115.0));
    assert_eq!(
        first.avg,
        (4.5 + 112.0) / 2.0,
        "the two layouts' rows weigh alike"
    );
    assert_eq!(cpu.points[2].avg, 50.0);
    assert_eq!(all.gaps.len(), 1);
    // Inside the window: minutes.
    let recent = r
        .history(&query(h.id, now - 3 * DAY, now, TierChoice::Auto))
        .unwrap();
    assert_eq!(recent.tier, Tier::M1);
    assert_eq!(recent.series[0].points.len(), 60);
}

/// A 7-day range ending a little before the last prune starts a few minutes before the
/// roll cut. It stays on minutes: flipping to 15-minute buckets on prune timing would
/// change the 7d chart's resolution from one refresh to the next.
#[test]
fn a_range_starting_less_than_a_quarter_before_the_roll_cut_reads_minutes() {
    let dir = TempDir::new("m15-auto-slack");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    let h = host(1);
    w.upsert_host(h.clone()).unwrap();
    let l = layout(&["cpu.total"]);
    let now = T0 + 8 * DAY;
    let cut = now - 7 * DAY;
    for i in -60..60 {
        minute(&w, h.id, cut + i * MIN, &l, vec![1.0, 1.0, 1.0]);
    }
    w.flush().unwrap();
    w.prune(now, Retention::default()).unwrap();
    let mut r = store.reader().unwrap();

    let lagging = r
        .history(&query(h.id, cut - 5 * MIN, now - 5 * MIN, TierChoice::Auto))
        .unwrap();
    assert_eq!(lagging.tier, Tier::M1);
    assert_eq!(lagging.series[0].points.first().map(|p| p.t), Some(cut));

    let older = r
        .history(&query(h.id, cut - 20 * MIN, now, TierChoice::Auto))
        .unwrap();
    assert_eq!(
        older.tier,
        Tier::M15,
        "more than a quarter rolled down: 15-minute buckets"
    );
}

/// A slot that holds a 15-minute row and minutes (where the window starts) weighs each
/// by the time it covers.
#[test]
fn a_slot_mixing_quarters_and_minutes_weighs_them_by_width() {
    let dir = TempDir::new("m15-mix");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    let h = host(1);
    w.upsert_host(h.clone()).unwrap();
    let l = layout(&["cpu.total"]);
    // An hour that rolls down entirely, then one minute of the next hour that stays.
    let start = T0 + DAY - HOUR;
    for i in 0..60 {
        minute(&w, h.id, start + i * MIN, &l, vec![0.0, 0.0, 0.0]);
    }
    minute(&w, h.id, T0 + DAY, &l, vec![60.0, 60.0, 60.0]);
    w.flush().unwrap();
    w.prune(T0 + 8 * DAY, Retention::default()).unwrap();

    let mut r = store.reader().unwrap();
    let q = HistoryQuery {
        max_points: 1,
        ..query(h.id, start, T0 + DAY + 2 * Q, TierChoice::Fixed(Tier::M15))
    };
    let res = r.history(&q).unwrap();
    let p = res.series[0].points[0];
    // Four quarters at 0 (60 minutes) and one minute at 60: 60 / 61.
    assert!((p.avg - 60.0 / 61.0).abs() < 1e-6, "{}", p.avg);
}

#[test]
fn process_minutes_roll_into_15_minute_top_5() {
    let dir = TempDir::new("m15-procs");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    let h = host(1);
    w.upsert_host(h.clone()).unwrap();
    let proc = |name: &str, cpu: f32| ProcRow {
        name: name.into(),
        pid: 7,
        cpu_pct: cpu,
        mem_bytes: 1 << 20,
        threads: 2,
        idle_wakeups_per_s: 0.0,
        energy: 0.0,
    };
    // Minute 0: "a" busy; minute 1: "b" busier. Each minute has one snapshot.
    w.write_proc_snapshot(h.id, T0, vec![proc("a", 30.0)])
        .unwrap();
    w.write_proc_snapshot(h.id, T0 + MIN, vec![proc("b", 60.0)])
        .unwrap();
    w.flush().unwrap();
    let now = T0 + 8 * DAY;
    let report = w.prune(now, Retention::default()).unwrap();
    assert_eq!(report.proc_snaps_rolled, 2);
    assert_eq!(report.proc_top_rolled, 2);
    let db = raw(&store);
    assert_eq!(count(&db, "SELECT count(*) FROM proc_top_1m"), 0);
    assert_eq!(count(&db, "SELECT count(*) FROM proc_top_15m"), 1);

    let mut r = store.reader().unwrap();
    let at = r.processes_at(h.id, T0 + 7 * MIN).unwrap().unwrap();
    assert_eq!(at.resolution, ProcResolution::Top5Per15Minutes);
    assert_eq!(at.ts_ms, T0);
    let got: Vec<(&str, f32)> = at
        .rows
        .iter()
        .map(|p| (p.name.as_str(), p.cpu_pct))
        .collect();
    // Means over the two minutes present; each process was missing from one.
    assert_eq!(got, [("b", 30.0), ("a", 15.0)]);
}

#[test]
fn the_15_minute_history_ends_with_retention() {
    let dir = TempDir::new("m15-retention");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    let h = host(1);
    w.upsert_host(h.clone()).unwrap();
    let l = layout(&["cpu.total"]);
    for i in 0..(2 * 60) {
        minute(&w, h.id, T0 + i * MIN, &l, vec![1.0, 1.0, 1.0]);
    }
    w.flush().unwrap();
    w.prune(T0 + 8 * DAY, Retention::default()).unwrap();
    let db = raw(&store);
    assert_eq!(count(&db, "SELECT count(*) FROM tier_15m"), 8);

    // 30 days and an hour later the first hour is past retention, the second is not.
    let report = w.prune(T0 + 30 * DAY + HOUR, Retention::default()).unwrap();
    assert_eq!(report.m15_rows, 4);
    assert_eq!(count(&db, "SELECT count(*) FROM tier_15m"), 4);
    assert_eq!(
        count(&db, "SELECT ts FROM pruned WHERE tier = 'm15'"),
        T0 + HOUR
    );

    // With 7-day retention nothing is rolled: minutes simply end with retention.
    for i in 0..60 {
        minute(&w, h.id, T0 + 40 * DAY + i * MIN, &l, vec![1.0, 1.0, 1.0]);
    }
    w.flush().unwrap();
    let report = w.prune(T0 + 48 * DAY, Retention::with_days(7)).unwrap();
    assert_eq!((report.m1_rolled, report.m1_rows), (0, 60));
}
