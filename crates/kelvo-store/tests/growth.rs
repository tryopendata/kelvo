//! `history_growth`: the Settings size projection measured from the file. A few hours
//! of recording must predict what the tables cost once they hold a full retention, and
//! time Kelvo was not running must not dilute the rates.

#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;

use common::*;
use kelvo_schema::{HostId, SeriesKey, Tier};
use kelvo_store::{BucketRow, HistoryGrowth, ProcRow, Retention, Store, Writer};

const SERIES: usize = 60;

fn series() -> Arc<[SeriesKey]> {
    (0..SERIES)
        .map(|i| key(&format!("cpu.load{{core=P{i}}}")))
        .collect::<Vec<_>>()
        .into()
}

/// Values that vary like real readings, so pages fill the way they do on a host.
fn stats(seed: i64) -> Vec<f32> {
    (0..SERIES as i64 * 3)
        .map(|i| ((seed / 1000 + i * 7919) % 10_007) as f32 / 100.0)
        .collect()
}

fn procs(seed: i64) -> Vec<ProcRow> {
    (0..12)
        .map(|i| ProcRow {
            name: format!("process-{}", (seed / 10_000 + i) % 40),
            pid: 1000 + i as i32,
            cpu_pct: (12 - i) as f32 * 1.5,
            mem_bytes: ((i as u64 + 1) * 50) << 20,
            threads: 4 + i as u32,
            idle_wakeups_per_s: i as f32,
            energy: 0.5,
        })
        .collect()
}

/// What the engine writes for `h` over `[from, from + span)`: minute buckets, and with
/// `tens` the 10 s buckets and a process snapshot every 10 s.
fn record_as(w: &Writer, h: HostId, from: i64, span: i64, tens: bool) {
    let layout = series();
    for minute in 0..span / MIN {
        let ts = from + minute * MIN;
        w.write_bucket(BucketRow {
            host: h,
            tier: Tier::M1,
            bucket_ts: ts,
            series: Arc::clone(&layout),
            stats: stats(ts),
        })
        .unwrap();
        for s in 0..if tens { 6 } else { 0 } {
            let t = ts + s * 10_000;
            w.write_bucket(BucketRow {
                host: h,
                tier: Tier::S10,
                bucket_ts: t,
                series: Arc::clone(&layout),
                stats: stats(t + 1),
            })
            .unwrap();
            w.write_proc_snapshot(h, t, procs(t)).unwrap();
        }
        if minute % 60 == 59 {
            w.flush().unwrap();
        }
    }
    w.flush().unwrap();
}

/// A store with `spans` recorded (start, length), measured at the end of the last.
fn measure(name: &str, spans: &[(i64, i64)]) -> (TempDir, Store, Option<HistoryGrowth>) {
    let spans: Vec<_> = spans.iter().map(|&(f, s)| (1, f, s, true)).collect();
    measure_as(name, &spans)
}

/// [`measure`] with each span's host number and whether it has 10 s rows.
fn measure_as(
    name: &str,
    spans: &[(u128, i64, i64, bool)],
) -> (TempDir, Store, Option<HistoryGrowth>) {
    let dir = TempDir::new(name);
    let store = open(&dir, "history.sqlite");
    let w = store.writer();
    for &(n, from, span, tens) in spans {
        w.upsert_host(host(n)).unwrap();
        record_as(&w, host(n).id, from, span, tens);
    }
    let now = spans.iter().map(|&(_, f, s, _)| f + s).max().unwrap();
    w.prune(now, Retention::default()).unwrap();
    w.flush().unwrap();
    let growth = store
        .reader()
        .unwrap()
        .history_growth(now, &Retention::default())
        .unwrap();
    (dir, store, growth)
}

fn within(actual: u64, expected: u64, pct: f64) -> bool {
    (actual as f64 - expected as f64).abs() <= expected as f64 * pct / 100.0
}

#[test]
fn no_estimate_before_an_hour_is_recorded() {
    let (_dir, _store, growth) = measure("growth-short", &[(T0, 50 * MIN)]);
    assert_eq!(growth, None);
}

