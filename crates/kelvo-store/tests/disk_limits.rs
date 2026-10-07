//! The byte cap and the low-disk guard (D-057), on real SQLite files.

// clippy.toml allows unwrap inside #[test] functions only; the helpers in this test-only
// file panic on failure by design.
#![allow(clippy::unwrap_used)]

mod common;

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use common::*;
use kelvo_schema::{
    Cursor, Gap, GapReason, Labels, MetricId, SeriesKey, SeriesSelector, SyncKinds, Tier,
};
use kelvo_store::{
    CursorRead, FreeSpace, HistoryQuery, LowDiskGuard, Retention, Store, TierChoice, VolumeSpace,
    Writer,
};

fn sel(metric: &'static str) -> SeriesSelector {
    SeriesSelector {
        metric: MetricId::from_static(metric),
        labels: Labels::new(),
    }
}

fn wide(n: usize) -> Arc<[SeriesKey]> {
    let mut keys = vec![key("cpu.total")];
    keys.extend((1..n).map(|i| key(&format!("cpu.load{{core=P{i}}}"))));
    keys.into()
}

/// `minutes` of M1 buckets from `start`, flushed.
fn fill_m1(w: &Writer, h: kelvo_schema::HostId, l: &Arc<[SeriesKey]>, start: i64, minutes: i64) {
    for i in 0..minutes {
        w.write_bucket(bucket(h, Tier::M1, start + i * MIN, l, i as f32))
            .unwrap();
        if i % 1000 == 999 {
            w.flush().unwrap();
        }
    }
    w.flush().unwrap();
}

fn query(h: kelvo_schema::HostId, from: i64, to: i64) -> HistoryQuery {
    HistoryQuery {
        host: h,
        selectors: vec![sel("cpu.total")],
        from_ms: from,
        to_ms: to,
        tier: TierChoice::Auto,
        max_points: 100_000,
    }
}

const DAYS: i64 = 10;

/// 30 days of history, all of it minutes: no roll-down into M15, so the cap arithmetic
/// below stays in minutes. The roll-down's interplay with the cap has its own test.
fn minutes_only() -> Retention {
    Retention {
        m1_ms: 30 * DAY,
        ..Retention::with_days(30)
    }
}

