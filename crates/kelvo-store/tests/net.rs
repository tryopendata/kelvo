//! Per-app network history (D-089): the `proc_net_*` tiers through the public API, on
//! real SQLite files.

// clippy.toml allows unwrap inside #[test] functions only; the helpers in this test-only
// file panic on failure by design.
#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use kelvo_schema::{HostId, Tier};
use kelvo_store::{
    NET_NEW_NAMES_PER_HOUR, NET_TOP_APPS, NetApp, NetBucket, NetByApp, NetSpan, ProcRow, Retention,
    Store, StoreError, Writer,
};

const S10: i64 = 10_000;
const M15: i64 = 15 * MIN;

fn app(name: &str, rx: u64, tx: u64) -> NetApp {
    NetApp {
        name: Some(name.into()),
        rx_bytes: rx,
        tx_bytes: tx,
    }
}

fn other(rx: u64, tx: u64) -> NetApp {
    NetApp {
        name: None,
        rx_bytes: rx,
        tx_bytes: tx,
    }
}

/// The `i`th bucket of a deterministic stream: three apps plus unnamed bytes, interface
/// counters above the app bytes.
fn nb(i: u64) -> NetBucket {
    NetBucket {
        measured_ms: 10_000,
        iface_rx_bytes: 10_000 + 100 * i,
        iface_tx_bytes: 2_000 + i,
        iface_rx_pkts: 20 + i % 7,
        iface_tx_pkts: 10 + i % 3,
        apps: vec![
            app("Safari", 50 * i + 1, i % 11),
            app("curl", 1_000, 3),
            app("git", i % 5, 1),
            other(7, 2),
        ],
    }
}

/// Bucket index of the 10 s bucket at `ts` (T0 is 0).
fn idx(ts: i64) -> u64 {
    ((ts - T0) / S10) as u64
}

/// Writes the stream's buckets for `[from, to)` and flushes.
fn write_range(w: &Writer, h: HostId, from: i64, to: i64) {
    let mut ts = from;
    while ts < to {
        w.write_net_bucket(h, ts, nb(idx(ts))).unwrap();
        ts += S10;
    }
    w.flush().unwrap();
}

/// What `net_by_app` must return for the stream's buckets in `[from, to)`: the plain sum
/// of every bucket, apps merged by name.
fn expected(from: i64, to: i64) -> (u64, u64, u64, u64, u64, Vec<NetApp>) {
    let (mut rx, mut tx, mut rp, mut tp, mut ms) = (0, 0, 0, 0, 0);
    let mut apps: std::collections::BTreeMap<Option<String>, (u64, u64)> = Default::default();
    let mut ts = from;
    while ts < to {
        let b = nb(idx(ts));
        rx += b.iface_rx_bytes;
        tx += b.iface_tx_bytes;
        rp += b.iface_rx_pkts;
        tp += b.iface_tx_pkts;
        ms += u64::from(b.measured_ms);
        for a in b.apps {
            let e = apps.entry(a.name).or_default();
            e.0 += a.rx_bytes;
            e.1 += a.tx_bytes;
        }
        ts += S10;
    }
    let mut apps: Vec<NetApp> = apps
        .into_iter()
        .map(|(name, (rx, tx))| NetApp {
            name,
            rx_bytes: rx,
            tx_bytes: tx,
        })
        .filter(|a| a.total() > 0)
        .collect();
    apps.sort_by(|a, b| b.total().cmp(&a.total()).then_with(|| a.name.cmp(&b.name)));
    (ms, rx, tx, rp, tp, apps)
}

fn assert_sums(got: &NetByApp, from: i64, to: i64) {
    let (ms, rx, tx, rp, tp, apps) = expected(from, to);
    assert_eq!(
        (got.from_ms, got.to_ms),
        (from, to),
        "snapped range {:?}",
        got.coverage
    );
    assert_eq!(got.measured_ms, ms, "measured");
    assert_eq!(
        (
            got.iface_rx_bytes,
            got.iface_tx_bytes,
            got.iface_rx_pkts,
            got.iface_tx_pkts
        ),
        (rx, tx, rp, tp),
        "interface counters"
    );
    assert_eq!(got.apps, apps, "apps");
}