#[test]
fn three_hours_predict_the_cost_of_a_full_day_of_10s_rows() {
    let (_a, _sa, early) = measure("growth-3h", &[(T0, 3 * HOUR)]);
    // 26 hours, pruned: the 10 s table holds exactly its 24-hour retention.
    let (_b, _sb, full) = measure("growth-26h", &[(T0, 26 * HOUR)]);
    let (early, full) = (early.unwrap(), full.unwrap());
    assert_eq!(early.measured_ms, 3 * HOUR);
    assert!(
        within(early.fixed_bytes, full.fixed_bytes, 15.0),
        "fixed part: {} after 3 h, {} after 26 h",
        early.fixed_bytes,
        full.fixed_bytes
    );
    assert!(
        within(early.minute_day_bytes, full.minute_day_bytes, 15.0),
        "minute day: {} after 3 h, {} after 26 h",
        early.minute_day_bytes,
        full.minute_day_bytes
    );
}

#[test]
fn time_kelvo_was_not_running_does_not_dilute_the_rates() {
    let (_a, _sa, contiguous) = measure("growth-contiguous", &[(T0, 2 * HOUR)]);
    // The same two hours with a 5-hour stretch of no recording between them.
    let (_b, _sb, split) = measure("growth-split", &[(T0, HOUR), (T0 + 6 * HOUR, HOUR)]);
    let (contiguous, split) = (contiguous.unwrap(), split.unwrap());
    assert_eq!(split.measured_ms, 2 * HOUR);
    assert!(within(split.fixed_bytes, contiguous.fixed_bytes, 10.0));
    assert!(within(
        split.minute_day_bytes,
        contiguous.minute_day_bytes,
        10.0
    ));
}

#[test]
fn no_estimate_until_the_10s_window_holds_an_hour_after_a_relaunch() {
    // Two hours, six days off, then two minutes: the 10 s window holds two minutes,
    // which scaled to a day would be noise.
    let (_dir, _store, growth) = measure(
        "growth-relaunch",
        &[(T0, 2 * HOUR), (T0 + 2 * HOUR + 6 * DAY, 2 * MIN)],
    );
    assert_eq!(growth, None);
}

#[test]
fn a_minute_two_hosts_recorded_counts_once() {
    let (_a, _sa, one) = measure("growth-one-host", &[(T0, 2 * HOUR)]);
    let (_b, _sb, two) = measure_as(
        "growth-two-hosts",
        &[(1, T0, 2 * HOUR, true), (2, T0, 2 * HOUR, true)],
    );
    assert_eq!(one.unwrap().measured_ms, 2 * HOUR);
    assert_eq!(two.unwrap().measured_ms, 2 * HOUR);
}

#[test]
fn rolled_down_15_minute_rows_are_not_counted_as_fixed() {
    let end = T0 + 30 * DAY;
    // Thirty days of minutes, so pruning rolls 23 of them into 15-minute rows as on a
    // full store, against the last seven days alone; both end with 3 hours of 10 s rows.
    let (_a, sa, rolled) = measure_as(
        "growth-rolled",
        &[
            (1, T0, 30 * DAY - 3 * HOUR, false),
            (1, end - 3 * HOUR, 3 * HOUR, true),
        ],
    );
    let (_b, _sb, week) = measure_as(
        "growth-week",
        &[
            (1, end - 7 * DAY, 7 * DAY - 3 * HOUR, false),
            (1, end - 3 * HOUR, 3 * HOUR, true),
        ],
    );
    assert!(count(&raw(&sa), "SELECT COUNT(*) FROM tier_15m") > 2000);
    let (rolled, week) = (rolled.unwrap(), week.unwrap());
    assert!(
        within(rolled.fixed_bytes, week.fixed_bytes, 3.0),
        "fixed part: {} with 23 rolled days, {} without",
        rolled.fixed_bytes,
        week.fixed_bytes
    );
}
