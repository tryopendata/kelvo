//! Store behaviour through the public API, on real SQLite files.

// clippy.toml allows unwrap inside #[test] functions only; the helpers in this test-only
// file panic on failure by design.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::*;
use kelvo_schema::{
    Cursor, Event, EventDetail, Gap, GapReason, HostId, Labels, MetricId, Module, SeriesSelector,
    SyncKinds, ThermalState, Tier,
};
use kelvo_store::{
    CursorRead, HistoryQuery, ProcResolution, ProcRow, Retention, Store, StoreConfig, StoreError,
    TierChoice,
};

fn sel(metric: &'static str) -> SeriesSelector {
    SeriesSelector {
        metric: MetricId::from_static(metric),
        labels: Labels::new(),
    }
}

#[test]
fn host_upsert_is_keyed_by_uuid() {
    let dir = temp_dir("hosts");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    let mut h = host(1);
    w.upsert_host(h.clone()).unwrap();
    h.display_name = "renamed".into();
    w.upsert_host(h.clone()).unwrap();
    w.upsert_host(host(2)).unwrap();
    w.flush().unwrap();
    let hosts = store.reader().unwrap().hosts().unwrap();
    assert_eq!(hosts, vec![h, host(2)], "one row per uuid, local first");
}

#[test]
fn writes_for_an_unknown_host_fail() {
    let dir = temp_dir("unknown-host");
    let store = open(&dir, "h.sqlite");
    let err = store
        .writer()
        .begin_session(HostId(uuid::Uuid::from_u128(9)), T0)
        .unwrap_err();
    assert!(matches!(err, StoreError::UnknownHost(_)), "{err}");
}

#[test]
fn interning_and_layout_dedupe() {
    let dir = temp_dir("intern");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    let h = host(1);
    w.upsert_host(h.clone()).unwrap();
    let a = layout(&["cpu.total", "cpu.load{core=P0}"]);
    let same = layout(&["cpu.total", "cpu.load{core=P0}"]);
    let reordered = layout(&["cpu.load{core=P0}", "cpu.total"]);
    for (i, l) in [&a, &same, &reordered].into_iter().enumerate() {
        w.write_bucket(bucket(h.id, Tier::M1, T0 + i as i64 * MIN, l, 1.0))
            .unwrap();
    }
    w.flush().unwrap();
    let db = raw(&store);
    assert_eq!(count(&db, "SELECT count(*) FROM series"), 2);
    assert_eq!(
        count(&db, "SELECT count(*) FROM layouts"),
        2,
        "equal lists share a layout; a reordering is a new one"
    );
    assert_eq!(count(&db, "SELECT count(*) FROM tier_1m"), 3);
    assert_eq!(
        count(&db, "SELECT count(DISTINCT labels) FROM series"),
        2,
        "canonical label text: '' and 'core=P0'"
    );
}

#[test]
fn bad_rows_are_rejected_before_queueing() {
    let dir = temp_dir("bad-rows");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    let h = host(1);
    let l = layout(&["cpu.total"]);
    let mut short = bucket(h.id, Tier::M1, T0, &l, 1.0);
    short.stats.pop();
    assert!(matches!(
        w.write_bucket(short),
        Err(StoreError::StatsLength { .. })
    ));
    assert!(matches!(
        w.write_bucket(bucket(h.id, Tier::Live1s, T0, &l, 1.0)),
        Err(StoreError::NotPersisted(Tier::Live1s))
    ));
    assert!(
        w.write_gap(h.id, Gap::module_disabled(5, Some(1), Module::Cpu))
            .is_err()
    );
}

#[test]
fn seq_is_monotonic_and_replaying_a_batch_changes_nothing() {
    let dir = temp_dir("replay");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    let h = host(1);
    w.upsert_host(h.clone()).unwrap();
    let l = layout(&["cpu.total", "mem.used"]);
    let batch: Vec<_> = (0..5)
        .map(|i| bucket(h.id, Tier::S10, T0 + i * 10_000, &l, i as f32))
        .collect();
    for b in &batch {
        w.write_bucket(b.clone()).unwrap();
    }
    w.flush().unwrap();
    let db = raw(&store);
    let before = dump(&db, "tier_10s");
    let next_before = count(&db, "SELECT value FROM meta WHERE key = 'next_seq'");
    let seqs: Vec<i64> = db
        .prepare("SELECT seq FROM tier_10s ORDER BY bucket_ts")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert!(seqs.windows(2).all(|w| w[0] < w[1]), "{seqs:?}");
    assert_eq!(next_before, seqs.last().unwrap() + 1);

    for b in &batch {
        w.write_bucket(b.clone()).unwrap();
    }
    w.flush().unwrap();
    assert_eq!(dump(&db, "tier_10s"), before, "identical rows, same seq");
    assert_eq!(
        count(&db, "SELECT value FROM meta WHERE key = 'next_seq'"),
        next_before,
        "a no-op upsert does not consume a seq"
    );

    // A changed value is a new version of the row: same key, new seq.
    w.write_bucket(bucket(h.id, Tier::S10, T0, &l, 42.0))
        .unwrap();
    w.flush().unwrap();
    assert_eq!(count(&db, "SELECT count(*) FROM tier_10s"), 5);
    assert_eq!(
        count(
            &db,
            &format!("SELECT seq FROM tier_10s WHERE bucket_ts = {T0}")
        ),
        next_before
    );
}