fn span(from_ms: i64, to_ms: i64, tier: Option<Tier>) -> NetSpan {
    NetSpan {
        from_ms,
        to_ms,
        tier,
    }
}

fn setup(name: &str) -> (TempDir, Store, Writer, HostId) {
    let dir = TempDir::new(name);
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    let h = host(1);
    w.upsert_host(h.clone()).unwrap();
    (dir, store, w, h.id)
}

/// Retention with short 10 s and minute windows, so a few hours of fixtures cover all
/// three tiers.
fn short() -> Retention {
    Retention {
        proc_snap_ms: 30 * MIN,
        m1_ms: HOUR,
        ..Retention::with_days(30)
    }
}

#[test]
fn minutes_are_the_exact_sum_of_their_10s_buckets() {
    let (_dir, store, w, h) = setup("net-minutes");
    // Three minutes and a bit, starting mid-minute.
    let (from, to) = (T0 + 3 * S10, T0 + 3 * MIN + 2 * S10);
    write_range(&w, h, from, to);
    let db = raw(&store);
    assert_eq!(
        count(&db, "SELECT count(*) FROM proc_net_10s"),
        (to - from) / S10
    );
    assert_eq!(count(&db, "SELECT count(*) FROM proc_net_1m"), 4);
    drop(db);

    // With the 10 s rows still there, the read is at 10 s.
    let mut r = store.reader().unwrap();
    let fine = r.net_by_app(h, from, to).unwrap();
    assert_eq!(fine.coverage, vec![span(from, to, Some(Tier::S10))]);
    assert_eq!(fine.resolution_ms, S10);
    assert_sums(&fine, from, to);

    // Prune the 10 s rows: the minutes alone give the same sums, over whole minutes.
    let retention = Retention {
        proc_snap_ms: 30 * MIN,
        ..Retention::default()
    };
    let report = w.prune(T0 + 2 * HOUR, retention).unwrap();
    assert_eq!(report.net_rows as i64, (to - from) / S10);
    let coarse = r.net_by_app(h, from, to).unwrap();
    assert_eq!(
        coarse.coverage,
        vec![span(T0, T0 + 4 * MIN, Some(Tier::M1))]
    );
    assert_eq!(coarse.resolution_ms, MIN);
    // Widened to whole minutes, which hold only the buckets written.
    assert_eq!((coarse.from_ms, coarse.to_ms), (T0, T0 + 4 * MIN));
    let (ms, rx, ..) = expected(from, to);
    assert_eq!((coarse.measured_ms, coarse.iface_rx_bytes), (ms, rx));
    assert_eq!(coarse.apps, fine.apps);
}

#[test]
fn a_session_starts_after_the_newest_per_app_bucket() {
    let (_dir, _store, w, h) = setup("net-session");
    assert_eq!(w.begin_session(h, T0).unwrap().net_written_to_ms, None);
    write_range(&w, h, T0, T0 + 3 * S10);
    assert_eq!(
        w.begin_session(h, T0 + MIN).unwrap().net_written_to_ms,
        Some(T0 + 3 * S10),
        "the end of the newest 10 s row"
    );
}

#[test]
fn replaying_buckets_changes_nothing() {
    let (_dir, store, w, h) = setup("net-replay");
    write_range(&w, h, T0, T0 + 2 * MIN);
    let db = raw(&store);
    let before: Vec<Vec<String>> = ["proc_net_10s", "proc_net_1m", "proc_names", "meta"]
        .iter()
        .map(|t| dump(&db, t))
        .collect();
    write_range(&w, h, T0, T0 + 2 * MIN);
    let after: Vec<Vec<String>> = ["proc_net_10s", "proc_net_1m", "proc_names", "meta"]
        .iter()
        .map(|t| dump(&db, t))
        .collect();
    assert_eq!(before, after, "same rows, same seq, same next_seq");

    // A changed bucket rewrites its 10 s row and its minute.
    let mut b = nb(3);
    b.apps.push(app("ssh", 500, 500));
    w.write_net_bucket(h, T0 + 3 * S10, b).unwrap();
    w.flush().unwrap();
    let r = store.reader().unwrap().net_by_app(h, T0, T0 + MIN).unwrap();
    assert!(r.apps.contains(&app("ssh", 500, 500)), "{:?}", r.apps);
    let minutes = dump(&db, "proc_net_1m");
    assert_ne!(minutes[0], before[1][0], "the first minute changed");
    assert_eq!(minutes[1], before[1][1], "the second did not");
}

