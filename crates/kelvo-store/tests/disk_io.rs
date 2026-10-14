#![allow(clippy::too_many_lines)] // Integration scenarios keep setup and assertions together.
//! Paired disk request integrals through real SQLite, overlays, read merging and roll-down.
#![allow(clippy::unwrap_used)]
mod common;
use common::*;
use kelvo_schema::{Gap, GapReason, Labels, MetricId, SeriesSelector, Tier};
use kelvo_store::{HistoryQuery, Retention, TierChoice, disk_io::disk_io_page};

#[test]
fn operation_weighted_requests_survive_missing_minutes_open_replacement_and_roll_down() {
    let dir = temp_dir("disk-io");
    let store = open(&dir, "h.sqlite");
    let h = host(1);
    let writer = store.writer();
    writer.upsert_host(h.clone()).unwrap();
    let keys = layout(&[
        "disk.read{dev=disk0}",
        "disk.read_ops{dev=disk0}",
        "disk.read_timed_ops{dev=disk0}",
        "disk.read_time{dev=disk0}",
    ]);
    let row = |time, ops: f32, ms: f32| {
        let mut row = bucket(h.id, Tier::M1, time, &keys, 0.);
        row.stats = [100., ops / 60., ops / 60., ms / 60.]
            .into_iter()
            .flat_map(|v| [v, v, v])
            .collect();
        row
    };
    // One slow request in a short observed interval, then ninety fast requests
    // in a full interval. Already-normalized internal averages retain those counts.
    writer.write_bucket(row(T0, 1., 100.)).unwrap();
    writer.write_bucket(row(T0 + MIN, 10., 10.)).unwrap();
    writer
        .write_bucket(row(T0 + 15 * MIN, 1000., 1000.))
        .unwrap();
    writer
        .write_gap(
            h.id,
            Gap::host(T0 + 2 * MIN, Some(T0 + 15 * MIN), GapReason::Sleep).unwrap(),
        )
        .unwrap();
    writer.flush().unwrap();
    let query = HistoryQuery {
        host: h.id,
        selectors: keys
            .iter()
            .map(|k| SeriesSelector {
                metric: k.metric.clone(),
                labels: k.labels.clone(),
            })
            .collect(),
        from_ms: T0,
        to_ms: T0 + 30 * MIN,
        tier: TierChoice::Fixed(Tier::M1),
        max_points: 100,
    };
    let recent = [row(T0 + MIN, 90., 90.)];
    let read = store
        .reader()
        .unwrap()
        .history_after(&query, || &recent)
        .unwrap();
    let page = disk_io_page(read, Some("disk0"), T0, T0 + 30 * MIN, T0 + 16 * MIN, 1);
    assert_eq!(page.points.len(), 1);
    assert!((page.points[0].read_request_ms.unwrap() - 1190. / 1091.).abs() < 0.00001);
    assert_eq!(page.read_bytes, Some(18_000.));
    assert_eq!(
        page.read_covered_ms,
        3 * MIN,
        "sparse source buckets do not become covered after downsampling"
    );
    assert_eq!(page.to_ms, T0 + 16 * MIN);
    assert_eq!(page.gaps.len(), 1);
    writer.write_bucket(recent[0].clone()).unwrap();
    writer.flush().unwrap();
    writer.prune(T0 + 8 * DAY, Retention::default()).unwrap();
    writer.flush().unwrap();
    let after = store
        .reader()
        .unwrap()
        .history(&HistoryQuery {
            tier: TierChoice::Fixed(Tier::M15),
            max_points: 1,
            ..query.clone()
        })
        .unwrap();
    let coarse = disk_io_page(after, Some("disk0"), T0, T0 + 30 * MIN, T0 + 30 * MIN, 1);
    assert!(
        (coarse.points[0].read_request_ms.unwrap() - 1190. / 1091.).abs() < 0.00001,
        "integrals must survive sparse minute roll-down"
    );
    // Read-time merging (without a public-view transform) retains normalized sums too.
    let value = |id: &str| {
        store
            .reader()
            .unwrap()
            .history(&HistoryQuery {
                selectors: vec![SeriesSelector {
                    metric: MetricId::parse(id).unwrap(),
                    labels: Labels::single("dev", "disk0"),
                }],
                tier: TierChoice::Fixed(Tier::M15),
                max_points: 1,
                ..query.clone()
            })
            .unwrap()
            .series[0]
            .points[0]
            .avg
    };
    assert!(
        (value("disk.read_time") / value("disk.read_timed_ops") - 1190. / 1091.).abs() < 0.00001
    );
}