#[test]
fn cap_trims_oldest_history_and_truncates_cursors_behind_it() {
    let dir = temp_dir("cap");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    let h = host(1);
    w.upsert_host(h.clone()).unwrap();
    let l = wide(100);
    fill_m1(&w, h.id, &l, T0, DAYS * 24 * 60);
    // A gap early in history, one straddling day 5, and events at both ends.
    w.write_gap(
        h.id,
        Gap::host(T0 + HOUR, Some(T0 + 2 * HOUR), GapReason::Sleep).unwrap(),
    )
    .unwrap();
    let straddle = Gap::host(T0 + 5 * DAY - HOUR, Some(T0 + 8 * DAY), GapReason::Paused).unwrap();
    w.write_gap(h.id, straddle).unwrap();
    w.write_event(h.id, T0 + 10 * MIN, "fans_ramped", vec![1])
        .unwrap();
    w.write_event(h.id, T0 + 9 * DAY, "fans_ramped", vec![2])
        .unwrap();
    w.flush().unwrap();
    let mut r = store.reader().unwrap();
    let early = Some(Cursor {
        epoch: store.epoch(),
        seq: 10,
    });

    let now = T0 + DAYS * DAY;
    let full = w.prune(now, minutes_only()).unwrap();
    assert!(full.cap_trim.is_none(), "default cap: nothing to trim");
    assert_eq!(full.m1_rows, 0, "all within 30 days");
    assert_eq!(full.m1_rolled, 0);
    assert!(full.size_bytes > 10_000_000, "{} bytes", full.size_bytes);

    // A cap of about half the file.
    let retention = Retention {
        max_bytes: full.size_bytes / 2,
        ..minutes_only()
    };
    let report = w.prune(now, retention).unwrap();
    let trim = report.cap_trim.expect("over the cap: trimmed");
    assert!(trim.cap_met);
    assert_eq!(trim.size_before, full.size_bytes);
    assert!(
        report.size_bytes <= retention.max_bytes,
        "{} on disk vs cap {}",
        report.size_bytes,
        retention.max_bytes
    );
    assert_eq!(report.size_bytes, store.size_on_disk().unwrap());
    let earliest = trim.earliest_ts_ms;
    assert!(earliest > T0 && earliest < T0 + 8 * DAY, "{earliest}");
    assert_eq!(earliest % MIN, 0, "cutoff on a minute boundary");

    let db = raw(&store);
    // Everything before the cutoff went; everything after it stayed.
    assert_eq!(count(&db, "SELECT min(bucket_ts) FROM tier_1m"), earliest);
    assert_eq!(
        count(&db, "SELECT count(*) FROM tier_1m"),
        (now - earliest) / MIN
    );
    assert_eq!(trim.m1_rows as i64, (earliest - T0) / MIN);
    assert_eq!(trim.gaps, 1, "the early sleep gap");
    assert_eq!(trim.events, 1, "the early event");
    // No gap was written for the trimmed span: before `earliest` is simply no data.
    let gaps = r.gaps(h.id, T0, now).unwrap();
    assert_eq!(
        gaps,
        vec![straddle],
        "the straddling gap stays, nothing new"
    );
    let hist = r.history(&query(h.id, T0, now)).unwrap();
    assert_eq!(hist.tier, Tier::M1);
    assert_eq!(hist.series[0].points[0].t, earliest);

    // The pruned mark moved to the cutoff: an old cursor gets Truncated there.
    match r
        .read_after(h.id, Tier::M1, early, 10, SyncKinds::ALL)
        .unwrap()
    {
        CursorRead::Truncated { earliest_ts_ms, .. } => assert_eq!(earliest_ts_ms, earliest),
        other => panic!("expected Truncated, got {other:?}"),
    }
    assert_eq!(
        count(&db, "SELECT ts FROM pruned WHERE tier = 'm1'"),
        earliest
    );
    // A cursor at the mark reads on from the first kept row.
    let at_mark = Some(Cursor {
        epoch: store.epoch(),
        seq: count(&db, "SELECT seq FROM pruned WHERE tier = 'm1'"),
    });
    match r
        .read_after(h.id, Tier::M1, at_mark, 10, SyncKinds::ALL)
        .unwrap()
    {
        CursorRead::Page(p) => assert!(p.rows.iter().all(|row| row.bucket_ts >= earliest)),
        other => panic!("expected a page, got {other:?}"),
    }

    // Low water: the trim went to 90% of the cap, so an hour more data and another prune
    // trims nothing.
    fill_m1(&w, h.id, &l, now, 60);
    let again = w.prune(now + HOUR, retention).unwrap();
    assert!(again.cap_trim.is_none(), "{again:?}");
    assert!(again.size_bytes <= retention.max_bytes);
}