#[test]
fn a_bucket_keeps_its_top_20_and_folds_the_rest_into_other_apps() {
    let (_dir, store, w, h) = setup("net-top20");
    let apps: Vec<NetApp> = (1..=30u64)
        .map(|i| app(&format!("app{i:02}"), 1_000 * i, i))
        .chain([other(5, 5), app("", 1, 1)])
        .collect();
    w.write_net_bucket(
        h,
        T0,
        NetBucket {
            measured_ms: 10_000,
            apps,
            ..NetBucket::default()
        },
    )
    .unwrap();
    w.flush().unwrap();
    let r = store.reader().unwrap().net_by_app(h, T0, T0 + S10).unwrap();
    assert_eq!(r.apps.len(), NET_TOP_APPS + 1);
    let named: Vec<String> = r.apps.iter().filter_map(|a| a.name.clone()).collect();
    let top: Vec<String> = (11..=30).rev().map(|i| format!("app{i:02}")).collect();
    assert_eq!(named, top);
    // Apps 1-10 plus the unnamed and empty-named bytes.
    let folded_rx: u64 = (1..=10).map(|i| 1_000 * i).sum::<u64>() + 5 + 1;
    let folded_tx: u64 = (1..=10).sum::<u64>() + 5 + 1;
    let rest = r.apps.iter().find(|a| a.name.is_none()).unwrap();
    assert_eq!((rest.rx_bytes, rest.tx_bytes), (folded_rx, folded_tx));
    let total_rx: u64 = r.apps.iter().map(|a| a.rx_bytes).sum();
    assert_eq!(total_rx, (1..=30).map(|i| 1_000 * i).sum::<u64>() + 6);
}

#[test]
fn minutes_past_the_window_roll_into_quarters_exactly() {
    let (_dir, store, w, h) = setup("net-rollup");
    let end = T0 + 3 * HOUR;
    write_range(&w, h, T0, end);
    let mut r = store.reader().unwrap();
    let before = r.net_by_app(h, T0, end).unwrap();
    assert_sums(&before, T0, end);

    let report = w.prune(end, short()).unwrap();
    // Minutes before end - 1 h roll down: 2 hours of them, into 8 quarters.
    assert_eq!(report.net_rolled, 120);
    let db = raw(&store);
    assert_eq!(count(&db, "SELECT count(*) FROM proc_net_15m"), 8);
    assert_eq!(
        count(&db, "SELECT min(bucket_ts) FROM proc_net_1m"),
        T0 + 2 * HOUR
    );
    assert_eq!(
        count(&db, "SELECT min(bucket_ts) FROM proc_net_10s"),
        end - 30 * MIN
    );

    // A range starting inside a quarter is widened to it, and stitched from all three
    // tiers with the same sums.
    let after = r.net_by_app(h, T0 + 7 * MIN, end - 5_000).unwrap();
    assert_eq!(
        after.coverage,
        vec![
            span(T0, T0 + 2 * HOUR, Some(Tier::M15)),
            span(T0 + 2 * HOUR, end - 30 * MIN, Some(Tier::M1)),
            span(end - 30 * MIN, end, Some(Tier::S10)),
        ]
    );
    assert_eq!(after.resolution_ms, M15);
    assert_sums(&after, T0, end);

    // A second prune finds nothing more to roll.
    let again = w.prune(end, short()).unwrap();
    assert_eq!((again.net_rolled, again.net_rows), (0, 0));
}