#[test]
fn writer_batches_until_flush_or_interval() {
    let dir = temp_dir("batch");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    let h = host(1);
    w.upsert_host(h.clone()).unwrap();
    w.flush().unwrap();
    let l = layout(&["cpu.total"]);
    w.write_bucket(bucket(h.id, Tier::M1, T0, &l, 1.0)).unwrap();
    // A round trip that does not commit (nothing to repair at T0), so the bucket has been
    // applied, but not committed: a reader must not see it yet.
    w.begin_session(h.id, T0).unwrap();
    let db = raw(&store);
    assert_eq!(count(&db, "SELECT count(*) FROM tier_1m"), 0);
    w.flush().unwrap();
    assert_eq!(count(&db, "SELECT count(*) FROM tier_1m"), 1);
    drop(store);

    // With a short interval the batch commits on its own.
    let mut cfg = StoreConfig::new(dir.path().join("interval.sqlite"));
    cfg.commit_interval = Duration::from_millis(50);
    let store = kelvo_store::Store::open(cfg).unwrap();
    let w = store.writer();
    w.upsert_host(h.clone()).unwrap();
    w.write_bucket(bucket(h.id, Tier::M1, T0, &l, 1.0)).unwrap();
    let db = raw(&store);
    let deadline = Instant::now() + Duration::from_secs(5);
    while count(&db, "SELECT count(*) FROM tier_1m") == 0 {
        assert!(Instant::now() < deadline, "interval commit never happened");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// D-083: an event is readable as soon as the engine asks for a commit, not at the
/// 5-minute batch commit; the reader decodes it whole and skips what it cannot read.
#[test]
fn events_commit_soon_and_read_back_in_range() {
    let dir = temp_dir("events");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    let h = host(1);
    w.upsert_host(h.clone()).unwrap();
    w.flush().unwrap();
    let thermal = |ts: i64, to| Event {
        ts_ms: ts,
        start_ms: ts,
        processes: vec![],
        detail: EventDetail::ThermalState {
            from: Some(ThermalState::Nominal),
            to,
        },
    };
    let fans = Event {
        ts_ms: T0 + MIN,
        start_ms: T0 + 30_000,
        processes: vec!["kernel_task".into(), "clang".into()],
        detail: EventDetail::FansRamped {
            from_rpm: 1200.0,
            to_rpm: 3400.0,
        },
    };
    w.record_event(h.id, &thermal(T0, ThermalState::Fair))
        .unwrap();
    w.record_event(h.id, &fans).unwrap();
    w.record_event(h.id, &thermal(T0 + HOUR, ThermalState::Fair))
        .unwrap();
    // Same ts and kind: the second replaces the first.
    w.record_event(h.id, &thermal(T0, ThermalState::Serious))
        .unwrap();
    // A kind from a newer build and a payload that is not CBOR.
    w.write_event(h.id, T0 + 2 * MIN, "battery_swap", {
        let mut b = Vec::new();
        ciborium::into_writer(
            &serde_json::json!({"ts_ms": T0 + 2 * MIN, "start_ms": 0, "processes": [], "detail": {"kind": "battery_swap"}}),
            &mut b,
        )
        .unwrap();
        b
    })
    .unwrap();
    w.write_event(h.id, T0 + 3 * MIN, "power_spike", vec![0xff, 0x00])
        .unwrap();

    let db = raw(&store);
    assert_eq!(
        count(&db, "SELECT count(*) FROM events"),
        0,
        "not committed yet"
    );
    w.commit_soon().unwrap();
    // A round trip queued behind the commit: once it answers, the commit has run.
    w.begin_session(h.id, T0).unwrap();
    assert_eq!(count(&db, "SELECT count(*) FROM events"), 5);

    let mut r = store.reader().unwrap();
    let got = r.events(h.id, T0, T0 + HOUR).unwrap();
    assert_eq!(got, vec![thermal(T0, ThermalState::Serious), fans]);
    assert_eq!(r.events(h.id, T0 + HOUR, T0 + HOUR + 1).unwrap().len(), 1);
    assert!(r.events(h.id, T0 - DAY, T0).unwrap().is_empty());
}

#[test]
fn close_commits_and_checkpoints() {
    let dir = temp_dir("close");
    let path = dir.path().join("h.sqlite");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    let h = host(1);
    w.upsert_host(h.clone()).unwrap();
    w.write_bucket(bucket(h.id, Tier::M1, T0, &layout(&["cpu.total"]), 1.0))
        .unwrap();
    store.close().unwrap();
    assert!(matches!(w.flush(), Err(StoreError::WriterGone)));
    let wal = std::fs::metadata(dir.path().join("h.sqlite-wal"))
        .map(|m| m.len())
        .unwrap_or(0);
    assert_eq!(wal, 0, "WAL truncated on close");
    let reopened = open(&dir, "h.sqlite");
    assert_eq!(reopened.path(), path);
    assert_eq!(count(&raw(&reopened), "SELECT count(*) FROM tier_1m"), 1);
}

#[test]
fn gaps_open_close_and_module_scope() {
    let dir = temp_dir("gaps");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    let h = host(1);
    w.upsert_host(h.clone()).unwrap();
    w.open_gap(h.id, T0, None, GapReason::Sleep).unwrap();
    w.open_gap(h.id, T0 + 5, None, GapReason::Sleep).unwrap(); // already open: no-op
    w.open_gap(h.id, T0, Some(Module::Disk), GapReason::ModuleDisabled)
        .unwrap();
    w.close_gap(h.id, GapReason::Sleep, None, None, T0 + HOUR)
        .unwrap();
    w.flush().unwrap();
    let gaps = store
        .reader()
        .unwrap()
        .gaps(h.id, T0 - DAY, T0 + DAY)
        .unwrap();
    assert_eq!(
        gaps,
        vec![
            Gap::host(T0, Some(T0 + HOUR), GapReason::Sleep).unwrap(),
            Gap::module_disabled(T0, None, Module::Disk),
        ]
    );
    assert!(!gaps[1].affects(Module::Cpu));
}

#[test]
fn startup_closes_open_gaps_and_writes_app_not_running() {
    let dir = temp_dir("startup");
    let h = host(1);
    let l = layout(&["cpu.total"]);
    {
        let store = open(&dir, "h.sqlite");
        let w = store.writer();
        w.upsert_host(h.clone()).unwrap();
        w.write_bucket(bucket(h.id, Tier::M1, T0, &l, 1.0)).unwrap();
        w.write_bucket(bucket(h.id, Tier::S10, T0 + 50_000, &l, 1.0))
            .unwrap();
        // The machine went to sleep and the app died before waking.
        w.open_gap(h.id, T0 + 60_000, None, GapReason::Sleep)
            .unwrap();
        w.flush().unwrap();
    }
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    let now = T0 + 3 * HOUR;
    let start = w.begin_session(h.id, now).unwrap();
    assert_eq!(start.closed_gaps, 1);
    let anr = Gap::host(T0 + 60_000, Some(now), GapReason::AppNotRunning).unwrap();
    assert_eq!(start.app_not_running, Some(anr));
    w.flush().unwrap();
    let gaps = store.reader().unwrap().gaps(h.id, T0, now).unwrap();
    assert_eq!(
        gaps,
        vec![
            Gap::host(T0 + 60_000, Some(T0 + 60_000), GapReason::Sleep).unwrap(),
            anr
        ],
        "the sleep gap ends at the last data; app_not_running covers the rest"
    );

    // A second startup right after does not duplicate anything.
    let again = w.begin_session(h.id, now).unwrap();
    assert_eq!(again.closed_gaps, 0);
    assert_eq!(again.app_not_running, None);

    // A host with no history gets nothing.
    w.upsert_host(host(2)).unwrap();
    assert_eq!(
        w.begin_session(host(2).id, now).unwrap(),
        Default::default()
    );
}

#[test]
fn history_across_layouts_merges_to_max_points_and_skips_nan() {
    let dir = temp_dir("history");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    let h = host(1);
    w.upsert_host(h.clone()).unwrap();
    // The layout changes after 30 minutes: a disk appears.
    let before = layout(&["cpu.total", "cpu.load{core=P0}"]);
    let after = layout(&["cpu.total", "cpu.load{core=P0}", "disk.read{dev=disk4}"]);
    for i in 0..60 {
        let l = if i < 30 { &before } else { &after };
        let mut b = bucket(h.id, Tier::M1, T0 + i * MIN, l, i as f32);
        if i == 10 {
            // cpu.load not sampled in this bucket.
            b.stats[3..6].copy_from_slice(&[f32::NAN; 3]);
        }
        w.write_bucket(b).unwrap();
    }
    w.write_gap(
        h.id,
        Gap::host(T0 + 70 * MIN, Some(T0 + 80 * MIN), GapReason::Sleep).unwrap(),
    )
    .unwrap();
    w.flush().unwrap();

    let mut r = store.reader().unwrap();
    let q = HistoryQuery {
        host: h.id,
        selectors: vec![sel("cpu.total"), sel("cpu.load"), sel("disk.read")],
        from_ms: T0,
        to_ms: T0 + 2 * HOUR,
        tier: TierChoice::Fixed(Tier::M1),
        max_points: 1000,
    };
    let full = r.history(&q).unwrap();
    assert_eq!(full.tier, Tier::M1);
    let names: Vec<String> = full.series.iter().map(|s| s.key.to_string()).collect();
    assert_eq!(
        names,
        ["cpu.load{core=P0}", "cpu.total", "disk.read{dev=disk4}"]
    );
    assert_eq!(
        full.series[1].points.len(),
        60,
        "one point per bucket, across both layouts"
    );
    assert_eq!(
        full.series[0].points.len(),
        59,
        "the NaN bucket is absent, not zero"
    );
    assert!(full.series[0].points.iter().all(|p| p.t != T0 + 10 * MIN));
    assert_eq!(full.series[2].points.len(), 30);
    assert_eq!(full.series[2].points[0].t, T0 + 30 * MIN);
    let p = full.series[1].points[5];
    assert_eq!((p.t, p.min, p.max, p.avg), (T0 + 5 * MIN, 4.0, 6.0, 5.0));
    assert_eq!(
        full.gaps,
        vec![Gap::host(T0 + 70 * MIN, Some(T0 + 80 * MIN), GapReason::Sleep).unwrap()]
    );

    // Merged down: 120 one-minute buckets into at most 12 slots of 10 minutes.
    let merged = r
        .history(&HistoryQuery {
            max_points: 12,
            ..q.clone()
        })
        .unwrap();
    let total = &merged.series[1].points;
    assert_eq!(total.len(), 6, "data covers the first 6 slots");
    assert!(merged.series.iter().all(|s| s.points.len() <= 12));
    let first = total[0];
    assert_eq!(first.t, T0);
    assert_eq!(
        (first.min, first.max),
        (-1.0, 10.0),
        "min of mins, max of maxes"
    );
    assert_eq!(first.avg, 4.5, "mean of 0..=9");

    // Only cpu.load selected, by label.
    let one = r
        .history(&HistoryQuery {
            selectors: vec![SeriesSelector {
                metric: MetricId::from_static("cpu.load"),
                labels: Labels::single("core", "P0"),
            }],
            ..q.clone()
        })
        .unwrap();
    assert_eq!(one.series.len(), 1);
}

#[test]
fn auto_tier_prefers_s10_until_it_is_pruned_past_the_range() {
    let dir = temp_dir("auto-tier");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    let h = host(1);
    w.upsert_host(h.clone()).unwrap();
    let l = layout(&["cpu.total"]);
    for i in 0..(3 * 360) {
        w.write_bucket(bucket(h.id, Tier::S10, T0 + i * 10_000, &l, 1.0))
            .unwrap();
    }
    for i in 0..180 {
        w.write_bucket(bucket(h.id, Tier::M1, T0 + i * MIN, &l, 1.0))
            .unwrap();
    }
    w.flush().unwrap();
    let mut r = store.reader().unwrap();
    let q = |from: i64, to: i64| HistoryQuery {
        host: h.id,
        selectors: vec![sel("cpu.total")],
        from_ms: from,
        to_ms: to,
        tier: TierChoice::Auto,
        max_points: 10_000,
    };
    assert_eq!(r.history(&q(T0, T0 + HOUR)).unwrap().tier, Tier::S10);
    assert_eq!(r.history(&q(T0, T0 + 2 * DAY)).unwrap().tier, Tier::M1);
    // Pruning the 10 s tier at T0 + 2h (now = T0 + 26h) moves early ranges to M1.
    w.prune(T0 + 26 * HOUR, Retention::default()).unwrap();
    assert_eq!(r.history(&q(T0, T0 + HOUR)).unwrap().tier, Tier::M1);
    assert_eq!(
        r.history(&q(T0 + 2 * HOUR, T0 + 3 * HOUR)).unwrap().tier,
        Tier::S10
    );
}

fn proc(name: &str, pid: i32, cpu: f32) -> ProcRow {
    ProcRow {
        name: name.into(),
        pid,
        cpu_pct: cpu,
        mem_bytes: 300 << 20,
        threads: 12,
        idle_wakeups_per_s: 3.5,
        energy: 1.25,
    }
}

#[test]
fn processes_snapshot_then_roll_down_to_top_five() {
    let dir = temp_dir("procs");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    let h = host(1);
    w.upsert_host(h.clone()).unwrap();
    // Six snapshots in the first minute, seven processes each.
    for s in 0..6 {
        let rows = (0..7)
            .map(|p| proc(&format!("proc{p}"), 100 + p, (10 * p + s) as f32))
            .collect();
        w.write_proc_snapshot(h.id, T0 + s as i64 * 10_000, rows)
            .unwrap();
    }
    w.write_proc_snapshot(h.id, T0 + 5 * MIN, vec![proc("late", 7, 1.0)])
        .unwrap();
    w.flush().unwrap();

    let mut r = store.reader().unwrap();
    let at = r.processes_at(h.id, T0 + 12_000).unwrap().unwrap();
    assert_eq!(at.resolution, ProcResolution::Snapshot);
    assert_eq!(at.ts_ms, T0 + 10_000, "nearest snapshot");
    assert_eq!(at.rows.len(), 7);
    assert_eq!(at.rows[3], proc("proc3", 103, 31.0));
    assert!(r.processes_at(h.id, T0 + 2 * MIN).unwrap().is_none());

    // 72 h later the first minute is rolled down; the 5-minute snapshot is still young
    // enough relative to a cutoff of T0 + 3 min.
    let report = w
        .prune(T0 + 3 * MIN + 3 * DAY, Retention::default())
        .unwrap();
    assert_eq!(report.proc_snaps_rolled, 6);
    let at = r.processes_at(h.id, T0 + 12_000).unwrap().unwrap();
    assert_eq!(at.resolution, ProcResolution::Top5PerMinute);
    assert_eq!(at.ts_ms, T0);
    let names: Vec<&str> = at.rows.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["proc6", "proc5", "proc4", "proc3", "proc2"]);
    assert_eq!(at.rows[0].cpu_pct, 62.5, "mean of 60..=65");
    assert_eq!(
        r.processes_at(h.id, T0 + 5 * MIN)
            .unwrap()
            .unwrap()
            .resolution,
        ProcResolution::Snapshot
    );
}

/// Performance mode writes a snapshot every 30 s (D-088): any instant between two still
/// finds the nearer one.
#[test]
fn processes_at_finds_a_snapshot_between_thirty_second_ones() {
    let dir = temp_dir("procs-30s");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    let h = host(1);
    w.upsert_host(h.clone()).unwrap();
    for s in 0..3 {
        w.write_proc_snapshot(h.id, T0 + s * 30_000, vec![proc("p", 1, s as f32)])
            .unwrap();
    }
    w.flush().unwrap();
    let mut r = store.reader().unwrap();
    for (t, nearest) in [
        (14_000, 0),
        (16_000, 30_000),
        (45_000, 30_000),
        (59_000, 60_000),
    ] {
        let at = r.processes_at(h.id, T0 + t).unwrap().unwrap();
        assert_eq!(at.resolution, ProcResolution::Snapshot);
        assert_eq!(at.ts_ms, T0 + nearest, "at +{t} ms");
    }
}

#[test]
fn cursor_reads_page_in_seq_order_with_gaps_and_events_on_m1() {
    let dir = temp_dir("cursor");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    let h = host(1);
    w.upsert_host(h.clone()).unwrap();
    let a = layout(&["cpu.total"]);
    let b = layout(&["cpu.total", "mem.used"]);
    w.write_bucket(bucket(h.id, Tier::M1, T0, &a, 1.0)).unwrap();
    w.write_bucket(bucket(h.id, Tier::S10, T0, &a, 1.0))
        .unwrap();
    w.write_gap(
        h.id,
        Gap::host(T0 + MIN, Some(T0 + 2 * MIN), GapReason::Paused).unwrap(),
    )
    .unwrap();
    w.write_event(h.id, T0 + 3 * MIN, "fans_ramped", vec![0xa0])
        .unwrap();
    w.write_bucket(bucket(h.id, Tier::M1, T0 + 4 * MIN, &b, 2.0))
        .unwrap();
    w.flush().unwrap();

    let mut r = store.reader().unwrap();
    let epoch = store.epoch();
    let CursorRead::Page(p1) = r
        .read_after(h.id, Tier::M1, None, 2, SyncKinds::ALL)
        .unwrap()
    else {
        panic!("expected a page");
    };
    assert_eq!(p1.epoch, epoch);
    assert_eq!(p1.rows.len(), 1);
    assert_eq!(p1.gaps.len(), 1);
    assert!(p1.events.is_empty());
    assert!(p1.more);
    assert_eq!(p1.layouts.len(), 1);
    assert_eq!(p1.layouts[0].series, a.to_vec());

    let next = Some(Cursor {
        epoch,
        seq: p1.last_seq,
    });
    let CursorRead::Page(p2) = r
        .read_after(h.id, Tier::M1, next, 100, SyncKinds::ALL)
        .unwrap()
    else {
        panic!("expected a page");
    };
    assert_eq!(p2.events.len(), 1);
    assert_eq!(p2.rows.len(), 1);
    assert_eq!(p2.rows[0].stats.len(), 6);
    assert_eq!(p2.layouts[0].series, b.to_vec());
    assert!(!p2.more);
    let mut seqs: Vec<i64> = p1.rows.iter().map(|x| x.seq).collect();
    seqs.extend(p1.gaps.iter().map(|x| x.seq));
    seqs.extend(p2.events.iter().map(|x| x.seq));
    seqs.extend(p2.rows.iter().map(|x| x.seq));
    assert!(seqs.windows(2).all(|w| w[0] < w[1]), "{seqs:?}");

    // The S10 cursor sees only its own rows.
    let CursorRead::Page(s10) = r
        .read_after(h.id, Tier::S10, None, 100, SyncKinds::ALL)
        .unwrap()
    else {
        panic!("expected a page");
    };
    assert_eq!(
        (s10.rows.len(), s10.gaps.len(), s10.events.len()),
        (1, 0, 0)
    );

    // Caught up: empty page, cursor unchanged.
    let end = Some(Cursor {
        epoch,
        seq: p2.last_seq,
    });
    let CursorRead::Page(empty) = r
        .read_after(h.id, Tier::M1, end, 100, SyncKinds::ALL)
        .unwrap()
    else {
        panic!("expected a page");
    };
    assert!(empty.rows.is_empty() && !empty.more);
    assert_eq!(empty.last_seq, p2.last_seq);

    // A cursor from another database reads from the start.
    let foreign = Some(Cursor {
        epoch: uuid::Uuid::from_u128(77),
        seq: p2.last_seq,
    });
    let CursorRead::Page(all) = r
        .read_after(h.id, Tier::M1, foreign, 100, SyncKinds::ALL)
        .unwrap()
    else {
        panic!("expected a page");
    };
    assert_eq!(all.rows.len(), 2);
}

#[test]
fn pruning_deletes_by_retention_and_truncates_old_cursors() {
    let dir = temp_dir("prune");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    let h = host(1);
    w.upsert_host(h.clone()).unwrap();
    let l = layout(&["cpu.total"]);
    // 12,000 M1 rows: more than two prune batches.
    for i in 0..12_000 {
        w.write_bucket(bucket(h.id, Tier::M1, T0 + i * MIN, &l, 1.0))
            .unwrap();
    }
    w.write_gap(
        h.id,
        Gap::host(T0 + 5, Some(T0 + 10), GapReason::Sleep).unwrap(),
    )
    .unwrap();
    w.open_gap(h.id, T0 + 20, None, GapReason::Paused).unwrap();
    w.write_event(h.id, T0 + 1, "power_spike", vec![]).unwrap();
    w.flush().unwrap();

    let mut r = store.reader().unwrap();
    let early = Some(Cursor {
        epoch: store.epoch(),
        seq: 10,
    });
    assert!(matches!(
        r.read_after(h.id, Tier::M1, early, 10, SyncKinds::ALL)
            .unwrap(),
        CursorRead::Page(_)
    ));

    let now = T0 + 12_000 * MIN;
    let retention = Retention::with_days(7);
    let report = w.prune(now, retention).unwrap();
    let kept = 7 * 24 * 60;
    assert_eq!(report.m1_rows, 12_000 - kept as u64);
    assert_eq!(report.gaps, 1, "the closed gap; the open one stays");
    assert_eq!(report.events, 1);
    let db = raw(&store);
    assert_eq!(count(&db, "SELECT count(*) FROM tier_1m"), kept);
    assert_eq!(
        count(&db, "SELECT min(bucket_ts) FROM tier_1m"),
        now - retention.m1_ms
    );
    assert_eq!(count(&db, "SELECT count(*) FROM gaps"), 1);

    match r
        .read_after(h.id, Tier::M1, early, 10, SyncKinds::ALL)
        .unwrap()
    {
        CursorRead::Truncated {
            tier,
            earliest_ts_ms,
            epoch,
        } => {
            assert_eq!(tier, Tier::M1);
            assert_eq!(earliest_ts_ms, now - retention.m1_ms);
            assert_eq!(epoch, store.epoch());
        }
        other => panic!("expected Truncated, got {other:?}"),
    }
    // The pruned gap and event were written after the rows, so a cursor just below the
    // last row is also behind pruning.
    let max_row_seq = count(&db, "SELECT max(seq) FROM tier_1m");
    let behind = Some(Cursor {
        epoch: store.epoch(),
        seq: max_row_seq - 1,
    });
    assert!(matches!(
        r.read_after(h.id, Tier::M1, behind, 10, SyncKinds::ALL)
            .unwrap(),
        CursorRead::Truncated { .. }
    ));
    // A cursor at the pruned mark is not, and sees what came after.
    w.write_bucket(bucket(h.id, Tier::M1, now, &l, 1.0))
        .unwrap();
    w.flush().unwrap();
    let late = Some(Cursor {
        epoch: store.epoch(),
        seq: count(&db, "SELECT seq FROM pruned WHERE tier = 'm1'"),
    });
    match r
        .read_after(h.id, Tier::M1, late, 10, SyncKinds::ALL)
        .unwrap()
    {
        CursorRead::Page(p) => {
            assert_eq!(p.rows.len(), 1);
            assert_eq!(p.rows[0].bucket_ts, now);
        }
        other => panic!("expected a page, got {other:?}"),
    }
}

#[test]
fn incremental_vacuum_returns_space_and_size_counts_the_wal() {
    let dir = temp_dir("vacuum");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    let h = host(1);
    w.upsert_host(h.clone()).unwrap();
    let l: Arc<[_]> = (0..100)
        .map(|i| key(&format!("cpu.load{{core=P{i}}}")))
        .collect::<Vec<_>>()
        .into();
    for i in 0..3_000 {
        w.write_bucket(bucket(h.id, Tier::M1, T0 + i * MIN, &l, 1.0))
            .unwrap();
    }
    w.flush().unwrap();
    let wal = std::fs::metadata(dir.path().join("h.sqlite-wal"))
        .unwrap()
        .len();
    assert!(wal > 0);
    let full = store.size_on_disk().unwrap();
    let db_only = std::fs::metadata(store.path()).unwrap().len();
    assert!(full >= db_only + wal, "size includes the WAL");

    let freelist = |s: &kelvo_store::Store| count(&raw(s), "PRAGMA freelist_count");
    w.prune(T0 + 3_000 * MIN + 30 * DAY, Retention::default())
        .unwrap();
    assert_eq!(
        freelist(&store),
        0,
        "incremental_vacuum released the free pages"
    );
    store.close().unwrap();
    let after = kelvo_store::size_on_disk(&dir.path().join("h.sqlite")).unwrap();
    assert!(after < db_only / 4, "{after} vs {db_only}");
}

#[test]
fn clear_host_removes_history_and_truncates_cursors() {
    let dir = temp_dir("clear");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    let (a, b) = (host(1), host(2));
    w.upsert_host(a.clone()).unwrap();
    w.upsert_host(b.clone()).unwrap();
    let l = layout(&["cpu.total"]);
    for h in [&a, &b] {
        w.write_bucket(bucket(h.id, Tier::M1, T0, &l, 1.0)).unwrap();
        w.write_bucket(bucket(h.id, Tier::S10, T0, &l, 1.0))
            .unwrap();
        w.write_gap(h.id, Gap::host(T0, Some(T0 + 1), GapReason::Sleep).unwrap())
            .unwrap();
        w.write_proc_snapshot(h.id, T0, vec![proc("x", 1, 1.0)])
            .unwrap();
    }
    w.flush().unwrap();
    let mut r = store.reader().unwrap();
    let cur = match r
        .read_after(a.id, Tier::M1, None, 10, SyncKinds::ALL)
        .unwrap()
    {
        CursorRead::Page(p) => Cursor {
            epoch: p.epoch,
            seq: p.last_seq,
        },
        other => panic!("{other:?}"),
    };
    w.write_bucket(bucket(a.id, Tier::M1, T0 + MIN, &l, 1.0))
        .unwrap();
    w.clear_host(a.id, T0 + HOUR).unwrap();

    let db = raw(&store);
    for table in ["tier_1m", "tier_10s", "gaps", "proc_snap"] {
        assert_eq!(
            count(&db, &format!("SELECT count(*) FROM {table}")),
            1,
            "{table}: only host 2's row is left"
        );
    }
    assert!(matches!(
        r.read_after(a.id, Tier::M1, Some(cur), 10, SyncKinds::ALL).unwrap(),
        CursorRead::Truncated { earliest_ts_ms, .. } if earliest_ts_ms == T0 + HOUR
    ));
    assert_eq!(store.reader().unwrap().hosts().unwrap().len(), 2);
}

#[test]
fn unknown_gap_text_from_a_newer_build_reads_as_unknown() {
    let dir = temp_dir("gap-text");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    let h = host(1);
    w.upsert_host(h.clone()).unwrap();
    w.flush().unwrap();
    store.close().unwrap();
    // A newer build wrote reasons and modules this one has never heard of.
    let conn = rusqlite::Connection::open(dir.path().join("h.sqlite")).unwrap();
    conn.execute_batch(
        "INSERT INTO gaps (host_id, start_ts, end_ts, module, reason, seq) VALUES
           (1, 10, 20, NULL, 'lid_closed', 100),
           (1, 30, 40, 'npu', 'module_disabled', 101);",
    )
    .unwrap();
    drop(conn);
    let store = open(&dir, "h.sqlite");
    let gaps = store.reader().unwrap().gaps(h.id, 0, 100).unwrap();
    assert_eq!(gaps[0].reason, GapReason::Unknown);
    assert!(gaps[0].affects(Module::Cpu), "still a whole-host gap band");
    assert_eq!(gaps[1].module, Some(Module::Unknown));
    assert!(Module::ALL.iter().all(|&m| !gaps[1].affects(m)));
}

/// architecture.md infra 8: exactly one writer per file. A second `Store` on the same
/// file (another app instance, a dev build sharing the data dir) gets `Locked` instead
/// of a second writer; once the first closes, the file opens again.
#[test]
fn a_second_store_on_the_same_file_is_locked_out() {
    let dir = temp_dir("locked");
    let first = open(&dir, "h.sqlite");
    let err = Store::open(StoreConfig::new(dir.path().join("h.sqlite"))).err();
    assert!(
        matches!(&err, Some(StoreError::Locked { path }) if *path == dir.path().join("h.sqlite")),
        "{err:?}"
    );
    // A different file is not affected.
    drop(open(&dir, "other.sqlite"));
    first.close().unwrap();
    open(&dir, "h.sqlite").close().unwrap();
}

#[test]
fn move_aside_starts_a_fresh_database_and_keeps_the_old_one() {
    let dir = temp_dir("move-aside");
    let path = dir.path().join("history.sqlite");
    let store = open(&dir, "history.sqlite");
    store.writer().upsert_host(host(1)).unwrap();
    let epoch = store.epoch();
    // Refused while the store is open: it holds the lock.
    assert!(matches!(
        kelvo_store::move_aside(&path, T0),
        Err(StoreError::Locked { .. })
    ));
    store.close().unwrap();

    let moved = kelvo_store::move_aside(&path, T0).unwrap().unwrap();
    assert_eq!(moved, dir.path().join(format!("history-reset-{T0}.sqlite")));
    assert!(moved.exists() && !path.exists());
    let fresh = open(&dir, "history.sqlite");
    assert_ne!(fresh.epoch(), epoch, "a new file, a new epoch");
    assert!(fresh.reader().unwrap().hosts().unwrap().is_empty());
    fresh.close().unwrap();
    // Nothing to move is not an error.
    assert_eq!(
        kelvo_store::move_aside(&dir.path().join("missing.sqlite"), T0).unwrap(),
        None
    );
}

/// `is_local` is a column of this store, never a fact a peer asserts (D-064). At most one
/// host is local: registering a new local host (a new install identity) demotes the old
/// one instead of failing.
#[test]
fn one_local_host_and_the_newest_wins() {
    let dir = temp_dir("one-local");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    let a = host(1);
    let mut b = host(2);
    w.upsert_host(a.clone()).unwrap();
    w.upsert_host(b.clone()).unwrap();
    let r = store.reader().unwrap();
    assert_eq!(r.local_host().unwrap(), Some(a.clone()));
    b.is_local = true;
    w.upsert_host(b.clone()).unwrap();
    assert_eq!(r.local_host().unwrap(), Some(b.clone()));
    let hosts = r.hosts().unwrap();
    assert_eq!(hosts.iter().filter(|h| h.is_local).count(), 1);
    assert!(hosts.iter().any(|h| h.id == a.id && !h.is_local));
}

/// D-071: a non-local host with the local host's uuid (a disk-cloned Mac syncing to its
/// original, v4) is refused, not merged into the local host and not demoting it.
#[test]
fn a_remote_host_cannot_take_the_local_hosts_uuid() {
    let dir = temp_dir("host-conflict");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    let local = host(1);
    w.upsert_host(local.clone()).unwrap();
    let clone = kelvo_schema::HostRecord {
        is_local: false,
        display_name: "the clone".into(),
        ..local.clone()
    };
    let err = w.upsert_host(clone).unwrap_err();
    assert!(
        matches!(err, StoreError::HostConflict(id) if id == local.id),
        "{err}"
    );
    let r = store.reader().unwrap();
    assert_eq!(r.local_host().unwrap(), Some(local.clone()), "untouched");
    // An ordinary remote host still registers.
    w.upsert_host(host(2)).unwrap();
    assert_eq!(r.hosts().unwrap(), vec![local, host(2)]);
}

/// D-064: a shutdown queued behind a long prune ends the prune between batches instead of
/// waiting for it (app quit must not hang on a prune). What was deleted stays deleted;
/// the next prune finishes the job. A batch of 7 makes the prune 3,000 batches long, so
/// it cannot finish before the shutdown is queued, and the rows deleted are a whole
/// number of batches: the prune stopped at a batch boundary. (7 does not divide the
/// default batch, so this also fails if the configured size is ignored.)
#[test]
fn shutdown_ends_a_long_prune_early() {
    const BATCH: u64 = 7;
    const OLD: i64 = 21_000;
    let dir = temp_dir("prune-shutdown");
    let mut cfg = StoreConfig::new(dir.path().join("h.sqlite"));
    cfg.prune_batch = BATCH;
    let store = Store::open(cfg).unwrap();
    let h = host(1);
    let w = store.writer();
    w.upsert_host(h.clone()).unwrap();
    w.write_bucket(bucket(h.id, Tier::S10, T0, &layout(&["cpu.total"]), 1.0))
        .unwrap();
    w.flush().unwrap();
    // Old 10 s rows, written straight into the file for speed.
    let conn = rusqlite::Connection::open(store.path()).unwrap();
    conn.busy_timeout(Duration::from_secs(5)).unwrap();
    conn.execute(
        "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < ?2)
         INSERT INTO tier_10s (host_id, bucket_ts, layout_id, seq, blob)
         SELECT 1, ?1 - i * 10000, 1, -i, x'0000803f0000803f0000803f' FROM n",
        [T0 - DAY, OLD],
    )
    .unwrap();
    let old = |c: &rusqlite::Connection| -> i64 {
        c.query_row(
            "SELECT count(*) FROM tier_10s WHERE bucket_ts < ?1",
            [T0 - DAY],
            |r| r.get(0),
        )
        .unwrap()
    };
    let pruner = std::thread::spawn(move || w.prune(T0, Retention::with_days(1)));
    let deadline = Instant::now() + Duration::from_secs(30);
    while old(&conn) == OLD {
        assert!(Instant::now() < deadline, "prune never started");
        std::thread::sleep(Duration::from_millis(1));
    }
    store.close().unwrap();
    assert!(matches!(
        pruner.join().unwrap(),
        Err(StoreError::WriterGone)
    ));
    let left = old(&conn);
    assert!(left > 0, "stopped early: {left} of {OLD} rows left");
    assert_eq!(
        (OLD - left) % BATCH as i64,
        0,
        "{} rows deleted: whole batches of {BATCH}",
        OLD - left
    );
}

/// D-070: after the wall clock stepped back a long way, rows stamped by the wrong clock
/// from `from` on are dropped, so the new timeline never upserts into them. A gap that
/// began before `from` and ended after it is cut at `from`.
#[test]
fn discard_from_drops_rows_of_a_wrong_clock() {
    let dir = temp_dir("discard");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    let (a, b) = (host(1), host(2));
    w.upsert_host(a.clone()).unwrap();
    w.upsert_host(b.clone()).unwrap();
    let l = layout(&["cpu.total"]);
    for h in [&a, &b] {
        for ts in [T0 - MIN, T0, T0 + DAY] {
            w.write_bucket(bucket(h.id, Tier::M1, ts, &l, 1.0)).unwrap();
            w.write_bucket(bucket(h.id, Tier::S10, ts, &l, 1.0))
                .unwrap();
            w.write_proc_snapshot(h.id, ts, vec![proc("x", 1, 1.0)])
                .unwrap();
            w.write_event(h.id, ts, "fans_ramped", vec![0xa0]).unwrap();
        }
        w.write_gap(
            h.id,
            Gap::host(T0 - HOUR, Some(T0 - MIN), GapReason::Sleep).unwrap(),
        )
        .unwrap();
        w.write_gap(
            h.id,
            Gap::host(T0 - MIN, Some(T0 + HOUR), GapReason::Paused).unwrap(),
        )
        .unwrap();
        w.write_gap(
            h.id,
            Gap::host(T0 + MIN, Some(T0 + HOUR), GapReason::Sleep).unwrap(),
        )
        .unwrap();
    }
    w.discard_from(a.id, T0).unwrap();
    w.flush().unwrap();

    let db = raw(&store);
    for (table, col) in [
        ("tier_1m", "bucket_ts"),
        ("tier_10s", "bucket_ts"),
        ("proc_snap", "ts"),
        ("events", "ts"),
    ] {
        assert_eq!(
            count(
                &db,
                &format!("SELECT count(*) FROM {table} WHERE host_id = 1")
            ),
            1,
            "{table}: host 1 keeps only the row before T0"
        );
        assert_eq!(
            count(
                &db,
                &format!("SELECT count(*) FROM {table} WHERE host_id = 1 AND {col} >= {T0}")
            ),
            0,
            "{table}"
        );
        assert_eq!(
            count(
                &db,
                &format!("SELECT count(*) FROM {table} WHERE host_id = 2")
            ),
            3,
            "{table}: host 2 untouched"
        );
    }
    let gaps = store.reader().unwrap().gaps(a.id, 0, i64::MAX).unwrap();
    assert_eq!(
        gaps,
        vec![
            Gap::host(T0 - HOUR, Some(T0 - MIN), GapReason::Sleep).unwrap(),
            Gap::host(T0 - MIN, Some(T0), GapReason::Paused).unwrap(),
        ]
    );
    assert_eq!(
        store
            .reader()
            .unwrap()
            .gaps(b.id, 0, i64::MAX)
            .unwrap()
            .len(),
        3
    );
}