/// D-076: with minutes older than 7 days rolled into 15-minute rows, the cap trims the
/// 15-minute history first and then the minutes, on 15-minute boundaries, and moves both
/// tiers' pruned marks.
#[test]
fn cap_trims_through_the_15_minute_tier_into_minutes() {
    let dir = temp_dir("cap-m15");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    let h = host(1);
    w.upsert_host(h.clone()).unwrap();
    let l = wide(100);
    fill_m1(&w, h.id, &l, T0, DAYS * 24 * 60);
    let now = T0 + DAYS * DAY;
    let full = w.prune(now, Retention::with_days(30)).unwrap();
    assert!(full.cap_trim.is_none());
    // Days 0 to 3 rolled down: 4 days of minutes into 4 * 96 quarters.
    assert_eq!(full.m1_rolled, 3 * 24 * 60);
    assert_eq!(full.m15_written, 3 * 24 * 4);
    let db = raw(&store);
    assert_eq!(count(&db, "SELECT count(*) FROM tier_15m"), 3 * 24 * 4);
    assert_eq!(
        count(&db, "SELECT min(bucket_ts) FROM tier_1m"),
        now - 7 * DAY
    );
    let m15_seq = Some(Cursor {
        epoch: store.epoch(),
        seq: 1,
    });

    // A cap that the 15-minute rows alone cannot meet: about a third of the file.
    let retention = Retention {
        max_bytes: full.size_bytes / 3,
        ..Retention::with_days(30)
    };
    let report = w.prune(now, retention).unwrap();
    let trim = report.cap_trim.expect("over the cap: trimmed");
    assert!(trim.cap_met, "{trim:?}");
    assert_eq!(trim.m15_rows, 3 * 24 * 4, "all of the 15-minute history");
    assert!(trim.m1_rows > 0, "and some minutes: {trim:?}");
    let earliest = trim.earliest_ts_ms;
    assert_eq!(earliest % (15 * MIN), 0, "cutoff on a 15-minute boundary");
    assert_eq!(count(&db, "SELECT count(*) FROM tier_15m"), 0);
    assert_eq!(count(&db, "SELECT min(bucket_ts) FROM tier_1m"), earliest);
    // The M15 mark stops at the round that removed the last 15-minute row, like every
    // mark: past the end of the 15-minute history, not past the cutoff.
    let m15_mark = count(&db, "SELECT ts FROM pruned WHERE tier = 'm15'");
    assert!(
        (T0 + 3 * DAY..=earliest).contains(&m15_mark),
        "{m15_mark} vs earliest {earliest}"
    );
    let mut r = store.reader().unwrap();
    match r
        .read_after(h.id, Tier::M15, m15_seq, 10, SyncKinds::ALL)
        .unwrap()
    {
        CursorRead::Truncated { earliest_ts_ms, .. } => assert_eq!(earliest_ts_ms, m15_mark),
        other => panic!("expected Truncated, got {other:?}"),
    }
    // A long range still reads from the first kept minute, at 15-minute resolution.
    let hist = r.history(&query(h.id, T0, now)).unwrap();
    assert_eq!(hist.tier, Tier::M15);
    assert_eq!(hist.series[0].points[0].t, earliest);
}

#[test]
fn cap_never_trims_the_last_day() {
    let dir = temp_dir("cap-floor");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    let h = host(1);
    w.upsert_host(h.clone()).unwrap();
    let l = wide(100);
    fill_m1(&w, h.id, &l, T0, 3 * 24 * 60);
    let now = T0 + 3 * DAY;
    let retention = Retention {
        max_bytes: 100_000,
        ..Retention::default()
    };
    let report = w.prune(now, retention).unwrap();
    let trim = report.cap_trim.expect("trimmed");
    assert!(!trim.cap_met, "a 100 kB cap cannot hold a day");
    assert_eq!(trim.earliest_ts_ms, now - DAY);
    let db = raw(&store);
    assert_eq!(count(&db, "SELECT count(*) FROM tier_1m"), 24 * 60);
    // A second prune has nothing more it may trim and changes nothing.
    let again = w.prune(now, retention).unwrap();
    assert_eq!(again.cap_trim.unwrap().m1_rows, 0);
    assert_eq!(count(&db, "SELECT count(*) FROM tier_1m"), 24 * 60);
}

#[test]
fn prune_checkpoints_so_the_file_shrinks_right_away() {
    let dir = temp_dir("shrink");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    let h = host(1);
    w.upsert_host(h.clone()).unwrap();
    let l = wide(100);
    fill_m1(&w, h.id, &l, T0, 3_000);
    let before = store.size_on_disk().unwrap();
    // Everything is past a 7-day retention.
    let report = w
        .prune(T0 + 3_000 * MIN + 8 * DAY, Retention::with_days(7))
        .unwrap();
    assert_eq!(report.m1_rows, 3_000);
    let after = store.size_on_disk().unwrap();
    assert_eq!(report.size_bytes, after);
    let wal = std::fs::metadata(dir.path().join("h.sqlite-wal")).map_or(0, |m| m.len());
    assert_eq!(wal, 0, "WAL truncated by the checkpoint");
    assert!(
        after < before / 4,
        "{after} after vs {before} before, store still open"
    );
}

/// Free space the test sets by hand.
#[derive(Clone)]
struct FakeSpace(Arc<AtomicU64>);

const TOTAL: u64 = 1_000_000_000_000;

impl FreeSpace for FakeSpace {
    fn volume_space(&self, _: &Path) -> std::io::Result<VolumeSpace> {
        Ok(VolumeSpace {
            available_bytes: self.0.load(Ordering::Relaxed),
            total_bytes: TOTAL,
        })
    }
}