#[test]
fn retention_prunes_each_tier_at_its_own_boundary() {
    let (_dir, store, w, h) = setup("net-retention");
    write_range(&w, h, T0, T0 + 2 * HOUR);
    // 10 s keep 30 min from a cut in the middle of a minute: that minute's 10 s rows stay.
    let now = T0 + 2 * HOUR + 25_000;
    w.prune(now, short()).unwrap();
    let db = raw(&store);
    assert_eq!(
        count(&db, "SELECT min(bucket_ts) FROM proc_net_10s"),
        T0 + 90 * MIN,
        "the cut is floored to the minute"
    );

    assert_eq!(
        count(&db, "SELECT count(*) FROM proc_net_15m"),
        4,
        "T0 to T0 + 1 h"
    );
    assert_eq!(count(&db, "SELECT count(*) FROM proc_net_1m"), 60);

    // An hour of history at T0 + 2.5 h: quarters and minutes before T0 + 1.5 h go, and
    // the 10 s rows (kept at most as long as history) all go.
    let retention = Retention {
        history_ms: HOUR,
        ..short()
    };
    let report = w.prune(T0 + 2 * HOUR + 30 * MIN, retention).unwrap();
    assert_eq!(report.net_rows, 4 + 30 + 30 * 6);
    assert_eq!(count(&db, "SELECT count(*) FROM proc_net_15m"), 0);
    assert_eq!(count(&db, "SELECT count(*) FROM proc_net_10s"), 0);
    assert_eq!(count(&db, "SELECT count(*) FROM proc_net_1m"), 30);
    assert_eq!(
        count(&db, "SELECT min(bucket_ts) FROM proc_net_1m"),
        T0 + 90 * MIN
    );
}

#[test]
fn ranges_without_rows_are_reported_as_uncovered() {
    let (_dir, store, w, h) = setup("net-holes");
    // Two minutes of data with a 30 s hole in the first and a partly measured bucket.
    for i in 0..12 {
        let ts = T0 + i * S10;
        if (2..5).contains(&i) {
            continue;
        }
        let mut b = nb(i as u64);
        if i == 5 {
            b.measured_ms = 4_000;
        }
        w.write_net_bucket(h, ts, b).unwrap();
    }
    w.flush().unwrap();
    let mut r = store.reader().unwrap();
    let got = r.net_by_app(h, T0 - MIN, T0 + 3 * MIN).unwrap();
    assert_eq!(
        got.coverage,
        vec![
            span(T0 - MIN, T0, None),
            span(T0, T0 + 2 * S10, Some(Tier::S10)),
            span(T0 + 2 * S10, T0 + 5 * S10, None),
            span(T0 + 5 * S10, T0 + 2 * MIN, Some(Tier::S10)),
            span(T0 + 2 * MIN, T0 + 3 * MIN, None),
        ]
    );
    assert_eq!(got.measured_ms, 8 * 10_000 + 4_000);

    // An empty range, and one before any data.
    let empty = r.net_by_app(h, T0, T0).unwrap();
    assert!(empty.coverage.is_empty() && empty.apps.is_empty());
    let before = r.net_by_app(h, T0 - HOUR, T0 - MIN).unwrap();
    assert_eq!(before.coverage, vec![span(T0 - HOUR, T0 - MIN, None)]);
    assert_eq!((before.measured_ms, before.apps.len()), (0, 0));
}

#[test]
fn in_memory_buckets_replace_stored_ones_and_extend_the_range() {
    let (_dir, store, w, h) = setup("net-recent");
    write_range(&w, h, T0, T0 + MIN);
    // The engine's ring: the last 30 s already committed, plus 20 s not yet written,
    // one of them the open bucket.
    let recent: Vec<(i64, NetBucket)> = (3..8).map(|i| (T0 + i * S10, nb(i as u64))).collect();
    let mut r = store.reader().unwrap();
    let got = r.net_by_app_with(h, T0, T0 + 80_000, &recent).unwrap();
    assert_eq!(got.coverage, vec![span(T0, T0 + 80_000, Some(Tier::S10))]);
    assert_sums(&got, T0, T0 + 80_000);

    // Ring timestamps are floored to the grid: an unaligned copy of a stored bucket
    // still replaces it.
    let ring = vec![(T0 + 2 * S10 + 3, nb(2))];
    let got = r.net_by_app_with(h, T0, T0 + MIN, &ring).unwrap();
    assert_sums(&got, T0, T0 + MIN);
}