#[test]
fn authoritative_totals_survive_newcomers_detach_and_missing_timing() {
    let keys = layout(&[
        "disk.read_ops_total",
        "disk.read_timed_ops_total",
        "disk.read_time_total",
        "disk.read_ops{dev=disk0}",
        "disk.read_ops{dev=disk8}",
    ]);
    let make = |t, values: [f32; 5]| {
        let mut row = bucket(host(1).id, Tier::S10, t, &keys, 0.);
        row.stats = values.into_iter().flat_map(|v| [v, v, v]).collect();
        row
    };
    let rows = [
        make(T0, [100., 100., 100., 100., f32::NAN]),
        make(T0 + 10_000, [101., 101., 110., 100., 1.]),
        make(T0 + 20_000, [100., 100., 100., 100., f32::NAN]),
        make(T0 + 30_000, [101., f32::NAN, f32::NAN, 100., 1.]),
    ];
    let query = HistoryQuery {
        host: host(1).id,
        selectors: keys
            .iter()
            .map(|k| SeriesSelector {
                metric: k.metric.clone(),
                labels: k.labels.clone(),
            })
            .collect(),
        from_ms: T0,
        to_ms: T0 + 40_000,
        tier: TierChoice::Fixed(Tier::S10),
        max_points: 100,
    };
    let read = kelvo_store::history_recent(&query, &rows).unwrap();
    let page = disk_io_page(read, None, T0, T0 + 40_000, T0 + 40_000, 100);
    assert_eq!(page.points[0].read_request_ms, Some(1.));
    assert!((page.points[1].read_request_ms.unwrap() - 110. / 101.).abs() < 0.00001);
    assert_eq!(page.points[2].read_request_ms, Some(1.));
    assert_eq!(page.points[3].read_ops_per_sec, Some(101.));
    assert_eq!(page.points[3].read_request_ms, None);
}

#[test]
fn merged_points_use_the_public_epoch_grid_with_a_strict_bound() {
    let keys = layout(&["disk.read{dev=disk0}"]);
    let rows: Vec<_> = (0..11)
        .map(|i| bucket(host(1).id, Tier::S10, T0 + i * 10_000, &keys, 100.))
        .collect();
    let query = HistoryQuery {
        host: host(1).id,
        selectors: keys
            .iter()
            .map(|k| SeriesSelector {
                metric: k.metric.clone(),
                labels: k.labels.clone(),
            })
            .collect(),
        from_ms: T0 + 1_234,
        to_ms: T0 + 105_678,
        tier: TierChoice::Fixed(Tier::S10),
        max_points: 100,
    };
    for limit in [1, 2, 3, 7] {
        let read = kelvo_store::history_recent(&query, &rows).unwrap();
        let page = disk_io_page(
            read,
            Some("disk0"),
            query.from_ms,
            query.to_ms,
            query.to_ms,
            limit,
        );
        assert!(page.points.len() <= limit as usize);
        assert!(page.points.iter().all(|p| p.t % page.bucket_ms == 0));
    }
}

#[test]
fn byte_summary_clips_open_source_bucket_and_explicit_gap_before_downsampling() {
    let keys = layout(&["disk.read{dev=disk0}"]);
    let rows = [bucket(host(1).id, Tier::S10, T0, &keys, 100.)];
    let query = HistoryQuery {
        host: host(1).id,
        selectors: keys
            .iter()
            .map(|k| SeriesSelector {
                metric: k.metric.clone(),
                labels: k.labels.clone(),
            })
            .collect(),
        from_ms: T0 + 123,
        to_ms: T0 + 10_000,
        tier: TierChoice::Fixed(Tier::S10),
        max_points: 100,
    };
    let mut read = kelvo_store::history_recent(&query, &rows).unwrap();
    read.gaps
        .push(Gap::host(T0 + 1000, Some(T0 + 2000), GapReason::Sleep).unwrap());
    let page = disk_io_page(
        read,
        Some("disk0"),
        query.from_ms,
        query.to_ms,
        T0 + 2500,
        1,
    );
    assert_eq!((page.from_ms, page.to_ms), (T0, T0 + 2500));
    assert_eq!((page.read_bytes, page.read_covered_ms), (Some(150.), 1500));
    assert_eq!((page.write_bytes, page.write_covered_ms), (None, 0));
}