#[test]
fn low_disk_pauses_s10_keeps_m1_and_resumes_with_hysteresis() {
    let dir = temp_dir("low-disk");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    let h = host(1);
    w.upsert_host(h.clone()).unwrap();
    let l = layout(&["cpu.total"]);
    let free = Arc::new(AtomicU64::new(100_000_000_000));
    let mut guard = LowDiskGuard::new(FakeSpace(Arc::clone(&free)), store.path(), w.clone());
    let write_minute = |m: i64| {
        let ts = T0 + m * MIN;
        w.write_bucket(bucket(h.id, Tier::M1, ts, &l, 1.0)).unwrap();
        for s in 0..6 {
            w.write_bucket(bucket(h.id, Tier::S10, ts + s * 10_000, &l, 1.0))
                .unwrap();
        }
        w.flush().unwrap();
    };
    let db = raw(&store);
    let s10 = || count(&db, "SELECT count(*) FROM tier_10s");
    let m1 = || count(&db, "SELECT count(*) FROM tier_1m");

    assert!(!guard.check(T0).unwrap());
    write_minute(0);
    assert_eq!((s10(), m1()), (6, 1));

    // 1.5 GB free of 1 TB: under the 2 GB threshold.
    free.store(1_500_000_000, Ordering::Relaxed);
    assert!(guard.check(T0 + MIN).unwrap());
    assert!(w.s10_paused(), "every writer clone sees the pause");
    write_minute(1);
    assert_eq!((s10(), m1()), (6, 2), "S10 dropped, M1 kept");
    let mut r = store.reader().unwrap();
    // The 10 s tier has a hole from the pause on, so short ranges read M1 there.
    assert_eq!(
        r.history(&query(h.id, T0 + MIN, T0 + 2 * MIN))
            .unwrap()
            .tier,
        Tier::M1
    );

    // 2.5 GB is above the threshold but under the 3 GB resume line: still paused.
    free.store(2_500_000_000, Ordering::Relaxed);
    assert!(guard.check(T0 + 2 * MIN).unwrap());
    write_minute(2);
    assert_eq!(s10(), 6);

    free.store(10_000_000_000, Ordering::Relaxed);
    assert!(!guard.check(T0 + 3 * MIN).unwrap());
    write_minute(3);
    assert_eq!((s10(), m1()), (12, 4), "S10 written again");
    // After the resume S10 is whole again; a range reaching into the hole is not.
    assert_eq!(
        r.history(&query(h.id, T0 + 3 * MIN, T0 + 4 * MIN))
            .unwrap()
            .tier,
        Tier::S10
    );
    assert_eq!(
        r.history(&query(h.id, T0 + 2 * MIN, T0 + 4 * MIN))
            .unwrap()
            .tier,
        Tier::M1
    );
    // One marker is conservative: even a range wholly before the pause reads M1 until
    // S10 retention has moved past the resume.
    assert_eq!(
        r.history(&query(h.id, T0, T0 + MIN)).unwrap().tier,
        Tier::M1
    );
}

#[test]
fn a_run_that_ended_paused_starts_unpaused_with_the_hole_closed() {
    let dir = temp_dir("low-disk-reopen");
    let path = dir.path().join("h.sqlite");
    let h = host(1);
    {
        let store = open(&dir, "h.sqlite");
        let w = store.writer();
        w.upsert_host(h.clone()).unwrap();
        w.set_s10_paused(true, T0).unwrap();
        store.close().unwrap();
    }
    let store = Store::open(kelvo_store::StoreConfig::new(&path)).unwrap();
    let w = store.writer();
    assert!(!w.s10_paused());
    let l = layout(&["cpu.total"]);
    // Written well after the reopen (which used the wall clock), so the hole cannot be
    // still open: the marker was set to the open time, not left at "forever".
    let late = T0 + 365 * DAY;
    w.write_bucket(bucket(h.id, Tier::S10, late, &l, 1.0))
        .unwrap();
    w.flush().unwrap();
    let mut r = store.reader().unwrap();
    assert_eq!(
        r.history(&query(h.id, late, late + MIN)).unwrap().tier,
        Tier::S10
    );
}