#[test]
fn the_ring_is_copied_after_the_stored_rows_are_read() {
    let (_dir, store, w, h) = setup("net-after");
    // The open bucket at 10 s was flushed partly (a pause) and is in the ring, open.
    let mut partial = nb(1);
    partial.measured_ms = 3_000;
    w.write_net_bucket(h, T0 + S10, partial).unwrap();
    w.flush().unwrap();
    let mut r = store.reader().unwrap();
    let got = r
        .net_by_app_after(h, T0, T0 + 2 * S10, || {
            // Between the read and the copy, the bucket closes and is committed whole;
            // the ring the query copies now has it whole too. A row committed now that
            // the ring does not hold shows whether the stored read already happened.
            w.write_net_bucket(h, T0 + S10, nb(1)).unwrap();
            w.write_net_bucket(h, T0, nb(0)).unwrap();
            w.flush().unwrap();
            vec![(T0 + S10, nb(1))]
        })
        .unwrap();
    assert_eq!(
        got.measured_ms, 10_000,
        "the stored rows were read before the ring was copied: the ring's newer copy \
         counted once, the row committed in between not yet seen"
    );
    assert!(!r.in_transaction(), "the read snapshot is released");
}

#[test]
fn ring_alone_answers_what_it_holds() {
    let recent: Vec<(i64, NetBucket)> = [2, 3, 5].map(|i| (T0 + i * S10, nb(i as u64))).into();
    let got = kelvo_store::net_by_app_recent(T0, T0 + MIN, &recent).unwrap();
    assert_eq!(
        got.coverage,
        vec![
            span(T0, T0 + 2 * S10, None),
            span(T0 + 2 * S10, T0 + 4 * S10, Some(Tier::S10)),
            span(T0 + 4 * S10, T0 + 5 * S10, None),
            span(T0 + 5 * S10, T0 + MIN, Some(Tier::S10)),
        ]
    );
    let (ms, rx, ..) = expected(T0 + 2 * S10, T0 + 4 * S10);
    let (ms5, rx5, ..) = expected(T0 + 5 * S10, T0 + MIN);
    assert_eq!((got.measured_ms, got.iface_rx_bytes), (ms + ms5, rx + rx5));
    assert!(got.apps.iter().any(|a| a.name.is_none()), "other apps");
    let empty = kelvo_store::net_by_app_recent(T0, T0, &recent).unwrap();
    assert!(empty.coverage.is_empty());
}

#[test]
fn new_names_are_capped_per_hour_and_overflow_into_other_apps() {
    let (_dir, store, w, h) = setup("net-cap");
    // Interned by a process snapshot first: never counted against the cap.
    w.write_proc_snapshot(
        h,
        T0,
        vec![ProcRow {
            name: "Safari".into(),
            pid: 1,
            cpu_pct: 1.0,
            mem_bytes: 0,
            threads: 1,
            idle_wakeups_per_s: 0.0,
            energy: 0.0,
        }],
    )
    .unwrap();
    let n = NET_NEW_NAMES_PER_HOUR as u64 + 10;
    for i in 0..n {
        w.write_net_bucket(
            h,
            T0 + i as i64 * S10,
            NetBucket {
                measured_ms: 10_000,
                apps: vec![app(&format!("argv-{i}"), 100, 0), app("Safari", 1, 0)],
                ..NetBucket::default()
            },
        )
        .unwrap();
    }
    // The next hour may intern again.
    w.write_net_bucket(
        h,
        T0 + HOUR,
        NetBucket {
            apps: vec![app("late", 9, 0)],
            ..NetBucket::default()
        },
    )
    .unwrap();
    w.flush().unwrap();
    let db = raw(&store);
    assert_eq!(
        count(&db, "SELECT count(*) FROM proc_names"),
        1 + NET_NEW_NAMES_PER_HOUR as i64 + 1
    );
    assert_eq!(
        count(&db, "SELECT min(id) FROM proc_names"),
        1,
        "ids start at 1"
    );
    let mut r = store.reader().unwrap();
    let hour = r.net_by_app(h, T0, T0 + HOUR).unwrap();
    let rest = hour.apps.iter().find(|a| a.name.is_none()).unwrap();
    assert_eq!(rest.rx_bytes, 10 * 100, "the 10 names past the cap");
    assert!(hour.apps.contains(&app("Safari", n, 0)));
    let late = r.net_by_app(h, T0 + HOUR, T0 + HOUR + S10).unwrap();
    assert_eq!(late.apps, vec![app("late", 9, 0)]);
}

#[test]
fn misaligned_buckets_are_refused() {
    let (_dir, _store, w, h) = setup("net-misaligned");
    assert!(matches!(
        w.write_net_bucket(h, T0 + 1, nb(0)),
        Err(StoreError::Misaligned {
            ts_ms,
            width_ms: 10_000
        }) if ts_ms == T0 + 1
    ));
}

fn net_counts(db: &rusqlite::Connection, host_id: i64) -> [i64; 3] {
    ["proc_net_10s", "proc_net_1m", "proc_net_15m"].map(|t| {
        count(
            db,
            &format!("SELECT count(*) FROM {t} WHERE host_id = {host_id}"),
        )
    })
}

#[test]
fn clear_host_and_discard_from_include_the_network_tables() {
    let dir = TempDir::new("net-clear");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    let (a, b) = (host(1), host(2));
    w.upsert_host(a.clone()).unwrap();
    w.upsert_host(b.clone()).unwrap();
    for h in [&a, &b] {
        write_range(&w, h.id, T0, T0 + 2 * HOUR);
    }
    w.prune(T0 + 2 * HOUR, short()).unwrap();
    let db = raw(&store);
    let full = net_counts(&db, 1);
    assert!(full.iter().all(|&n| n > 0), "{full:?}");

    w.clear_host(a.id, T0 + 3 * HOUR).unwrap();
    assert_eq!(net_counts(&db, 1), [0, 0, 0]);
    assert_eq!(net_counts(&db, 2), full, "host 2 untouched");

    // Discard host 2 from 25 s into its last minute: its 10 s rows from there go, and
    // the minute is summed again from what is left.
    let minute = T0 + 2 * HOUR - MIN;
    w.discard_from(b.id, minute + 25_000).unwrap();
    assert_eq!(
        count(
            &db,
            "SELECT count(*) FROM proc_net_10s WHERE host_id = 2 AND bucket_ts >= 0"
        ),
        full[0] - 3
    );
    // Drop the 10 s rows so the minute row answers alone.
    let now = T0 + 2 * HOUR + HOUR;
    w.prune(
        now,
        Retention {
            m1_ms: 2 * HOUR,
            ..short()
        },
    )
    .unwrap();
    let got = store
        .reader()
        .unwrap()
        .net_by_app(b.id, minute, minute + MIN)
        .unwrap();
    assert_eq!(
        got.coverage,
        vec![span(minute, minute + MIN, Some(Tier::M1))]
    );
    let (ms, rx, _, _, _, apps) = expected(minute, minute + 3 * S10);
    assert_eq!(
        (got.measured_ms, got.iface_rx_bytes, &got.apps),
        (ms, rx, &apps)
    );
}

#[test]
fn the_cap_trims_the_network_tables_too() {
    let (_dir, store, w, h) = setup("net-cap-trim");
    let l = layout(&["cpu.total"]);
    // Two days of minutes (the cap trims on what tier_1m and tier_15m span) and network
    // buckets in the first hours.
    for i in 0..2 * 24 * 60 {
        w.write_bucket(bucket(h, Tier::M1, T0 + i * MIN, &l, 1.0))
            .unwrap();
    }
    write_range(&w, h, T0, T0 + HOUR);
    let now = T0 + 2 * DAY;
    let report = w
        .prune(
            now,
            Retention {
                max_bytes: 1,
                ..Retention::default()
            },
        )
        .unwrap();
    let trim = report.cap_trim.expect("over a 1-byte cap");
    assert!(!trim.cap_met);
    let db = raw(&store);
    // The trim keeps the last day; the network rows were all older.
    assert_eq!(net_counts(&db, 1), [0, 0, 0]);
    assert_eq!(trim.net_rows, 360 + 60);
}
